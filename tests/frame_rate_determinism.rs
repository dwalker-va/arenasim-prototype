//! The sim is a function of the seed, not of the display's frame rate.
//!
//! The client steps the sim on a fixed 60Hz clock (`FixedUpdate`) and renders
//! at whatever rate the display runs, so the same seed must produce the same
//! sim state at every tick whether the frames arrive at 60Hz, 120Hz, 143Hz or
//! irregularly. These tests boot the REAL graphical `StatesPlugin` schedule
//! (no GPU, the `--replay` entrance straight into `PlayMatch`), run one seeded
//! match to the Results screen under several frame schedules, and compare a
//! hash of the sim state after every tick.
//!
//! ## The one place the frame clock legitimately decides something
//!
//! The victory celebration is presentation: it counts its five seconds on the
//! frame clock and then leaves for Results, so how many ticks it lasts depends
//! on where the frames fall — for the match below, one more at 120Hz than at
//! 60Hz, and two more at 25Hz. That is harmless ONLY because nothing in the
//! sim advances once the match is decided (`combat_sim_runs` in
//! `src/states/mod.rs`). When the sim still stepped through the celebration —
//! survivors walking, aura timers running down — the extra tick a faster
//! display ran moved them one step further: the same seed ended in a
//! different state at different frame rates (AS-170).
//!
//! So the comparison is: every tick both schedules ran must match exactly, and
//! any ticks one schedule ran past the other must be the frozen final state.
//! The always-on test also asserts the schedules DO disagree on the
//! celebration's length, so that tail check is never vacuous.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ActiveAuras, CastingState, ChannelingState, Combatant, MatchCountdown, Pet, Projectile, Trap,
    TrapLaunchProjectile, VictoryCelebration,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

/// The match AS-170 was found on: it is decided at tick 2653 and its
/// celebration ends on a different tick at 60Hz than at 120Hz.
const FOUND_ON: &str =
    r#"{"team1":["Warrior"],"team2":["Hunter"],"map":"BasicArena","random_seed":11}"#;

/// A plain 60Hz display, 120Hz, and ~143Hz — never a multiple of the 60Hz
/// tick, so frames with no tick and frames with one interleave irregularly.
const DISPLAYS: [u64; 3] = [16_667, 8_333, 7_000];

/// Give up on a run that has not reached Results after this much sim time.
const MAX_SIM_SECS: f32 = 330.0;

/// One frame of a schedule: its duration, and a new relative game speed to
/// set before it (the time controls), if any.
type Frame = (Duration, Option<f32>);

fn fixed_rate(micros: u64) -> impl Iterator<Item = Frame> {
    std::iter::repeat((Duration::from_micros(micros), None))
}

/// Irregular frame times from 1ms to 40ms, with a pause, a 3x fast-forward
/// and a 0.5x slow-down — the time controls a player actually touches.
fn irregular() -> impl Iterator<Item = Frame> {
    const PATTERN: [u64; 7] = [3_000, 25_000, 11_000, 40_000, 7_000, 1_000, 16_667];
    let speed_at = |i: usize| match i {
        300..=379 => 0.0,
        380..=899 => 3.0,
        900..=1299 => 0.5,
        _ => 1.0,
    };
    (0..).map(move |i| {
        let changed = i == 0 || speed_at(i) != speed_at(i - 1);
        (
            Duration::from_micros(PATTERN[i % PATTERN.len()]),
            changed.then(|| speed_at(i)),
        )
    })
}

fn boot(cfg_json: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg_json).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .init_resource::<Ticks>()
        .add_systems(FixedPostUpdate, record_tick);
    app
}

/// What one run saw, tick by tick, while the match was on screen.
#[derive(Resource, Default)]
struct Ticks {
    /// Sim-state hash after every `PlayMatch` tick.
    hashes: Vec<u64>,
    /// The first tick the victory celebration was running.
    decided_at: Option<usize>,
    /// Combat-log entries already folded into `log_hash`.
    log_seen: usize,
    /// Running hash of every combat-log entry, in order.
    log_hash: u64,
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
        self.u32(0xffff_ffff);
    }
}

