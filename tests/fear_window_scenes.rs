//! Scene tests for the Warlock's Fear window (AS-495, `warlock::fear_window_step`):
//! while its OWN Fear holds its target, the Warlock
//!
//! - Fears its own target only at full length (`warlock::fear_lands_full`),
//!   so it never re-Fears into diminishing returns, not even inside the
//!   hold rule's chain window, where it used to;
//! - times Shadow Bolt to land in the Fear's last `FEAR_WINDOW_LEAD`, never
//!   before (the cast gate's `time_own_fear`), holding the global cooldown
//!   just before that moment;
//! - spends the globals before it on what does not break the Fear: Life Tap
//!   topping up, a curse that deals no damage, Corruption on another enemy.
//!
//! Each test runs the real class dispatch for one tick and is paired with its
//! control. The staged 1v1 that plays a whole window is
//! `tests/fear_window_staged.rs`.

use arenasim::states::match_config::CharacterClass::{Druid, Mage, Warlock, Warrior};
use arenasim::states::match_config::WarlockCurse;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::cast_guard::lands_after;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::class_ai::warlock::{
    FEAR_DIMINISHED_NOTE, FEAR_WINDOW_HOLD_NOTE,
};
use arenasim::states::play_match::class_ai::{CC_CHAIN_WINDOW, FEAR_WINDOW_AIM, FEAR_WINDOW_LEAD};
use arenasim::states::play_match::components::{DRCategory, DRTracker};
use arenasim::states::play_match::constants::GCD;
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::{AbilityType, Aura, AuraType, SpellSchool};

const WARLOCK: &str = "warlock";
const HEALER: &str = "enemy druid";
const OTHER: &str = "enemy warrior";
const ALLY: &str = "ally mage";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn dot(name: &str, caster: bevy::prelude::Entity) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 15.0,
        magnitude: 10.0,
        tick_interval: 3.0,
        // A DoT never breaks on damage (`Aura`'s default 0.0 is "breaks on
        // any damage", the Polymorph's threshold).
        break_on_damage_threshold: -1.0,
        caster: Some(caster),
        ability_name: name.into(),
        ..Default::default()
    }
}

/// The 1v1 the AS-436 loss was measured in: the Warlock 20 yards from a lone
/// Druid, its Corruption, Unstable Affliction and Immolate already on it, so
/// the rotation's own turn reaches its Fear.
fn lone() -> Scene {
    let scene = Scene::new()
        .unit(WARLOCK, 1, Warlock)
        .at(0.0, 0.0)
        .targeting(HEALER)
        .unit(HEALER, 2, Druid)
        .at(20.0, 0.0)
        .targeting(WARLOCK);
    let me = scene.entity(WARLOCK);
    scene
        .aura_full(HEALER, dot("Corruption", me))
        .aura_full(HEALER, dot("Unstable Affliction", me))
        .aura_full(HEALER, dot("Immolate", me))
}

/// `who`'s diminishing returns on Fears, `fears` of them landed.
fn feared_before(scene: Scene, who: &'static str, fears: u8) -> Scene {
    let mut tracker = DRTracker::default();
    for _ in 0..fears {
        tracker.apply(DRCategory::Fears);
    }
    scene.dr_tracker(who, tracker)
}

/// A Fear on `who`, `secs` left, cast by `caster`, breaking at 100 damage as
/// `abilities.ron` gives it.
fn fear(scene: Scene, who: &'static str, caster: &'static str, secs: f32) -> Scene {
    let caster = scene.entity(caster);
    scene.aura_full(
        who,
        Aura {
            effect_type: AuraType::Fear,
            duration: secs,
            break_on_damage_threshold: 100.0,
            caster: Some(caster),
            ability_name: "Fear".into(),
            ..Default::default()
        },
    )
}

/// The Warlock's own Fear on its target, `secs` left, as a landed Fear
/// leaves it: one step of diminishing returns spent.
fn own_fear(scene: Scene, secs: f32) -> Scene {
    feared_before(fear(scene, HEALER, WARLOCK, secs), HEALER, 1)
}

/// When a Shadow Bolt begun now from the Warlock lands on the Druid.
fn bolt_lands_after(scene: &Scene) -> f32 {
    let defs = load_ability_definitions().expect("abilities.ron");
    lands_after(
        defs.get(&AbilityType::Shadowbolt).unwrap(),
        scene.position(WARLOCK),
        scene.position(HEALER),
        None,
    )
}

