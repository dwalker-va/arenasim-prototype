//! Scene tests for the Burst play (`situation::Play::Burst`,
//! `class_ai::burst`): one per DPS class and per burst tool, each paired with
//! its control — the same scene with the play withheld from the team
//! (`Scene::withhold`), the one condition removed — and under `Legacy`, where
//! the play does not exist.
//!
//! The scene: an enemy Warrior is LOW (20%) and its Priest is stunned for
//! [`LOCK`] seconds, so the team's `kill_window` names the Warrior with an
//! `until` [`LOCK`] seconds out. The deciding unit targets the Warrior unless
//! the test says otherwise.
//!
//! The Chain play is withheld throughout: the Priest's stun is inside its
//! horizon, and these scenes judge the Burst play alone. The burst step
//! below a chain step that does not fire is `tests/chain_scenes.rs`'s.
//!
//! The negative cases: no burst outside a kill window (the Priest free), none
//! by a healer, none by a class the exceptions name; and no cast on the
//! window's subject that would land after `until` — neither from the burst
//! step nor from the chain below it.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Hunter, Mage, Paladin, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::burst::burst_tools;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{PlayStep, PlayTag, RejectionReason};
use arenasim::states::play_match::situation::{ClassSet, Play, PlaySet, CHAIN};
use arenasim::states::play_match::{AbilityType, AuraType};

const ME: &str = "me";
const ALLY: &str = "ally";
const LOW: &str = "low enemy";
const ENEMY_HEALER: &str = "enemy priest";
const OTHER: &str = "other enemy";

/// The stun on the enemy Priest: the kill window ends this far out.
const LOCK: f32 = 4.0;

