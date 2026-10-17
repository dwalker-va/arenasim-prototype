//! Scene tests for the Chain play (`situation::Play::Chain`,
//! `class_ai::chain`): every class's every hand-off, each paired with its
//! control — the same scene with the play withheld from the team
//! (`Scene::withhold`), the one condition removed — and under `Legacy`, where
//! the play does not exist.
//!
//! The scene: the deciding unit and its Priest fight an enemy Warrior (their
//! target) and an enemy Priest. The team holds the enemy Priest in a stun
//! (cast by the deciding unit's ally) with a few seconds or less left: the
//! lock the Chain play hands off from. The timing cases pin where a hand-off
//! LANDS against that lock's end: inside its last `HANDOFF_LEAD` (handed
//! off), up to one global cooldown before that (the global cooldown held),
//! further out (nothing held), and after it (too late: not handed off).
//!
//! The negative cases are the hand-off's own rules: no damage onto a lock
//! that breaks on damage, no breakable or shielding crowd control onto an
//! enemy the team is hitting, nothing onto a unit immune to it by
//! diminishing returns, no second hand-off while a teammate's is mid-cast,
//! no healer chaining while a teammate of its own is dying, and no class the
//! exceptions name.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Mage, Paladin, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::chain::{chain_tools, HANDOFF_LEAD};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{CompoundDebuff, DRCategory, DRTracker};
use arenasim::states::play_match::decision_trace::{PlayStep, PlayTag, RejectionReason};
use arenasim::states::play_match::situation::{ClassSet, PlaySet, CHAIN};
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

const ME: &str = "me";
const ALLY: &str = "ally";
const TARGET: &str = "enemy warrior";
const HEALER: &str = "enemy priest";

/// Where the scene's other units stand, and who the ally is.
#[derive(Clone, Copy)]
struct Layout {
    ally: CharacterClass,
    ally_health: f32,
    /// The enemy Warrior's spot.
    target_at: (f32, f32),
    target_health: f32,
}

const LAYOUT: Layout = Layout {
    ally: Priest,
    ally_health: 1.0,
    target_at: (-3.0, -20.0),
    target_health: 1.0,
};

/// The scene: the deciding unit of `class` at the origin, the enemy Priest
/// `dist` yards off, held in a stun with `left` seconds to run.
fn scene(class: CharacterClass, dist: f32, left: f32, profile: AiProfile) -> Scene {
    laid_out(class, dist, left, profile, LAYOUT)
}

fn laid_out(
    class: CharacterClass,
    dist: f32,
    left: f32,
    profile: AiProfile,
    layout: Layout,
) -> Scene {
    let s = Scene::new()
        .ai_profile(1, profile)
        .unit(ME, 1, class)
        .at(0.0, 0.0)
        .targeting(TARGET)
        .unit(ALLY, 1, layout.ally)
        .at(-5.0, 0.0)
        .health(layout.ally_health)
        .targeting(TARGET)
        .unit(TARGET, 2, Warrior)
        .at(layout.target_at.0, layout.target_at.1)
        .health(layout.target_health)
        .targeting(ALLY)
        .unit(HEALER, 2, Priest)
        .at(0.0, dist);
    let mut s = if left > 0.0 {
        lock(s, HEALER, AuraType::Stun, left, -1.0)
    } else {
        s
    };
    s.combatant_mut(ME).stealthed = false;
    upkept(s)
}

/// The team's buffs up and the Mage's Ice Barrier spent, so a decision
/// reaches past "raise the buff".
fn upkept(mut s: Scene) -> Scene {
    s = s.cooldown(ME, AbilityType::IceBarrier, 20.0);
    s = s.aura(ME, AuraType::FrostArmorBuff, 600.0);
    for who in [ME, ALLY] {
        s = s.aura(who, AuraType::MaxManaIncrease, 600.0).aura_full(
            who,
            Aura {
                effect_type: AuraType::MaxHealthIncrease,
                duration: 600.0,
                compound: Some(CompoundDebuff::MarkOfTheWild),
                ..Default::default()
            },
        );
    }
    s
}