fn chose_ability(view: &DecisionView) -> Option<AbilityType> {
    view.chose().map(|(a, _)| a)
}

fn noted(view: &DecisionView, ability: AbilityType, note: &str) -> bool {
    matches!(
        view.rejected(ability),
        Some(RejectionReason::PreconditionUnmet { note: n }) if n == note
    )
}

// ---------------------------------------------------------------------------
// Full-length Fears only: never into diminishing returns.
// ---------------------------------------------------------------------------

#[test]
fn the_warlock_fears_its_own_target_only_at_full_length() {
    let fresh = lone().decide(WARLOCK);
    assert_eq!(
        fresh.chose(),
        Some((AbilityType::Fear, Some(HEALER))),
        "a target fresh on Fears is Feared: {}",
        describe(&fresh)
    );

    // The control: one Fear already landed would make the next 4s.
    for landed in [1, 2] {
        let diminished = feared_before(lone(), HEALER, landed).decide(WARLOCK);
        assert_ne!(
            chose_ability(&diminished),
            Some(AbilityType::Fear),
            "a Fear at diminishing returns level {landed} is not cast: {}",
            describe(&diminished)
        );
        assert!(
            noted(&diminished, AbilityType::Fear, FEAR_DIMINISHED_NOTE),
            "traced as diminished: {}",
            describe(&diminished)
        );
    }
}

#[test]
fn only_its_own_target_is_held_to_full_length_its_cc_target_is_not() {
    // The rotation's Fear goes to its CC target when it has one. That target
    // is not the Warlock's own (the ruling's scope), so a diminished Fear on
    // it still goes, as on main.
    let with_cc_target = |cc_target: bool| {
        let scene = lone().unit(OTHER, 2, Warrior).at(18.0, 8.0);
        let mut scene = feared_before(feared_before(scene, HEALER, 1), OTHER, 1);
        if cc_target {
            let other = scene.entity(OTHER);
            scene.combatant_mut(WARLOCK).cc_target = Some(other);
        }
        scene.decide(WARLOCK)
    };
    let cc = with_cc_target(true);
    assert_eq!(
        cc.chose(),
        Some((AbilityType::Fear, Some(OTHER))),
        "a diminished Fear on the CC target still goes: {}",
        describe(&cc)
    );

    // The control: no CC target, so the rotation's Fear is on its own
    // target, diminished, and is not cast.
    let own = with_cc_target(false);
    assert_ne!(
        chose_ability(&own),
        Some(AbilityType::Fear),
        "{}",
        describe(&own)
    );
    assert!(
        noted(&own, AbilityType::Fear, FEAR_DIMINISHED_NOTE),
        "its own target is held to full length: {}",
        describe(&own)
    );
}

#[test]
fn no_re_fear_onto_its_own_fear_in_the_chain_window() {
    // Inside the hold rule's chain window, where the rotation's Fear used to
    // go out and land a second after the Fear ended, at 50%.
    let ending = own_fear(lone(), CC_CHAIN_WINDOW - 0.2).decide(WARLOCK);
    assert_ne!(
        chose_ability(&ending),
        Some(AbilityType::Fear),
        "no re-Fear onto its own ending Fear: {}",
        describe(&ending)
    );
    assert_eq!(
        ending.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "the bolt goes out instead: {}",
        describe(&ending)
    );
}

// ---------------------------------------------------------------------------
// Shadow Bolt lands in the Fear's last moment, never before.
// ---------------------------------------------------------------------------

