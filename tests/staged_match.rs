//! The staged-match harness (tier 3, `arenasim::headless::stage`) is
//! faithful: it plays a staged world through the real schedule exactly as a
//! match plays its own.
//!
//! Two claims, each against a seeded match:
//! - **the harness perturbs nothing**: an empty stage plays the seed byte for
//!   byte, landed at the gates or mid-match;
//! - **a staged state plays on as the state it equals**: a match on ANOTHER
//!   seed, its state at time T overwritten with the seeded match's state at T,
//!   plays on from T exactly as the seeded match does.

mod common;

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use regex::Regex;

use arenasim::headless::{HeadlessMatchConfig, HeadlessMatchState, Stage, Unit};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::situation::Situations;
use arenasim::states::play_match::team_plan::TeamPlans;
use arenasim::states::play_match::{
    AbilityType, ActiveAuras, ArenaDampening, Aura, AuraType, CastingState, ChannelingState,
    ChargingState, Combatant, DRTracker, DisengagingState, GameRng, HealerPosture,
    InterruptPending, KitePosture, MeleeResetState, MovementDirective, PetCommand, RogueFlareState,
    ShadowSightState,
};
use common::staged::{play, play_seeded, sim_time, Landed, Played};

/// A busy 2v2 on Nagrand under `TeamPlan`: pets, projectiles, a Druid's
/// instants, pillars.
fn nagrand(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Priest".into()],
        team2: vec!["Warlock".into(), "Druid".into()],
        map: "PillaredArena".into(),
        ai_profile: Some("TeamPlan".into()),
        random_seed: Some(seed),
        max_duration_secs: 90.0,
        ..Default::default()
    }
}

/// An empty stage changes nothing: a staged match whose stage writes nothing
/// plays its seed byte for byte — every frame's observation, the trace and the
/// combat log — whether it lands at the gates or mid-match. So the harness
/// itself (its extra system, the frame it lands on) perturbs nothing a probe
/// reads.
#[test]
fn an_empty_stage_plays_the_seed_byte_for_byte() {
    let seeded = play_seeded(nagrand(3));
    for at in [0.0, 20.0] {
        let staged = play(nagrand(3), Stage::new().after_gates(at));
        let landed = staged.landed().combat;
        assert!(
            (at..at + 0.02).contains(&landed),
            "a stage due at {at}s landed at {landed}s"
        );
        assert!(
            staged.trace_raw == seeded.trace_raw,
            "an empty stage at {at}s moved the trace"
        );
        assert!(
            staged.log == seeded.log,
            "an empty stage at {at}s moved the combat log"
        );
        assert!(
            staged.frames == seeded.frames,
            "an empty stage at {at}s moved the frames"
        );
    }
    // Not vacuous: the match fought.
    assert!(seeded.log.matches(" damage").count() > 50, "{}", seeded.log);
}

