//! Healer pillar play — a MEASUREMENT a human reads, not an assertion (AS-373).
//!
//! AS-373 asks whether healers play a pillar the way a WoW healer does: either
//! HUG it (stay loosely strapped to it, so following a teammate round it is a
//! short walk) or play in the OPEN (follow the teammate in sight, nearer the
//! enemy and exposed to its crowd control). Before proposing modes it measures
//! what the two AI profiles do today. This runs a JSONL of ordinary
//! `HeadlessMatchConfig`s (the `--batch` input format) through the observed
//! runner, with the decision trace on (for the healer's posture), and writes
//! one JSON row per healer per match:
//!
//! - `occ` — per-frame occupancy keyed `phase|posture|bin`: alive seconds, and
//!   within them hard-CC'd seconds, seconds in reach of an enemy hard CC (range
//!   and sight, readiness ignored), seconds blind to the most injured teammate,
//!   seconds a teammate was dying (below 50%) and blind to a dying teammate,
//!   and seconds a teammate stood near a pillar. `bin` is the healer's distance
//!   from the SKIN of the nearest pillar (its footprint disc), `phase` is
//!   `pre` until the first health loss of the match and `post` after;
//! - `cc` — each hard-CC onset on the healer, with where the healer stood;
//! - `orbits` — each quarter turn the healer made round one pillar while within
//!   `ORBIT_BAND` of its skin: the path walked and the mean skin distance (the
//!   model's claim is that a side switch costs about `radius x angle`);
//! - `blind` — each episode in which a pillar hid a teammate from the healer,
//!   both alive: how long, how far each walked, who broke sight, how it ended;
//! - `deaths` — each teammate death with the healer alive, and how much of the
//!   last 5s the teammate spent hidden from its healer;
//! - `modes` — the post-contact occupancy again, keyed `posture|mode` by the
//!   healer's TRACED pillar mode (`hug` / `open`, from its `PillarModeEnter` /
//!   `PillarModeLeave` decisions; every frame `open` on a build without the
//!   modes), with `mode_changes` and `mode_dwells` (seconds each mode was held),
//!   and each blind episode's `mode` as sight was lost (AS-373's build).
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`).
//! Aggregation: `docs/design/balance/sweeps/2026-10-19-as373-reduce.py`.
//!
//! ```bash
//! HPM_IN=bank.jsonl HPM_OUT=rows.jsonl HPM_TMP=/tmp/hpm HPM_JOBS=4 \
//!   cargo test --release --test healer_pillar_modes -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::{ArenaMap, CharacterClass};
use bevy::prelude::{Entity, Vec2, Vec3};
use serde::Serialize;

const FRAME: f32 = 1.0 / 60.0;
/// `urgency_hp_threshold`.
const DYING: f32 = 0.5;
/// A teammate this close to a pillar's skin counts as fighting at it.
const MATE_NEAR: f32 = 6.0;
/// Orbits are tracked while the healer is within this distance of the skin.
const ORBIT_BAND: f32 = 15.0;
/// "Who broke sight" compares positions this many frames apart.
const BREAK_LOOKBACK: usize = 30;
/// A death's look-back window.
const DEATH_WINDOW: usize = 300;
const HEAL_RANGE: f32 = 40.0;

fn bin(skin: f32) -> &'static str {
    match skin {
        s if s < 2.0 => "00-02",
        s if s < 4.0 => "02-04",
        s if s < 6.0 => "04-06",
        s if s < 10.0 => "06-10",
        s if s < 15.0 => "10-15",
        s if s < 25.0 => "15-25",
        _ => "25+",
    }
}

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

#[derive(Clone)]
struct CcTool {
    class: CharacterClass,
    range: f32,
}

fn cc_tools(defs: &AbilityDefinitions) -> Vec<CcTool> {
    let mut out = Vec::new();
    for class in CharacterClass::all() {
        for a in defs.own_abilities_for_class(*class) {
            let c = defs.get_unchecked(&a);
            if c.applies_aura
                .as_ref()
                .is_some_and(|x| is_hard(x.aura_type))
            {
                out.push(CcTool {
                    class: *class,
                    range: c.range,
                });
            }
        }
    }
    out
}

#[derive(Clone, Copy)]
struct Unit {
    pos: Vec2,
    team: u8,
    class: CharacterClass,
    alive: bool,
    hp: f32,
    hard: bool,
}