#[test]
fn shadow_bolt_is_timed_to_land_as_the_fear_ends() {
    // The control: no Fear, so it bolts at once.
    let free = feared_before(lone(), HEALER, 1).decide(WARLOCK);
    assert_eq!(
        free.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "with no Fear up the Warlock bolts at once: {}",
        describe(&free)
    );

    let lands = bolt_lands_after(&lone());
    // Begun now, it lands in the Fear's last FEAR_WINDOW_LEAD: it goes.
    let due = own_fear(lone(), lands + FEAR_WINDOW_AIM - 0.05).decide(WARLOCK);
    assert_eq!(
        due.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "a bolt landing in the Fear's last moment goes: {}",
        describe(&due)
    );

    // Begun now, it would land before that moment, which is less than a
    // global cooldown away: the Warlock holds its global cooldown.
    let held = own_fear(lone(), lands + FEAR_WINDOW_LEAD + GCD / 2.0).decide(WARLOCK);
    assert_eq!(held.chose(), None, "it holds: {}", describe(&held));
    assert!(
        noted(&held, AbilityType::Shadowbolt, FEAR_WINDOW_HOLD_NOTE),
        "traced as the hold: {}",
        describe(&held)
    );

    // Earlier still: the bolt is refused, the Fear would break.
    let early = own_fear(lone(), 6.0).decide(WARLOCK);
    assert_ne!(
        chose_ability(&early),
        Some(AbilityType::Shadowbolt),
        "{}",
        describe(&early)
    );
    assert!(
        matches!(
            early.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "a bolt landing early is refused for the Fear: {}",
        describe(&early)
    );
}

/// The ruling's window in literal seconds — a quarter second — so that a
/// change of `FEAR_WINDOW_LEAD` or of the gate's exemption fails here, where
/// every other scene derives its times from the constant.
#[test]
fn the_gate_lets_damage_land_only_in_the_fears_last_quarter_second() {
    // Locked out of Shadow (an interrupt's lockout), the Warlock cannot bolt,
    // so nothing holds its global cooldown, and its Fire Immolate (a 2s cast,
    // its hit landing as the cast ends) meets the gate alone.
    let immolate = |left: f32| {
        let scene = Scene::new()
            .unit(WARLOCK, 1, Warlock)
            .at(0.0, 0.0)
            .targeting(HEALER)
            .unit(HEALER, 2, Druid)
            .at(20.0, 0.0)
            .targeting(WARLOCK);
        let me = scene.entity(WARLOCK);
        let scene = scene
            .aura_full(HEALER, dot("Corruption", me))
            .aura_full(HEALER, dot("Unstable Affliction", me))
            .aura_full(
                WARLOCK,
                Aura {
                    effect_type: AuraType::SpellSchoolLockout,
                    duration: 4.0,
                    magnitude: SpellSchool::Shadow.to_lockout_magnitude(),
                    caster: Some(me),
                    ability_name: "Spell Lock".into(),
                    ..Default::default()
                },
            );
        own_fear(scene, left).decide(WARLOCK)
    };
    let in_window = immolate(2.0 + 0.2);
    assert_eq!(
        in_window.chose(),
        Some((AbilityType::Immolate, Some(HEALER))),
        "landing 0.2s before the Fear ends, it goes: {}",
        describe(&in_window)
    );
    let early = immolate(2.0 + 0.35);
    assert_ne!(
        chose_ability(&early),
        Some(AbilityType::Immolate),
        "{}",
        describe(&early)
    );
    assert!(
        matches!(
            early.rejected(AbilityType::Immolate),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "landing 0.35s before the Fear ends, it is refused: {}",
        describe(&early)
    );
}

#[test]
fn the_bolt_is_aimed_inside_the_quarter_second_not_at_its_edge() {
    let lands = bolt_lands_after(&lone());
    // Landing 0.1s before the end, inside the window: it goes.
    let inside = own_fear(lone(), lands + 0.1).decide(WARLOCK);
    assert_eq!(
        inside.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "{}",
        describe(&inside)
    );
    // Landing 0.2s before the end, inside the window but short of its middle:
    // the gate would let it go, but the Warlock waits for the middle.
    let edge = own_fear(lone(), lands + 0.2).decide(WARLOCK);
    assert_eq!(edge.chose(), None, "{}", describe(&edge));
    assert!(
        noted(&edge, AbilityType::Shadowbolt, FEAR_WINDOW_HOLD_NOTE),
        "held for the window's middle: {}",
        describe(&edge)
    );
}

#[test]
fn a_bolt_it_cannot_cast_holds_nothing_up() {
    // Inside the hold band. With the mana for a bolt the Warlock holds its
    // global cooldown for it; without, the bolt holds nothing up and the
    // global goes on a Life Tap.
    let lands = bolt_lands_after(&lone());
    let with_mana = |fraction: f32| {
        own_fear(lone(), lands + FEAR_WINDOW_LEAD + GCD / 2.0)
            .mana_of(WARLOCK, fraction)
            .decide(WARLOCK)
    };
    let affordable = with_mana(0.35);
    assert_eq!(
        affordable.chose(),
        None,
        "the control holds: {}",
        describe(&affordable)
    );
    let broke = with_mana(0.1);
    assert!(
        matches!(
            broke.rejected(AbilityType::Shadowbolt),
            None | Some(RejectionReason::InsufficientMana { .. })
        ),
        "{}",
        describe(&broke)
    );
    assert_eq!(
        broke.chose(),
        Some((AbilityType::LifeTap, None)),
        "short of a bolt's mana, it taps instead of holding: {}",
        describe(&broke)
    );
}

#[test]
fn the_timed_bolt_goes_ahead_of_the_rotation() {
    // Starved of mana, the rotation would Life Tap first and the bolt would
    // land a global cooldown after the Fear ends.
    let starved = 0.2;
    let free = feared_before(lone(), HEALER, 1)
        .mana_of(WARLOCK, starved)
        .decide(WARLOCK);
    assert_eq!(
        free.chose(),
        Some((AbilityType::LifeTap, None)),
        "the control: with no Fear up, a starved Warlock taps first: {}",
        describe(&free)
    );

    let lands = bolt_lands_after(&lone());
    let due = own_fear(lone(), lands + FEAR_WINDOW_AIM - 0.05)
        .mana_of(WARLOCK, starved)
        .decide(WARLOCK);
    assert_eq!(
        due.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "at the Fear's moment the bolt goes first: {}",
        describe(&due)
    );
}

#[test]
fn the_bolt_leads_a_feared_target_along_its_run() {
    // Timed for a Druid standing still, the bolt is due. A Druid seen running
    // AT the Warlock will be closer when the bolt flies, so it would land
    // early: the Warlock holds. One running away is further: it bolts.
    let lands = bolt_lands_after(&lone());
    let running = |x: f32| {
        let scene = Scene::new()
            .unit(WARLOCK, 1, Warlock)
            .at(0.0, 0.0)
            .targeting(HEALER)
            .unit(HEALER, 2, Druid)
            .at(20.0, 0.0)
            .facing(x, 0.0)
            .targeting(WARLOCK);
        let me = scene.entity(WARLOCK);
        let scene = scene
            .aura_full(HEALER, dot("Corruption", me))
            .aura_full(HEALER, dot("Unstable Affliction", me))
            .aura_full(HEALER, dot("Immolate", me));
        own_fear(scene, lands + FEAR_WINDOW_AIM - 0.05).decide(WARLOCK)
    };
    let away = running(1.0);
    assert_eq!(
        away.chose(),
        Some((AbilityType::Shadowbolt, Some(HEALER))),
        "a Druid running away is bolted: {}",
        describe(&away)
    );
    let toward = running(-1.0);
    assert_eq!(
        toward.chose(),
        None,
        "a Druid running at the Warlock is waited for: {}",
        describe(&toward)
    );
}

#[test]
fn only_its_own_fear_is_timed_a_teammates_is_waited_out() {
    // The same last moment, but the Fear is the Mage teammate's: the team
    // waits it out as the friendly-hold rule says, timing or not.
    let scene = lone().unit(ALLY, 1, Mage).at(-5.0, 0.0);
    let lands = bolt_lands_after(&scene);
    let theirs = feared_before(
        fear(scene, HEALER, ALLY, lands + FEAR_WINDOW_LEAD - 0.05),
        HEALER,
        1,
    )
    .decide(WARLOCK);
    assert_ne!(
        chose_ability(&theirs),
        Some(AbilityType::Shadowbolt),
        "{}",
        describe(&theirs)
    );
    assert!(
        matches!(
            theirs.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "a teammate's Fear is not timed: {}",
        describe(&theirs)
    );
}

// ---------------------------------------------------------------------------
// The free globals: what does not break the Fear.
// ---------------------------------------------------------------------------

#[test]
fn the_window_tops_mana_up_with_life_tap() {
    // Above the starved line, below a full pool less one tap.
    let mana = 0.35;
    let window = own_fear(lone(), 6.0).mana_of(WARLOCK, mana).decide(WARLOCK);
    assert_eq!(
        window.chose(),
        Some((AbilityType::LifeTap, None)),
        "a window global tops the pool up: {}",
        describe(&window)
    );

    // The control: the same mana with no Fear up is not starved, so no tap.
    let free = feared_before(lone(), HEALER, 1)
        .mana_of(WARLOCK, mana)
        .decide(WARLOCK);
    assert_ne!(
        chose_ability(&free),
        Some(AbilityType::LifeTap),
        "outside the window the tap waits for starvation: {}",
        describe(&free)
    );
}

#[test]
fn the_window_tap_never_overfills_the_pool() {
    // Near full, one tap would overfill: no tap, in the window or out.
    let full = own_fear(lone(), 6.0).mana_of(WARLOCK, 0.95).decide(WARLOCK);
    assert_ne!(
        chose_ability(&full),
        Some(AbilityType::LifeTap),
        "{}",
        describe(&full)
    );
    assert!(
        matches!(
            full.rejected(AbilityType::LifeTap),
            Some(RejectionReason::TapHeld { .. })
        ),
        "a tap that would overfill is held: {}",
        describe(&full)
    );
}

#[test]
fn a_curse_that_deals_no_damage_goes_on_the_feared_target_one_that_does_waits() {
    let with = |curse| {
        let mut scene = own_fear(lone(), 6.0);
        scene.combatant_mut(WARLOCK).warlock_curse_prefs = vec![curse];
        scene.decide(WARLOCK)
    };
    let tongues = with(WarlockCurse::Tongues);
    assert_eq!(
        tongues.chose(),
        Some((AbilityType::CurseOfTongues, Some(HEALER))),
        "Curse of Tongues breaks no Fear: {}",
        describe(&tongues)
    );

    // The control: Curse of Agony deals damage, so it waits for the end.
    let agony = with(WarlockCurse::Agony);
    assert_ne!(
        chose_ability(&agony),
        Some(AbilityType::CurseOfAgony),
        "{}",
        describe(&agony)
    );
    assert!(
        matches!(
            agony.rejected(AbilityType::CurseOfAgony),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "a damaging curse on the feared target is refused: {}",
        describe(&agony)
    );
}

#[test]
fn the_window_spends_corruption_on_another_enemy_not_the_feared_one() {
    // A second enemy without the Warlock's Corruption.
    let scene = lone().unit(OTHER, 2, Warrior).at(18.0, 8.0);
    let window = own_fear(scene, 6.0).decide(WARLOCK);
    assert_eq!(
        window.chose(),
        Some((AbilityType::Corruption, Some(OTHER))),
        "the window's global goes on the other enemy: {}",
        describe(&window)
    );

    // The control: the feared Druid itself lacks Corruption, and the
    // Warlock's damage on it is refused.
    let mut alone = own_fear(lone(), 6.0);
    alone.clear_auras(HEALER);
    let me = alone.entity(WARLOCK);
    let alone = fear(alone, HEALER, WARLOCK, 6.0)
        .aura_full(HEALER, dot("Unstable Affliction", me))
        .aura_full(HEALER, dot("Immolate", me))
        .decide(WARLOCK);
    assert_ne!(
        chose_ability(&alone),
        Some(AbilityType::Corruption),
        "{}",
        describe(&alone)
    );
    assert!(
        matches!(
            alone.rejected(AbilityType::Corruption),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "Corruption on the feared target is refused: {}",
        describe(&alone)
    );
}

// ---------------------------------------------------------------------------
// A curse that deals no damage is not a damage the team holds for: it goes on
// a teammate's Polymorph too (the friendly-hold rule is about damage).
// ---------------------------------------------------------------------------

#[test]
fn a_curse_that_deals_no_damage_goes_on_a_teammates_polymorph() {
    let with = |curse| {
        let scene = Scene::new()
            .unit(WARLOCK, 1, Warlock)
            .at(0.0, 0.0)
            .targeting(OTHER)
            .unit(ALLY, 1, Mage)
            .at(-5.0, 0.0)
            .unit(OTHER, 2, Warrior)
            .at(20.0, 0.0);
        let mage = scene.entity(ALLY);
        let mut scene = scene.aura_full(
            OTHER,
            Aura {
                effect_type: AuraType::Polymorph,
                duration: 6.0,
                break_on_damage_threshold: 0.0,
                caster: Some(mage),
                ability_name: "Polymorph".into(),
                ..Default::default()
            },
        );
        scene.combatant_mut(WARLOCK).warlock_curse_prefs = vec![curse];
        scene.decide(WARLOCK)
    };
    let weakness = with(WarlockCurse::Weakness);
    assert_eq!(
        weakness.chose(),
        Some((AbilityType::CurseOfWeakness, Some(OTHER))),
        "Curse of Weakness breaks no Polymorph: {}",
        describe(&weakness)
    );

    let agony = with(WarlockCurse::Agony);
    assert_ne!(
        chose_ability(&agony),
        Some(AbilityType::CurseOfAgony),
        "{}",
        describe(&agony)
    );
    assert!(
        matches!(
            agony.rejected(AbilityType::CurseOfAgony),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "Curse of Agony on a teammate's Polymorph is refused: {}",
        describe(&agony)
    );
}
