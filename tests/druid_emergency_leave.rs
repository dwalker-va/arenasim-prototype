//! AS-242 — a dying Druid leaves Travel Form for its emergency heal, and the
//! leave does not strobe.
//!
//! A shifted Druid is kept in the form by a melee or pet chaser inside the
//! danger radius; ranged damage killing it there was a third of its deaths
//! (AS-164 §4). Below `DRUID_EMERGENCY_HP` it now leaves as soon as the
//! emergency heal (Swiftmend, or the Rejuvenation that arms it) would go out
//! on its first frame out of the form — but only with no chaser within
//! `DRUID_EMERGENCY_LEAVE_YARDS`, so the chaser it ran from cannot close and
//! force the shift straight back (`class_ai::druid::should_leave_form`).
//!
//! SCENE tests (tier 1): one tick of the real class dispatch, each a pair —
//! the leave, and the same scene with one condition changed.
//! STAGED matches (tier 3): the strobe guard over played frames.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass::{Druid, Mage, Rogue, Warrior};
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::druid::{
    DRUID_EMERGENCY_HP, DRUID_EMERGENCY_LEAVE_YARDS, DRUID_MIN_FORM_SECS, DRUID_RESHIFT_HOLD_SECS,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraPending, AuraType, Shift};
use arenasim::states::play_match::constants::{GCD, MELEE_RANGE};
use arenasim::states::play_match::AbilityType;
use bevy::prelude::*;
use common::staged::{play, Played};

const DRUID: &str = "druid";
const ALLY: &str = "ally warrior";
const CHASER: &str = "enemy rogue";
const CASTER: &str = "enemy mage";

/// Seconds the scene's Druid has been shifted: under the dwell
/// (`DRUID_MIN_FORM_SECS`), so the ordinary leave cannot be what fires.
const FRESH_SHIFT: f32 = 0.5;

/// The Druid's health in the scenes that leave: below the emergency bar.
const DYING: f32 = 0.3;

/// A shifted Druid at `hp`, shifted [`FRESH_SHIFT`] ago, a healthy Warrior
/// teammate 10yd off, an enemy Rogue `chaser` yards off attacking the Druid,
/// and an enemy Mage 25yd off attacking it too.
fn scene(hp: f32, chaser: f32) -> Scene {
    let mut scene = Scene::new()
        .unit(DRUID, 1, Druid)
        .health(hp)
        .targeting(CHASER)
        .unit(ALLY, 1, Warrior)
        .at(-10.0, 0.0)
        .targeting(CHASER)
        .unit(CHASER, 2, Rogue)
        .at(chaser, 0.0)
        .targeting(DRUID)
        .unit(CASTER, 2, Mage)
        .at(0.0, 25.0)
        .targeting(DRUID);
    // Out of stealth: a Rogue starts stealthed, and the Druid cannot see one.
    scene.combatant_mut(CHASER).stealthed = false;
    let druid = scene.entity(DRUID);
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let mut form = AuraPending::from_ability(
        druid,
        druid,
        abilities.get_unchecked(&AbilityType::TravelForm),
    )
    .expect("Travel Form applies an aura")
    .aura;
    form.duration -= FRESH_SHIFT;
    scene.aura_full(DRUID, form)
}

/// The Druid's own Rejuvenation on itself.
fn own_rejuvenation(scene: Scene) -> Scene {
    let druid = scene.entity(DRUID);
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let rejuv = AuraPending::from_ability(
        druid,
        druid,
        abilities.get_unchecked(&AbilityType::Rejuvenation),
    )
    .expect("Rejuvenation applies an aura")
    .aura;
    scene.aura_full(DRUID, rejuv)
}

/// The same scene a frame later, once the leave has landed: the form gone.
fn out_of_form(mut scene: Scene) -> Scene {
    let kept: Vec<Aura> = scene
        .auras_of(DRUID)
        .iter()
        .filter(|a| a.effect_type != AuraType::TravelForm)
        .cloned()
        .collect();
    scene.clear_auras(DRUID);
    kept.into_iter().fold(scene, |s, a| s.aura_full(DRUID, a))
}