/// Each `Stage` setter writes what it names, on the unit it names — a pet by
/// its owner's slot included — and `land` lands its aura through
/// `apply_pending_auras` on the stage's own frame.
#[test]
fn every_stage_setter_writes_what_it_names() {
    const WARRIOR: Unit = Unit::new(1, 0);
    const HUNTER: Unit = Unit::new(1, 1);
    const PET: Unit = Unit::pet_of(1, 1);
    const MAGE: Unit = Unit::new(2, 0);
    const PRIEST: Unit = Unit::new(2, 1);
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Hunter".into()],
        team2: vec!["Mage".into(), "Priest".into()],
        random_seed: Some(1),
        max_duration_secs: 1.0,
        ..Default::default()
    };
    let root = Aura {
        effect_type: AuraType::Root,
        duration: 5.0,
        ..Default::default()
    };
    let read = Arc::new(Mutex::new(Vec::new()));
    let out = read.clone();
    let played = play(
        config,
        Stage::new()
            .place(WARRIOR, 5.0, 6.0)
            .place(PET, -3.0, 4.0)
            .health(WARRIOR, 0.4)
            .resource(WARRIOR, 55.0)
            .mana(MAGE, 0.25)
            .target(PRIEST, HUNTER)
            .cooldown(MAGE, AbilityType::FrostNova, 12.0)
            .clear_auras(PRIEST)
            .aura(PRIEST, root)
            .land(HUNTER, AbilityType::Polymorph, MAGE)
            .healer_posture(PRIEST, HealerPosture::new(3.0))
            .kite_posture(MAGE, KitePosture::new(4.0))
            .melee_reset(
                WARRIOR,
                MeleeResetState {
                    armed_until: 7.0,
                    active: true,
                },
            )
            .clock(40.0)
            .world(move |w| {
                let (warrior, hunter, pet, mage, priest) = (
                    WARRIOR.entity(w),
                    HUNTER.entity(w),
                    PET.entity(w),
                    MAGE.entity(w),
                    PRIEST.entity(w),
                );
                let c = |e: Entity| w.get::<Combatant>(e).unwrap();
                let at = |e: Entity| {
                    let t = w.get::<Transform>(e).unwrap().translation;
                    (t.x, t.z)
                };
                let mut out = out.lock().unwrap();
                out.push(format!("warrior at {:?}", at(warrior)));
                out.push(format!("pet at {:?}", at(pet)));
                out.push(format!(
                    "warrior hp {:.2} rage {}",
                    c(warrior).current_health / c(warrior).max_health,
                    c(warrior).current_mana
                ));
                out.push(format!(
                    "mage mana {:.2}",
                    c(mage).current_mana / c(mage).max_mana
                ));
                out.push(format!(
                    "priest targets hunter {}",
                    c(priest).target == Some(hunter)
                ));
                out.push(format!(
                    "frost nova {:?}",
                    c(mage).ability_cooldowns.get(&AbilityType::FrostNova)
                ));
                out.push(format!(
                    "priest auras {:?}",
                    w.get::<ActiveAuras>(priest).map(|a| a
                        .auras
                        .iter()
                        .map(|a| a.effect_type)
                        .collect::<Vec<_>>())
                ));
                out.push(format!(
                    "postures {:?} {:?} {:?}",
                    w.get::<HealerPosture>(priest).map(|p| p.since),
                    w.get::<KitePosture>(mage).map(|p| p.since),
                    w.get::<MeleeResetState>(warrior).map(|p| p.armed_until)
                ));
                out.push(format!(
                    "clock {} {} {} {}",
                    w.resource::<DecisionTrace>().current_sim_time,
                    w.resource::<ArenaDampening>().time_since_gates,
                    w.resource::<ShadowSightState>().combat_time,
                    w.resource::<HeadlessMatchState>().elapsed_time
                ));
            }),
    );
    assert_eq!(
        *read.lock().unwrap(),
        [
            "warrior at (5.0, 6.0)",
            "pet at (-3.0, 4.0)",
            "warrior hp 0.40 rage 55",
            "mage mana 0.25",
            "priest targets hunter true",
            "frost nova Some(12.0)",
            "priest auras Some([Root])",
            "postures Some(3.0) Some(4.0) Some(7.0)",
            "clock 40 40 40 40",
        ]
    );
    // `land` went through the funnel: the Hunter is polymorphed on the frame
    // the stage landed on, and the trace's clock reads the staged 40s.
    let stage_frame = played.staged_frames().next().expect("the stage frame");
    let hunter = stage_frame
        .combatants
        .values()
        .find(|c| c.team == 1 && c.slot == 1 && !c.is_pet)
        .unwrap();
    assert!(
        hunter.aura_types.contains(&AuraType::Polymorph),
        "{:?}",
        hunter.aura_types
    );
    assert!(played.log.contains("Polymorph"), "{}", played.log);
    assert!(played
        .staged_trace()
        .all(|e| (40.0..41.1).contains(&sim_time(e))));
}

// ---------------------------------------------------------------------------
// Reconstruction: a stage built to equal a seeded match's state at time T
// ---------------------------------------------------------------------------