/// The scene, with the deciding unit of `class` `dist` yards from the low
/// enemy Warrior, the enemy Priest stunned for `lock` seconds (`None`: free).
fn scene(class: CharacterClass, dist: f32, lock: Option<f32>, profile: AiProfile) -> Scene {
    let mut s = Scene::new()
        .ai_profile(1, profile)
        .withhold(1, PlaySet::of(&[CHAIN]))
        .unit(ME, 1, class)
        .at(0.0, 0.0)
        .targeting(LOW)
        .unit(ALLY, 1, Priest)
        .at(-5.0, 0.0)
        .unit(LOW, 2, Warrior)
        .at(dist, 0.0)
        .health(0.2)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(dist, 30.0);
    s.combatant_mut(ME).stealthed = false;
    // The Warrior's shout is up on both, so its chain reaches its strikes.
    for who in [ME, ALLY] {
        s = s.aura(who, AuraType::AttackPowerIncrease, 60.0);
    }
    if let Some(secs) = lock {
        s = s.aura(ENEMY_HEALER, AuraType::Stun, secs);
    }
    s
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// `build` under `TeamPlan` bursts with `tool` on the low enemy, traced as the
/// burst step; with the play withheld it does not choose that (tool, target),
/// nor under `Legacy`.
fn bursts_in(build: impl Fn(AiProfile) -> Scene, class: CharacterClass, tool: AbilityType) {
    assert!(burst_tools(class).contains(&tool), "{class:?} {tool:?}");
    let with = build(AiProfile::TeamPlan).decide(ME);
    let situation = with.situation.expect("a situation");
    assert!(
        situation.kill_window.is_some(),
        "{class:?}: {}",
        describe(&with)
    );
    assert_eq!(
        situation.play,
        Some(PlayTag::Burst),
        "{class:?}: {}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((tool, Some(LOW))),
        "{class:?} bursts: {}",
        describe(&with)
    );
    assert_eq!(situation.play_step, Some(PlayStep::Burst), "{class:?}");

    let without = build(AiProfile::TeamPlan)
        .withhold(
            1,
            PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
        )
        .decide(ME);
    let situation = without.situation.expect("a situation");
    assert!(situation.kill_window.is_some());
    assert_eq!(situation.play, None, "{class:?}: {}", describe(&without));
    assert_eq!(situation.play_step, None);
    assert_ne!(
        without.chose(),
        Some((tool, Some(LOW))),
        "{class:?} without the play: {}",
        describe(&without)
    );

    let legacy = build(AiProfile::Legacy).decide(ME);
    assert_eq!(legacy.situation.and_then(|s| s.play), None);
    assert_eq!(legacy.situation.and_then(|s| s.play_step), None);
    assert_ne!(
        legacy.chose(),
        Some((tool, Some(LOW))),
        "{class:?} under Legacy"
    );
}

fn bursts(class: CharacterClass, dist: f32, tool: AbilityType) {
    bursts_in(|p| scene(class, dist, Some(LOCK), p), class, tool);
}

// ---------------------------------------------------------------------------
// Every class's burst step, every tool
// ---------------------------------------------------------------------------

/// The Warrior targets another enemy in melee beside the low one: the burst
/// lands Mortal Strike on the low one. Without the play its chain — the lock
/// window's Mortal Strike first, since the Priest is locked — strikes its own
/// target.
#[test]
fn a_warrior_mortal_strikes_into_a_kill_window() {
    bursts_in(
        |p| {
            with_rage(
                scene(Warrior, 2.0, Some(LOCK), p)
                    .unit(OTHER, 2, Mage)
                    .at(1.0, 2.0)
                    .target(ME, OTHER),
                80.0,
            )
        },
        Warrior,
        AbilityType::MortalStrike,
    );
}

/// Mortal Strike cooling down, Rend up, 40 rage: the chain holds its rage for
/// the next Mortal Strike (Heroic Strike needs 15 + 50); the burst spends it.
#[test]
fn a_warrior_spends_its_held_rage_on_heroic_strike_into_a_kill_window() {
    let build = |p| {
        let s = scene(Warrior, 2.0, Some(LOCK), p).cooldown(ME, AbilityType::MortalStrike, 4.0);
        with_rage(rend_up(s), 40.0)
    };
    let with = build(AiProfile::TeamPlan).decide(ME);
    assert_eq!(
        with.chose(),
        Some((AbilityType::HeroicStrike, None)),
        "{}",
        describe(&with)
    );
    assert_eq!(with.situation.unwrap().play_step, Some(PlayStep::Burst));
    let without = build(AiProfile::TeamPlan)
        .withhold(
            1,
            PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
        )
        .decide(ME);
    assert_ne!(
        without.chose(),
        Some((AbilityType::HeroicStrike, None)),
        "{}",
        describe(&without)
    );
    assert!(matches!(
        without.rejected(AbilityType::HeroicStrike),
        Some(RejectionReason::InsufficientResource { .. })
    ));
}

/// The Rogue's planner would Kidney Shot a Warrior in melee; the burst lands
/// Sinister Strike.
#[test]
fn a_rogue_sinister_strikes_into_a_kill_window() {
    bursts(Rogue, 2.0, AbilityType::SinisterStrike);
}

/// The Mage's chain would raise Ice Barrier first; the burst lands Frostbolt.
#[test]
fn a_mage_frostbolts_into_a_kill_window() {
    bursts(Mage, 25.0, AbilityType::Frostbolt);
}

#[test]
fn a_warlock_death_coils_into_a_kill_window() {
    bursts(Warlock, 25.0, AbilityType::DeathCoil);
}

/// Death Coil cooling down: Shadow Bolt, which lands inside the window.
#[test]
fn a_warlock_shadow_bolts_into_a_kill_window_with_death_coil_down() {
    bursts_in(
        |p| scene(Warlock, 25.0, Some(LOCK), p).cooldown(ME, AbilityType::DeathCoil, 20.0),
        Warlock,
        AbilityType::Shadowbolt,
    );
}

/// The Hunter targets another enemy: the burst Aimed Shots the low one.
/// Without the play its chain — the lock window's Aimed Shot first, since the
/// Priest is locked — shoots its own target.
#[test]
fn a_hunter_aimed_shots_into_a_kill_window() {
    bursts_in(
        |p| {
            scene(Hunter, 25.0, Some(LOCK), p)
                .unit(OTHER, 2, Mage)
                .at(25.0, -10.0)
                .target(ME, OTHER)
        },
        Hunter,
        AbilityType::AimedShot,
    );
}

/// Aimed Shot cooling down: Arcane Shot.
#[test]
fn a_hunter_arcane_shots_into_a_kill_window_with_aimed_shot_down() {
    bursts_in(
        |p| scene(Hunter, 25.0, Some(LOCK), p).cooldown(ME, AbilityType::AimedShot, 8.0),
        Hunter,
        AbilityType::ArcaneShot,
    );
}

/// An enemy within 20yd of the Hunter: Aimed Shot is not begun, Arcane Shot
/// goes instead.
#[test]
fn a_hunter_with_an_enemy_close_arcane_shots_rather_than_aimed_shots() {
    let view = scene(Hunter, 15.0, Some(LOCK), AiProfile::TeamPlan).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::ArcaneShot, Some(LOW))),
        "{}",
        describe(&view)
    );
    assert!(matches!(
        view.rejected(AbilityType::AimedShot),
        Some(RejectionReason::WithinDeadZone { .. })
    ));
}