/// Everything the sim decides, as text-free bits wherever it is a number.
/// Entities are named by (team, slot, pet) rather than by `Entity`: visual
/// systems spawn entities per rendered frame, so entity indices legitimately
/// differ between frame schedules while the sim is identical.
#[allow(clippy::type_complexity)]
fn record_tick(
    mut ticks: ResMut<Ticks>,
    state: Res<State<GameState>>,
    celebration: Option<Res<VictoryCelebration>>,
    countdown: Option<Res<MatchCountdown>>,
    log: Res<CombatLog>,
    combatants: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&Pet>,
        Option<&ActiveAuras>,
        Option<&CastingState>,
        Option<&ChannelingState>,
    )>,
    projectiles: Query<&Transform, Or<(With<Projectile>, With<TrapLaunchProjectile>)>>,
    traps: Query<&Transform, With<Trap>>,
) {
    if *state.get() != GameState::PlayMatch {
        return;
    }
    let names: BTreeMap<Entity, (u8, u8, bool)> = combatants
        .iter()
        .map(|(e, c, _, pet, ..)| (e, (c.team, c.slot, pet.is_some())))
        .collect();
    let name = |e: Option<Entity>| format!("{:?}", e.map(|e| names.get(&e)));

    let mut rows: Vec<(u64, u64)> = combatants
        .iter()
        .map(|(_, c, t, pet, auras, casting, channeling)| {
            let mut h = Fnv::new();
            h.str(&format!("{:?}", (c.team, c.slot, pet.is_some(), c.class)));
            for v in [
                t.translation.x,
                t.translation.y,
                t.translation.z,
                t.rotation.x,
                t.rotation.y,
                t.rotation.z,
                t.rotation.w,
                c.current_health,
                c.max_health,
                c.current_mana,
                c.attack_timer,
                c.offhand_timer,
                c.global_cooldown,
                c.damage_dealt,
                c.damage_taken,
                c.healing_done,
                c.next_attack_bonus_damage,
            ] {
                h.f32(v);
            }
            h.str(&format!(
                "{} {} {:?}",
                c.is_dead, c.stealthed, c.rogue_opener
            ));
            h.str(&name(c.target));
            h.str(&name(c.cc_target));
            let cooldowns: BTreeMap<String, u32> = c
                .ability_cooldowns
                .iter()
                .map(|(a, cd)| (format!("{a:?}"), cd.to_bits()))
                .collect();
            h.str(&format!("{cooldowns:?}"));
            let mut auras: Vec<String> = auras
                .map(|a| {
                    a.auras
                        .iter()
                        .map(|a| {
                            format!(
                                "{:?} {} {} {} {} {} {}",
                                a.effect_type,
                                a.ability_name,
                                a.duration.to_bits(),
                                a.magnitude.to_bits(),
                                a.accumulated_damage.to_bits(),
                                a.time_until_next_tick.to_bits(),
                                name(a.caster),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            auras.sort();
            h.str(&auras.join("|"));
            if let Some(cast) = casting {
                h.str(&format!(
                    "cast {:?} {} {} {}",
                    cast.ability,
                    cast.time_remaining.to_bits(),
                    name(cast.target),
                    cast.interrupted
                ));
            }
            if let Some(ch) = channeling {
                h.str(&format!(
                    "channel {:?} {} {}",
                    ch.ability,
                    ch.duration_remaining.to_bits(),
                    ch.time_until_next_tick.to_bits()
                ));
            }
            (
                (c.team as u64) << 32 | (c.slot as u64) << 1 | pet.is_some() as u64,
                h.0,
            )
        })
        .collect();
    rows.sort();

    let mut objects: Vec<[u32; 4]> = projectiles
        .iter()
        .map(|t| (1, t))
        .chain(traps.iter().map(|t| (2, t)))
        .map(|(kind, t)| {
            [
                kind,
                t.translation.x.to_bits(),
                t.translation.y.to_bits(),
                t.translation.z.to_bits(),
            ]
        })
        .collect();
    objects.sort();

    // Fold new combat-log entries into the running hash (the log only grows).
    let mut log_hash = Fnv(ticks.log_hash);
    if ticks.log_seen == 0 {
        log_hash = Fnv::new();
    }
    for entry in &log.entries[ticks.log_seen..] {
        log_hash.f32(entry.timestamp);
        log_hash.str(&format!("{:?}", entry.event_type));
        log_hash.str(&entry.message);
    }
    ticks.log_seen = log.entries.len();
    ticks.log_hash = log_hash.0;

    let mut h = Fnv::new();
    for (key, row) in rows {
        h.bytes(&key.to_le_bytes());
        h.bytes(&row.to_le_bytes());
    }
    for o in objects {
        for v in o {
            h.u32(v);
        }
    }
    h.f32(countdown.map_or(f32::NAN, |c| c.time_remaining));
    h.f32(log.match_time);
    h.bytes(&ticks.log_hash.to_le_bytes());
    ticks.hashes.push(h.0);

    if celebration.is_some() && ticks.decided_at.is_none() {
        ticks.decided_at = Some(ticks.hashes.len());
    }
}

/// Run `cfg` to the Results screen under one frame schedule.
fn run(cfg: &str, frames: impl Iterator<Item = Frame>) -> Ticks {
    let mut app = boot(cfg);
    let mut sim_secs = 0.0;
    for (frame, speed) in frames {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        if let Some(speed) = speed {
            app.world_mut()
                .resource_mut::<Time<Virtual>>()
                .set_relative_speed(speed);
        }
        app.update();
        if *app.world().resource::<State<GameState>>().get() == GameState::Results {
            break;
        }
        sim_secs += frame.as_secs_f32();
        assert!(
            sim_secs < MAX_SIM_SECS,
            "{cfg} never reached Results within {MAX_SIM_SECS}s"
        );
    }
    std::mem::take(&mut *app.world_mut().resource_mut::<Ticks>())
}

/// Every tick both runs saw is identical, and any tick one ran past the
/// other is the frozen final state.
fn assert_same_sim(cfg: &str, a_label: &str, a: &Ticks, b_label: &str, b: &Ticks) {
    let decided = a.decided_at.expect("the match was decided");
    assert_eq!(
        a.decided_at, b.decided_at,
        "{cfg}: decided at tick {:?} under {a_label}, {:?} under {b_label}",
        a.decided_at, b.decided_at
    );
    let common = a.hashes.len().min(b.hashes.len());
    if let Some(tick) = (0..common).find(|&i| a.hashes[i] != b.hashes[i]) {
        panic!(
            "{cfg}: sim state differs at tick {} (decided at {decided}) between {a_label} and {b_label}",
            tick + 1
        );
    }
    let (longer_label, longer) = if a.hashes.len() > b.hashes.len() {
        (a_label, a)
    } else {
        (b_label, b)
    };
    let last = longer.hashes[common - 1];
    if let Some(i) = (common..longer.hashes.len()).find(|&i| longer.hashes[i] != last) {
        panic!(
            "{cfg}: {longer_label} ran {} celebration tick(s) past the other schedule and the \
             sim moved on tick {} — something advances after the match is decided (tick {decided})",
            longer.hashes.len() - common,
            i + 1
        );
    }
}

fn assert_frame_rate_independent(cfg: &str, schedules: Vec<(String, Ticks)>) {
    let (base_label, base) = &schedules[0];
    for (label, ticks) in &schedules[1..] {
        assert_same_sim(cfg, base_label, base, label, ticks);
    }
}

fn displays(cfg: &str) -> Vec<(String, Ticks)> {
    DISPLAYS
        .iter()
        .map(|&us| (format!("{us}us frames"), run(cfg, fixed_rate(us))))
        .collect()
}

#[test]
fn same_seed_same_sim_at_every_display_rate() {
    let schedules = displays(FOUND_ON);

    // Non-vacuity, twice over. The match really was decided, a long way in...
    let decided = schedules[0].1.decided_at.expect("the match was decided");
    assert!(decided > 2000, "decided at tick {decided}");
    // ...and the schedules disagree on how long the celebration lasts, so
    // the frozen-tail half of `assert_same_sim` actually checks something.
    // If a change ever makes them agree, this says so rather than letting
    // that half pass without looking at a single tick.
    let lengths: Vec<usize> = schedules.iter().map(|(_, t)| t.hashes.len()).collect();
    assert!(
        lengths.iter().any(|&l| l != lengths[0]),
        "every schedule ran {} PlayMatch ticks; the frozen-tail check is vacuous",
        lengths[0]
    );

    assert_frame_rate_independent(FOUND_ON, schedules);
}

/// The wider sweep: more matchups, maps and seeds, and the frame schedules a
/// player can actually produce — 25Hz, and irregular frames under pause,
/// fast-forward and slow-motion. Minutes of simulation, so opt-in:
/// `cargo test --release --test frame_rate_determinism -- --ignored`
#[test]
#[ignore]
fn sweep_matchups_and_schedules() {
    const MATCHES: [&str; 8] = [
        FOUND_ON,
        r#"{"team1":["Warrior"],"team2":["Hunter"],"map":"BasicArena","random_seed":3}"#,
        r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#,
        r#"{"team1":["Warlock"],"team2":["Priest"],"map":"TwinPillars","random_seed":5}"#,
        r#"{"team1":["Warrior","Priest"],"team2":["Hunter","Mage"],"map":"TwinPillars","random_seed":7}"#,
        r#"{"team1":["Warlock","Rogue"],"team2":["Hunter","Paladin"],"map":"PillaredArena","random_seed":3}"#,
        r#"{"team1":["Rogue","Shaman"],"team2":["Warlock","Priest"],"map":"PillaredArena","random_seed":13}"#,
        r#"{"team1":["Warrior","Priest","Mage"],"team2":["Hunter","Warlock","Shaman"],"map":"TwinPillars","random_seed":21}"#,
    ];
    for cfg in MATCHES {
        let mut schedules = displays(cfg);
        schedules.push(("40000us frames".into(), run(cfg, fixed_rate(40_000))));
        schedules.push(("irregular frames".into(), run(cfg, irregular())));
        eprintln!(
            "{cfg}: decided at tick {:?}, PlayMatch ticks {:?}",
            schedules[0].1.decided_at,
            schedules
                .iter()
                .map(|(_, t)| t.hashes.len())
                .collect::<Vec<_>>()
        );
        assert_frame_rate_independent(cfg, schedules);
    }
}