#[derive(Serialize, Default, Clone)]
struct Occ {
    secs: f32,
    hard_cc: f32,
    exposed: f32,
    blind_anchor: f32,
    dying: f32,
    blind_dying: f32,
    mate_near: f32,
}

#[derive(Serialize)]
struct CcOnset {
    t: f32,
    skin: f32,
    posture: String,
}

#[derive(Serialize)]
struct Orbit {
    t: f32,
    secs: f32,
    path: f32,
    mean_skin: f32,
    posture: String,
}

#[derive(Serialize)]
struct Blind {
    t: f32,
    mate_class: String,
    secs: f32,
    healer_path: f32,
    mate_path: f32,
    healer_skin: f32,
    mate_skin: f32,
    /// "healer" / "mate" / "both" / "joint"
    broke: String,
    /// "regained" / "mate_died" / "healer_died" / "end"
    end: String,
    mate_hp: f32,
    mate_min_hp: f32,
    posture: String,
    healer_hard_cc_secs: f32,
    /// The healer's traced pillar mode as sight was lost: "hug" or "open".
    mode: String,
}

#[derive(Serialize)]
struct Death {
    t: f32,
    mate_class: String,
    occl_5s: f32,
    occl_3s: f32,
    final_blind_secs: f32,
    healer_cc_5s: f32,
    out_of_range_5s: f32,
    healer_skin: f32,
    mate_skin: f32,
    posture: String,
    final_broke: Option<String>,
}

#[derive(Serialize, Default)]
struct Row {
    label: String,
    map: String,
    profile: String,
    team: String,
    enemy: String,
    class: String,
    side: u8,
    seed: u64,
    result: String,
    duration: f32,
    occ: BTreeMap<String, Occ>,
    /// Post-contact occupancy keyed `posture|mode` by the healer's TRACED
    /// pillar mode (`PillarModeEnter` / `PillarModeLeave`, AS-373): `hug` or
    /// `open`. Every frame is `open` on a build without the modes.
    modes: BTreeMap<String, Occ>,
    /// The traced pillar-mode changes, and the seconds each mode was held
    /// before it changed (the dwell distribution).
    mode_changes: u32,
    mode_dwells: Vec<f32>,
    cc: Vec<CcOnset>,
    orbits: Vec<Orbit>,
    blind: Vec<Blind>,
    deaths: Vec<Death>,
}