/// `who` held by the ally in `effect` with `left` seconds to run, breaking
/// at `threshold` damage (negative: never).
fn lock(s: Scene, who: &'static str, effect: AuraType, left: f32, threshold: f32) -> Scene {
    let caster = s.entity(ALLY);
    s.aura_full(
        who,
        Aura {
            effect_type: effect,
            duration: left,
            break_on_damage_threshold: threshold,
            caster: Some(caster),
            ability_name: format!("{effect:?}"),
            ..Default::default()
        },
    )
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

fn withheld(s: Scene) -> Scene {
    s.withhold(1, PlaySet::of(&[CHAIN]))
}

/// `build` under `TeamPlan` hands `tool` off onto the enemy Priest, traced as
/// the chain step under the Chain play on that Priest; with the play
/// withheld it does not choose that (tool, target), nor under `Legacy`.
fn hands_off(build: impl Fn(AiProfile) -> Scene, class: CharacterClass, tool: AbilityType) {
    assert!(chain_tools(class).contains(&tool), "{class:?} {tool:?}");
    let s = build(AiProfile::TeamPlan);
    let healer = s.entity(HEALER).index();
    let with = s.decide(ME);
    let situation = with.situation.expect("a situation");
    assert_eq!(
        situation.play,
        Some(PlayTag::Chain { subject: healer }),
        "{class:?}: {}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((tool, Some(HEALER))),
        "{class:?} hands off: {}",
        describe(&with)
    );
    assert_eq!(situation.play_step, Some(PlayStep::Chain), "{class:?}");

    let without = withheld(build(AiProfile::TeamPlan)).decide(ME);
    let situation = without.situation.expect("a situation");
    assert_eq!(situation.play, None, "{class:?}: {}", describe(&without));
    assert_eq!(situation.play_step, None);
    assert_ne!(
        without.chose(),
        Some((tool, Some(HEALER))),
        "{class:?} without the play: {}",
        describe(&without)
    );

    let legacy = build(AiProfile::Legacy).decide(ME);
    assert_eq!(legacy.situation.and_then(|s| s.play), None);
    assert_ne!(
        legacy.chose(),
        Some((tool, Some(HEALER))),
        "{class:?} under Legacy: {}",
        describe(&legacy)
    );
}

// ---------------------------------------------------------------------------
// Every class's every hand-off, landing in the lock's last moments
// ---------------------------------------------------------------------------

/// An instant: the stun has 0.2s left, inside the last `HANDOFF_LEAD`.
#[test]
fn a_paladin_hands_hammer_of_justice_off_into_the_end_of_a_lock() {
    hands_off(
        |p| scene(Paladin, 6.0, 0.2, p),
        Paladin,
        AbilityType::HammerOfJustice,
    );
}

#[test]
fn a_rogue_hands_kidney_shot_off_into_the_end_of_a_lock() {
    hands_off(
        |p| scene(Rogue, 2.0, 0.2, p),
        Rogue,
        AbilityType::KidneyShot,
    );
}

/// Death Coil flies 8 yards at 40 a second: it lands 0.2s after it leaves,
/// so it goes with 0.3s of the stun left. Fear, tried first, would land
/// after the stun ends.
#[test]
fn a_warlock_hands_death_coil_off_into_the_end_of_a_lock() {
    hands_off(
        |p| scene(Warlock, 8.0, 0.3, p),
        Warlock,
        AbilityType::DeathCoil,
    );
    let with = scene(Warlock, 8.0, 0.3, AiProfile::TeamPlan).decide(ME);
    assert!(matches!(
        with.rejected(AbilityType::Fear),
        Some(RejectionReason::MissesHandoff { lands_at, until }) if lands_at > until
    ));
}

/// A 1.5s cast: begun with 1.7s of the stun left, it lands in its last
/// 0.2s. The hold rule judges it where it LANDS — without the play it is
/// refused at its start for the stun it would chain onto.
#[test]
fn a_warlock_hands_fear_off_into_the_end_of_a_lock() {
    hands_off(|p| scene(Warlock, 20.0, 1.7, p), Warlock, AbilityType::Fear);
}

#[test]
fn a_mage_hands_polymorph_off_into_the_end_of_a_lock() {
    hands_off(|p| scene(Mage, 20.0, 1.7, p), Mage, AbilityType::Polymorph);
    // The enemy Priest is the Mage's crowd-control target, so its rotation
    // would polymorph it — but for the stun.
    let mut without = withheld(scene(Mage, 20.0, 1.7, AiProfile::TeamPlan));
    let healer = without.entity(HEALER);
    without.combatant_mut(ME).cc_target = Some(healer);
    let without = without.decide(ME);
    assert!(
        matches!(
            without.rejected(AbilityType::Polymorph),
            Some(RejectionReason::TargetAlreadyCCd { .. })
        ),
        "without the play the hold rule refuses it at its start: {}",
        describe(&without)
    );
}

#[test]
fn a_druid_hands_cyclone_off_into_the_end_of_a_lock() {
    hands_off(|p| scene(Druid, 15.0, 1.7, p), Druid, AbilityType::Cyclone);
}

/// A hand-off needs no lock of a teammate's: a lock that does not break on
/// damage (a stun) takes a hand-off whatever cast it. And a hand-off that
/// deals no damage lands on one that does break — the Paladin's Hammer of
/// Justice into the end of a Polymorph.
#[test]
fn a_hammer_of_justice_hands_off_into_the_end_of_a_polymorph() {
    let build = |p| {
        let mut s = scene(Paladin, 6.0, 0.2, p);
        s.clear_auras(HEALER);
        lock(s, HEALER, AuraType::Polymorph, 0.2, 0.0)
    };
    hands_off(build, Paladin, AbilityType::HammerOfJustice);
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// The Polymorph would land a second before the stun's last moments: within
/// one global cooldown of its moment, so the Mage holds its global cooldown
/// (no cast at all, traced as the chain step) instead of a Frostbolt that
/// would still be casting then. Without the play it casts the Frostbolt.
#[test]
fn a_hand_off_due_within_one_global_cooldown_holds_it() {
    let left = 1.5 + HANDOFF_LEAD + 1.0;
    let with = scene(Mage, 20.0, left, AiProfile::TeamPlan).decide(ME);
    assert_eq!(with.chose(), None, "{}", describe(&with));
    assert!(with.choices.is_empty());
    assert_eq!(
        with.situation.unwrap().play_step,
        Some(PlayStep::Chain),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(AbilityType::Polymorph),
        Some(RejectionReason::MissesHandoff { lands_at, until })
            if (until - HANDOFF_LEAD - lands_at - 1.0).abs() < 1e-3
    ));
    let without = withheld(scene(Mage, 20.0, left, AiProfile::TeamPlan)).decide(ME);
    assert_eq!(
        without.chose(),
        Some((AbilityType::Frostbolt, Some(TARGET))),
        "{}",
        describe(&without)
    );
}

/// More than a global cooldown out, nothing is held: the Mage's chain casts
/// its Frostbolt under the Chain play, the chain step having chosen nothing.
#[test]
fn a_hand_off_more_than_a_global_cooldown_out_holds_nothing() {
    let left = 1.5 + HANDOFF_LEAD + 1.7;
    let with = scene(Mage, 20.0, left, AiProfile::TeamPlan).decide(ME);
    let situation = with.situation.unwrap();
    assert!(matches!(situation.play, Some(PlayTag::Chain { .. })));
    assert_eq!(situation.play_step, None, "{}", describe(&with));
    assert_eq!(
        with.chose(),
        Some((AbilityType::Frostbolt, Some(TARGET))),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(AbilityType::Polymorph),
        Some(RejectionReason::MissesHandoff { .. })
    ));
}

