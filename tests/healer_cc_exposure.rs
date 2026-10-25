//! Healer crowd-control exposure — a MEASUREMENT a human reads, not an
//! assertion (AS-362).
//!
//! The question AS-362 starts from: how much of a healer's match does it spend
//! crowd-controlled, by which crowd control, and how much of that was a matter
//! of WHERE it stood? This runs a JSONL of ordinary `HeadlessMatchConfig`s (the
//! `--batch` input format) through the observed runner and writes one JSON row
//! per healer per match:
//!
//! - `cc_secs` — seconds the healer carried each crowd-control aura type, read
//!   off the frame observer (so a fear that broke on damage counts only for as
//!   long as it held);
//! - `cc_events` — every `[CC]` log line that landed on the healer, with the
//!   ability, its DR-scaled duration, and where its likely source (the nearest
//!   living enemy of the ability's class) stood at that moment;
//! - `exposed_secs` — seconds the healer stood inside the REACH of each enemy
//!   crowd-control ability (its range from `AbilityDefinitions`, and line of
//!   sight on a map with pillars), whatever its cooldown. This is the quantity
//!   a positioning rule could change; `cc_secs` is the cost;
//! - heal delivered to teammates, distance kept from enemy casters, and heading
//!   reversals per minute (AS-249's strobe metric), so a rule that buys CC time
//!   by giving up heals or by bobbing shows up in the same row;
//! - `blind_secs` and `occluded_secs` (AS-381) — the two sides of the solve's
//!   sight-against-cover trade: seconds the healer could not see the teammate
//!   the `TeamPlan` solve anchors on, and seconds no living enemy caster could
//!   see the healer, with `exposure` (caster-seconds of sight on the healer)
//!   between them, and each again over the frames with five or more enemy
//!   casters alive (`*_ge5_secs`), where a flat sight weight lost to cover.
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`), so a
//! row's winner and duration are the batch runner's for the same config.
//! Aggregation: `docs/design/balance/sweeps/2026-10-18-as362-reduce.py`.
//!
//! ```bash
//! CCX_IN=bank.jsonl CCX_OUT=rows.jsonl CCX_LOGS=/tmp/logs CCX_JOBS=6 \
//!   cargo test --release --test healer_cc_exposure -- --ignored --nocapture
//! ```
//!
//! With `CCX_KEEP_LOGS=1` every match log stays in `CCX_LOGS` (named by the
//! config's line), so two binaries' runs of one bank diff log for log.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig, ObservedCombatant};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{AuraType, PetType};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::team_solve::pet_is_melee;
use arenasim::{ArenaMap, CharacterClass};
use bevy::prelude::{Entity, Vec2, Vec3};
use serde::Serialize;

/// The countdown before the gates; `sim_time` includes it.
const COUNTDOWN_SECS: f32 = 10.0;
const FRAME: f32 = 1.0 / 60.0;
/// AS-249's reversal metric: a step this long, turning back within 45 degrees
/// of the reverse of the previous one.
const REVERSAL_STEP_SECS: f32 = 0.5;
const REVERSAL_MIN_STEP: f32 = 0.1;

/// The aura types that take control away from a unit. `SpellSchoolLockout` and
/// `Silence` stop casting only; `Root` stops movement only.
const CC_TYPES: [AuraType; 8] = [
    AuraType::Stun,
    AuraType::Fear,
    AuraType::Polymorph,
    AuraType::Incapacitate,
    AuraType::Cyclone,
    AuraType::Root,
    AuraType::SpellSchoolLockout,
    AuraType::Silence,
];

/// The aura types that make a unit unable to act at all.
fn is_hard(a: AuraType) -> bool {
    matches!(
        a,
        AuraType::Stun
            | AuraType::Fear
            | AuraType::Polymorph
            | AuraType::Incapacitate
            | AuraType::Cyclone
    )
}

/// One enemy crowd-control ability a unit of `class` owns, as the config
/// declares it: its display name, reach and aura type.
#[derive(Clone)]
struct CcTool {
    name: String,
    class: CharacterClass,
    range: f32,
    aura: AuraType,
}

/// Every crowd-control ability a non-pet unit casts, read from the config —
/// never a hardcoded list, so a new one is measured with no edit here.
fn cc_tools(defs: &AbilityDefinitions) -> Vec<CcTool> {
    let mut out = Vec::new();
    for class in CharacterClass::all() {
        for a in defs.own_abilities_for_class(*class) {
            let c = defs.get_unchecked(&a);
            if let Some(aura) = c.applies_aura.as_ref().map(|x| x.aura_type) {
                if is_hard(aura) || aura == AuraType::Root {
                    out.push(CcTool {
                        name: c.name.clone(),
                        class: *class,
                        range: c.range,
                        aura,
                    });
                }
            }
        }
    }
    out
}

