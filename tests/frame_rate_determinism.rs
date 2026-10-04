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
//! any ticks one schedule ran past the other must leave the sim state frozen.
//! Positions are hashed apart from the rest of the state, because one thing
//! does move after the decision: the victory choreography walks the winners
//! together (`play_match::celebration`). It runs on the sim clock, so their
//! positions must still match tick for tick wherever both schedules ran.
//! The always-on test also asserts the schedules DO disagree on the
//! celebration's length, so that tail check is never vacuous.
//!
//! ## What the player reads, not just the state
//!
//! Identical state can still be LISTED differently. The Results screen and the
//! saved match report are built by `check_match_end` from a query, and
//! graphical-only components (a hit flinch, a charge trail) move combatants
//! between archetypes on the frame clock, so query order depended on the
//! display: the same seed read "Priest, Warrior" on one screen and "Warrior,
//! Priest" on another, every number identical. So each run also captures the
//! Results rows IN ORDER and the report's full text at the deciding tick, and
//! both must match across schedules and list every team in slot order.
//!
//! ## The order combatants are visited in
//!
//! The listing bug had a cause that reaches the sim: every sim loop iterates
//! combatants in ECS query order, which is the order of their table rows, and
//! a component inserted on or removed from an entity moves it to another table
//! and reshuffles that order. Done on the frame clock, that made the order —
//! and with it the order of RNG draws and same-tick log lines — depend on the
//! display (AS-175). Graphical-only state on a sim entity is therefore
//! `SparseSet`: it changes the entity's archetype, never its table. Two checks
//! hold that: each run records the order a combatant query visits them in
//! after every tick, which must match tick for tick across schedules; and a
//! watch fails the run outright the moment a combatant changes table anywhere
//! but inside a fixed tick, naming the component — so a new frame-clock insert
//! is caught even on a frame where it happens not to reorder anything. The
//! watch only sees visuals a match actually draws, so the three default
//! matches between them field all nine classes, and the third asserts that the four classes the first two leave out each had a
//! visual land on a combatant.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use bevy::app::RunFixedMainLoopSystem;
use bevy::asset::AssetPlugin;
use bevy::ecs::archetype::ArchetypeId;
use bevy::ecs::component::StorageType;
use bevy::ecs::storage::TableId;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ActiveAuras, AuraType, CastingState, ChannelingState, Combatant, MatchCountdown, MatchResults,
    Pet, Projectile, Trap, TrapLaunchProjectile, VictoryCelebration,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

/// A match decided a long way in, whose celebration ends on a different tick
/// at 60Hz than at 120Hz. AS-170 was found on Warrior v Hunter seed 11
/// (decided at tick 2653); since the Hunter's traps share one cooldown
/// (AS-196) that matchup is decided by tick 1802 in every one of seeds 0-49,
/// so this is the first qualifying seed of a matchup without a Hunter.
const FOUND_ON: &str =
    r#"{"team1":["Warlock"],"team2":["Priest"],"map":"TwinPillars","random_seed":0}"#;

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
        .init_resource::<TableWatch>()
        .add_systems(FixedPostUpdate, record_tick)
        .add_systems(
            RunFixedMainLoop,
            (
                (|world: &mut World| watch_tables(world, "between frames"))
                    .in_set(RunFixedMainLoopSystem::BeforeFixedMainLoop),
                (|world: &mut World| watch_tables(world, ""))
                    .in_set(RunFixedMainLoopSystem::AfterFixedMainLoop),
            ),
        )
        .add_systems(Last, |world: &mut World| {
            watch_tables(world, "on the frame clock (Update)")
        });
    app
}

/// What one run saw, tick by tick, while the match was on screen.
#[derive(Resource, Default)]
struct Ticks {
    /// Sim-state hash after every `PlayMatch` tick — everything but where the
    /// combatants stand.
    hashes: Vec<u64>,
    /// Where every combatant stands and faces, after every `PlayMatch` tick.
    /// Kept apart from `hashes` because the victory choreography moves the
    /// winners after the sim has frozen.
    poses: Vec<u64>,
    /// The order a combatant query visits the combatants in, after every
    /// `PlayMatch` tick — the order every sim loop that iterates combatants
    /// sees, and so the order their RNG draws and same-tick log lines fall in.
    orders: Vec<u64>,
    /// The first tick the victory celebration was running.
    decided_at: Option<usize>,
    /// Combat-log entries already folded into `log_hash`.
    log_seen: usize,
    /// Running hash of every combat-log entry, in order.
    log_hash: u64,
    /// The Results screen's rows, IN the order the screen lists them, and the
    /// saved match report's full text — both taken the tick the match was
    /// decided, which is when `check_match_end` builds and saves them.
    results: Option<String>,
    report: Option<String>,
    /// Components the frame clock put on a combatant, with the class each one
    /// is credited to (from the table watch).
    inserted_off_tick: BTreeSet<(String, String)>,
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