/// A hand-off that would land after the lock has ended is not one: a 1.5s
/// Polymorph with 1s of the stun left. Nothing is held for it either.
#[test]
fn a_hand_off_that_would_land_after_the_lock_is_not_made() {
    let with = scene(Mage, 20.0, 1.0, AiProfile::TeamPlan).decide(ME);
    let situation = with.situation.unwrap();
    assert_ne!(
        situation.play_step,
        Some(PlayStep::Chain),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(AbilityType::Polymorph),
        Some(RejectionReason::MissesHandoff { lands_at, until }) if lands_at > until
    ));
}

/// The Chain play runs only within `CHAIN_HORIZON_SECS` of the lock's end.
#[test]
fn a_lock_further_out_than_the_horizon_is_no_chain() {
    let with = scene(Mage, 20.0, 6.0, AiProfile::TeamPlan).decide(ME);
    assert_eq!(with.situation.unwrap().play, None, "{}", describe(&with));
}

// ---------------------------------------------------------------------------
// What a hand-off asks of the lock and its subject
// ---------------------------------------------------------------------------

/// Death Coil's damage would break a lock that breaks on damage: a
/// Polymorph (on any damage) or a Fear (past its threshold).
#[test]
fn no_damage_is_handed_onto_a_lock_that_breaks_on_damage() {
    for (effect, threshold) in [(AuraType::Polymorph, 0.0), (AuraType::Fear, 100.0)] {
        let mut s = scene(Warlock, 8.0, 0.3, AiProfile::TeamPlan);
        s.clear_auras(HEALER);
        let with = lock(s, HEALER, effect, 0.3, threshold).decide(ME);
        assert_ne!(
            with.chose(),
            Some((AbilityType::DeathCoil, Some(HEALER))),
            "{effect:?}: {}",
            describe(&with)
        );
        assert!(
            matches!(
                with.rejected(AbilityType::DeathCoil),
                Some(RejectionReason::PreconditionUnmet { note }) if note.contains("break the lock")
            ),
            "{effect:?}: {}",
            describe(&with)
        );
    }
}

