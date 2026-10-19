//! AS-362 — a `TeamPlan` healer keeps out of the reach of the enemy crowd
//! control ready to land on it (`class_ai::cc_reach`).
//!
//! STAGED matches (tier 3, `headless::stage`), each paired with a control that
//! removes its one condition — the crowd control being READY: the same stage
//! with it on a cooldown well past the readiness horizon — so neither half can
//! pass vacuously:
//!
//! - an enemy Priest within Psychic Scream reach of a PRESSURED Priest on an
//!   obstacle-free map — the team solve's `C_CC_REACH`;
//! - an enemy Warlock closing on a Priest with Fear ready — the closing
//!   caster holds it PRESSURED, so the solve again;
//! - an enemy Mage on our Warrior with Polymorph ready, nothing pressuring the
//!   healer — the FREE formation point's reach repair, for the Priest's
//!   machine and the caster-healer machine the Druid and Shaman share;
//! - a Warlock with Fear ready against a PRESSURED Paladin, which the
//!   band-hold would otherwise park in reach;
//! - an enemy Rogue in melee with Kidney Shot ready — the solve, against a
//!   chaser's stun.
//!
//! Every PRESSURED stage holds the healer near the edge of heal range of its
//! Warrior, so the solve's field is not flat and its anti-statue key stays out
//! of it: what moves the healer here is the crowd control and nothing else.
//! The healer's reach is asserted not to be bought with its heal range.
//!
//! Then two anti-strobe pins: AS-249's two caster standoffs jittered with the
//! teammate fighting at the caster's feet, so that stage is scored for
//! heading reversals against its control; and a `TeamPlan` healer parked at
//! its formation point stays parked (its walk no longer lapses into target
//! pursuit), against `Legacy`, where it does.
//!
//! `Legacy` runs none of it; `the_legacy_healer_does_not_move_for_crowd_control`
//! pins that each stage plays the same under `Legacy` whether the crowd control
//! is ready or not.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::components::{HealerPosture, Posture};
use arenasim::states::play_match::{AbilityType, AuraType};
use bevy::prelude::*;
use common::staged::{play, Played};

const WARRIOR: Unit = Unit::new(1, 0);
const HEALER: Unit = Unit::new(1, 1);
const ENEMY: Unit = Unit::new(2, 0);
const ENEMY_WARRIOR: Unit = Unit::new(2, 1);

/// Our Warrior's spot: 39yd from the healer, so a step away from it leaves
/// heal range (40yd).
const WARRIOR_AT: (f32, f32) = (-24.0, 0.0);
const HEALER_AT: (f32, f32) = (15.0, 0.0);

/// The CC under test is ready, or on a cooldown well beyond the horizon.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Arm {
    Ready,
    Control,
}

/// Holds `who` where the stage places it: no movement speed. Not a root — a
/// rooted threat opens the healer's ESCAPE window, which is not under test.
fn planted(stage: Stage, who: Unit) -> Stage {
    stage.combatant(who, |c| c.base_movement_speed = 0.0)
}

fn config(case: &Case, seed: u64, profile: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), case.healer.into()],
        team2: vec![case.enemy.into(), "Warrior".into()],
        map: "BasicArena".into(),
        ai_profile: Some(profile.into()),
        // Our Warrior is after the enemy Warrior; the enemy after our Priest
        // (1) or our Warrior (0).
        team1_kill_target: Some(1),
        team2_kill_target: Some(case.after),
        random_seed: Some(seed),
        max_duration_secs: 8.0,
        ..Default::default()
    }
}