fn describe(view: &DecisionView) -> String {
    format!(
        "shifts {:?}; chose {:?}; candidates {:?}",
        view.shifts_queued,
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn leaves(scene: &Scene) -> bool {
    let view = scene.decide(DRUID);
    assert!(
        view.chosen.is_none(),
        "a shifted Druid casts nothing: {}",
        describe(&view)
    );
    view.shifts_queued == [Shift::Out]
}

/// Just past the clearance: outside it, inside the danger radius (12yd).
fn clear() -> f32 {
    DRUID_EMERGENCY_LEAVE_YARDS + 1.0
}

/// Dying, its emergency heal ready, the Rogue just outside the clearance but
/// inside the danger radius, on a fresh shift: it leaves, and its first
/// decision out of the form is the emergency heal on itself. Healthier, it
/// stays shifted.
#[test]
fn a_dying_druid_leaves_the_form_for_its_emergency_heal() {
    let dying = scene(DYING, clear());
    assert!(
        leaves(&dying),
        "dying, the heal ready, the chaser clear: it leaves — {}",
        describe(&dying.decide(DRUID))
    );
    let out = out_of_form(scene(DYING, clear())).decide(DRUID);
    assert_eq!(
        out.chosen,
        Some((AbilityType::Rejuvenation, Some(DRUID))),
        "the heal on the same beat: {}",
        describe(&out)
    );

    assert!(
        !leaves(&scene(DRUID_EMERGENCY_HP + 0.05, clear())),
        "not dying: a fresh shift is kept"
    );
    // The leave is for the Druid's own life: a dying teammate, with a heal
    // for it ready, does not pull a Druid that is not dying out of the form.
    assert!(
        !leaves(&scene(DRUID_EMERGENCY_HP + 0.05, clear()).health_of(ALLY, DYING)),
        "a teammate dying, the Druid not: a fresh shift is kept"
    );
}

/// The chaser inside the clearance keeps a dying Druid shifted, however
/// ready its heal: a leave there is answered by the shift straight back.
#[test]
fn a_chaser_inside_the_clearance_keeps_a_dying_druid_shifted() {
    assert!(leaves(&scene(DYING, clear())), "control: the leave");
    let inside = DRUID_EMERGENCY_LEAVE_YARDS - 1.0;
    assert!(
        inside > MELEE_RANGE + 5.0,
        "the scene sits between striking reach and the clearance"
    );
    assert!(
        !leaves(&scene(DYING, inside)),
        "a chaser inside the clearance: stay shifted"
    );
}

/// No heal on the way out, no leave: a global cooldown still running, no
/// mana for the Rejuvenation, or a Rejuvenation already rolling with
/// Swiftmend on cooldown (so the emergency step has nothing to cast). With
/// Swiftmend ready, that last Druid leaves, and Swiftmends itself.
#[test]
fn a_dying_druid_without_a_heal_to_cast_stays_shifted() {
    assert!(leaves(&scene(DYING, clear())), "control: the leave");

    let mut gcd = scene(DYING, clear());
    gcd.combatant_mut(DRUID).global_cooldown = GCD;
    assert!(!leaves(&gcd), "the global cooldown still running: stay");

    assert!(
        !leaves(&scene(DYING, clear()).mana_of(DRUID, 0.0)),
        "no mana for the heal: stay"
    );

    let rolling = || own_rejuvenation(scene(DYING, clear()));
    assert!(
        !leaves(&rolling().cooldown(DRUID, AbilityType::Swiftmend, 10.0)),
        "Rejuvenation rolling, Swiftmend on cooldown: nothing to cast, stay"
    );
    assert!(
        leaves(&rolling()),
        "Rejuvenation rolling, Swiftmend ready: out"
    );
    let out = out_of_form(rolling()).decide(DRUID);
    assert_eq!(
        out.chosen,
        Some((AbilityType::Swiftmend, Some(DRUID))),
        "...and it Swiftmends itself: {}",
        describe(&out)
    );
}

// ── the strobe guard, over played frames ───────────────────────────────────

const S_DRUID: Unit = Unit::new(1, 1);
const S_ALLY: Unit = Unit::new(1, 0);
const S_CHASER: Unit = Unit::new(2, 0);
const S_CASTER: Unit = Unit::new(2, 1);

const SEEDS: [u64; 3] = [1, 2, 3];

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Druid".into()],
        team2: vec!["Rogue".into(), "Mage".into()],
        map: "BasicArena".into(),
        team2_kill_target: Some(1),
        random_seed: Some(seed),
        max_duration_secs: 14.0,
        ..Default::default()
    }
}