/// The team is hitting the stunned enemy (the Warrior, the ally's target —
/// the deciding unit targets the enemy Priest): a Polymorph would break at
/// once, and a Cyclone would shield it from the damage. Neither is handed
/// off; a Hammer of Justice is.
#[test]
fn no_breakable_or_shielding_cc_is_handed_onto_an_enemy_the_team_is_hitting() {
    let on_target = |class, dist: f32, left| {
        let layout = Layout {
            target_at: (0.0, dist),
            ..LAYOUT
        };
        let s = laid_out(class, 30.0, 0.0, AiProfile::TeamPlan, layout).target(ME, HEALER);
        lock(s, TARGET, AuraType::Stun, left, -1.0).decide(ME)
    };
    for (class, tool, dist) in [
        (Mage, AbilityType::Polymorph, 20.0),
        (Druid, AbilityType::Cyclone, 15.0),
    ] {
        let with = on_target(class, dist, 1.7);
        assert_ne!(
            with.chose(),
            Some((tool, Some(TARGET))),
            "{}",
            describe(&with)
        );
        assert!(
            matches!(
                with.rejected(tool),
                Some(RejectionReason::PreconditionUnmet { note }) if note.contains("team's damage")
            ),
            "{class:?}: {}",
            describe(&with)
        );
    }
    let paladin = on_target(Paladin, 6.0, 0.2);
    assert_eq!(
        paladin.chose(),
        Some((AbilityType::HammerOfJustice, Some(TARGET))),
        "{}",
        describe(&paladin)
    );
}

/// An enemy immune to stuns by diminishing returns takes no Hammer.
#[test]
fn nothing_is_handed_onto_an_enemy_immune_to_it() {
    let mut tracker = DRTracker::default();
    for _ in 0..3 {
        tracker.apply(DRCategory::Stuns);
    }
    let with = scene(Paladin, 6.0, 0.2, AiProfile::TeamPlan)
        .dr_tracker(HEALER, tracker)
        .decide(ME);
    assert_ne!(
        with.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(AbilityType::HammerOfJustice),
        Some(RejectionReason::DRImmune {
            category: DRCategory::Stuns
        })
    ));
}

