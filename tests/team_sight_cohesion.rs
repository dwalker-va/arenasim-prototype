//! Team sight cohesion and pillar stance — a MEASUREMENT a human reads, not
//! an assertion (AS-385).
//!
//! AS-385 asks whether a TEAM plays its pillars together: teammates keep line
//! of sight of one another, nobody lines the unit it should heal, help or
//! damage (breaking that sight is crowd-controlling yourself), and when a team
//! pillars it pillars as a team. AS-373's `healer_pillar_modes` measures the
//! healer's half; this measures every unit and the team. It runs a JSONL of
//! ordinary `HeadlessMatchConfig`s (the `--batch` input format) through the
//! observed runner, with the decision trace on (for the healers' posture and
//! traced pillar mode), and writes one JSON row per match:
//!
//! - `units` — per non-pet unit, post-contact occupancy while alive: hard-CC'd
//!   seconds; seconds HIDDEN (no living enemy ranged unit, pets included, sees
//!   it); seconds in reach of an enemy hard crowd control (range and sight,
//!   readiness ignored); seconds blind to its own target (an alive, visible
//!   enemy) and blind while within its reach; for a DPS on a team with a
//!   healer, seconds blind to that healer, while dying, and while IN DANGER
//!   (dying and targeted); for a healer, seconds blind to its anchor (the most
//!   injured teammate) and blind while that anchor is hurt. "Lining" seconds
//!   are the blind seconds the unit was free to act (not hard-CC'd);
//! - `teams` — per team, post-contact frame shares: how many of its living
//!   units are hidden (all / some / none, and which roles), whether every pair
//!   of living teammates sees each other (cohesion), and all of them within
//!   `AT_PILLAR` of a pillar's skin;
//! - `pairs` — each DPS-healer blind episode (a pillar between them, both
//!   alive) with who broke sight (whose step, as AS-373: positions
//!   `BREAK_LOOKBACK` frames apart), whether the DPS was dying or in danger as
//!   it broke, and the healer's posture and traced pillar mode;
//! - `lines` — each episode of a DPS blind to its own target (the same
//!   attribution: the DPS's step or the target's);
//! - `lulls` — each span of `LULL_MIN` seconds or more in which no non-pet
//!   unit on either side lost health, with both teams' health and mana at its
//!   ends: what a stall converts into, per side;
//! - `mana` — each healer's mana fraction at fixed times after the gates and
//!   its seconds below 10%;
//! - `deaths` — every non-pet death with its team's healer alive: how long it
//!   had been hidden from that healer, who broke the final sight, the healer's
//!   hard-CC share of the last 5s, and the dead unit's own target-blind share.
//!
//! The observer is read-only (`observed_run_does_not_perturb_outcomes`).
//! Aggregation: `docs/design/balance/sweeps/2026-10-24-as385-reduce.py`.
//!
//! ```bash
//! TSC_IN=bank.jsonl TSC_OUT=rows.jsonl TSC_TMP=/tmp/tsc TSC_JOBS=8 \
//!   cargo test --release --test team_sight_cohesion -- --ignored --nocapture
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
/// A healer's anchor below this is hurt enough that a heal is owed.
const HURT: f32 = 0.8;
/// "Who broke sight" compares positions this many frames apart (AS-373).
const BREAK_LOOKBACK: usize = 30;
/// A death's look-back window.
const DEATH_WINDOW: usize = 300;
/// A unit this close to a pillar's skin stands at it.
const AT_PILLAR: f32 = 6.0;
/// The shortest lull recorded, in seconds.
const LULL_MIN: f32 = 2.0;
/// The reach within which a blind DPS is lining a target it could act on.
const MELEE_REACH: f32 = 10.0;
const RANGED_REACH: f32 = 36.0;
/// Episodes shorter than this are not written (they are still in the
/// occupancy totals).
const EPISODE_MIN: f32 = 0.25;

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
    pet: bool,
    alive: bool,
    hp: f32,
    mana: f32,
    has_mana: bool,
    hard: bool,
    stealthed: bool,
    target: Option<Entity>,
}