#[derive(Serialize, Default, Clone)]
struct CcEvent {
    /// Seconds after the gates.
    t: f32,
    ability: String,
    /// The DR-scaled duration the log states (an upper bound: some break).
    dur: f32,
    /// The ability's owning class.
    src_class: String,
    /// Yards from the healer to the nearest living enemy of that class.
    src_dist: Option<f32>,
    /// Yards from the healer to its nearest living non-pet teammate.
    ally_dist: Option<f32>,
    /// Yards from the source to that teammate.
    src_ally_dist: Option<f32>,
}

#[derive(Serialize, Default)]
struct HealerRow {
    label: String,
    map: String,
    profile: String,
    /// The healer's own team, then the enemy team, `+`-joined.
    team: String,
    enemy: String,
    class: String,
    side: u8,
    seed: u64,
    /// "win" / "loss" / "draw" from this healer's side.
    result: String,
    duration: f32,
    alive_secs: f32,
    died: bool,
    cc_secs: BTreeMap<String, f32>,
    hard_cc_secs: f32,
    cc_events: Vec<CcEvent>,
    /// Seconds within reach of each enemy CC ability (by ability name).
    exposed_secs: BTreeMap<String, f32>,
    /// Seconds within reach of at least one enemy hard-CC ability.
    exposed_any_hard_secs: f32,
    /// Health restored to living non-pet teammates (positive deltas).
    heal_to_allies: f32,
    /// Mean yards to the nearest living enemy non-melee non-pet unit.
    mean_caster_dist: Option<f32>,
    /// Mean yards to the nearest living non-pet teammate.
    mean_ally_dist: Option<f32>,
    /// Frames out of `heal_range` (40yd) of every living teammate.
    out_of_heal_range_secs: f32,
    reversals: u32,
    /// Seconds the healer could not see its anchor — the `TeamPlan` solve's
    /// (`team_solve::anchor_ally_pos`): the most hurt living non-pet teammate
    /// below `URGENCY_HP`, else the nearest. Only frames with a teammate alive.
    blind_secs: f32,
    /// Seconds no living enemy caster could see the healer, among frames with
    /// one alive. Casters as `team_solve::enemy_casters_of` counts them
    /// (`team_solve::is_ranged_threat`): every non-melee class, and every pet
    /// whose abilities reach — the Felhunter and the Spider, not the Boar or
    /// the Bird.
    occluded_secs: f32,
    /// Caster-seconds of sight on the healer: the count of living enemy
    /// casters that see it, summed over frames.
    exposure: f32,
    /// Seconds with five or more enemy casters alive, and the two measures
    /// above over those seconds only.
    casters_ge5_secs: f32,
    blind_ge5_secs: f32,
    occluded_ge5_secs: f32,
}

/// `movement.ron`'s `urgency_hp_threshold`, the solve's dying-teammate bar.
const URGENCY_HP: f32 = 0.5;

#[derive(Clone, Copy)]
struct Seen {
    pos: Vec2,
    team: u8,
    class: CharacterClass,
    alive: bool,
}

#[derive(Default)]
struct Track {
    prev_ally_hp: BTreeMap<Entity, f32>,
    last_sample: Option<(f32, Vec2)>,
    last_step: Option<Vec2>,
    caster_sum: f32,
    caster_n: u32,
    ally_sum: f32,
    ally_n: u32,
}

/// Whether `o` is a caster for the sight measures, as
/// `team_solve::is_ranged_threat` judges one: a non-melee class, or a pet
/// whose abilities reach (`team_solve::pet_is_melee`).
fn counts_as_caster(o: &ObservedCombatant) -> bool {
    if o.is_pet {
        !o.pet_type.is_none_or(pet_is_melee)
    } else {
        !o.class.is_melee()
    }
}