/// Everything a seeded match carries forward from one frame to the next, at a
/// QUIET frame: no cast, channel, Charge or Disengage in progress and nothing
/// in flight or pending (no projectile, trap, zone, totem or queued effect).
/// Per combatant, its whole state; per match, the RNG stream's position, the
/// clocks, and the AI's team-level state.
struct Snapshot {
    units: Vec<UnitSnapshot>,
    rng: GameRng,
    since_gates: f32,
    dampening: (f32, usize),
    shadow_sight: (f32, bool),
    situations: Situations,
    plans: TeamPlans,
    elapsed: f32,
    trace_clock: (u64, f32),
}

struct UnitSnapshot {
    entity: Entity,
    transform: Transform,
    combatant: Combatant,
    auras: Option<Vec<Aura>>,
    dr: DRTracker,
    healer: Option<HealerPosture>,
    kite: Option<KitePosture>,
    directive: Option<MovementDirective>,
    reset: Option<MeleeResetState>,
    flare: Option<RogueFlareState>,
    pet_command: Option<PetCommand>,
}

fn component_names(world: &World, e: Entity) -> Vec<String> {
    world
        .inspect_entity(e)
        .map(|it| it.map(|c| c.name().to_string()).collect())
        .unwrap_or_default()
}

/// An entity the simulation spawns for the client to draw — a landing, a
/// swing, a floating number — and never reads again. Headless leaves them be
/// (the client's systems despawn them), so they pile up; nothing in the
/// schedule reads one.
fn visual_only(world: &World, e: Entity) -> bool {
    component_names(world, e)
        .iter()
        .all(|n| n.contains("::components::visual::") || n.ends_with("::PlayMatchEntity"))
}

/// What is in progress or in flight: a combatant mid-cast, channel, Charge or
/// Disengage, or any entity that is not a combatant.
fn busy(world: &World) -> Vec<String> {
    let mut busy = Vec::new();
    for e in world.iter_entities() {
        let id = e.id();
        if e.contains::<Combatant>() {
            for (has, what) in [
                (e.contains::<CastingState>(), "casting"),
                (e.contains::<ChannelingState>(), "channeling"),
                (e.contains::<ChargingState>(), "charging"),
                (e.contains::<DisengagingState>(), "disengaging"),
                (e.contains::<InterruptPending>(), "interrupted"),
            ] {
                if has {
                    busy.push(format!("{id} {what}"));
                }
            }
        } else if !visual_only(world, id) {
            busy.push(format!("{id} in flight: {:?}", component_names(world, id)));
        }
    }
    busy
}

fn capture(world: &mut World) -> Snapshot {
    let mut query = world.query::<(
        Entity,
        &Transform,
        &Combatant,
        Option<&ActiveAuras>,
        &DRTracker,
    )>();
    let mut units: Vec<UnitSnapshot> = query
        .iter(world)
        .map(|(entity, transform, combatant, auras, dr)| UnitSnapshot {
            entity,
            transform: *transform,
            combatant: combatant.clone(),
            auras: auras.map(|a| a.auras.clone()),
            dr: dr.clone(),
            healer: None,
            kite: None,
            directive: None,
            reset: None,
            flare: None,
            pet_command: None,
        })
        .collect();
    units.sort_by_key(|u| u.entity);
    for u in &mut units {
        let e = world.entity(u.entity);
        u.healer = e.get::<HealerPosture>().copied();
        u.kite = e.get::<KitePosture>().copied();
        u.directive = e.get::<MovementDirective>().copied();
        u.reset = e.get::<MeleeResetState>().copied();
        u.flare = e.get::<RogueFlareState>().copied();
        u.pet_command = e.get::<PetCommand>().copied();
    }
    let dampening = world.resource::<ArenaDampening>();
    let shadow = world.resource::<ShadowSightState>();
    Snapshot {
        units,
        rng: world.resource::<GameRng>().clone(),
        since_gates: dampening.time_since_gates,
        dampening: (dampening.reduction, dampening.next_milestone),
        shadow_sight: (shadow.combat_time, shadow.orbs_spawned),
        situations: world.resource::<Situations>().clone(),
        plans: world.resource::<TeamPlans>().clone(),
        elapsed: world.resource::<HeadlessMatchState>().elapsed_time,
        trace_clock: {
            let trace = world.resource::<DecisionTrace>();
            (trace.current_frame, trace.current_sim_time)
        },
    }
}