// ---------------------------------------------------------------------------
// Aim: the burst goes to the window's subject
// ---------------------------------------------------------------------------

/// A ranged DPS targeting another enemy bursts the window's subject, not its
/// own target; the control casts on its own target or not at all.
#[test]
fn a_ranged_burst_is_aimed_at_the_subject_not_the_units_target() {
    for (class, tool) in [
        (Mage, AbilityType::Frostbolt),
        (Warlock, AbilityType::DeathCoil),
        (Hunter, AbilityType::AimedShot),
    ] {
        let build = |p| {
            scene(class, 25.0, Some(LOCK), p)
                .unit(OTHER, 2, Mage)
                .at(25.0, -10.0)
                .target(ME, OTHER)
        };
        let with = build(AiProfile::TeamPlan).decide(ME);
        assert_eq!(
            with.chose(),
            Some((tool, Some(LOW))),
            "{class:?}: {}",
            describe(&with)
        );
        let without = build(AiProfile::TeamPlan)
            .withhold(
                1,
                PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
            )
            .decide(ME);
        assert!(
            without.choices.iter().all(|(_, t)| *t != Some(LOW)),
            "{class:?} without the play touched the low enemy: {}",
            describe(&without)
        );
    }
}

/// A melee DPS bursts the subject only within reach: out of melee range the
/// burst step's strike is refused for range, and the chain runs.
#[test]
fn a_melee_burst_out_of_reach_does_not_fire() {
    let view = scene(Rogue, 6.0, Some(LOCK), AiProfile::TeamPlan).decide(ME);
    assert_eq!(view.situation.unwrap().play, Some(PlayTag::Burst));
    assert_eq!(
        view.situation.unwrap().play_step,
        None,
        "{}",
        describe(&view)
    );
    assert!(matches!(
        view.rejected(AbilityType::SinisterStrike),
        Some(RejectionReason::OutOfRange { .. })
    ));
}

// ---------------------------------------------------------------------------
// The fit: nothing on the subject that lands after `until`
// ---------------------------------------------------------------------------

/// Frostbolt from 25yd lands 1.5s + 25/35s ≈ 2.21s out. A 2.0s lock refuses it
/// (`OutlastsWindow`) — the burst step's and the chain's alike, so no
/// Frostbolt goes to the subject at all; a 2.5s lock lets it go.
#[test]
fn a_frostbolt_that_lands_after_the_window_ends_is_not_started() {
    let short = scene(Mage, 25.0, Some(2.0), AiProfile::TeamPlan).decide(ME);
    assert!(
        matches!(
            short.rejected(AbilityType::Frostbolt),
            Some(RejectionReason::OutlastsWindow { lands_at, until })
                if *lands_at > *until && (*until - 32.0).abs() < 1e-3
        ),
        "{}",
        describe(&short)
    );
    assert!(
        short
            .choices
            .iter()
            .all(|c| *c != (AbilityType::Frostbolt, Some(LOW))),
        "{}",
        describe(&short)
    );
    let long = scene(Mage, 25.0, Some(2.5), AiProfile::TeamPlan).decide(ME);
    assert_eq!(
        long.chose(),
        Some((AbilityType::Frostbolt, Some(LOW))),
        "{}",
        describe(&long)
    );
    // The control: with the play withheld nothing is fitted, so the same
    // short window refuses no Frostbolt for it.
    let control = scene(Mage, 25.0, Some(2.0), AiProfile::TeamPlan)
        .withhold(
            1,
            PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
        )
        .decide(ME);
    assert!(control
        .candidates
        .iter()
        .all(|c| !matches!(c.reason, Some(RejectionReason::OutlastsWindow { .. }))));
}