    // The visiting order, NOT sorted: it is the thing being checked.
    let mut order = Fnv::new();
    for (_, c, _, pet, ..) in combatants.iter() {
        order.bytes(&[c.team, c.slot, pet.is_some() as u8]);
    }
    ticks.orders.push(order.0);

    let mut rows: Vec<(u64, u64, u64)> = combatants
        .iter()
        .map(|(_, c, t, pet, auras, casting, channeling)| {
            let mut pose = Fnv::new();
            for v in [
                t.translation.x,
                t.translation.y,
                t.translation.z,
                t.rotation.x,
                t.rotation.y,
                t.rotation.z,
                t.rotation.w,
            ] {
                pose.f32(v);
            }
            let mut h = Fnv::new();
            h.str(&format!("{:?}", (c.team, c.slot, pet.is_some(), c.class)));
            for v in [
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
                pose.0,
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

    // Fold this tick's new combat-log entries into the running hash (the log
    // only grows), in the order they were logged.
    let mut log_hash = Fnv(ticks.log_hash);
    if ticks.log_seen == 0 {
        log_hash = Fnv::new();
    }
    for e in &log.entries[ticks.log_seen..] {
        log_hash.str(&format!(
            "{} {:?} {}",
            e.timestamp.to_bits(),
            e.event_type,
            e.message
        ));
    }
    ticks.log_seen = log.entries.len();
    ticks.log_hash = log_hash.0;

    let mut h = Fnv::new();
    let mut poses = Fnv::new();
    for (key, row, pose) in rows {
        h.bytes(&key.to_le_bytes());
        h.bytes(&row.to_le_bytes());
        poses.bytes(&key.to_le_bytes());
        poses.bytes(&pose.to_le_bytes());
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
    ticks.poses.push(poses.0);

    if let Some(celebration) = celebration {
        if ticks.decided_at.is_none() {
            ticks.decided_at = Some(ticks.hashes.len());
            ticks.results = Some(results_rows(&celebration.match_results));
            let mut report = Vec::new();
            log.write_report(&celebration.match_metadata, &mut report)
                .unwrap();
            ticks.report = Some(String::from_utf8(report).unwrap());
        }
    }
}

/// The Results screen's content, row by row in its own order. Deliberately
/// NOT sorted: the order is what a player sees, and what must not depend on
/// the display.
fn results_rows(results: &MatchResults) -> String {
    let mut out = format!(
        "winner {:?} duration {}\n",
        results.winner, results.duration_secs
    );
    for (team, rows) in [
        (1, &results.team1_combatants),
        (2, &results.team2_combatants),
    ] {
        for row in rows {
            out += &format!(
                "team {team} slot {} {:?} dealt {} taken {} healed {} survived {}\n",
                row.slot,
                row.class,
                row.damage_dealt,
                row.damage_taken,
                row.healing_done,
                row.survived
            );
        }
    }
    let links: BTreeMap<_, _> = results.pet_damage_links.iter().collect();
    out += &format!("pet links {links:?}\n");
    out
}

/// Every combatant's table and archetype as last seen, and every time one
/// changed table outside a fixed tick.
#[derive(Resource, Default)]
struct TableWatch {
    seen: BTreeMap<Entity, (TableId, ArchetypeId)>,
    moves: Vec<String>,
    /// Non-vacuity: combatants compared off the tick, and table changes the
    /// sim made on it — the watch saw real moves and told them apart.
    checked_off_tick: usize,
    moves_on_tick: usize,
    /// Every component put on a combatant off the tick, with the class whose
    /// kit it shows (see `credited_class`) — what the frame clock exercised,
    /// whatever its storage.
    inserted_off_tick: BTreeSet<(String, String)>,
}

/// Compare every combatant's table with where it was last seen, and record it.
/// Runs at the start of the frame's fixed loop, at its end, and at the end of
/// the frame: a table change found by the first or last is one made off the
/// fixed tick (`phase` names where), and fails the run. The end of the fixed
/// loop (empty `phase`) counts its changes without flagging them: the sim
/// moving its own entities between tables on the tick is the sim clock, which
/// every schedule shares. Checked while
/// the match is undecided — once it is, nothing in the sim iterates again.
fn watch_tables(world: &mut World, phase: &'static str) {
    let live = *world.resource::<State<GameState>>().get() == GameState::PlayMatch
        && !world.contains_resource::<VictoryCelebration>();
    if !live {
        world.resource_mut::<TableWatch>().seen.clear();
        return;
    }
    let mut combatants = world.query_filtered::<Entity, With<Combatant>>();
    let now: BTreeMap<Entity, (TableId, ArchetypeId)> = combatants
        .iter(world)
        .map(|e| {
            let at = world.entities().get(e).expect("a live entity");
            (e, (at.table_id, at.archetype_id))
        })
        .collect();
    let (mut moves, mut checked, mut on_tick) = (Vec::new(), 0, 0);
    let mut inserted = Vec::new();
    let watch = world.resource::<TableWatch>();
    for (e, &(table, archetype)) in &now {
        let Some(&(was_table, was_archetype)) = watch.seen.get(e) else {
            continue;
        };
        if phase.is_empty() {
            on_tick += usize::from(table != was_table);
            continue;
        }
        checked += 1;
        if archetype != was_archetype {
            let (added, _) = component_change(world, was_archetype, archetype, None);
            for component in added.iter().map(|c| short_name(c)) {
                for class in credited_class(world, *e, &component) {
                    inserted.push((component.clone(), class));
                }
            }
        }
        if table != was_table {
            let c = world.get::<Combatant>(*e).expect("a combatant");
            moves.push(format!(
                "team {} slot {} {:?}{} changed table {phase}: {}",
                c.team,
                c.slot,
                c.class,
                if world.get::<Pet>(*e).is_some() {
                    " (pet)"
                } else {
                    ""
                },
                table_component_change(world, was_archetype, archetype)
            ));
        }
    }
    let mut watch = world.resource_mut::<TableWatch>();
    watch.moves.extend(moves);
    watch.inserted_off_tick.extend(inserted);
    watch.checked_off_tick += checked;
    watch.moves_on_tick += on_tick;
    watch.seen = now;
}

/// The components added and removed between two archetypes, by name — only
/// those of `storage`, when given.
fn component_change(
    world: &World,
    from: ArchetypeId,
    to: ArchetypeId,
    storage: Option<StorageType>,
) -> (Vec<String>, Vec<String>) {
    let archetypes = world.archetypes();
    let components = world.components();
    let set = |id: ArchetypeId| -> Vec<String> {
        archetypes[id]
            .components()
            .filter(|&c| {
                storage.is_none() || components.get_info(c).map(|i| i.storage_type()) == storage
            })
            .map(|c| components.get_name(c).map_or("?".into(), |n| n.to_string()))
            .collect()
    };
    let (before, after) = (set(from), set(to));
    let added = after
        .iter()
        .filter(|c| !before.contains(c))
        .cloned()
        .collect();
    let removed = before
        .iter()
        .filter(|c| !after.contains(c))
        .cloned()
        .collect();
    (added, removed)
}

/// The table-stored components that differ between two archetypes, by name.
fn table_component_change(world: &World, from: ArchetypeId, to: ArchetypeId) -> String {
    let (added, removed) = component_change(world, from, to, Some(StorageType::Table));
    format!(
        "added {added:?}, removed {removed:?} — give a graphical-only component \
         `#[component(storage = \"SparseSet\")]`"
    )
}

/// The class whose kit a visual component just put on `bearer` shows: the
/// caster of the bearer's aura that component draws, or the bearer itself for
/// its own heal cast. `"-"` for a component no one class owns (a flinch, a
/// death fall).
fn credited_class(world: &World, bearer: Entity, component: &str) -> Vec<String> {
    let class_of = |e: Entity| world.get::<Combatant>(e).map(|c| format!("{:?}", c.class));
    let drawn = match component {
        "StunnedVisual" => AuraType::Stun,
        "RootedVisual" => AuraType::Root,
        "SlowTrailEmitter" => AuraType::MovementSpeedSlow,
        "FearedVisual" => AuraType::Fear,
        "PolymorphedVisual" => AuraType::Polymorph,
        "HealCastPosture" => return class_of(bearer).into_iter().collect(),
        _ => return vec!["-".into()],
    };
    let casters: BTreeSet<String> = world
        .get::<ActiveAuras>(bearer)
        .map(|a| {
            a.auras
                .iter()
                .filter(|a| a.effect_type == drawn)
                .filter_map(|a| a.caster.and_then(class_of))
                .collect()
        })
        .unwrap_or_default();
    casters.into_iter().collect()
}

/// `a::b::HitFlinch` -> `HitFlinch`.
fn short_name(path: &str) -> String {
    path.rsplit("::").next().unwrap_or(path).to_string()
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
    let watch = std::mem::take(&mut *app.world_mut().resource_mut::<TableWatch>());
    assert!(
        watch.checked_off_tick > 0 && watch.moves_on_tick > 0,
        "{cfg}: the table watch is vacuous — {} off-tick comparisons, {} on-tick table changes",
        watch.checked_off_tick,
        watch.moves_on_tick
    );
    let moves = watch.moves;
    assert!(
        moves.is_empty(),
        "{cfg}: {} combatant table change(s) off the fixed tick — each one reorders the \
         combatants every sim loop visits, by frame rate. The first:\n{}",
        moves.len(),
        moves[..moves.len().min(8)].join("\n")
    );
    let mut ticks = std::mem::take(&mut *app.world_mut().resource_mut::<Ticks>());
    ticks.inserted_off_tick = watch.inserted_off_tick;
    ticks
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
    // Combatant ORDER, tick for tick: every sim loop iterates combatants in
    // query order, so an order that followed the frame clock would reorder
    // their RNG draws and same-tick log lines. Checked first, because it is
    // the cause of the differences the checks below would only show later.
    if let Some(tick) = (0..common).find(|&i| a.orders[i] != b.orders[i]) {
        panic!(
            "{cfg}: combatants are visited in a different order at tick {} (decided at \
             {decided}) between {a_label} and {b_label} — a component inserted on or removed \
             from a combatant on the frame clock moved it between tables",
            tick + 1
        );
    }
    assert_eq!(
        a.results, b.results,
        "{cfg}: the Results screen differs between {a_label} and {b_label}"
    );
    let (a_text, b_text) = (
        a.report.as_deref().expect("report"),
        b.report.as_deref().expect("report"),
    );
    if a_text != b_text {
        let line = a_text
            .lines()
            .zip(b_text.lines())
            .position(|(x, y)| x != y)
            .unwrap_or(0);
        panic!(
            "{cfg}: the saved match report differs between {a_label} and {b_label} at line {}:\n  {}\n  {}",
            line + 1,
            a_text.lines().nth(line).unwrap_or(""),
            b_text.lines().nth(line).unwrap_or(""),
        );
    }
    if let Some(tick) = (0..common).find(|&i| a.hashes[i] != b.hashes[i]) {
        panic!(
            "{cfg}: sim state differs at tick {} (decided at {decided}) between {a_label} and {b_label}",
            tick + 1
        );
    }
    // Positions too, through the celebration: the victory choreography runs on
    // the sim clock, so the winners walk the same path at any display rate.
    if let Some(tick) = (0..common).find(|&i| a.poses[i] != b.poses[i]) {
        panic!(
            "{cfg}: combatant positions differ at tick {} (decided at {decided}) between {a_label} and {b_label}",
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
    for (label, ticks) in &schedules {
        assert_listed_in_slot_order(cfg, label, ticks);
    }
}

/// The Results rows and the saved report list each team in slot order.
///
/// Agreement between two schedules is not enough on its own: ECS query order
/// is whatever the archetype history left, which can coincide at two frame
/// rates and still be wrong — and differ at a third. The order is pinned to
/// (team, slot), the one order nothing on the frame clock can move.
fn assert_listed_in_slot_order(cfg: &str, label: &str, ticks: &Ticks) {
    let results = ticks.results.as_deref().expect("results");
    let report = ticks.report.as_deref().expect("report");
    let mut listed: Vec<(u32, u32, String)> = Vec::new();
    for line in results.lines().filter(|l| l.starts_with("team ")) {
        let words: Vec<&str> = line.split(' ').collect();
        listed.push((
            words[1].parse().unwrap(),
            words[3].parse().unwrap(),
            words[4].to_string(),
        ));
    }
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(
        listed, sorted,
        "{cfg}: under {label} the Results rows are not in (team, slot) order:\n{results}"
    );
    // The report numbers its combatants 1..n within each team, in the same
    // order as the Results rows.
    let report_classes: Vec<&str> = report
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("Slot "))
        .map(|l| l.split(' ').nth(1).unwrap())
        .collect();
    let results_classes: Vec<&str> = listed.iter().map(|(_, _, c)| c.as_str()).collect();
    assert_eq!(
        report_classes, results_classes,
        "{cfg}: under {label} the saved report lists combatants in a different order"
    );
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

/// A match whose combatants change archetype on the frame clock (hit flinches,
/// a Charge trail), so its query order at the deciding tick is not slot order.
/// This is where the listing bug was found: built in query order, one display
/// showed "Warrior, Priest" and a 25Hz one "Priest, Warrior", with every
/// number identical. The current sim plays this seed differently, and query
/// order now puts the Priest first at every schedule here. That is exactly the
/// case where agreement between schedules proves nothing, and why
/// `assert_listed_in_slot_order` checks the order itself.
const QUERY_ORDER_SENSITIVE: &str = r#"{"team1":["Warrior","Priest"],"team2":["Hunter","Mage"],"map":"TwinPillars","random_seed":7}"#;

#[test]
fn results_and_report_do_not_depend_on_the_display_rate() {
    let schedules = vec![
        (
            "16667us frames".to_string(),
            run(QUERY_ORDER_SENSITIVE, fixed_rate(16_667)),
        ),
        (
            "40000us frames".to_string(),
            run(QUERY_ORDER_SENSITIVE, fixed_rate(40_000)),
        ),
    ];
    // Non-vacuity: both teams listed more than one combatant, so an order
    // existed to get wrong.
    let results = schedules[0].1.results.as_deref().expect("results");
    assert_eq!(results.matches("team 1 slot").count(), 2, "{results}");
    assert_eq!(results.matches("team 2 slot").count(), 2, "{results}");
    assert_frame_rate_independent(QUERY_ORDER_SENSITIVE, schedules);
}

/// A match that exercises the frame-clock visuals of the four classes the
/// other two matches do not field, so the table watch sees them land on
/// combatants: the Rogue's stun and Crippling Poison slow, the Druid's
/// Entangling Roots, the Shaman's Frost Shock slow and the Paladin's heal-cast
/// posture.
const KIT_COVERAGE: &str = r#"{"team1":["Druid","Paladin"],"team2":["Rogue","Shaman"],"map":"BasicArena","random_seed":0}"#;

#[test]
fn rogue_druid_shaman_and_paladin_visuals_stay_off_the_tables() {
    let schedules = vec![
        (
            "16667us frames".to_string(),
            run(KIT_COVERAGE, fixed_rate(16_667)),
        ),
        (
            "40000us frames".to_string(),
            run(KIT_COVERAGE, fixed_rate(40_000)),
        ),
    ];
    // Non-vacuity: under every schedule, the frame clock put a visual
    // component on a combatant for each of the four classes — credited to the
    // class by the aura it draws, or by its own heal cast (`credited_class`).
    for (label, ticks) in &schedules {
        let credited: BTreeSet<&str> = ticks
            .inserted_off_tick
            .iter()
            .map(|(_, class)| class.as_str())
            .collect();
        for class in ["Rogue", "Druid", "Shaman", "Paladin"] {
            assert!(
                credited.contains(class),
                "{KIT_COVERAGE}: under {label} no frame-clock visual was credited to the \
                 {class}, so the watch never saw its kit land: {:?}",
                ticks.inserted_off_tick
            );
        }
    }
    assert_frame_rate_independent(KIT_COVERAGE, schedules);
}

/// The wider sweep: more matchups, maps and seeds, and the frame schedules a
/// player can actually produce — 25Hz, and irregular frames under pause,
/// fast-forward and slow-motion. Minutes of simulation, so opt-in:
/// `cargo test --release --test frame_rate_determinism -- --ignored`
#[test]
#[ignore]
fn sweep_matchups_and_schedules() {
    const MATCHES: [&str; 9] = [
        FOUND_ON,
        r#"{"team1":["Warrior"],"team2":["Hunter"],"map":"BasicArena","random_seed":3}"#,
        r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#,
        r#"{"team1":["Warlock"],"team2":["Priest"],"map":"TwinPillars","random_seed":5}"#,
        QUERY_ORDER_SENSITIVE,
        r#"{"team1":["Warlock","Rogue"],"team2":["Hunter","Paladin"],"map":"PillaredArena","random_seed":3}"#,
        r#"{"team1":["Rogue","Shaman"],"team2":["Warlock","Priest"],"map":"PillaredArena","random_seed":13}"#,
        r#"{"team1":["Warrior","Priest","Mage"],"team2":["Hunter","Warlock","Shaman"],"map":"TwinPillars","random_seed":21}"#,
        KIT_COVERAGE,
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
