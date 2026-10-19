//! AS-370 — the FREE formation point turns round its centroid instead of
//! flipping across it.
//!
//! The formation point stands `formation_offset` behind the engaged-ally
//! centroid, away from the enemy nearest it. When that enemy crosses the
//! centroid — a melee at its teammate's feet, or a teammate walking past an
//! enemy — "away" turns half way round in one frame, and the point used to jump
//! to the far side with it: 5-16yd in a frame, under both AI profiles. Under
//! `TeamPlan` the heading now turns from the last one no faster than the healer
//! can walk round the centroid (`class_ai::formation::turn_limited`); `Legacy`
//! is unchanged (`formation::continuous`).
//!
//! - `a_teammate_walking_past_an_enemy_swings_the_point_round` stages the
//!   crossing (tier 3) on BasicArena, where no sight repair touches the point.
//!   Under `TeamPlan` the healer's walk never steps more than a re-commit and
//!   swings round; its pair is `Legacy`, the one condition removed, whose walk
//!   jumps to the far side at the crossing.
//! - `scenes::an_enemy_crossing_the_centroid_turns_the_point` is the defect in
//!   one tick for each formation machine, paired with the same ticks with the
//!   formation memory forgotten between them, and with `Legacy`.
//! - `scenes::a_stale_walk_is_reissued_at_the_formation_point` holds the free
//!   tick to re-issuing a walk that aims more than the shift threshold off the
//!   formation point, even while the healer stands at that point, paired with a
//!   walk inside the threshold that is left alone, and with `Legacy`.

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{AbilityType, MovementConfig};
use bevy::prelude::{Vec2, Vec3};

mod common;
use common::staged::{entity_id, play, sim_time, Played};

const FRAME: f32 = 1.0 / 60.0;

const WARRIOR: Unit = Unit::new(1, 0);
const HEALER: Unit = Unit::new(1, 1);
const MAGE: Unit = Unit::new(2, 0);
const FAR: Unit = Unit::new(2, 1);

/// Warrior + Druid vs Mage + Priest on BasicArena. Both enemies are
/// Hammer-of-Justice'd: the Mage at (-12, -8), on the Warrior's line, the
/// enemy Priest at (34, -8), the Warrior's kill target. The Warrior walks east
/// at it, straight past the Mage, so the enemy nearest the centroid (the
/// Warrior, the healer's one teammate) crosses it about a second in. The
/// healer stands at its formation point west of the Warrior with no mana, so
/// nothing it casts holds it still. The Druid, because it has no wand: a
/// Priest's formation point is clamped into wand range of its kill target, and
/// with that target 40yd off the clamp, not the heading, places the point. The
/// Priest's machine turns its heading through the same function
/// (`turn_limited`), held by the scene pair below.
fn crossing(seed: u64, profile: &str) -> (HeadlessMatchConfig, Stage) {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Druid".into()],
        team2: vec!["Mage".into(), "Priest".into()],
        map: "BasicArena".to_string(),
        ai_profile: Some(profile.to_string()),
        team1_kill_target: Some(1),
        team2_kill_target: Some(0),
        random_seed: Some(seed),
        max_duration_secs: 6.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(WARRIOR, -17.0, -8.0)
        .target(WARRIOR, FAR)
        .place(HEALER, -25.0, -6.0)
        .mana(HEALER, 0.0)
        .place(MAGE, -12.0, -8.0)
        .target(MAGE, WARRIOR)
        .land(MAGE, AbilityType::HammerOfJustice, HEALER)
        .place(FAR, 34.0, -8.0)
        .target(FAR, WARRIOR)
        .land(FAR, AbilityType::HammerOfJustice, HEALER);
    (config, stage)
}

/// The window the scene is read over: the enemies stay stunned and the
/// Warrior has not reached Charge range of its target.
const WINDOW: f32 = 4.5;