/// Aimed Shot (2.5s and its flight) does not fit a 2.0s window; the burst
/// step falls to Arcane Shot, which does.
#[test]
fn an_aimed_shot_that_outlasts_the_window_gives_way_to_arcane_shot() {
    let view = scene(Hunter, 25.0, Some(2.0), AiProfile::TeamPlan).decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::AimedShot),
            Some(RejectionReason::OutlastsWindow { .. })
        ),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::ArcaneShot, Some(LOW))),
        "{}",
        describe(&view)
    );
    assert_eq!(view.situation.unwrap().play_step, Some(PlayStep::Burst));
}

/// Death Coil down and a 1.0s window: Shadow Bolt (2.0s cast) does not fit,
/// and the chain's own casts on the subject that land late — Immolate,
/// Unstable Affliction, Corruption's first tick — are refused too.
#[test]
fn a_warlock_starts_nothing_on_the_subject_that_lands_after_the_window() {
    let view = scene(Warlock, 25.0, Some(1.0), AiProfile::TeamPlan)
        .cooldown(ME, AbilityType::DeathCoil, 20.0)
        .decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::OutlastsWindow { .. })
        ),
        "{}",
        describe(&view)
    );
    let late: Vec<_> = view
        .choices
        .iter()
        .filter(|(a, t)| {
            *t == Some(LOW)
                && matches!(
                    a,
                    AbilityType::Shadowbolt
                        | AbilityType::Immolate
                        | AbilityType::UnstableAffliction
                        | AbilityType::Corruption
                )
        })
        .collect();
    assert!(late.is_empty(), "{}", describe(&view));
    assert!(
        view.candidates
            .iter()
            .any(|c| c.ability == AbilityType::Corruption
                && matches!(c.reason, Some(RejectionReason::OutlastsWindow { .. }))),
        "Corruption's first tick is fitted: {}",
        describe(&view)
    );
}

/// A Rend whose first tick would land after the window is not applied under
/// the play — and is, with it withheld.
#[test]
fn a_rend_whose_first_tick_outlasts_the_window_is_not_applied() {
    // 12 rage: Rend's 10, short of the burst's Heroic Strike (15).
    let build = |p| {
        with_rage(
            scene(Warrior, 2.0, Some(1.0), p).cooldown(ME, AbilityType::MortalStrike, 4.0),
            12.0,
        )
    };
    let with = build(AiProfile::TeamPlan).decide(ME);
    assert!(
        matches!(
            with.rejected(AbilityType::Rend),
            Some(RejectionReason::OutlastsWindow { .. })
        ),
        "{}",
        describe(&with)
    );
    assert_ne!(with.chose(), Some((AbilityType::Rend, Some(LOW))));
    let without = build(AiProfile::TeamPlan)
        .withhold(
            1,
            PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
        )
        .decide(ME);
    assert_eq!(
        without.chose(),
        Some((AbilityType::Rend, Some(LOW))),
        "{}",
        describe(&without)
    );
}

/// A window with no known end (the enemy Priest out of mana) fits every cast.
#[test]
fn a_window_with_no_end_fits_any_cast() {
    let mut s = scene(Mage, 25.0, None, AiProfile::TeamPlan);
    s.combatant_mut(ENEMY_HEALER).current_mana = 0.0;
    let view = s.decide(ME);
    let window = view
        .situation
        .unwrap()
        .kill_window
        .expect("an OOM healer opens it");
    assert_eq!(window.until, None);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Frostbolt, Some(LOW))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Who does not burst