/// The Druid shifted and dying at the origin, the enemy Rogue `chaser` yards
/// off on it, the enemy Mage 25yd off on it, the Warrior teammate 10yd
/// behind the Druid.
fn stage(chaser: f32) -> Stage {
    Stage::new()
        .place(S_DRUID, 0.0, 0.0)
        .clear_auras(S_DRUID)
        .health(S_DRUID, DYING)
        .land(S_DRUID, AbilityType::TravelForm, S_DRUID)
        .place(S_CHASER, chaser, 0.0)
        .clear_auras(S_CHASER)
        .combatant(S_CHASER, |c| c.stealthed = false)
        .target(S_CHASER, S_DRUID)
        .place(S_CASTER, 0.0, 25.0)
        .target(S_CASTER, S_DRUID)
        .place(S_ALLY, -10.0, 0.0)
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// One staged frame's Druid: alive, its health fraction, shifted, rooted, and
/// the nearest living melee or pet enemy's distance.
struct DruidFrame {
    t: f32,
    alive: bool,
    hp: f32,
    shifted: bool,
    rooted: bool,
    chaser: f32,
}

fn druid_frames(played: &Played) -> Vec<DruidFrame> {
    played
        .staged_frames()
        .map(|f: &FrameObservation| {
            let (_, d) = f
                .combatants
                .iter()
                .find(|(_, c)| c.team == 1 && c.slot == 1 && !c.is_pet)
                .expect("the Druid");
            let chaser = f
                .combatants
                .values()
                .filter(|c| c.team == 2 && c.alive && (c.is_pet || c.class.is_melee()))
                .map(|c| xz(c.position).distance(xz(d.position)))
                .fold(f32::INFINITY, f32::min);
            DruidFrame {
                t: f.sim_time,
                alive: d.alive,
                hp: d.current_health / d.max_health,
                shifted: d.aura_types.contains(&AuraType::TravelForm),
                rooted: d.aura_types.contains(&AuraType::Root),
                chaser,
            }
        })
        .collect()
}

/// The log's casts of `ability` by the Druid, on the `sim_time` clock.
fn druid_casts(played: &Played, ability: &str) -> Vec<f32> {
    let me = "Team 1 Druid #2 casts ";
    played
        .log
        .lines()
        .filter(|l| l.contains(me) && l.contains(&format!("{me}{ability}")))
        .filter_map(|l| {
            let open = l.find('[')?;
            let close = l.find("s]")?;
            l[open + 1..close].trim().parse().ok()
        })
        .collect()
}

/// The strobe guard. Every frame on which the Druid leaves the form
/// unrooted inside the dwell is an emergency leave: it was dying, and had no
/// chaser within the clearance on the frame it decided; its first one puts a
/// heal out on the same beat; and no unrooted leave is followed by a shift
/// back inside [`DRUID_RESHIFT_HOLD_SECS`]. Played with the Rogue just
/// outside the clearance (the Druid leaves at once), and just inside it (it
/// holds the form until its run has opened the gap past the clearance).
#[test]
fn an_emergency_leave_does_not_strobe() {
    let mut emergency_leaves = 0;
    for (chaser, leaves_at_once) in [(clear(), true), (DRUID_EMERGENCY_LEAVE_YARDS - 1.0, false)] {
        for seed in SEEDS {
            let played = play(config(seed), stage(chaser));
            let frames = druid_frames(&played);
            assert!(frames[0].shifted, "seed {seed}: the Druid starts shifted");
            let mut shifted_at = Some(frames[0].t);
            let mut left_at: Option<f32> = None;
            for pair in frames.windows(2) {
                let (before, now) = (&pair[0], &pair[1]);
                if !now.alive {
                    break;
                }
                if before.shifted && !now.shifted && !now.rooted {
                    let in_form = now.t - shifted_at.unwrap_or(f32::NEG_INFINITY);
                    if in_form < DRUID_MIN_FORM_SECS {
                        // An emergency leave: dying, and clear of the chaser
                        // on the frame it decided.
                        assert!(
                            before.hp < DRUID_EMERGENCY_HP,
                            "seed {seed}, chaser {chaser}: left after {in_form:.2}s at {:.0}% \
                             health",
                            before.hp * 100.0
                        );
                        assert!(
                            before.chaser > DRUID_EMERGENCY_LEAVE_YARDS,
                            "seed {seed}, chaser {chaser}: an emergency leave with a chaser \
                             {:.1}yd off",
                            before.chaser
                        );
                        emergency_leaves += 1;
                        if left_at.is_none() {
                            let heal = druid_casts(&played, "Rejuvenation")
                                .into_iter()
                                .chain(druid_casts(&played, "Swiftmend"))
                                .any(|c| c >= now.t - 0.02 && c <= now.t + 0.1);
                            assert!(heal, "seed {seed}: no heal on the same beat as the leave");
                        }
                    }
                    left_at = Some(now.t);
                    shifted_at = None;
                }
                if !before.shifted && now.shifted {
                    if let Some(left) = left_at {
                        assert!(
                            now.t - left >= DRUID_RESHIFT_HOLD_SECS - 0.5 / 60.0,
                            "seed {seed}, chaser {chaser}: left at {left:.2}s, shifted back at \
                             {:.2}s",
                            now.t
                        );
                    }
                    shifted_at = Some(now.t);
                }
            }
            // Both scenes end in an emergency leave: at once from outside the
            // clearance, and only once the run has opened the gap from inside.
            let first_out = frames
                .iter()
                .find(|f| !f.shifted && f.alive)
                .map(|f| f.t - frames[0].t);
            match (leaves_at_once, first_out) {
                (true, Some(after)) => assert!(after < 0.1, "seed {seed}: left after {after:.2}s"),
                (false, Some(after)) => assert!(
                    after > 0.1,
                    "seed {seed}: left at once with the chaser inside the clearance"
                ),
                (_, None) => panic!("seed {seed}, chaser {chaser}: the Druid never left"),
            }
        }
    }
    assert!(
        emergency_leaves >= 2 * SEEDS.len(),
        "only {emergency_leaves} emergency leaves — the scene did not build"
    );
}