/// The caster filter counts the Felhunter and the Spider, as the solve does,
/// and not the Boar or the Bird, which report their Hunter's class.
#[test]
fn the_caster_filter_counts_only_pets_that_reach() {
    let unit = |class: CharacterClass, pet_type: Option<PetType>| ObservedCombatant {
        position: Vec3::ZERO,
        team: 2,
        slot: 0,
        class,
        is_pet: pet_type.is_some(),
        pet_type,
        alive: true,
        current_health: 1.0,
        max_health: 1.0,
        current_mana: 0.0,
        max_mana: 0.0,
        aura_types: Vec::new(),
        target: None,
        stealthed: false,
        point_goal: None,
    };
    use CharacterClass::{Hunter, Mage, Warlock, Warrior};
    assert!(counts_as_caster(&unit(Mage, None)));
    assert!(!counts_as_caster(&unit(Warrior, None)));
    assert!(counts_as_caster(&unit(Warlock, Some(PetType::Felhunter))));
    assert!(counts_as_caster(&unit(Hunter, Some(PetType::Spider))));
    assert!(!counts_as_caster(&unit(Hunter, Some(PetType::Boar))));
    assert!(!counts_as_caster(&unit(Hunter, Some(PetType::Bird))));
}

fn log_name(team: u8, slot: u8, class: CharacterClass) -> String {
    format!("Team {team} {class:?} #{}", slot + 1)
}

fn parse_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