/// The common lineup: our Warrior planted at `WARRIOR_AT` with the enemy
/// Warrior planted just beyond it, our Priest at `HEALER_AT` with its own Mind
/// Blast, Mana Burn and Psychic Scream spent (so a cast neither plants it nor
/// takes the enemy out of the picture). The enemy under test is placed by each
/// stage.
fn lineup() -> Stage {
    let stage = Stage::new()
        .place(WARRIOR, WARRIOR_AT.0, WARRIOR_AT.1)
        .place(ENEMY_WARRIOR, WARRIOR_AT.0 - 2.0, WARRIOR_AT.1)
        .target(WARRIOR, ENEMY_WARRIOR)
        .place(HEALER, HEALER_AT.0, HEALER_AT.1)
        .cooldown(HEALER, AbilityType::MindBlast, 60.0)
        .cooldown(HEALER, AbilityType::ManaBurn, 60.0)
        .cooldown(HEALER, AbilityType::PsychicScream, 60.0);
    planted(planted(stage, WARRIOR), ENEMY_WARRIOR)
}

/// The CC's cooldown for `arm`: one that comes off INSIDE the readiness
/// horizon (1.5s), so the enemy cannot use it on the stage's first frame but
/// it counts as ready; or one far beyond it.
fn cooldown(arm: Arm) -> f32 {
    match arm {
        Arm::Ready => 1.2,
        Arm::Control => 60.0,
    }
}

fn unit_in<'a>(
    frame: &'a FrameObservation,
    who: Unit,
) -> &'a arenasim::headless::ObservedCombatant {
    frame
        .combatants
        .values()
        .find(|c| c.team == who.team && c.slot == who.slot && !c.is_pet)
        .unwrap_or_else(|| panic!("no {who:?}"))
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// What a staged window measured.
#[derive(Debug)]
struct Window {
    frames: usize,
    /// Frames the healer stood within `reach` of the enemy.
    in_reach: usize,
    /// Frames the healer stood beyond heal range of its Warrior.
    out_of_heal_range: usize,
    /// Frames the healer was held in hard crowd control.
    held: usize,
}

impl Window {
    fn in_reach_share(&self) -> f32 {
        self.in_reach as f32 / self.frames.max(1) as f32
    }
}

/// The staged frames from `from` to `to` seconds after the stage landed.
fn window(played: &Played, from: f32, to: f32, reach: f32) -> Window {
    let landed = played.landed().app;
    let mut w = Window {
        frames: 0,
        in_reach: 0,
        out_of_heal_range: 0,
        held: 0,
    };
    for frame in played.staged_frames() {
        let t = frame.sim_time - landed;
        if t < from || t > to {
            continue;
        }
        let healer = unit_in(frame, HEALER);
        let enemy = unit_in(frame, ENEMY);
        let warrior = unit_in(frame, WARRIOR);
        w.frames += 1;
        if xz(healer.position).distance(xz(enemy.position)) <= reach {
            w.in_reach += 1;
        }
        if xz(healer.position).distance(xz(warrior.position)) > 40.0 {
            w.out_of_heal_range += 1;
        }
        if healer.aura_types.iter().any(|a| {
            matches!(
                a,
                AuraType::Stun
                    | AuraType::Fear
                    | AuraType::Polymorph
                    | AuraType::Incapacitate
                    | AuraType::Cyclone
            )
        }) {
            w.held += 1;
        }
    }
    w
}

/// An enemy Priest planted 7yd from ours — inside the Scream's 8yd reach
/// — and the enemy Warrior planted 11yd off on our
/// Priest, which holds it in PRESSURED. Scream ready, or on cooldown.
fn scream_stage(arm: Arm) -> Stage {
    let stage = lineup()
        .place(ENEMY, HEALER_AT.0 + 7.0, HEALER_AT.1)
        .target(ENEMY, HEALER)
        .cooldown(ENEMY, AbilityType::PsychicScream, cooldown(arm))
        .place(ENEMY_WARRIOR, HEALER_AT.0, HEALER_AT.1 + 11.0)
        .target(ENEMY_WARRIOR, HEALER);
    planted(stage, ENEMY)
}