fn put<C: Component>(world: &mut World, e: Entity, c: Option<C>) {
    match c {
        Some(c) => {
            world.entity_mut(e).insert(c);
        }
        None => {
            world.entity_mut(e).remove::<C>();
        }
    }
}

/// Write `snap` over the running match: every combatant's state, the RNG,
/// the clocks and the team-level AI state. Anything in progress or in flight
/// is cleared, as it was on the quiet frame `snap` was taken on.
fn restore(world: &mut World, snap: Snapshot) {
    let in_flight: Vec<Entity> = world
        .iter_entities()
        .filter(|e| !e.contains::<Combatant>() && !visual_only(world, e.id()))
        .map(|e| e.id())
        .collect();
    for e in in_flight {
        world.despawn(e);
    }
    for u in snap.units {
        let e = u.entity;
        let c = world.get::<Combatant>(e).expect("the same combatant");
        assert_eq!((c.team, c.slot), (u.combatant.team, u.combatant.slot));
        world.entity_mut(e).remove::<(
            CastingState,
            ChannelingState,
            ChargingState,
            DisengagingState,
            InterruptPending,
        )>();
        *world.get_mut::<Transform>(e).unwrap() = u.transform;
        *world.get_mut::<Combatant>(e).unwrap() = u.combatant;
        *world.get_mut::<DRTracker>(e).unwrap() = u.dr;
        put(world, e, u.auras.map(|auras| ActiveAuras { auras }));
        put(world, e, u.healer);
        put(world, e, u.kite);
        put(world, e, u.directive);
        put(world, e, u.reset);
        put(world, e, u.flare);
        put(world, e, u.pet_command);
    }
    *world.resource_mut::<GameRng>() = snap.rng;
    {
        let mut d = world.resource_mut::<ArenaDampening>();
        d.time_since_gates = snap.since_gates;
        (d.reduction, d.next_milestone) = snap.dampening;
    }
    {
        let mut s = world.resource_mut::<ShadowSightState>();
        (s.combat_time, s.orbs_spawned) = snap.shadow_sight;
    }
    *world.resource_mut::<Situations>() = snap.situations;
    *world.resource_mut::<TeamPlans>() = snap.plans;
    world.resource_mut::<HeadlessMatchState>().elapsed_time = snap.elapsed;
    {
        let mut trace = world.resource_mut::<DecisionTrace>();
        (trace.current_frame, trace.current_sim_time) = snap.trace_clock;
    }
}

/// The first quiet frame at or after `from` seconds of combat in `config`'s
/// match: its combat time and the state on it.
fn quiet_state(config: impl Fn() -> HeadlessMatchConfig, from: f32) -> (f32, Snapshot) {
    for step in 0..200 {
        let at = from + step as f32 * 0.1;
        let found = Arc::new(Mutex::new(None));
        let keep = found.clone();
        play(
            config(),
            Stage::new().after_gates(at).world(move |w| {
                let quiet = busy(w).is_empty();
                *keep.lock().unwrap() = Some(quiet.then(|| capture(w)));
            }),
        );
        let snap = found.lock().unwrap().take().expect("the stage landed");
        if let Some(snap) = snap {
            return (at, snap);
        }
    }
    panic!("no quiet frame in 20s of combat from {from}s");
}