fn run_one(cfg: HeadlessMatchConfig, tools: &[CcTool], log_path: String) -> Vec<HealerRow> {
    let mut cfg = cfg;
    cfg.output_path = Some(log_path.clone());
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    // Per team: a head-to-head config sets the two teams apart.
    let profiles = [
        cfg.team1_ai_profile.clone().or(cfg.ai_profile.clone()),
        cfg.team2_ai_profile.clone().or(cfg.ai_profile.clone()),
    ]
    .map(|p| p.unwrap_or_else(|| "Legacy".to_string()));
    let teams = [cfg.team1.join("+"), cfg.team2.join("+")];
    let seed = cfg.random_seed.unwrap_or(0);
    let arena: ArenaMap = *ArenaMap::all()
        .iter()
        .find(|m| format!("{m:?}") == map)
        .expect("a map name");
    let volumes: Vec<ObstacleVolume> = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(arena)
        .volumes;
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
    let sees = |a: Vec2, b: Vec2| volumes.is_empty() || has_line_of_sight(&volumes, eye(a), eye(b));

    let mut rows: BTreeMap<Entity, HealerRow> = BTreeMap::new();
    let mut tracks: BTreeMap<Entity, Track> = BTreeMap::new();
    let mut names: BTreeMap<Entity, String> = BTreeMap::new();
    // Positions of every non-pet unit at every post-gate frame, for the log join.
    let mut history: Vec<(f32, BTreeMap<Entity, Seen>)> = Vec::new();

    let result = run_headless_match_observed(cfg, false, None, |frame| {
        if !frame.gates_open {
            return;
        }
        let t = frame.sim_time - COUNTDOWN_SECS;
        let seen: BTreeMap<Entity, Seen> = frame
            .combatants
            .iter()
            .filter(|(_, c)| !c.is_pet)
            .map(|(e, c)| {
                (
                    *e,
                    Seen {
                        pos: Vec2::new(c.position.x, c.position.z),
                        team: c.team,
                        class: c.class,
                        alive: c.alive,
                    },
                )
            })
            .collect();
        for (e, c) in frame.combatants.iter().filter(|(_, c)| !c.is_pet) {
            names
                .entry(*e)
                .or_insert_with(|| log_name(c.team, c.slot, c.class));
            if !c.class.is_healer() {
                continue;
            }
            let row = rows.entry(*e).or_insert_with(|| HealerRow {
                label: label.clone(),
                map: map.clone(),
                profile: profiles[(c.team - 1) as usize].clone(),
                team: teams[(c.team - 1) as usize].clone(),
                enemy: teams[(2 - c.team) as usize].clone(),
                class: format!("{:?}", c.class),
                side: c.team,
                seed,
                ..Default::default()
            });
            let tr = tracks.entry(*e).or_default();
            if !c.alive {
                row.died = true;
                continue;
            }
            row.alive_secs += FRAME;
            let me = Vec2::new(c.position.x, c.position.z);

            // Crowd control carried this frame.
            let mut hard = false;
            for ty in CC_TYPES {
                if c.aura_types.contains(&ty) {
                    *row.cc_secs.entry(format!("{ty:?}")).or_default() += FRAME;
                    hard |= is_hard(ty);
                }
            }
            if hard {
                row.hard_cc_secs += FRAME;
            }

            // Reach: every living enemy's CC tools whose range covers this spot.
            let mut any_hard = false;
            for other in seen.values() {
                if other.team == c.team || !other.alive {
                    continue;
                }
                let d = me.distance(other.pos);
                let mut los: Option<bool> = None;
                for tool in tools.iter().filter(|x| x.class == other.class) {
                    if d > tool.range {
                        continue;
                    }
                    let visible = *los.get_or_insert_with(|| sees(me, other.pos));
                    if !visible {
                        continue;
                    }
                    *row.exposed_secs.entry(tool.name.clone()).or_default() += FRAME;
                    any_hard |= is_hard(tool.aura);
                }
            }
            if any_hard {
                row.exposed_any_hard_secs += FRAME;
            }

            // Sight against cover (AS-381).
            let casters: Vec<Vec2> = frame
                .combatants
                .values()
                .filter(|o| o.team != c.team && o.alive && counts_as_caster(o))
                .map(|o| Vec2::new(o.position.x, o.position.z))
                .collect();
            let watching = casters.iter().filter(|p| sees(me, **p)).count();
            let ge5 = casters.len() >= 5;
            row.exposure += watching as f32 * FRAME;
            if ge5 {
                row.casters_ge5_secs += FRAME;
            }
            if !casters.is_empty() && watching == 0 {
                row.occluded_secs += FRAME;
                if ge5 {
                    row.occluded_ge5_secs += FRAME;
                }
            }
            let mates = || {
                frame
                    .combatants
                    .iter()
                    .filter(|(oe, o)| **oe != *e && o.team == c.team && o.alive && !o.is_pet)
                    .map(|(_, o)| {
                        (
                            Vec2::new(o.position.x, o.position.z),
                            o.current_health / o.max_health,
                        )
                    })
            };
            let dying = mates()
                .filter(|(_, hp)| *hp < URGENCY_HP)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(p, _)| p);
            let anchor = dying.or_else(|| {
                mates()
                    .map(|(p, _)| p)
                    .min_by(|a, b| me.distance(*a).total_cmp(&me.distance(*b)))
            });
            if anchor.is_some_and(|a| !sees(me, a)) {
                row.blind_secs += FRAME;
                if ge5 {
                    row.blind_ge5_secs += FRAME;
                }
            }

            // Distances kept.
            let caster = seen
                .values()
                .filter(|o| o.team != c.team && o.alive && !o.class.is_melee())
                .map(|o| me.distance(o.pos))
                .fold(None, |m: Option<f32>, d| Some(m.map_or(d, |m| m.min(d))));
            if let Some(d) = caster {
                tr.caster_sum += d;
                tr.caster_n += 1;
            }
            let ally = seen
                .iter()
                .filter(|(oe, o)| **oe != *e && o.team == c.team && o.alive)
                .map(|(_, o)| me.distance(o.pos))
                .fold(None, |m: Option<f32>, d| Some(m.map_or(d, |m| m.min(d))));
            if let Some(d) = ally {
                tr.ally_sum += d;
                tr.ally_n += 1;
                if d > 40.0 {
                    row.out_of_heal_range_secs += FRAME;
                }
            }

            // Heal delivered to teammates: positive health deltas. (Any heal,
            // whoever cast it — in a one-healer team that is this healer, and
            // regen does not raise health.)
            for (oe, oc) in frame.combatants.iter() {
                if oc.team != c.team || oc.is_pet || *oe == *e {
                    continue;
                }
                let hp = if oc.alive { oc.current_health } else { 0.0 };
                if let Some(p) = tr.prev_ally_hp.insert(*oe, hp) {
                    if hp > p && oc.alive {
                        row.heal_to_allies += hp - p;
                    }
                }
            }

            // Heading reversals.
            match tr.last_sample {
                None => tr.last_sample = Some((t, me)),
                Some((t0, p0)) if t - t0 >= REVERSAL_STEP_SECS - 1e-4 => {
                    let step = me - p0;
                    if step.length() >= REVERSAL_MIN_STEP {
                        if let Some(prev) = tr.last_step {
                            let cos = step.normalize().dot(-prev.normalize());
                            if cos >= std::f32::consts::FRAC_1_SQRT_2 {
                                row.reversals += 1;
                            }
                        }
                        tr.last_step = Some(step);
                    }
                    tr.last_sample = Some((t, me));
                }
                _ => {}
            }
        }
        history.push((t, seen));
    })
    .expect("match runs");

    for (e, row) in rows.iter_mut() {
        let tr = &tracks[e];
        row.mean_caster_dist = (tr.caster_n > 0).then(|| tr.caster_sum / tr.caster_n as f32);
        row.mean_ally_dist = (tr.ally_n > 0).then(|| tr.ally_sum / tr.ally_n as f32);
        row.duration = result.match_time;
        row.result = match result.winner {
            Some(w) if w == row.side => "win".into(),
            Some(_) => "loss".into(),
            None => "draw".into(),
        };
    }

    // The log join: `[CC] <Ability> on <Team N Class #k> (<dur>s, DR: ...)`.
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    let by_name: BTreeMap<&str, &CcTool> = tools.iter().map(|x| (x.name.as_str(), x)).collect();
    for line in log.lines() {
        let Some(i) = line.find("] [CC] ") else {
            continue;
        };
        let body = &line[i + 7..];
        let Some(on) = body.find(" on ") else {
            continue;
        };
        let ability = &body[..on];
        let rest = &body[on + 4..];
        let Some(paren) = rest.find(" (") else {
            continue;
        };
        let target = &rest[..paren];
        let dur: f32 = rest[paren + 2..]
            .split('s')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let Some((healer, _)) = names
            .iter()
            .find(|(e, n)| n.as_str() == target && rows.contains_key(e))
        else {
            continue;
        };
        let healer = *healer;
        let Some(t) = parse_time(line).map(|t| t - COUNTDOWN_SECS) else {
            continue;
        };
        let src_class = by_name.get(ability).map(|x| x.class);
        let frame = history
            .iter()
            .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap())
            .map(|(_, s)| s);
        let mut ev = CcEvent {
            t,
            ability: ability.to_string(),
            dur,
            src_class: src_class.map_or("?".into(), |c| format!("{c:?}")),
            ..Default::default()
        };
        if let Some(seen) = frame {
            let me = seen[&healer];
            let src = src_class.and_then(|cls| {
                seen.values()
                    .filter(|o| o.team != me.team && o.alive && o.class == cls)
                    .min_by(|a, b| {
                        me.pos
                            .distance(a.pos)
                            .partial_cmp(&me.pos.distance(b.pos))
                            .unwrap()
                    })
                    .map(|o| o.pos)
            });
            let ally = seen
                .iter()
                .filter(|(oe, o)| **oe != healer && o.team == me.team && o.alive)
                .map(|(_, o)| o.pos)
                .min_by(|a, b| {
                    me.pos
                        .distance(*a)
                        .partial_cmp(&me.pos.distance(*b))
                        .unwrap()
                });
            ev.src_dist = src.map(|s| me.pos.distance(s));
            ev.ally_dist = ally.map(|a| me.pos.distance(a));
            ev.src_ally_dist = src.zip(ally).map(|(s, a)| s.distance(a));
        }
        rows.get_mut(&healer).unwrap().cc_events.push(ev);
    }
    // `CCX_KEEP_LOGS` keeps every match log, so two binaries' runs of one
    // bank can be diffed log for log (AS-362's `Legacy` byte-identity check).
    if std::env::var_os("CCX_KEEP_LOGS").is_none() {
        let _ = std::fs::remove_file(&log_path);
    }
    rows.into_values().collect()
}