/// Our FREE Priest at its formation point behind the Warrior, and an enemy
/// Warlock 36yd off and closing on it, Death Coil spent; Fear ready, or on
/// cooldown.
fn fear_stage(arm: Arm) -> Stage {
    lineup()
        .place(HEALER, -16.0, 0.0)
        .place(ENEMY, 20.0, 6.0)
        .target(ENEMY, HEALER)
        .cooldown(ENEMY, AbilityType::DeathCoil, 60.0)
        .cooldown(ENEMY, AbilityType::Fear, cooldown(arm))
}

/// Our FREE Priest at its formation point behind the Warrior, and an enemy
/// Mage planted 24yd off it, on our Warrior; Polymorph ready, or on cooldown.
/// Nothing pressures the Priest, so this is the formation point's reach
/// repair.
fn polymorph_stage(arm: Arm) -> Stage {
    let stage = lineup()
        .place(HEALER, -16.0, 0.0)
        .place(ENEMY, 8.0, 0.0)
        .target(ENEMY, WARRIOR)
        .cooldown(ENEMY, AbilityType::Polymorph, cooldown(arm));
    planted(stage, ENEMY)
}

/// Our Paladin held PRESSURED with nothing within its 15yd retreat band —
/// so its band-hold would park it where it stands — and an enemy Warlock
/// planted 24yd off on our Warrior, Death Coil spent; Fear ready, or on
/// cooldown. The Paladin's FREE is its melee pursuit, which the rule leaves
/// alone: it reaches the Paladin only in PRESSURED.
fn paladin_stage(arm: Arm) -> Stage {
    let stage = lineup()
        .place(HEALER, -16.0, 0.0)
        .healer_posture(
            HEALER,
            HealerPosture {
                posture: Posture::Pressured,
                hold_until: 60.0,
                ..HealerPosture::new(0.0)
            },
        )
        .place(ENEMY, 8.0, 0.0)
        .target(ENEMY, WARRIOR)
        .cooldown(ENEMY, AbilityType::DeathCoil, 60.0)
        .cooldown(ENEMY, AbilityType::Fear, cooldown(arm));
    planted(stage, ENEMY)
}

/// An enemy Rogue planted 2yd from our Priest — in melee, which holds it in
/// PRESSURED; Kidney Shot ready, or on cooldown.
fn kidney_stage(arm: Arm) -> Stage {
    let stage = lineup()
        .place(ENEMY, HEALER_AT.0 + 2.0, HEALER_AT.1)
        .target(ENEMY, HEALER)
        .cooldown(ENEMY, AbilityType::KidneyShot, cooldown(arm))
        // Out of stealth: in the healer's sight, and with no Cheap Shot.
        .combatant(ENEMY, |c| c.stealthed = false);
    planted(stage, ENEMY)
}

struct Case {
    name: &'static str,
    /// Our healer.
    healer: &'static str,
    enemy: &'static str,
    /// Whom the enemy is after: our Priest (1) or our Warrior (0).
    after: usize,
    stage: fn(Arm) -> Stage,
    /// The CC's reach, from the config (the disc without the margin).
    reach: f32,
    /// The window it is measured over, in seconds after the stage lands.
    from: f32,
    to: f32,
}

const CASES: [Case; 6] = [
    Case {
        name: "Psychic Scream from an enemy Priest",
        healer: "Priest",
        enemy: "Priest",
        after: 1,
        stage: scream_stage,
        reach: 8.0,
        from: 1.0,
        to: 3.0,
    },
    Case {
        name: "Fear from a closing Warlock",
        healer: "Priest",
        enemy: "Warlock",
        after: 1,
        stage: fear_stage,
        reach: 30.0,
        from: 1.0,
        to: 4.0,
    },
    Case {
        name: "Polymorph from a Mage on the Warrior",
        healer: "Priest",
        enemy: "Mage",
        after: 0,
        stage: polymorph_stage,
        reach: 30.0,
        from: 1.0,
        to: 3.0,
    },
    Case {
        name: "Polymorph from a Mage on the Warrior, against a Druid",
        healer: "Druid",
        enemy: "Mage",
        after: 0,
        stage: polymorph_stage,
        reach: 30.0,
        from: 1.0,
        to: 3.0,
    },
    Case {
        name: "Fear from a Warlock, against a PRESSURED Paladin",
        healer: "Paladin",
        enemy: "Warlock",
        after: 0,
        stage: paladin_stage,
        reach: 30.0,
        from: 1.0,
        to: 4.0,
    },
    Case {
        name: "Kidney Shot from a Rogue in melee",
        healer: "Priest",
        enemy: "Rogue",
        after: 1,
        stage: kidney_stage,
        reach: 2.5,
        from: 0.8,
        to: 3.0,
    },
];