/// The part of a played match from the frame its stage landed on: frames,
/// trace lines, and log entries.
fn from_stage(p: &Played, at: Landed) -> (Vec<String>, Vec<String>, Vec<String>) {
    let frames = p
        .frames
        .iter()
        .filter(|f| f.sim_time >= at.app)
        .map(|f| format!("{f:?}"))
        .collect();
    let from_frame =
        |e: &serde_json::Value| e["frame"].as_u64().is_some_and(|f| f >= at.trace_frame);
    // The trace stamps every line with the run's seed: the one thing the two
    // runs are told differently, not something either played.
    let seed = Regex::new(r#""seed":\d+,"#).unwrap();
    let trace = p
        .trace
        .iter()
        .zip(p.trace_raw.lines())
        .filter(|(e, _)| from_frame(e))
        .map(|(_, l)| seed.replace(l, "").into_owned())
        .collect();
    // Log entries carry `[ 12.34s]` stamps on the app's clock; the report's own
    // sections are unstamped and summarise the match from the combatants' end
    // state. Keep the stamped lines at or past the stage, and the summary less
    // its `Seed:` line.
    let log = p
        .log
        .lines()
        .filter(|l| log_stamp(l).is_none_or(|t| t >= at.app) && !l.starts_with("Seed: "))
        .map(str::to_string)
        .collect();
    (frames, trace, log)
}

/// A combat-log line's `[ 12.34s]` stamp, in seconds since the app started.
fn log_stamp(line: &str) -> Option<f32> {
    let (stamp, _) = line.strip_prefix('[')?.split_once(']')?;
    stamp.trim().strip_suffix('s')?.parse().ok()
}

/// The first line two runs disagree on, for the failure message.
fn first_difference(a: &[String], b: &[String]) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!("line {i}:\n  seeded: {}\n  staged: {}", a[i], b[i]),
        None => format!("lengths {} vs {}", a.len(), b.len()),
    }
}

/// A match on another seed, its state at a quiet frame T replaced by the
/// seeded match's state at T, plays on from T as the seeded match does: every
/// frame, every trace line and every log entry from T to the end, and the
/// result. The stage describes the whole state, so nothing the staged match
/// played before T reaches past it.
#[test]
fn a_stage_built_to_a_seeded_state_plays_on_as_the_seed_does() {
    const SEEDED: u64 = 3;
    const OTHER: u64 = 11;
    let (at, snap) = quiet_state(|| nagrand(SEEDED), 18.0);
    let seeded = play_seeded(nagrand(SEEDED));
    let staged = play(
        nagrand(OTHER),
        Stage::new()
            .after_gates(at)
            .world(move |w| restore(w, snap)),
    );
    let landed = staged.landed();

    // The two matches really were different up to T.
    let before = |p: &Played| -> Vec<String> {
        p.frames
            .iter()
            .filter(|f| f.sim_time < landed.app)
            .map(|f| format!("{f:?}"))
            .collect()
    };
    let (sb, gb) = (before(&seeded), before(&staged));
    let diverged = sb.iter().zip(&gb).filter(|(a, b)| a != b).count();
    eprintln!("before T: {diverged} of {} frames differ", sb.len());
    assert!(
        diverged > 60,
        "seed {OTHER} played as seed {SEEDED} up to T"
    );

    let (sf, st, sl) = from_stage(&seeded, landed);
    let (gf, gt, gl) = from_stage(&staged, landed);
    // Not vacuous: a long fight follows T, and the comparison covers it.
    assert!(sf.len() > 600, "the seeded match plays on past T={at}s");
    let hits = sl.iter().filter(|l| l.contains("[DMG]")).count();
    let heals = sl.iter().filter(|l| l.contains("[HEAL]")).count();
    let casts = st
        .iter()
        .filter(|l| l.contains(r#""type":"action_taken""#))
        .count();
    eprintln!(
        "from T={at}s: {} frames, {casts} casts, {hits} hits, {heals} heals",
        sf.len()
    );
    assert!(
        hits > 30 && heals > 30 && casts > 20,
        "{hits} hits, {heals} heals, {casts} casts"
    );
    assert!(
        sf == gf,
        "frames from T={at}s: {}",
        first_difference(&sf, &gf)
    );
    assert!(
        st == gt,
        "trace from T={at}s: {}",
        first_difference(&st, &gt)
    );
    assert!(sl == gl, "log from T={at}s: {}", first_difference(&sl, &gl));
    assert_eq!(seeded.result.winner, staged.result.winner);
    assert_eq!(seeded.result.match_time, staged.result.match_time);
}