fn run_one(cfg: HeadlessMatchConfig, tools: &[CcTool], tmp: &str, i: usize) -> Vec<Row> {
    let mut cfg = cfg;
    cfg.output_path = Some(format!("{tmp}/match_{i:06}.txt"));
    let trace_path = std::path::PathBuf::from(format!("{tmp}/trace_{i:06}.jsonl"));
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    let profile = cfg.ai_profile.clone().unwrap_or_else(|| "Legacy".into());
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
    let discs: Vec<(Vec2, f32)> = volumes.iter().map(|v| v.footprint_disc()).collect();
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
    let sees = |a: Vec2, b: Vec2| volumes.is_empty() || has_line_of_sight(&volumes, eye(a), eye(b));
    // (index of nearest pillar, distance from its skin)
    let nearest = |p: Vec2| -> (usize, f32) {
        discs
            .iter()
            .enumerate()
            .map(|(k, (c, r))| (k, p.distance(*c) - r))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((usize::MAX, f32::INFINITY))
    };

    // Every frame (pre-gate included, so the index is the trace's frame).
    let mut frames: Vec<(bool, BTreeMap<Entity, Unit>)> = Vec::new();
    let result = run_headless_match_observed(
        cfg,
        false,
        Some(TraceConfig {
            output_path: trace_path.clone(),
        }),
        |frame| {
            let units = frame
                .combatants
                .iter()
                .filter(|(_, c)| !c.is_pet)
                .map(|(e, c)| {
                    (
                        *e,
                        Unit {
                            pos: Vec2::new(c.position.x, c.position.z),
                            team: c.team,
                            class: c.class,
                            alive: c.alive,
                            hp: if c.max_health > 0.0 {
                                c.current_health / c.max_health
                            } else {
                                0.0
                            },
                            hard: c.aura_types.iter().any(|a| is_hard(*a)),
                        },
                    )
                })
                .collect();
            frames.push((frame.gates_open, units));
        },
    )
    .expect("match runs");

    // Posture timeline per healer, from the trace's movement decisions.
    let mut postures: BTreeMap<u32, Vec<(usize, String)>> = BTreeMap::new();
    // Pillar-mode timeline per healer: (frame, hugging from then on).
    let mut modes: BTreeMap<u32, Vec<(usize, bool)>> = BTreeMap::new();
    if let Ok(f) = std::fs::File::open(&trace_path) {
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            if !line.contains("\"movement_decision\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let (Some(id), Some(fr), Some(p)) = (
                v["actor"]["entity_id"].as_u64(),
                v["frame"].as_u64(),
                v["posture"].as_str(),
            ) else {
                continue;
            };
            postures
                .entry(id as u32)
                .or_default()
                .push((fr as usize, p.to_string()));
            match v["trigger"].as_str() {
                Some("PillarModeEnter") => modes
                    .entry(id as u32)
                    .or_default()
                    .push((fr as usize, true)),
                Some("PillarModeLeave") => modes
                    .entry(id as u32)
                    .or_default()
                    .push((fr as usize, false)),
                _ => {}
            }
        }
    }
    let mode_at = |e: Entity, i: usize| -> bool {
        let Some(v) = modes.get(&e.index()) else {
            return false;
        };
        let k = v.partition_point(|d| d.0 <= i);
        k > 0 && v[k - 1].1
    };
    let _ = std::fs::remove_file(&trace_path);
    let _ = std::fs::remove_file(format!("{tmp}/match_{i:06}.txt"));
    let posture_at = |e: Entity, i: usize| -> String {
        let Some(v) = postures.get(&e.index()) else {
            return "free".into();
        };
        let k = v.partition_point(|d| d.0 <= i);
        if k == 0 {
            "free".into()
        } else {
            v[k - 1].1.clone()
        }
    };

    let Some(gates) = frames.iter().position(|f| f.0) else {
        return Vec::new();
    };
    let contact = (gates..frames.len())
        .find(|&i| frames[i].1.values().any(|u| u.hp < 0.999))
        .unwrap_or(frames.len());
    let t_of = |i: usize| (i - gates) as f32 * FRAME;

    let healers: Vec<Entity> = frames[gates]
        .1
        .iter()
        .filter(|(_, u)| u.class.is_healer())
        .map(|(e, _)| *e)
        .collect();

    let mut rows = Vec::new();
    for h in healers {
        let me0 = frames[gates].1[&h];
        let mates: Vec<Entity> = frames[gates]
            .1
            .iter()
            .filter(|(e, u)| **e != h && u.team == me0.team)
            .map(|(e, _)| *e)
            .collect();
        let mut row = Row {
            label: label.clone(),
            map: map.clone(),
            profile: profile.clone(),
            team: teams[(me0.team - 1) as usize].clone(),
            enemy: teams[(2 - me0.team) as usize].clone(),
            class: format!("{:?}", me0.class),
            side: me0.team,
            seed,
            duration: result.match_time,
            result: match result.winner {
                Some(w) if w == me0.team => "win".into(),
                Some(_) => "loss".into(),
                None => "draw".into(),
            },
            ..Default::default()
        };
        if let Some(v) = modes.get(&h.index()) {
            row.mode_changes = v.len() as u32;
            row.mode_dwells = v
                .windows(2)
                .map(|w| (w[1].0 - w[0].0) as f32 * FRAME)
                .collect();
        }
        // Per-frame sight of each teammate (both alive), and the orbit state.
        let mut sight: BTreeMap<Entity, Vec<Option<bool>>> = mates
            .iter()
            .map(|m| (*m, vec![None; frames.len()]))
            .collect();
        let mut was_hard = false;
        let mut orbit: Option<(usize, f32, f32, f32, f32, usize, Vec2)> = None; // pillar, last bearing, accum, path, skin_sum, start frame, last pos
        let mut orbit_n = 0u32;

        for i in gates..frames.len() {
            let units = &frames[i].1;
            let me = units[&h];
            if !me.alive {
                continue;
            }
            let (pk, skin) = nearest(me.pos);
            let posture = posture_at(h, i);
            let phase = if i < contact { "pre" } else { "post" };
            let key = format!("{phase}|{posture}|{}", bin(skin));
            let living: Vec<(Entity, Unit)> = mates
                .iter()
                .map(|m| (*m, units[m]))
                .filter(|(_, u)| u.alive)
                .collect();
            for (m, u) in &living {
                sight.get_mut(m).unwrap()[i] = Some(sees(me.pos, u.pos));
            }
            let anchor = living
                .iter()
                .min_by(|a, b| a.1.hp.total_cmp(&b.1.hp))
                .map(|(m, _)| *m);
            let blind_anchor = anchor.is_some_and(|m| sight[&m][i] == Some(false));
            let dying: Vec<&Entity> = living
                .iter()
                .filter(|(_, u)| u.hp < DYING)
                .map(|(m, _)| m)
                .collect();
            let blind_dying = dying.iter().any(|m| sight[*m][i] == Some(false));
            let mate_near = living.iter().any(|(_, u)| nearest(u.pos).1 <= MATE_NEAR);
            let exposed = units.values().any(|o| {
                o.team != me.team
                    && o.alive
                    && !o.hard
                    && tools
                        .iter()
                        .any(|t| t.class == o.class && me.pos.distance(o.pos) <= t.range)
                    && sees(me.pos, o.pos)
            });
            let acc = row.occ.entry(key).or_default();
            acc.secs += FRAME;
            if me.hard {
                acc.hard_cc += FRAME;
            }
            if exposed {
                acc.exposed += FRAME;
            }
            if blind_anchor {
                acc.blind_anchor += FRAME;
            }
            if !dying.is_empty() {
                acc.dying += FRAME;
                if blind_dying {
                    acc.blind_dying += FRAME;
                }
            }
            if mate_near {
                acc.mate_near += FRAME;
            }
            if i >= contact {
                let mode = if mode_at(h, i) { "hug" } else { "open" };
                let m = row.modes.entry(format!("{posture}|{mode}")).or_default();
                m.secs += FRAME;
                m.hard_cc += if me.hard { FRAME } else { 0.0 };
                m.exposed += if exposed { FRAME } else { 0.0 };
                m.blind_anchor += if blind_anchor { FRAME } else { 0.0 };
                if !dying.is_empty() {
                    m.dying += FRAME;
                    m.blind_dying += if blind_dying { FRAME } else { 0.0 };
                }
                m.mate_near += if mate_near { FRAME } else { 0.0 };
            }
            if me.hard && !was_hard {
                row.cc.push(CcOnset {
                    t: t_of(i),
                    skin,
                    posture: posture.clone(),
                });
            }
            was_hard = me.hard;

            // Orbits: the unwrapped bearing about the nearest pillar.
            if skin <= ORBIT_BAND && pk != usize::MAX {
                let c = discs[pk].0;
                let bearing = (me.pos - c).y.atan2((me.pos - c).x);
                match orbit.as_mut() {
                    Some(o) if o.0 == pk => {
                        let mut d = bearing - o.1;
                        while d > std::f32::consts::PI {
                            d -= std::f32::consts::TAU;
                        }
                        while d < -std::f32::consts::PI {
                            d += std::f32::consts::TAU;
                        }
                        o.1 = bearing;
                        o.2 += d;
                        o.3 += me.pos.distance(o.6);
                        o.4 += skin;
                        o.6 = me.pos;
                        orbit_n += 1;
                        if o.2.abs() >= std::f32::consts::FRAC_PI_2 {
                            row.orbits.push(Orbit {
                                t: t_of(o.5),
                                secs: (i - o.5) as f32 * FRAME,
                                path: o.3,
                                mean_skin: o.4 / orbit_n as f32,
                                posture: posture_at(h, o.5),
                            });
                            *o = (pk, bearing, 0.0, 0.0, 0.0, i, me.pos);
                            orbit_n = 0;
                        }
                    }
                    _ => {
                        orbit = Some((pk, bearing, 0.0, 0.0, 0.0, i, me.pos));
                        orbit_n = 0;
                    }
                }
            } else {
                orbit = None;
            }
        }

        // Blind episodes and deaths, per teammate.
        let path = |e: Entity, a: usize, b: usize| -> f32 {
            (a + 1..=b)
                .map(|k| frames[k].1[&e].pos.distance(frames[k - 1].1[&e].pos))
                .sum()
        };
        for m in &mates {
            let s = &sight[m];
            let mut i = gates;
            let mut episodes: Vec<(usize, usize, String)> = Vec::new(); // start, end, broke
            while i < frames.len() {
                if s[i] == Some(false) && (i == gates || s[i - 1] != Some(false)) {
                    let start = i;
                    let mut j = i;
                    while j + 1 < frames.len() && s[j + 1] == Some(false) {
                        j += 1;
                    }
                    let end_kind = if j + 1 >= frames.len() {
                        "end"
                    } else if !frames[j + 1].1[m].alive {
                        "mate_died"
                    } else if !frames[j + 1].1[&h].alive {
                        "healer_died"
                    } else {
                        "regained"
                    };
                    let back = start.saturating_sub(BREAK_LOOKBACK).max(gates);
                    let (h0, m0) = (frames[back].1[&h].pos, frames[back].1[m].pos);
                    let (h1, m1) = (frames[start].1[&h].pos, frames[start].1[m].pos);
                    let hb = !sees(h1, m0);
                    let mb = !sees(h0, m1);
                    let broke = match (hb, mb) {
                        (true, true) => "both",
                        (true, false) => "healer",
                        (false, true) => "mate",
                        (false, false) => "joint",
                    }
                    .to_string();
                    let mate_min_hp = (start..=j)
                        .map(|k| frames[k].1[m].hp)
                        .fold(1.0f32, f32::min);
                    let cc = (start..=j).filter(|&k| frames[k].1[&h].hard).count() as f32 * FRAME;
                    row.blind.push(Blind {
                        t: t_of(start),
                        mate_class: format!("{:?}", frames[start].1[m].class),
                        secs: (j + 1 - start) as f32 * FRAME,
                        healer_path: path(h, start, j),
                        mate_path: path(*m, start, j),
                        healer_skin: nearest(h1).1,
                        mate_skin: nearest(m1).1,
                        broke: broke.clone(),
                        end: end_kind.into(),
                        mate_hp: frames[start].1[m].hp,
                        mate_min_hp,
                        posture: posture_at(h, start),
                        healer_hard_cc_secs: cc,
                        mode: if mode_at(h, start) { "hug" } else { "open" }.into(),
                    });
                    episodes.push((start, j, broke));
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            // The death: the first frame the teammate is dead.
            let Some(d) = (gates..frames.len()).find(|&k| !frames[k].1[m].alive) else {
                continue;
            };
            if !frames[d].1[&h].alive || d == 0 {
                continue;
            }
            let last = d - 1;
            let w0 = last.saturating_sub(DEATH_WINDOW - 1).max(gates);
            let w3 = last.saturating_sub(180 - 1).max(gates);
            let frac = |a: usize, f: &dyn Fn(usize) -> bool| -> f32 {
                (a..=last).filter(|&k| f(k)).count() as f32 / (last + 1 - a) as f32
            };
            let occl = |k: usize| s[k] == Some(false);
            let mut fb = 0usize;
            while fb <= last - gates && s[last - fb] == Some(false) {
                fb += 1;
            }
            let final_broke = episodes
                .iter()
                .rev()
                .find(|(_, e, _)| *e == last)
                .map(|(_, _, b)| b.clone());
            let hp = frames[last].1[&h].pos;
            let mp = frames[last].1[m].pos;
            row.deaths.push(Death {
                t: t_of(d),
                mate_class: format!("{:?}", frames[last].1[m].class),
                occl_5s: frac(w0, &occl),
                occl_3s: frac(w3, &occl),
                final_blind_secs: fb as f32 * FRAME,
                healer_cc_5s: frac(w0, &|k| frames[k].1[&h].hard),
                out_of_range_5s: frac(w0, &|k| {
                    frames[k].1[&h].pos.distance(frames[k].1[m].pos) > HEAL_RANGE
                }),
                healer_skin: nearest(hp).1,
                mate_skin: nearest(mp).1,
                posture: posture_at(h, last),
                final_broke,
            });
        }
        rows.push(row);
    }
    rows
}

#[test]
#[ignore = "a measurement a human reads; run with HPM_IN / HPM_OUT / HPM_TMP set"]
fn healer_pillar_modes() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("HPM_IN").expect("HPM_IN: the JSONL of match configs");
    let output = std::env::var("HPM_OUT").expect("HPM_OUT: where the rows go");
    let tmp = std::env::var("HPM_TMP").expect("HPM_TMP: a scratch directory");
    let jobs: usize = std::env::var("HPM_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::fs::create_dir_all(&tmp).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open HPM_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let tools = Arc::new(cc_tools(&AbilityDefinitions::default()));
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, tools, cursor, rows, tmp) = (
                Arc::clone(&configs),
                Arc::clone(&tools),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                tmp.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let out = run_one(configs[i].clone(), &tools, &tmp, i);
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