/// A teammate mid-cast on a hand-off (the ally's Polymorph, begun for this
/// lock) is the hand-off: the Paladin does not spend its Hammer on the same
/// moment.
#[test]
fn no_second_hand_off_while_a_teammates_is_mid_cast() {
    let layout = Layout {
        ally: Mage,
        ..LAYOUT
    };
    let with = laid_out(Paladin, 6.0, 0.2, AiProfile::TeamPlan, layout)
        .casting(ALLY, AbilityType::Polymorph)
        .decide(ME);
    assert_ne!(with.situation.unwrap().play_step, Some(PlayStep::Chain));
    assert_ne!(
        with.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "{}",
        describe(&with)
    );
}

/// A healer does not chain while a teammate of its own is dying: its heals
/// come first. The dying ally is under no attack (the enemy Warrior is on
/// the Paladin), so no ally is in danger and the play is not a Peel: the
/// dying rule alone keeps the Chain off. With the ally healthy, it chains.
#[test]
fn a_healer_does_not_chain_while_a_teammate_is_dying() {
    let build = |ally_health| {
        let layout = Layout {
            ally_health,
            ..LAYOUT
        };
        laid_out(Paladin, 6.0, 0.2, AiProfile::TeamPlan, layout).target(TARGET, ME)
    };
    let healthy = build(1.0).decide(ME);
    assert!(
        matches!(healthy.situation.unwrap().play, Some(PlayTag::Chain { .. })),
        "{}",
        describe(&healthy)
    );
    let with = build(0.3).decide(ME);
    let situation = with.situation.unwrap();
    assert_eq!(situation.ally_in_danger, None, "{}", describe(&with));
    assert_eq!(situation.play, None, "{}", describe(&with));
    assert_ne!(
        with.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "{}",
        describe(&with)
    );
}

/// A class `CHAIN_EXCEPTIONS` (or the measurement override) names runs its
/// fallback play.
#[test]
fn a_chain_exception_names_a_class_that_does_not_chain() {
    let with = scene(Paladin, 6.0, 0.2, AiProfile::TeamPlan)
        .exceptions(CHAIN, ClassSet::of(&[Paladin]))
        .decide(ME);
    assert_eq!(with.situation.unwrap().play, None, "{}", describe(&with));
    assert_ne!(
        with.chose(),
        Some((AbilityType::HammerOfJustice, Some(HEALER))),
        "{}",
        describe(&with)
    );
}

/// The lock a chain hands off from is the one ending SOONEST: with the
/// Warrior's stun ending first, the Paladin's Hammer goes on the Warrior.
#[test]
fn the_lock_ending_soonest_is_the_one_handed_off_from() {
    let layout = Layout {
        target_at: (6.0, 0.0),
        ..LAYOUT
    };
    let s = laid_out(Paladin, 9.0, 3.0, AiProfile::TeamPlan, layout);
    let s = lock(s, TARGET, AuraType::Stun, 0.2, -1.0);
    let target = s.entity(TARGET).index();
    let with = s.decide(ME);
    assert_eq!(
        with.situation.unwrap().play,
        Some(PlayTag::Chain { subject: target }),
        "{}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((AbilityType::HammerOfJustice, Some(TARGET))),
        "{}",
        describe(&with)
    );
}

/// A chain step that does not fire hands the chain to the play below it: a
/// kill window is open (the enemy Warrior low, its Priest stunned), so the
/// Mage, its Polymorph's moment more than a global cooldown out, bursts.
#[test]
fn below_a_chain_step_that_does_not_fire_the_burst_step_runs() {
    let layout = Layout {
        target_health: 0.2,
        ..LAYOUT
    };
    let left = 1.5 + HANDOFF_LEAD + 1.7;
    let with = laid_out(Mage, 20.0, left, AiProfile::TeamPlan, layout).decide(ME);
    let situation = with.situation.unwrap();
    assert!(situation.kill_window.is_some(), "{}", describe(&with));
    assert!(matches!(situation.play, Some(PlayTag::Chain { .. })));
    assert_eq!(
        situation.play_step,
        Some(PlayStep::Burst),
        "{}",
        describe(&with)
    );
    assert_eq!(
        with.chose(),
        Some((AbilityType::Frostbolt, Some(TARGET))),
        "{}",
        describe(&with)
    );
}