const SEEDS: [u64; 2] = [1, 2];

fn measure(case: &Case, profile: &str, arm: Arm, seed: u64) -> Window {
    let played = play(config(case, seed, profile), (case.stage)(arm));
    window(&played, case.from, case.to, case.reach)
}

/// The rule, and its control. With the CC ready the `TeamPlan` healer stands
/// out of its reach for (nearly) the whole window, without leaving heal range
/// of its Warrior; with it on cooldown the same healer on the same stage stays
/// in reach — so it is the readiness, read from the enemy's cooldown, that
/// moves it.
#[test]
fn a_teamplan_healer_keeps_out_of_ready_crowd_control() {
    for case in &CASES {
        for seed in SEEDS {
            let ready = measure(case, "TeamPlan", Arm::Ready, seed);
            let control = measure(case, "TeamPlan", Arm::Control, seed);
            println!(
                "{} seed {seed}: in reach ready {:.0}% / control {:.0}% ({} frames); \
                 out of heal range {} / {}; held {} / {}",
                case.name,
                100.0 * ready.in_reach_share(),
                100.0 * control.in_reach_share(),
                ready.frames,
                ready.out_of_heal_range,
                control.out_of_heal_range,
                ready.held,
                control.held,
            );
            assert!(ready.frames > 60, "{}: the window went vacuous", case.name);
            assert!(
                ready.in_reach_share() <= 0.1,
                "{} seed {seed}: the healer stood in reach {:.0}% of the window with the \
                 crowd control ready",
                case.name,
                100.0 * ready.in_reach_share()
            );
            assert!(
                control.in_reach_share() >= 0.9,
                "{} seed {seed}: the control healer stood in reach only {:.0}% of the \
                 window — the stage does not hold it there, so the pair proves nothing",
                case.name,
                100.0 * control.in_reach_share()
            );
            assert_eq!(
                ready.out_of_heal_range, 0,
                "{} seed {seed}: the healer left its Warrior's heal range to keep out of reach",
                case.name
            );
        }
    }
}

/// `Legacy` is untouched: the same stage plays the healer's position frame for
/// frame the same whether the crowd control is ready or not.
#[test]
fn the_legacy_healer_does_not_move_for_crowd_control() {
    for case in &CASES {
        let ready = play(config(case, 1, "Legacy"), (case.stage)(Arm::Ready));
        let control = play(config(case, 1, "Legacy"), (case.stage)(Arm::Control));
        let track = |p: &Played| -> Vec<Vec2> {
            let landed = p.landed().app;
            p.staged_frames()
                .filter(|f| f.sim_time - landed <= case.from)
                .map(|f| xz(unit_in(f, HEALER).position))
                .collect()
        };
        let (a, b) = (track(&ready), track(&control));
        assert!(a.len() > 30, "{}: vacuous", case.name);
        assert_eq!(
            a, b,
            "{}: the Legacy healer moved differently with the crowd control ready",
            case.name
        );
    }
}