#[derive(Serialize, Default, Clone)]
struct UnitOcc {
    class: String,
    team: u8,
    role: String,
    secs: f32,
    hard_cc: f32,
    hidden: f32,
    exposed_cc: f32,
    has_target: f32,
    blind_target: f32,
    in_reach: f32,
    blind_target_reach: f32,
    lining_target: f32,
    // DPS on a team with a living healer
    healer_secs: f32,
    blind_healer: f32,
    dying: f32,
    blind_healer_dying: f32,
    danger: f32,
    blind_healer_danger: f32,
    // healer
    anchor_secs: f32,
    blind_anchor: f32,
    hurt: f32,
    blind_anchor_hurt: f32,
    lining_anchor_hurt: f32,
}

#[derive(Serialize, Default, Clone)]
struct TeamOcc {
    team: u8,
    comp: String,
    has_healer: bool,
    secs: f32,
    all_hidden: f32,
    none_hidden: f32,
    healer_hidden_dps_exposed: f32,
    dps_hidden_healer_exposed: f32,
    cohesive: f32,
    all_hidden_cohesive: f32,
    all_at_pillar: f32,
    healer_at_pillar: f32,
    healer_at_pillar_dps_not: f32,
    healer_hug: f32,
    healer_hug_dps_hidden: f32,
    healer_hug_cohesive: f32,
}

#[derive(Serialize)]
struct PairEp {
    t: f32,
    team: u8,
    healer: String,
    dps: String,
    secs: f32,
    broke: String,
    end: String,
    dps_hp: f32,
    dps_dying: bool,
    dps_danger: bool,
    dps_min_hp: f32,
    healer_hard_secs: f32,
    healer_posture: String,
    healer_mode: String,
    dps_skin: f32,
    healer_skin: f32,
    dps_hidden: bool,
    dps_path: f32,
    healer_path: f32,
}

#[derive(Serialize)]
struct LineEp {
    t: f32,
    team: u8,
    class: String,
    target: String,
    secs: f32,
    broke: String,
    dist: f32,
    in_reach: bool,
    hp: f32,
    /// Another living, visible non-pet enemy stood within the DPS's reach
    /// and in its sight as the line began: a target it could have switched to.
    alt_in_sight: bool,
    /// The share of the episode's frames on which such an alternative stood.
    alt_share: f32,
}

#[derive(Serialize)]
struct Lull {
    t0: f32,
    secs: f32,
    /// [team1, team2]: team health fraction (sum over living non-pets of
    /// current / max, dead counting zero) at the start and the end.
    hp0: [f32; 2],
    hp1: [f32; 2],
    /// [team1, team2]: the healer's mana fraction (or -1 with none).
    hmana0: [f32; 2],
    hmana1: [f32; 2],
    /// [team1, team2]: the team's mean mana fraction over its mana users.
    mana0: [f32; 2],
    mana1: [f32; 2],
    /// Whether each team was entirely hidden for most of the lull.
    hidden_share: [f32; 2],
}

#[derive(Serialize)]
struct HealerMana {
    team: u8,
    class: String,
    at: BTreeMap<String, f32>,
    low_secs: f32,
    alive_secs: f32,
}

#[derive(Serialize)]
struct Death {
    t: f32,
    team: u8,
    class: String,
    healer: Option<String>,
    healer_alive: bool,
    hidden_from_healer_5s: f32,
    final_blind_secs: f32,
    final_broke: Option<String>,
    healer_hard_5s: f32,
    out_of_range_5s: f32,
    blind_target_5s: f32,
    hard_5s: f32,
    exposed_cc_5s: f32,
    healer_posture: String,
    healer_mode: String,
}