/// A lock that makes its subject immune — a Cyclone — takes no hand-off:
/// nothing lands on a cycloned unit, so the Kidney Shot would be wasted.
#[test]
fn nothing_is_handed_onto_a_subject_immune_to_it() {
    let mut s = scene(Rogue, 2.0, 0.2, AiProfile::TeamPlan);
    s.clear_auras(HEALER);
    let with = lock(s, HEALER, AuraType::Cyclone, 0.2, -1.0).decide(ME);
    assert_ne!(
        with.chose(),
        Some((AbilityType::KidneyShot, Some(HEALER))),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(AbilityType::KidneyShot),
        Some(RejectionReason::TargetImmune)
    ));
}

/// A class with nothing to hand off runs its fallback play: no Chain on its
/// decision, whatever lock is ending.
#[test]
fn a_class_with_no_hand_off_runs_its_fallback_play() {
    let classes: Vec<CharacterClass> = CharacterClass::all()
        .iter()
        .copied()
        .filter(|c| chain_tools(*c).is_empty())
        .collect();
    assert!(!classes.is_empty());
    for class in classes {
        let with = scene(class, 6.0, 0.2, AiProfile::TeamPlan).decide(ME);
        assert!(
            !matches!(with.situation.unwrap().play, Some(PlayTag::Chain { .. })),
            "{class:?}: {}",
            describe(&with)
        );
    }
}

/// Every class with a hand-off HOLDS its global cooldown when its hand-off's
/// moment is within one: no cast at all, traced as the chain step, its tool
/// rejected `MissesHandoff` with the moment one second away. Without the play
/// the same unit acts. An instant hand-off (Hammer of Justice, Kidney Shot)
/// with 1.25s of the lock left; a 1.5s one (Fear, Cyclone) with 2.75s. (The
/// Mage's is [`a_hand_off_due_within_one_global_cooldown_holds_it`].)
#[test]
fn every_class_holds_its_global_cooldown_for_a_hand_off_within_one() {
    // The Rogue's target in reach, so without the play it has a strike; the
    // others' far off, so no self-peel (a Warlock's Death Coil on a melee)
    // comes ahead of the chain step.
    let cases = [
        (
            Paladin,
            AbilityType::HammerOfJustice,
            6.0,
            0.0,
            LAYOUT.target_at,
        ),
        (Rogue, AbilityType::KidneyShot, 2.0, 0.0, (1.5, -1.0)),
        (Warlock, AbilityType::Fear, 20.0, 1.5, LAYOUT.target_at),
        (Druid, AbilityType::Cyclone, 15.0, 1.5, LAYOUT.target_at),
    ];
    for (class, tool, dist, cast, target_at) in cases {
        assert!(chain_tools(class).contains(&tool));
        let left = cast + HANDOFF_LEAD + 1.0;
        let layout = Layout {
            target_at,
            ..LAYOUT
        };
        let build = || laid_out(class, dist, left, AiProfile::TeamPlan, layout);
        let with = build().decide(ME);
        assert_eq!(with.chose(), None, "{class:?} holds: {}", describe(&with));
        assert!(with.choices.is_empty(), "{class:?}");
        assert_eq!(
            with.situation.unwrap().play_step,
            Some(PlayStep::Chain),
            "{class:?}: {}",
            describe(&with)
        );
        assert!(
            matches!(
                with.rejected(tool),
                Some(RejectionReason::MissesHandoff { lands_at, until })
                    if (until - HANDOFF_LEAD - lands_at - 1.0).abs() < 1e-3
            ),
            "{class:?}: {}",
            describe(&with)
        );
        let without = withheld(build()).decide(ME);
        assert!(
            without.chose().is_some(),
            "{class:?} without the play acts: {}",
            describe(&without)
        );
    }
}