// ---------------------------------------------------------------------------

/// The enemy Priest free and in reach: no kill window, no Burst play, and the
/// decision is the one the play withheld makes.
#[test]
fn no_burst_outside_a_kill_window() {
    for (class, dist) in [
        (Warrior, 2.0),
        (Rogue, 2.0),
        (Mage, 25.0),
        (Warlock, 25.0),
        (Hunter, 25.0),
    ] {
        let free = scene(class, dist, None, AiProfile::TeamPlan).rage_if_warrior(class);
        let view = free.decide(ME);
        let situation = view.situation.expect("a situation");
        assert_eq!(situation.kill_window, None, "{class:?}");
        assert_eq!(situation.play, None, "{class:?}: {}", describe(&view));
        assert_eq!(situation.play_step, None, "{class:?}");
        let withheld = scene(class, dist, None, AiProfile::TeamPlan)
            .rage_if_warrior(class)
            .withhold(
                1,
                PlaySet::of(&[arenasim::states::play_match::situation::Play::Burst, CHAIN]),
            )
            .decide(ME);
        assert_eq!(view.chose(), withheld.chose(), "{class:?}");
    }
}

/// A healer in a kill window runs its own chain: Burst is a DPS's play.
#[test]
fn a_healer_does_not_burst() {
    for class in [Priest, Paladin, Shaman, Druid] {
        assert!(burst_tools(class).is_empty());
        let view = scene(class, 20.0, Some(LOCK), AiProfile::TeamPlan).decide(ME);
        let situation = view.situation.expect("a situation");
        assert!(situation.kill_window.is_some(), "{class:?}");
        assert_eq!(situation.play, None, "{class:?}: {}", describe(&view));
        assert_eq!(situation.play_step, None, "{class:?}");
    }
}

/// A class the exceptions name does not burst, and the fit does not apply to
/// it: a Mage named runs its chain (Ice Barrier first) and is refused no
/// Frostbolt for the window.
#[test]
fn an_excepted_class_does_not_burst() {
    let view = scene(Mage, 25.0, Some(2.0), AiProfile::TeamPlan)
        .exceptions(Play::Burst, ClassSet::of(&[Mage]))
        .decide(ME);
    let situation = view.situation.expect("a situation");
    assert!(situation.kill_window.is_some());
    assert_eq!(situation.play, None, "{}", describe(&view));
    assert!(view
        .candidates
        .iter()
        .all(|c| !matches!(c.reason, Some(RejectionReason::OutlastsWindow { .. }))));
    // ...and one it does not name still bursts.
    let other = scene(Mage, 25.0, Some(LOCK), AiProfile::TeamPlan)
        .exceptions(Play::Burst, ClassSet::of(&[Hunter]))
        .decide(ME);
    assert_eq!(other.situation.unwrap().play_step, Some(PlayStep::Burst));
}

/// `s` with the deciding unit's rage at `points`.
fn with_rage(mut s: Scene, points: f32) -> Scene {
    s.combatant_mut(ME).current_mana = points;
    s
}

trait WarriorRage {
    fn rage_if_warrior(self, class: CharacterClass) -> Self;
}

impl WarriorRage for Scene {
    fn rage_if_warrior(self, class: CharacterClass) -> Self {
        if class == Warrior {
            with_rage(self, 80.0)
        } else {
            self
        }
    }
}

/// Rend already up on the low enemy, the Warrior's own (so its chain does not
/// re-apply it).
fn rend_up(mut s: Scene) -> Scene {
    let me = s.entity(ME);
    s.clear_auras(LOW);
    s.aura_full(
        LOW,
        arenasim::states::play_match::Aura {
            effect_type: AuraType::DamageOverTime,
            duration: 15.0,
            magnitude: 5.0,
            tick_interval: 3.0,
            time_until_next_tick: 3.0,
            break_on_damage_threshold: -1.0,
            caster: Some(me),
            ability_name: "Rend".into(),
            ..Default::default()
        },
    )
}