#[derive(Serialize, Default)]
struct Row {
    label: String,
    map: String,
    profile: String,
    team1: String,
    team2: String,
    seed: u64,
    winner: u8,
    duration: f32,
    contact: f32,
    units: Vec<UnitOcc>,
    teams: Vec<TeamOcc>,
    pairs: Vec<PairEp>,
    lines: Vec<LineEp>,
    lulls: Vec<Lull>,
    mana: Vec<HealerMana>,
    deaths: Vec<Death>,
}

fn broke_kind(a_moved_breaks: bool, b_moved_breaks: bool, a: &str, b: &str) -> String {
    match (a_moved_breaks, b_moved_breaks) {
        (true, true) => "both".into(),
        (true, false) => a.into(),
        (false, true) => b.into(),
        (false, false) => "joint".into(),
    }
}

fn run_one(cfg: HeadlessMatchConfig, tools: &[CcTool], tmp: &str, i: usize) -> Row {
    let mut cfg = cfg;
    cfg.output_path = Some(format!("{tmp}/match_{i:06}.txt"));
    let trace_path = std::path::PathBuf::from(format!("{tmp}/trace_{i:06}.jsonl"));
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    let profile = match (&cfg.team1_ai_profile, &cfg.team2_ai_profile) {
        (Some(a), Some(b)) => format!("{a}/{b}"),
        _ => cfg.ai_profile.clone().unwrap_or_else(|| "Legacy".into()),
    };
    let team_names = [cfg.team1.join("+"), cfg.team2.join("+")];
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
    let skin = |p: Vec2| -> f32 {
        discs
            .iter()
            .map(|(c, r)| p.distance(*c) - r)
            .fold(f32::INFINITY, f32::min)
    };

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
                .map(|(e, c)| {
                    (
                        *e,
                        Unit {
                            pos: Vec2::new(c.position.x, c.position.z),
                            team: c.team,
                            class: c.class,
                            pet: c.is_pet,
                            alive: c.alive,
                            hp: if c.max_health > 0.0 {
                                c.current_health / c.max_health
                            } else {
                                0.0
                            },
                            mana: if c.max_mana > 0.0 {
                                c.current_mana / c.max_mana
                            } else {
                                0.0
                            },
                            has_mana: c.max_mana > 0.0,
                            hard: c.aura_types.iter().any(|a| is_hard(*a)),
                            stealthed: c.stealthed,
                            target: c.target,
                        },
                    )
                })
                .collect();
            frames.push((frame.gates_open, units));
        },
    )
    .expect("match runs");

    // Healers' posture and traced pillar mode, from the trace.
    let mut postures: BTreeMap<u32, Vec<(usize, String)>> = BTreeMap::new();
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
                Some("PillarModeEnter") => {
                    modes.entry(id as u32).or_default().push((fr as usize, true))
                }
                Some("PillarModeLeave") => {
                    modes.entry(id as u32).or_default().push((fr as usize, false))
                }
                _ => {}
            }
        }
    }
    let _ = std::fs::remove_file(&trace_path);
    let _ = std::fs::remove_file(format!("{tmp}/match_{i:06}.txt"));
    let mode_at = |e: Entity, i: usize| -> bool {
        let Some(v) = modes.get(&e.index()) else {
            return false;
        };
        let k = v.partition_point(|d| d.0 <= i);
        k > 0 && v[k - 1].1
    };
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

    let mut row = Row {
        label,
        map,
        profile,
        team1: team_names[0].clone(),
        team2: team_names[1].clone(),
        seed,
        winner: result.winner.unwrap_or(0),
        duration: result.match_time,
        ..Default::default()
    };
    let Some(gates) = frames.iter().position(|f| f.0) else {
        return row;
    };
    let n = frames.len();
    let contact = (gates..n)
        .find(|&i| {
            frames[i]
                .1
                .values()
                .any(|u| !u.pet && u.hp < 0.999)
        })
        .unwrap_or(n);
    row.contact = (contact.min(n) - gates) as f32 * FRAME;
    let t_of = |i: usize| (i - gates) as f32 * FRAME;

    let people: Vec<Entity> = frames[gates]
        .1
        .iter()
        .filter(|(_, u)| !u.pet)
        .map(|(e, _)| *e)
        .collect();
    let healer_of: BTreeMap<u8, Entity> = people
        .iter()
        .filter(|e| frames[gates].1[*e].class.is_healer())
        .map(|e| (frames[gates].1[e].team, *e))
        .collect();
    let class_of = |e: &Entity| frames[gates].1[e].class;
    let team_of = |e: &Entity| frames[gates].1[e].team;

    // Per-frame per-unit facts (post-gate), reused by episodes and deaths.
    #[derive(Clone, Copy, Default)]
    struct Fact {
        hidden: bool,
        exposed: bool,
        blind_target: Option<bool>,
        blind_healer: Option<bool>,
        reach: bool,
    }
    let mut facts: BTreeMap<Entity, Vec<Fact>> =
        people.iter().map(|e| (*e, vec![Fact::default(); n])).collect();
    let mut occ: BTreeMap<Entity, UnitOcc> = people
        .iter()
        .map(|e| {
            let c = class_of(e);
            (
                *e,
                UnitOcc {
                    class: format!("{c:?}"),
                    team: team_of(e),
                    role: if c.is_healer() {
                        "healer"
                    } else if c.is_melee() {
                        "melee"
                    } else {
                        "ranged"
                    }
                    .into(),
                    ..Default::default()
                },
            )
        })
        .collect();
    let mut tocc: [TeamOcc; 2] = [1u8, 2u8].map(|t| TeamOcc {
        team: t,
        comp: team_names[(t - 1) as usize].clone(),
        has_healer: healer_of.contains_key(&t),
        ..Default::default()
    });

    let team_hp = |units: &BTreeMap<Entity, Unit>, t: u8| -> f32 {
        let v: Vec<&Unit> = units.values().filter(|u| !u.pet && u.team == t).collect();
        v.iter()
            .map(|u| if u.alive { u.hp } else { 0.0 })
            .sum::<f32>()
            / v.len().max(1) as f32
    };
    let team_mana = |units: &BTreeMap<Entity, Unit>, t: u8| -> f32 {
        let v: Vec<&Unit> = units
            .values()
            .filter(|u| !u.pet && u.team == t && u.alive && u.has_mana)
            .collect();
        if v.is_empty() {
            -1.0
        } else {
            v.iter().map(|u| u.mana).sum::<f32>() / v.len() as f32
        }
    };
    let healer_mana = |units: &BTreeMap<Entity, Unit>, t: u8| -> f32 {
        healer_of
            .get(&t)
            .map(|h| units[h])
            .filter(|u| u.alive)
            .map_or(-1.0, |u| u.mana)
    };

    let mut damage_frame = vec![false; n];
    let mut team_all_hidden = vec![[false; 2]; n];
    for i in gates..n {
        let units = &frames[i].1;
        if i > gates {
            let prev = &frames[i - 1].1;
            damage_frame[i] = people
                .iter()
                .any(|e| units[e].alive && units[e].hp + 1e-4 < prev[e].hp)
                || people.iter().any(|e| !units[e].alive && prev[e].alive);
        }
        // Enemy ranged watchers (pets included) per team.
        for e in &people {
            let me = units[e];
            if !me.alive {
                continue;
            }
            let hidden = !units.values().any(|o| {
                o.alive
                    && o.team != me.team
                    && (o.pet || !o.class.is_melee())
                    && !o.stealthed
                    && sees(o.pos, me.pos)
            });
            let exposed = units.values().any(|o| {
                o.team != me.team
                    && o.alive
                    && !o.pet
                    && !o.hard
                    && tools
                        .iter()
                        .any(|t| t.class == o.class && me.pos.distance(o.pos) <= t.range)
                    && sees(me.pos, o.pos)
            });
            let tgt = me
                .target
                .and_then(|t| units.get(&t).map(|u| (t, *u)))
                .filter(|(_, u)| u.alive && u.team != me.team && !u.stealthed);
            let reach_r = if me.class.is_melee() {
                MELEE_REACH
            } else {
                RANGED_REACH
            };
            let blind_target = tgt.map(|(_, u)| !sees(me.pos, u.pos));
            let reach = tgt.is_some_and(|(_, u)| u.pos.distance(me.pos) <= reach_r);
            let healer = healer_of
                .get(&me.team)
                .filter(|h| **h != *e)
                .map(|h| units[h])
                .filter(|h| h.alive);
            let blind_healer = healer.map(|h| !sees(me.pos, h.pos));
            facts.get_mut(e).unwrap()[i] = Fact {
                hidden,
                exposed,
                blind_target,
                blind_healer,
                reach,
            };
            if i < contact {
                continue;
            }
            let o = occ.get_mut(e).unwrap();
            o.secs += FRAME;
            o.hard_cc += if me.hard { FRAME } else { 0.0 };
            o.hidden += if hidden { FRAME } else { 0.0 };
            o.exposed_cc += if exposed { FRAME } else { 0.0 };
            if let Some(b) = blind_target {
                if !me.class.is_healer() {
                    o.has_target += FRAME;
                    o.blind_target += if b { FRAME } else { 0.0 };
                    if reach {
                        o.in_reach += FRAME;
                        o.blind_target_reach += if b { FRAME } else { 0.0 };
                        if b && !me.hard {
                            o.lining_target += FRAME;
                        }
                    }
                }
            }
            if !me.class.is_healer() {
                if let Some(b) = blind_healer {
                    let targeted = units.iter().any(|(_, x)| {
                        x.alive && x.team != me.team && x.target == Some(*e)
                    });
                    let dying = me.hp < DYING;
                    let danger = dying && targeted;
                    o.healer_secs += FRAME;
                    o.blind_healer += if b { FRAME } else { 0.0 };
                    if dying {
                        o.dying += FRAME;
                        o.blind_healer_dying += if b { FRAME } else { 0.0 };
                    }
                    if danger {
                        o.danger += FRAME;
                        o.blind_healer_danger += if b { FRAME } else { 0.0 };
                    }
                }
            } else {
                let anchor = people
                    .iter()
                    .filter(|m| **m != *e && team_of(m) == me.team && units[m].alive)
                    .min_by(|a, b| units[a].hp.total_cmp(&units[b].hp));
                if let Some(a) = anchor {
                    let b = !sees(me.pos, units[a].pos);
                    o.anchor_secs += FRAME;
                    o.blind_anchor += if b { FRAME } else { 0.0 };
                    if units[a].hp < HURT {
                        o.hurt += FRAME;
                        o.blind_anchor_hurt += if b { FRAME } else { 0.0 };
                        if b && !me.hard {
                            o.lining_anchor_hurt += FRAME;
                        }
                    }
                }
            }
        }
        // Team occupancy.
        for t in [1u8, 2u8] {
            let living: Vec<Entity> = people
                .iter()
                .filter(|e| team_of(e) == t && units[e].alive)
                .copied()
                .collect();
            let all_hidden = !living.is_empty() && living.iter().all(|e| facts[e][i].hidden);
            team_all_hidden[i][(t - 1) as usize] = all_hidden;
            if i < contact || living.len() < 2 {
                continue;
            }
            let k = &mut tocc[(t - 1) as usize];
            k.secs += FRAME;
            let any_hidden = living.iter().any(|e| facts[e][i].hidden);
            let cohesive = living.iter().enumerate().all(|(a, x)| {
                living[a + 1..]
                    .iter()
                    .all(|y| sees(units[x].pos, units[y].pos))
            });
            k.all_hidden += if all_hidden { FRAME } else { 0.0 };
            k.none_hidden += if !any_hidden { FRAME } else { 0.0 };
            k.cohesive += if cohesive { FRAME } else { 0.0 };
            k.all_hidden_cohesive += if all_hidden && cohesive { FRAME } else { 0.0 };
            let at = |e: &Entity| skin(units[e].pos) <= AT_PILLAR;
            k.all_at_pillar += if living.iter().all(at) { FRAME } else { 0.0 };
            if let Some(h) = healer_of.get(&t).filter(|h| units[*h].alive) {
                let dps: Vec<&Entity> = living.iter().filter(|e| *e != h).collect();
                let h_hidden = facts[h][i].hidden;
                let dps_exposed = dps.iter().any(|e| !facts[*e][i].hidden);
                let dps_hidden = dps.iter().any(|e| facts[*e][i].hidden);
                if h_hidden && dps_exposed {
                    k.healer_hidden_dps_exposed += FRAME;
                }
                if dps_hidden && !h_hidden {
                    k.dps_hidden_healer_exposed += FRAME;
                }
                if at(h) {
                    k.healer_at_pillar += FRAME;
                    if dps.iter().any(|e| !at(e)) {
                        k.healer_at_pillar_dps_not += FRAME;
                    }
                }
                if mode_at(*h, i) {
                    k.healer_hug += FRAME;
                    if dps.iter().all(|e| facts[*e][i].hidden) {
                        k.healer_hug_dps_hidden += FRAME;
                    }
                    if cohesive {
                        k.healer_hug_cohesive += FRAME;
                    }
                }
            }
        }
    }

    let path = |e: &Entity, a: usize, b: usize| -> f32 {
        (a + 1..=b)
            .map(|k| frames[k].1[e].pos.distance(frames[k - 1].1[e].pos))
            .sum()
    };

    // DPS-healer blind episodes.
    let mut pair_eps: BTreeMap<Entity, Vec<(usize, usize, String)>> = BTreeMap::new();
    for d in &people {
        let t = team_of(d);
        let Some(h) = healer_of.get(&t).filter(|h| *h != d) else {
            continue;
        };
        let f = &facts[d];
        let mut i = contact.max(gates);
        while i < n {
            if f[i].blind_healer == Some(true) && (i == 0 || f[i - 1].blind_healer != Some(true)) {
                let start = i;
                let mut j = i;
                while j + 1 < n && f[j + 1].blind_healer == Some(true) {
                    j += 1;
                }
                let end = if j + 1 >= n {
                    "end"
                } else if !frames[j + 1].1[d].alive {
                    "dps_died"
                } else if !frames[j + 1].1[h].alive {
                    "healer_died"
                } else {
                    "regained"
                };
                let back = start.saturating_sub(BREAK_LOOKBACK).max(gates);
                let (h0, d0) = (frames[back].1[h].pos, frames[back].1[d].pos);
                let (h1, d1) = (frames[start].1[h].pos, frames[start].1[d].pos);
                let broke = broke_kind(!sees(d1, h0), !sees(d0, h1), "dps", "healer");
                pair_eps.entry(*d).or_default().push((start, j, broke.clone()));
                let secs = (j + 1 - start) as f32 * FRAME;
                if secs >= EPISODE_MIN {
                    let u = frames[start].1[d];
                    let targeted = frames[start].1.iter().any(|(_, x)| {
                        x.alive && x.team != u.team && x.target == Some(*d)
                    });
                    row.pairs.push(PairEp {
                        t: t_of(start),
                        team: t,
                        healer: format!("{:?}", class_of(h)),
                        dps: format!("{:?}", class_of(d)),
                        secs,
                        broke,
                        end: end.into(),
                        dps_hp: u.hp,
                        dps_dying: u.hp < DYING,
                        dps_danger: u.hp < DYING && targeted,
                        dps_min_hp: (start..=j)
                            .map(|k| frames[k].1[d].hp)
                            .fold(1.0, f32::min),
                        healer_hard_secs: (start..=j).filter(|&k| frames[k].1[h].hard).count()
                            as f32
                            * FRAME,
                        healer_posture: posture_at(*h, start),
                        healer_mode: if mode_at(*h, start) { "hug" } else { "open" }.into(),
                        dps_skin: skin(d1),
                        healer_skin: skin(h1),
                        dps_hidden: facts[d][start].hidden,
                        dps_path: path(d, start, j),
                        healer_path: path(h, start, j),
                    });
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }

    // DPS lining its own target.
    for d in &people {
        if class_of(d).is_healer() {
            continue;
        }
        let f = &facts[d];
        let mut i = contact.max(gates);
        while i < n {
            let tgt_i = frames[i].1[d].target;
            if f[i].blind_target == Some(true) && (f[i - 1].blind_target != Some(true)) {
                let start = i;
                let mut j = i;
                while j + 1 < n
                    && f[j + 1].blind_target == Some(true)
                    && frames[j + 1].1[d].target == tgt_i
                {
                    j += 1;
                }
                let secs = (j + 1 - start) as f32 * FRAME;
                if let Some(tg) = tgt_i {
                    if secs >= EPISODE_MIN && frames[start].1.contains_key(&tg) {
                        let back = start.saturating_sub(BREAK_LOOKBACK).max(gates);
                        let (d0, g0) = (frames[back].1[d].pos, frames[back].1[&tg].pos);
                        let (d1, g1) = (frames[start].1[d].pos, frames[start].1[&tg].pos);
                        let broke = broke_kind(!sees(d1, g0), !sees(d0, g1), "dps", "target");
                        let reach_r = if class_of(d).is_melee() {
                            MELEE_REACH
                        } else {
                            RANGED_REACH
                        };
                        let alt = |k: usize| {
                            let me = frames[k].1[d];
                            frames[k].1.iter().any(|(e, o)| {
                                *e != tg
                                    && !o.pet
                                    && o.alive
                                    && !o.stealthed
                                    && o.team != me.team
                                    && o.pos.distance(me.pos) <= reach_r
                                    && sees(me.pos, o.pos)
                            })
                        };
                        let alt_share =
                            (start..=j).filter(|&k| alt(k)).count() as f32 / (j + 1 - start) as f32;
                        row.lines.push(LineEp {
                            t: t_of(start),
                            team: team_of(d),
                            class: format!("{:?}", class_of(d)),
                            target: format!(
                                "{:?}{}",
                                frames[start].1[&tg].class,
                                if frames[start].1[&tg].pet { "Pet" } else { "" }
                            ),
                            secs,
                            broke,
                            dist: d1.distance(g1),
                            in_reach: f[start].reach,
                            hp: frames[start].1[d].hp,
                            alt_in_sight: alt(start),
                            alt_share,
                        });
                    }
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }

    // Lulls: spans with no non-pet health lost on either side.
    {
        let mut i = contact.max(gates) + 1;
        while i < n {
            if !damage_frame[i] {
                let start = i;
                let mut j = i;
                while j + 1 < n && !damage_frame[j + 1] {
                    j += 1;
                }
                let secs = (j + 1 - start) as f32 * FRAME;
                if secs >= LULL_MIN {
                    let (a, b) = (&frames[start - 1].1, &frames[j].1);
                    let hid = |t: usize| {
                        (start..=j).filter(|&k| team_all_hidden[k][t]).count() as f32
                            / (j + 1 - start) as f32
                    };
                    row.lulls.push(Lull {
                        t0: t_of(start),
                        secs,
                        hp0: [team_hp(a, 1), team_hp(a, 2)],
                        hp1: [team_hp(b, 1), team_hp(b, 2)],
                        hmana0: [healer_mana(a, 1), healer_mana(a, 2)],
                        hmana1: [healer_mana(b, 1), healer_mana(b, 2)],
                        mana0: [team_mana(a, 1), team_mana(a, 2)],
                        mana1: [team_mana(b, 1), team_mana(b, 2)],
                        hidden_share: [hid(0), hid(1)],
                    });
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }

    // Healer mana over the match.
    for (t, h) in &healer_of {
        let mut at = BTreeMap::new();
        for s in [30u32, 60, 90, 120, 150] {
            let k = gates + (s as f32 / FRAME) as usize;
            if k < n && frames[k].1[h].alive {
                at.insert(format!("{s:03}"), frames[k].1[h].mana);
            }
        }
        let low = (gates..n)
            .filter(|&k| frames[k].1[h].alive && frames[k].1[h].mana < 0.1)
            .count() as f32
            * FRAME;
        let alive = (gates..n).filter(|&k| frames[k].1[h].alive).count() as f32 * FRAME;
        row.mana.push(HealerMana {
            team: *t,
            class: format!("{:?}", class_of(h)),
            at,
            low_secs: low,
            alive_secs: alive,
        });
    }

    // Deaths.
    for d in &people {
        let Some(dd) = (gates..n).find(|&k| !frames[k].1[d].alive) else {
            continue;
        };
        if dd == 0 {
            continue;
        }
        let last = dd - 1;
        let w0 = last.saturating_sub(DEATH_WINDOW - 1).max(gates);
        let frac = |f: &dyn Fn(usize) -> bool| -> f32 {
            (w0..=last).filter(|&k| f(k)).count() as f32 / (last + 1 - w0) as f32
        };
        let t = team_of(d);
        let h = healer_of.get(&t).filter(|h| *h != d);
        let healer_alive = h.is_some_and(|h| frames[dd].1[h].alive);
        let fd = &facts[d];
        let mut fb = 0usize;
        while fb <= last - gates && fd[last - fb].blind_healer == Some(true) {
            fb += 1;
        }
        let final_broke = pair_eps
            .get(d)
            .and_then(|v| v.iter().rev().find(|(_, e, _)| *e == last))
            .map(|(_, _, b)| b.clone());
        row.deaths.push(Death {
            t: t_of(dd),
            team: t,
            class: format!("{:?}", class_of(d)),
            healer: h.map(|h| format!("{:?}", class_of(h))),
            healer_alive,
            hidden_from_healer_5s: frac(&|k| fd[k].blind_healer == Some(true)),
            final_blind_secs: fb as f32 * FRAME,
            final_broke,
            healer_hard_5s: h.map_or(0.0, |h| frac(&|k| frames[k].1[h].hard)),
            out_of_range_5s: h.map_or(0.0, |h| {
                frac(&|k| frames[k].1[h].pos.distance(frames[k].1[d].pos) > 40.0)
            }),
            blind_target_5s: frac(&|k| fd[k].blind_target == Some(true)),
            hard_5s: frac(&|k| frames[k].1[d].hard),
            exposed_cc_5s: frac(&|k| fd[k].exposed),
            healer_posture: h.map_or(String::new(), |h| posture_at(*h, last)),
            healer_mode: h.map_or(String::new(), |h| {
                if mode_at(*h, last) { "hug" } else { "open" }.into()
            }),
        });
    }

    row.units = occ.into_values().collect();
    row.teams = tocc.to_vec();
    row
}

#[test]
#[ignore = "a measurement a human reads; run with TSC_IN / TSC_OUT / TSC_TMP set"]
fn team_sight_cohesion() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("TSC_IN").expect("TSC_IN: the JSONL of match configs");
    let output = std::env::var("TSC_OUT").expect("TSC_OUT: where the rows go");
    let tmp = std::env::var("TSC_TMP").expect("TSC_TMP: a scratch directory");
    let jobs: usize = std::env::var("TSC_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::fs::create_dir_all(&tmp).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open TSC_IN"))
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
                rows.lock().unwrap()[i] = Some(serde_json::to_string(&out).unwrap());
                if (i + 1) % 200 == 0 {
                    eprintln!("{} / {}", i + 1, total);
                }
            });
        }
    });

    let mut out = std::io::BufWriter::new(std::fs::File::create(&output).unwrap());
    for row in rows.lock().unwrap().iter() {
        writeln!(out, "{}", row.as_ref().expect("every match ran")).unwrap();
    }
    println!("{total} matches -> {output}");
}