fn flat(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Per staged frame up to `WINDOW`: the healer's point goal and position, the
/// formation
/// point the unconstrained machine computes from the frame (the Warrior's
/// position offset `formation_offset` away from the enemy nearest it, blended
/// toward the arena centre by `center_bias`), and that enemy's bearing from
/// the Warrior.
struct Track {
    goals: Vec<Option<Vec3>>,
    healer: Vec<Vec3>,
    unconstrained: Vec<Vec3>,
    bearings: Vec<Vec2>,
    warrior: Vec<Vec3>,
}

fn track(played: &Played) -> Track {
    let shared = MovementConfig::default().shared;
    let get = |f: &arenasim::headless::FrameObservation, team: u8, slot: u8| {
        f.combatants
            .values()
            .find(|c| c.team == team && c.slot == slot && !c.is_pet)
            .cloned()
            .expect("the unit")
    };
    let mut t = Track {
        goals: Vec::new(),
        healer: Vec::new(),
        unconstrained: Vec::new(),
        bearings: Vec::new(),
        warrior: Vec::new(),
    };
    for (k, f) in played.staged_frames().enumerate() {
        if k as f32 * FRAME >= WINDOW {
            break;
        }
        let warrior = get(f, 1, 0).position;
        let healer = get(f, 1, 1);
        let nearest = [get(f, 2, 0), get(f, 2, 1)]
            .into_iter()
            .min_by(|a, b| flat(a.position, warrior).total_cmp(&flat(b.position, warrior)))
            .unwrap()
            .position;
        let away = Vec2::new(warrior.x - nearest.x, warrior.z - nearest.z).normalize_or_zero();
        let to_center = Vec2::new(-warrior.x, -warrior.z).normalize_or_zero();
        let dir = (away * (1.0 - shared.center_bias) + to_center * shared.center_bias)
            .normalize_or_zero();
        t.goals.push(healer.point_goal);
        t.healer.push(healer.position);
        t.unconstrained.push(Vec3::new(
            warrior.x + dir.x * shared.formation_offset,
            0.0,
            warrior.z + dir.y * shared.formation_offset,
        ));
        t.bearings.push(-away);
        t.warrior.push(warrior);
    }
    t
}

fn largest_step(points: impl Iterator<Item = Vec3>) -> f32 {
    let points: Vec<Vec3> = points.collect();
    points
        .windows(2)
        .map(|w| flat(w[0], w[1]))
        .fold(0.0, f32::max)
}

/// The probe. Under both profiles the enemy nearest the healer's teammate
/// crosses it (its bearing from the Warrior turns past 150 degrees) while the
/// healer is FREE on its formation point the whole window, and the
/// unconstrained point, read off the frames, jumps over 10yd at the crossing.
/// Under `TeamPlan` the healer's walk never steps more than a re-commit
/// (`formation_shift_threshold` plus a frame's turn), and its heading round
/// the Warrior still turns past 90 degrees — swung round, not held on the near
/// side. The pair, `Legacy`, walks the unconstrained point: its goal jumps over
/// 10yd.
#[test]
fn a_teammate_walking_past_an_enemy_swings_the_point_round() {
    let movement = MovementConfig::default();
    let recommit = movement.druid.formation_shift_threshold + 1.0;
    for profile in ["Legacy", "TeamPlan"] {
        for seed in [1, 2, 3] {
            let label = format!("{profile} seed {seed}");
            let (config, stage) = crossing(seed, profile);
            let played = play(config, stage);
            let t = track(&played);

            // The situation: the enemy nearest the Warrior crosses it.
            let first = t.bearings[0];
            let crossed = t.bearings.iter().any(|b| b.dot(first) < -0.85);
            assert!(crossed, "{label}: the Mage never crossed the Warrior");

            // ... while the healer is FREE throughout.
            let healer = entity_id(&played, 1, 1);
            let landed = played.landed().combat;
            let not_free: Vec<_> = played
                .staged_trace()
                .filter(|e| {
                    e["kind"] == "movement_decision"
                        && e["actor"]["entity_id"].as_u64() == Some(healer)
                        && sim_time(e) - landed < WINDOW
                        && e["posture"] != "free"
                })
                .map(|e| (sim_time(e) - landed, e["posture"].clone()))
                .collect();
            assert!(
                not_free.is_empty(),
                "{label}: the healer left FREE: {not_free:?}"
            );

            // The pair: the unconstrained point jumps at the crossing.
            let raw_jump = largest_step(t.unconstrained.iter().copied());
            assert!(
                raw_jump > 10.0,
                "{label}: the unconstrained point stepped only {raw_jump:.1}yd — the stage \
                 no longer flips it"
            );

            // The healer's walk swings round instead. It has no walk while it
            // stands at its point (the stage puts it there) and keeps up with
            // it, so on a frame with no walk the healer's own position stands
            // in for the goal: its first walk is a step from where it stood.
            let goals: Vec<(usize, Vec3)> = t
                .goals
                .iter()
                .enumerate()
                .filter_map(|(k, g)| g.map(|g| (k, g)))
                .collect();
            assert!(
                goals.len() as f32 * FRAME > WINDOW / 2.0,
                "{label}: the healer had a point goal on only {} frames",
                goals.len()
            );
            let step = largest_step(t.goals.iter().zip(&t.healer).map(|(g, h)| g.unwrap_or(*h)));
            if profile == "Legacy" {
                assert!(
                    step > 10.0,
                    "{label}: the pair's formation goal stepped only {step:.1}yd — the \
                     unconstrained machine no longer jumps here"
                );
                continue;
            }
            assert!(
                step <= recommit,
                "{label}: the formation goal stepped {step:.1}yd in a frame"
            );
            let bearing = |k: usize, p: Vec3| {
                Vec2::new(p.x - t.warrior[k].x, p.z - t.warrior[k].z).normalize_or_zero()
            };
            // Before the crossing the point is the unconstrained one.
            let start = bearing(0, t.unconstrained[0]);
            let (k, last) = *goals.last().unwrap();
            let end = bearing(k, last);
            assert!(
                start.dot(end) < 0.0,
                "{label}: the heading turned only {:.0} degrees — the point stayed on the \
                 Warrior's near side",
                start.angle_to(end).abs().to_degrees()
            );
        }
    }
}

/// One-tick scenes of each formation machine (the Priest's and the
/// caster-healer one the Shaman and Druid share).
mod scenes {
    use std::collections::BTreeMap;

    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::ai_profile::AiProfile;
    use arenasim::states::play_match::class_ai::caster_healer_posture::evaluate_caster_healer_posture;
    use arenasim::states::play_match::class_ai::priest::evaluate_priest_posture;
    use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
    use arenasim::states::play_match::decision_trace::DecisionTrace;
    use arenasim::states::play_match::situation::{MatchClock, SituationConfig};
    use arenasim::states::play_match::{
        AbilityDefinitions, AbilityType, Combatant, DRTracker, HealerPosture, MovementConfig,
        MovementDirective, MovementGoal, Posture,
    };
    use bevy::ecs::world::CommandQueue;
    use bevy::prelude::*;

    static NO_DR: BTreeMap<Entity, DRTracker> = BTreeMap::new();
    static NO_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

    fn info(entity: Entity, team: u8, class: CharacterClass, x: f32, z: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::new(x, 1.0, z),
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
        }
    }

    /// One FREE posture tick at `now` under `profile` for a `class` healer
    /// standing at `pos` with `state` and the live walk `directive`: a Mage
    /// teammate at (10, 0) fighting a Warrior at `enemy`. Returns the walk the
    /// tick issued, if any.
    #[allow(clippy::too_many_arguments)]
    fn tick(
        profile: AiProfile,
        class: CharacterClass,
        pos: Vec3,
        state: &mut HealerPosture,
        directive: Option<&MovementDirective>,
        now: f32,
        enemy: (f32, f32),
    ) -> Option<MovementDirective> {
        let mut world = World::new();
        let healer = world.spawn_empty().id();
        let mage = world.spawn_empty().id();
        let warrior = world.spawn_empty().id();
        let mut ally = info(mage, 1, CharacterClass::Mage, 10.0, 0.0);
        ally.target = Some(warrior);
        let mut me = info(healer, 1, class, pos.x, pos.z);
        me.position = pos;
        let roster: BTreeMap<Entity, CombatantInfo> = [
            me,
            ally,
            info(warrior, 2, CharacterClass::Warrior, enemy.0, enemy.1),
        ]
        .into_iter()
        .map(|u| (u.entity, u))
        .collect();
        let auras = BTreeMap::new();
        let ctx = CombatContext::new(
            healer,
            1,
            &roster,
            &auras,
            &NO_DR,
            &NO_CD,
            &[],
            Default::default(),
            profile,
        )
        .classified(&SituationConfig::default(), MatchClock::after_gates(now));
        let combatant = Combatant::new(1, 0, class);
        let movement = MovementConfig::default();
        let abilities = AbilityDefinitions::default();
        let mut queue = CommandQueue::default();
        let mut trace = DecisionTrace::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            match class {
                CharacterClass::Priest => {
                    evaluate_priest_posture(
                        &mut commands,
                        healer,
                        &combatant,
                        pos,
                        &ctx,
                        &abilities,
                        None,
                        Some(state),
                        directive,
                        &movement,
                        now,
                        &mut trace,
                    );
                }
                _ => {
                    evaluate_caster_healer_posture(
                        &mut commands,
                        healer,
                        &combatant,
                        pos,
                        &ctx,
                        &abilities,
                        Some(state),
                        directive,
                        &movement,
                        &movement.druid,
                        false,
                        now,
                        &mut trace,
                    );
                }
            }
        }
        queue.apply(&mut world);
        world.get::<MovementDirective>(healer).copied()
    }

    /// The enemy Warrior, nowhere near.
    const FAR_OFF: (f32, f32) = (0.0, 60.0);

    fn point(d: &MovementDirective) -> Vec3 {
        match d.goal {
            MovementGoal::Point(p) => p,
            other => panic!("not a point walk: {other:?}"),
        }
    }

    /// Where the scenes put the healer: far from its formation point, so a
    /// tick with no walk issues one.
    const AFAR: Vec3 = Vec3::new(-20.0, 1.0, -20.0);

    /// The free tick re-issues a walk that has gone stale: a healer standing
    /// at its formation point whose walk still aims more than
    /// `formation_shift_threshold` away (the point moved on while it stood
    /// within the dead zone) gets a new walk at the point. Paired with a walk
    /// inside the threshold, which is left alone, and with `Legacy`, which
    /// leaves the stale walk too.
    #[test]
    fn a_stale_walk_is_reissued_at_the_formation_point() {
        for profile in [AiProfile::TeamPlan, AiProfile::Legacy] {
            for class in [CharacterClass::Priest, CharacterClass::Druid] {
                let label = format!("{profile:?} {class:?}");
                // First tick, from afar: the formation point.
                let mut state = HealerPosture::new(0.0);
                let walk = tick(profile, class, AFAR, &mut state, None, 5.0, FAR_OFF)
                    .expect("a walk to the formation point");
                assert_eq!(state.posture, Posture::Free, "{label}: the scene is FREE");
                let at = point(&walk);

                for (off, reissued) in [(5.0, profile.is_team_plan()), (1.0, false)] {
                    let mut state = state;
                    let old = MovementDirective {
                        goal: MovementGoal::Point(at + Vec3::new(off, 0.0, 0.0)),
                        expires: 5.9,
                        committed_until: 5.5,
                    };
                    // Standing at the point, inside its dead zone.
                    let issued = tick(profile, class, at, &mut state, Some(&old), 5.1, FAR_OFF);
                    assert_eq!(
                        issued.is_some(),
                        reissued,
                        "{label}: a walk {off}yd off the point it stands at"
                    );
                    if let Some(new) = issued {
                        assert!(
                            point(&new).distance(at) < 0.01,
                            "{label}: re-issued at {:?}, not the formation point {at:?}",
                            point(&new)
                        );
                    }
                }
            }
        }
    }

    /// The defect in one tick, for each formation machine (the Priest's, whose
    /// staged crossing its wand clamp would mask, and the caster-healer one):
    /// the Warrior fighting the healer's Mage steps from 6yd east of it to 6yd
    /// west in a tenth of a second. The healer, far off and FREE, is issued its
    /// formation point both times (it is not near it and has no walk). Under
    /// `TeamPlan` the point turns by what a tenth of a second allows — the
    /// healer's walking speed round the Mage. The pairs send it 16yd to the
    /// Mage's far side: the same two ticks with the formation memory forgotten
    /// between them, and `Legacy`.
    #[test]
    fn an_enemy_crossing_the_centroid_turns_the_point() {
        let offset = MovementConfig::default().shared.formation_offset;
        for class in [CharacterClass::Priest, CharacterClass::Druid] {
            let rate = Combatant::new(1, 0, class).base_movement_speed / offset;
            for (profile, forget) in [
                (AiProfile::TeamPlan, false),
                (AiProfile::TeamPlan, true),
                (AiProfile::Legacy, false),
            ] {
                let label = format!("{profile:?} {class:?} forget={forget}");
                let mut state = HealerPosture::new(0.0);
                let before = tick(profile, class, AFAR, &mut state, None, 5.0, (16.0, 0.0))
                    .map(|d| point(&d))
                    .expect("a walk");
                if forget {
                    state.formation.forget();
                }
                let after = tick(profile, class, AFAR, &mut state, None, 5.1, (4.0, 0.0))
                    .map(|d| point(&d))
                    .expect("a walk");
                let step = before.distance(after);
                if forget || !profile.is_team_plan() {
                    assert!(
                        step > 10.0,
                        "{label}: unconstrained, the point stepped only {step:.1}yd — the \
                         scene no longer flips it"
                    );
                } else {
                    let allowed = 2.0 * offset * (rate * 0.1 / 2.0).sin() + 1e-3;
                    assert!(
                        step <= allowed,
                        "{label}: the point stepped {step:.2}yd in 0.1s (allowed {allowed:.2})"
                    );
                    assert!(step > 0.1, "{label}: the point did not turn at all");
                }
            }
        }
    }
}