#[test]
#[ignore = "a measurement a human reads; run with CCX_IN / CCX_OUT / CCX_LOGS set"]
fn healer_cc_exposure() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("CCX_IN").expect("CCX_IN: the JSONL of match configs");
    let output = std::env::var("CCX_OUT").expect("CCX_OUT: where the rows go");
    let logs = std::env::var("CCX_LOGS").expect("CCX_LOGS: a directory for match logs");
    let jobs: usize = std::env::var("CCX_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    std::fs::create_dir_all(&logs).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open CCX_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let defs = Arc::new(AbilityDefinitions::default());
    let tools = Arc::new(cc_tools(&defs));
    for t in tools.iter() {
        println!(
            "tool {:?} {} {:.1}yd {:?}",
            t.class, t.name, t.range, t.aura
        );
    }
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, tools, cursor, rows, logs) = (
                Arc::clone(&configs),
                Arc::clone(&tools),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                logs.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let out = run_one(
                    configs[i].clone(),
                    &tools,
                    format!("{logs}/match_{i:06}.txt"),
                );
                let text = out
                    .iter()
                    .map(|r| serde_json::to_string(r).unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                rows.lock().unwrap()[i] = Some(text);
                if (i + 1) % 200 == 0 {
                    eprintln!("{} / {}", i + 1, total);
                }
            });
        }
    });

    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    for row in rows.lock().unwrap().iter() {
        let text = row.as_ref().expect("every match ran");
        if !text.is_empty() {
            writeln!(out, "{text}").unwrap();
        }
    }
    println!("{total} matches -> {output}");
}