/// A `TeamPlan` healer parked at its formation point keeps its `Point`
/// directive alive (`formation::continuous`), so it stays parked: the control
/// arm of the FREE stage parks the Priest and the Druid at their formation
/// points, and over the next five seconds — its own casts included — the
/// `TeamPlan` healer does not move. Under `Legacy` the same stage lets the
/// walk lapse whenever the healer stands at its point, target pursuit takes a
/// step, and the walk brings it back: the pair that shows the rule is what
/// holds it.
#[test]
fn a_parked_teamplan_healer_stays_parked() {
    for case in CASES
        .iter()
        .filter(|c| c.stage == polymorph_stage as fn(Arm) -> Stage)
    {
        let walked = |profile: &str| {
            let stage = polymorph_stage(Arm::Control)
                .place(ENEMY, 22.0, 0.0)
                .target(HEALER, ENEMY);
            let played = play(
                HeadlessMatchConfig {
                    team1_kill_target: Some(0),
                    ..config(case, 1, profile)
                },
                stage,
            );
            let landed = played.landed().app;
            let track: Vec<Vec2> = played
                .staged_frames()
                .filter(|f| (2.0..=7.0).contains(&(f.sim_time - landed)))
                .map(|f| xz(unit_in(f, HEALER).position))
                .collect();
            let path: f32 = track.windows(2).map(|w| w[0].distance(w[1])).sum();
            (path, track.len())
        };
        let (teamplan, n) = walked("TeamPlan");
        let (legacy, _) = walked("Legacy");
        println!(
            "{}: walked while parked TeamPlan {teamplan:.2}yd / Legacy {legacy:.2}yd over {n} frames",
            case.name
        );
        assert!(n > 200, "{}: vacuous", case.name);
        assert!(
            teamplan < 0.1,
            "{}: the parked TeamPlan healer walked {teamplan}yd",
            case.name
        );
        assert!(
            legacy > 1.0,
            "{}: the Legacy control never stepped off its point",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// Not strobing: the caster with the teammate at its feet
// ---------------------------------------------------------------------------

/// AS-249's reversal metric (`tests/healer_positioning.rs`): the path read
/// every half second; a step under half a yard is standing; two consecutive
/// headings within ~45 degrees of straight back are a reversal.
fn reversals(played: &Played, from: f32, to: f32) -> (u32, f32) {
    let landed = played.landed().app;
    let mut sampled: Vec<Vec2> = Vec::new();
    let mut last = f32::NEG_INFINITY;
    let mut walked = 0.0;
    let mut prev: Option<Vec2> = None;
    for f in played.staged_frames() {
        let t = f.sim_time - landed;
        if t < from || t > to {
            continue;
        }
        let p = xz(unit_in(f, HEALER).position);
        if let Some(q) = prev {
            walked += p.distance(q);
        }
        prev = Some(p);
        if t - last >= 0.5 - 1e-4 {
            sampled.push(p);
            last = t;
        }
    }
    let mut count = 0;
    let mut heading: Option<Vec2> = None;
    for pair in sampled.windows(2) {
        let step = pair[1] - pair[0];
        if step.length() < 0.5 {
            continue;
        }
        let dir = step.normalize();
        if heading.is_some_and(|h| h.dot(dir) < -0.7) {
            count += 1;
        }
        heading = Some(dir);
    }
    (count, walked)
}

/// The shape AS-249's two caster standoffs jittered on: a Warlock closing on
/// our Priest while our Warrior fights at the Warlock's feet, so every step
/// the caster takes drags the teammate — and the heal circle — with it. The
/// enemy Warrior is stunned out of the picture. Fear ready, or on cooldown.
const STROBE: Case = Case {
    name: "Fear from a Warlock with our Warrior at its feet",
    healer: "Priest",
    enemy: "Warlock",
    after: 1,
    stage: strobe_stage,
    reach: 30.0,
    from: 0.5,
    to: 8.0,
};

fn strobe_stage(arm: Arm) -> Stage {
    Stage::new()
        .place(ENEMY, 10.0, 0.0)
        .place(WARRIOR, 8.0, 0.0)
        .target(WARRIOR, ENEMY)
        .target(ENEMY, HEALER)
        .place(HEALER, -22.0, 0.0)
        .place(ENEMY_WARRIOR, 0.0, -32.0)
        .aura(
            ENEMY_WARRIOR,
            arenasim::states::play_match::Aura {
                effect_type: AuraType::Stun,
                duration: 30.0,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
        .cooldown(ENEMY, AbilityType::DeathCoil, 60.0)
        .cooldown(ENEMY, AbilityType::Fear, cooldown(arm))
}

/// The reach rule does not make the healer bob: over ten seconds of the
/// caster-at-the-teammate's-feet stage, the healer kept out of Fear reach
/// turns back on itself no more often than the control healer, which ignores
/// the reach — and it does walk (the rule is live, not parked).
#[test]
fn keeping_out_of_reach_does_not_bob_when_the_teammate_fights_at_the_casters_feet() {
    for seed in SEEDS {
        let ready = play(config(&STROBE, seed, "TeamPlan"), strobe_stage(Arm::Ready));
        let control = play(
            config(&STROBE, seed, "TeamPlan"),
            strobe_stage(Arm::Control),
        );
        let (r, walked) = reversals(&ready, 0.0, 8.0);
        let (c, _) = reversals(&control, 0.0, 8.0);
        let reach = window(&ready, 0.5, 8.0, 30.0);
        let reach_control = window(&control, 0.5, 8.0, 30.0);
        println!(
            "seed {seed}: reversals ready {r} / control {c}; walked {walked:.1}yd; in Fear \
             reach ready {:.0}% / control {:.0}%; out of heal range {} / {}",
            100.0 * reach.in_reach_share(),
            100.0 * reach_control.in_reach_share(),
            reach.out_of_heal_range,
            reach_control.out_of_heal_range,
        );
        assert!(
            walked > 5.0,
            "seed {seed}: the healer never walked — vacuous"
        );
        assert!(
            r <= c + 1,
            "seed {seed}: {r} reversals with the reach rule live against {c} without it"
        );
    }
}

/// Diagnostic, not a gate: the healer's track, goal and posture events on one
/// stage, both arms (`CASE=<index into CASES>`, anything past them for the
/// strobe stage).
#[test]
#[ignore]
fn print_staged_tracks() {
    let which: usize = std::env::var("CASE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let case = CASES.get(which).unwrap_or(&STROBE);
    for arm in [Arm::Ready, Arm::Control] {
        let played = play(config(case, 1, "TeamPlan"), (case.stage)(arm));
        let landed = played.landed().app;
        println!("== {} {arm:?}", case.name);
        for (i, f) in played.staged_frames().enumerate() {
            if i % 15 != 0 {
                continue;
            }
            let h = unit_in(f, HEALER);
            let e = unit_in(f, ENEMY);
            let w = unit_in(f, WARRIOR);
            println!(
                "t={:.2} healer=({:.1},{:.1}) goal={:?} enemy=({:.1},{:.1}) d={:.1} warrior_d={:.1} auras={:?}",
                f.sim_time - landed,
                h.position.x,
                h.position.z,
                h.point_goal.map(|g| (g.x as i32, g.z as i32)),
                e.position.x,
                e.position.z,
                xz(h.position).distance(xz(e.position)),
                xz(h.position).distance(xz(w.position)),
                h.aura_types
            );
        }
        for e in played
            .staged_trace()
            .filter(|e| e["actor"]["team"] == 1 && e["actor"]["class"] == "Priest")
        {
            if e["kind"] == "movement_decision" {
                println!(
                    "  mv t={} {} {} prev={}",
                    e["sim_time"], e["posture"], e["trigger"], e["previous_posture"]
                );
            } else if e["outcome"]["ability"].is_string() {
                println!("  ab t={} {}", e["sim_time"], e["outcome"]["ability"]);
            }
        }
    }
}
