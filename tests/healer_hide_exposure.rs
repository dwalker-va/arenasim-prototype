//! A healer that hides leaves its partner to the enemy focus — a MEASUREMENT a
//! human reads, not an assertion (AS-440).
//!
//! AS-399 found that the team target focus (AS-389) turns an enemy ranged DPS
//! off a healer hidden behind a pillar onto the partner that healer left in
//! the open. This harness says, per frame, when that happens and which of the
//! healer's machines put it there. It runs a JSONL of `HeadlessMatchConfig`s
//! (the `--batch` input) through the observed runner with the decision trace
//! on, and writes one JSON row per 2v2 side that fields one healer and one
//! DPS (its PARTNER):
//!
//! - **hidden** — the healer is seen by none of the living enemy ranged DPS
//!   (Mage, Hunter, Warlock; the units that follow a focus), among frames
//!   with one alive;
//! - **exposing** — hidden, while an enemy ranged DPS that sees the partner
//!   targets it: the partner is being shot by a unit its healer hides from;
//! - **link** — the healer sees its partner within heal range (40yd): the
//!   hide still heals the unit it exposed;
//! - **cause** — what holds the healer's legs on an exposing frame, read off
//!   its posture machine (`ObservedHealerState`): hard-CC'd, ESCAPE, a dip, a
//!   walk (medic chase, dispel, dying teammate, hunt), PRESSURED walking out of
//!   crowd-control reach (`cc_walk`), PRESSURED hugging, PRESSURED cover (the
//!   team solve's occlusion), FREE hugging, FREE reach repair (AS-362's), FREE
//!   sight repair (AS-206's) or the plain FREE formation point;
//! - **focus** — every enemy `target_focus` onset whose `from` is the healer,
//!   with the cause that held the healer over the 1.5s it hid before it
//!   (`FOCUS_HIDDEN_SECS`), whether it had the link at the onset, and how long
//!   the focus then stayed on the partner;
//! - the side's result, which of its two died first, and the healer's hard
//!   crowd-control seconds.
//!
//! The observer is read-only, so a row's winner is the batch runner's for the
//! same config. Reducer: `docs/design/balance/sweeps/2026-11-13-as440-reduce.py`.
//!
//! ```bash
//! HHX_IN=bank.jsonl HHX_OUT=rows.jsonl HHX_TMP=/tmp/hhx HHX_JOBS=4 \
//!   cargo test --release --test healer_hide_exposure -- --ignored --nocapture
//! ```
//! `HHX_TRACE=0` skips the trace (no focus columns; much faster), for the
//! outcome-only arms of a sweep.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig, ObservedCombatant};
use arenasim::states::play_match::components::{AuraType, Posture};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::{ArenaMap, CharacterClass};
use bevy::prelude::{Entity, Vec2, Vec3};
use serde::Serialize;

const FRAME: f32 = 1.0 / 60.0;
const HEAL_RANGE: f32 = 40.0;
/// `target_focus::FOCUS_HIDDEN_SECS`, in frames: the hide that moves a focus.
const HIDE_FRAMES: usize = 90;

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

/// The units that follow a team focus (`target_focus::follows_focus`).
fn is_ranged_dps(c: &ObservedCombatant) -> bool {
    !c.is_pet
        && matches!(
            c.class,
            CharacterClass::Mage | CharacterClass::Hunter | CharacterClass::Warlock
        )
}

/// What holds the healer's legs this frame.
fn cause(c: &ObservedCombatant) -> &'static str {
    if c.aura_types.iter().any(|a| is_hard(*a)) {
        return "ccd";
    }
    let Some(s) = c.healer_state else {
        return "none";
    };
    match s.posture {
        Posture::Escape => return "escape",
        Posture::Dip => return "dip",
        _ => {}
    }
    if s.walk {
        return "walk";
    }
    let formation = c.formation.unwrap_or_default();
    let locked = c
        .aura_types
        .iter()
        .any(|a| matches!(a, AuraType::SpellSchoolLockout | AuraType::Silence));
    match s.posture {
        Posture::Pressured if locked => "pressured_locked",
        Posture::Pressured if s.cc_walk => "pressured_cc",
        Posture::Pressured if s.hug.is_some() => "pressured_hug",
        Posture::Pressured => "pressured_cover",
        _ if s.hug.is_some() => "free_hug",
        _ if formation.reach.is_some() => "free_reach",
        _ if formation.exit.is_some() => "free_sight",
        _ => "free",
    }
}

#[derive(Serialize, Default, Clone)]
struct FocusMove {
    /// Seconds after the gates.
    t: f32,
    /// The cause that held the healer most over the hide before the onset.
    cause: String,
    /// The healer saw its partner in heal range at the onset.
    link: bool,
    /// The healer was PRESSURED at the onset.
    pressured: bool,
    /// At the onset: yards to the partner, and whether the healer saw it.
    dist: f32,
    sight: bool,
    /// Whose step broke the healer's sight of its partner over the hide
    /// (AS-373's attribution, the positions `HIDE_FRAMES` apart): "healer",
    /// "partner", "both" (either step alone breaks it), "joint" (neither alone
    /// does), "kept" (sight at the onset) or "before" (no sight at the hide's
    /// start either).
    broke: String,
    /// Whose step broke the sight between the healer and the enemy ranged DPS
    /// that was shooting it at the hide's start: "healer", "enemy", "both",
    /// "joint", or "none" when no enemy ranged DPS was on it in sight then.
    hid_by: String,
    /// Seconds the DPS on the healer had been blind to it at the onset.
    blind_for: f32,
    /// What held the healer's legs on the frame that DPS lost sight of it.
    break_cause: String,
    /// At the onset: an enemy melee (or melee pet) within 10yd of the healer.
    melee_near: bool,
    /// At the onset: the partner's target is an enemy ranged DPS.
    partner_on_ranged: bool,
    /// Share of the held seconds the healer kept its heal link, and was
    /// PRESSURED.
    held_link: f32,
    held_pressured: f32,
    /// Seconds the focus then stayed on the partner (to the match end at most).
    held: f32,
    /// Seconds of those the healer stayed hidden.
    held_hidden: f32,
    /// Partner health lost while it held, as a fraction of its maximum.
    partner_loss: f32,
    /// The partner died while it held.
    partner_died: bool,
}

#[derive(Serialize, Default)]
struct Row {
    label: String,
    map: String,
    seed: u64,
    side: u8,
    healer: String,
    partner: String,
    partner_melee: bool,
    enemy: String,
    enemy_ranged: u32,
    result: String,
    duration: f32,
    /// "healer", "partner" or "none": the side's first death.
    first_death: String,
    healer_hard_cc_secs: f32,
    /// Hard-CC onsets on the healer.
    healer_cc_onsets: u32,
    /// Seconds both were alive with an enemy ranged DPS alive.
    both_secs: f32,
    hidden_secs: f32,
    exposing_secs: f32,
    exposing_link_secs: f32,
    /// Exposing seconds, by cause.
    exposing_cause: BTreeMap<String, f32>,
    /// Exposing seconds with the heal link, by cause.
    exposing_link_cause: BTreeMap<String, f32>,
    /// Exposing seconds by why the link is lost: "sight", "range", "both".
    exposing_unlinked: BTreeMap<String, f32>,
    /// Hidden seconds, by cause.
    hidden_cause: BTreeMap<String, f32>,
    /// Seconds the healer could not see its partner, among `both_secs`.
    blind_secs: f32,
    /// Partner health lost (fractions of its max) while exposed, and in all.
    partner_loss_exposing: f32,
    partner_loss: f32,
    /// Seconds an enemy ranged DPS that sees the partner targets it.
    partner_shot_secs: f32,
    /// The same for the healer.
    healer_shot_secs: f32,
    focus: Vec<FocusMove>,
}

#[derive(Clone)]
struct Seen {
    /// The living enemy ranged DPS: entity, position, target.
    enemies: [Option<(Entity, Vec3, Option<Entity>)>; 3],
    hpos: Vec3,
    ppos: Vec3,
    hp: f32,
    dist: f32,
    sight: bool,
    melee_near: bool,
    partner_on_ranged: bool,
    hidden: bool,
    link: bool,
    cause: &'static str,
    pressured: bool,
}

fn arena_of(map: &str) -> ArenaMap {
    *ArenaMap::all()
        .iter()
        .find(|m| format!("{m:?}") == map)
        .expect("a map name")
}

fn run_one(cfg: HeadlessMatchConfig, tmp: &str, i: usize, trace: bool) -> Vec<Row> {
    let mut cfg = cfg;
    cfg.output_path = Some(format!("{tmp}/match_{i:06}.txt"));
    let trace_path = std::path::PathBuf::from(format!("{tmp}/trace_{i:06}.jsonl"));
    let label = cfg.label.clone().unwrap_or_default();
    let map = cfg.map.clone();
    let teams = [cfg.team1.join("+"), cfg.team2.join("+")];
    let seed = cfg.random_seed.unwrap_or(0);
    let volumes: Vec<ObstacleVolume> = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(arena_of(&map))
        .volumes;
    let eye = |p: Vec3| Vec3::new(p.x, EYE_HEIGHT, p.z);
    let sees = |a: Vec3, b: Vec3| volumes.is_empty() || has_line_of_sight(&volumes, eye(a), eye(b));
    let flat = |p: Vec3| Vec2::new(p.x, p.z);

    // Sides: (healer, partner) per team, found on the first frame.
    let mut sides: BTreeMap<u8, (Entity, Entity)> = BTreeMap::new();
    let mut rows: BTreeMap<u8, Row> = BTreeMap::new();
    // Per side, per frame index: what the healer and partner were doing.
    let mut history: BTreeMap<u8, Vec<Seen>> = BTreeMap::new();
    let mut deaths: BTreeMap<Entity, usize> = BTreeMap::new();
    let mut last_hp: BTreeMap<Entity, f32> = BTreeMap::new();
    let mut was_hard: BTreeMap<Entity, bool> = BTreeMap::new();
    let mut frame_ix = 0usize;
    // `HHX_TRACK_DIR`: every 0.1s, every unit's position, target and (for a
    // healer) cause, to `<dir>/track_<i>.jsonl` — the sketch's source.
    let track_dir = std::env::var("HHX_TRACK_DIR").ok();
    let mut track: Vec<String> = Vec::new();
    let mut gate_frame: Option<usize> = None;

    let result = run_headless_match_observed(
        cfg,
        false,
        trace.then(|| TraceConfig {
            output_path: trace_path.clone(),
        }),
        |frame| {
            frame_ix += 1;
            if sides.is_empty() {
                for team in [1u8, 2] {
                    let units: Vec<(&Entity, &ObservedCombatant)> = frame
                        .combatants
                        .iter()
                        .filter(|(_, c)| c.team == team && !c.is_pet)
                        .collect();
                    let healers: Vec<_> =
                        units.iter().filter(|(_, c)| c.class.is_healer()).collect();
                    if units.len() == 2 && healers.len() == 1 {
                        let h = *healers[0].0;
                        let (p, pc) = units.iter().find(|(e, _)| **e != h).unwrap();
                        sides.insert(team, (h, **p));
                        let enemy_ranged = frame
                            .combatants
                            .values()
                            .filter(|c| c.team != team && is_ranged_dps(c))
                            .count() as u32;
                        rows.insert(
                            team,
                            Row {
                                label: label.clone(),
                                map: map.clone(),
                                seed,
                                side: team,
                                healer: format!("{:?}", frame.combatants[&h].class),
                                partner: format!("{:?}", pc.class),
                                partner_melee: pc.class.is_melee(),
                                enemy: teams[(2 - team) as usize].clone(),
                                enemy_ranged,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            if !frame.gates_open {
                for team in sides.keys() {
                    history.entry(*team).or_default().push(Seen {
                        enemies: [None; 3],
                        hpos: Vec3::ZERO,
                        ppos: Vec3::ZERO,
                        hp: 1.0,
                        dist: 0.0,
                        sight: true,
                        melee_near: false,
                        partner_on_ranged: false,
                        hidden: false,
                        link: true,
                        cause: "none",
                        pressured: false,
                    });
                }
                return;
            }
            gate_frame.get_or_insert(frame_ix);
            if let Some(dir) = &track_dir {
                if frame_ix.is_multiple_of(6) {
                    let units: Vec<serde_json::Value> = frame
                        .combatants
                        .iter()
                        .map(|(e, c)| {
                            serde_json::json!({
                                "id": e.index(), "team": c.team, "class": format!("{:?}", c.class),
                                "pet": c.is_pet, "x": c.position.x, "z": c.position.z,
                                "alive": c.alive, "hp": c.current_health / c.max_health.max(1.0),
                                "target": c.target.map(|t| t.index()),
                                "cause": c.healer_state.map(|_| cause(c)),
                            })
                        })
                        .collect();
                    track.push(serde_json::json!({"frame": frame_ix, "units": units}).to_string());
                    let _ = dir;
                }
            }
            for (e, c) in frame.combatants.iter().filter(|(_, c)| !c.is_pet) {
                if !c.alive {
                    deaths.entry(*e).or_insert(frame_ix);
                }
            }
            for (team, (h, p)) in sides.iter() {
                let row = rows.get_mut(team).unwrap();
                let hc = &frame.combatants[h];
                let pcb = &frame.combatants[p];
                let hard = hc.alive && hc.aura_types.iter().any(|a| is_hard(*a));
                if hard {
                    row.healer_hard_cc_secs += FRAME;
                    if !was_hard.get(h).copied().unwrap_or(false) {
                        row.healer_cc_onsets += 1;
                    }
                }
                was_hard.insert(*h, hard);
                let php = if pcb.alive {
                    pcb.current_health / pcb.max_health
                } else {
                    0.0
                };
                let loss = last_hp
                    .insert(*p, php)
                    .map_or(0.0, |prev| (prev - php).max(0.0));
                let enemy: Vec<&ObservedCombatant> = frame
                    .combatants
                    .values()
                    .filter(|c| c.team != *team && c.alive && is_ranged_dps(c))
                    .collect();
                let both = hc.alive && pcb.alive;
                let melee_near = frame.combatants.values().any(|c| {
                    c.team != *team
                        && c.alive
                        && (c.class.is_melee() && !c.is_pet || c.is_pet)
                        && flat(c.position).distance(flat(hc.position)) <= 10.0
                });
                let partner_on_ranged = pcb
                    .target
                    .and_then(|t| frame.combatants.get(&t))
                    .is_some_and(is_ranged_dps);
                let mut enemies = [None; 3];
                for (slot, (e, c)) in frame
                    .combatants
                    .iter()
                    .filter(|(_, c)| c.team != *team && c.alive && is_ranged_dps(c))
                    .take(3)
                    .enumerate()
                {
                    enemies[slot] = Some((*e, c.position, c.target));
                }
                let mut seen = Seen {
                    enemies,
                    hpos: hc.position,
                    ppos: pcb.position,
                    hp: php,
                    dist: flat(hc.position).distance(flat(pcb.position)),
                    sight: false,
                    melee_near,
                    partner_on_ranged,
                    hidden: false,
                    link: false,
                    cause: cause(hc),
                    pressured: hc
                        .healer_state
                        .is_some_and(|s| s.posture == Posture::Pressured),
                };
                if both && !enemy.is_empty() {
                    row.both_secs += FRAME;
                    row.partner_loss += loss;
                    let hidden = enemy.iter().all(|d| !sees(d.position, hc.position));
                    let mutual = sees(hc.position, pcb.position);
                    let link =
                        mutual && flat(hc.position).distance(flat(pcb.position)) <= HEAL_RANGE;
                    let shot_p = enemy
                        .iter()
                        .any(|d| d.target == Some(*p) && sees(d.position, pcb.position));
                    let shot_h = enemy
                        .iter()
                        .any(|d| d.target == Some(*h) && sees(d.position, hc.position));
                    if shot_p {
                        row.partner_shot_secs += FRAME;
                    }
                    if shot_h {
                        row.healer_shot_secs += FRAME;
                    }
                    if !mutual {
                        row.blind_secs += FRAME;
                    }
                    seen.hidden = hidden;
                    seen.link = link;
                    seen.sight = mutual;
                    if hidden {
                        row.hidden_secs += FRAME;
                        *row.hidden_cause.entry(seen.cause.into()).or_default() += FRAME;
                        if shot_p {
                            row.exposing_secs += FRAME;
                            row.partner_loss_exposing += loss;
                            if link {
                                row.exposing_link_secs += FRAME;
                                *row.exposing_link_cause
                                    .entry(seen.cause.into())
                                    .or_default() += FRAME;
                            } else {
                                let far = seen.dist > HEAL_RANGE;
                                let why = match (mutual, far) {
                                    (false, true) => "both",
                                    (false, false) => "sight",
                                    _ => "range",
                                };
                                *row.exposing_unlinked.entry(why.into()).or_default() += FRAME;
                            }
                            *row.exposing_cause.entry(seen.cause.into()).or_default() += FRAME;
                        }
                    }
                }
                history.entry(*team).or_default().push(seen);
            }
        },
    )
    .expect("match runs");

    let gate = gate_frame.unwrap_or(0);
    for (team, (h, p)) in sides.iter() {
        let row = rows.get_mut(team).unwrap();
        row.duration = result.match_time;
        row.result = match result.winner {
            Some(w) if w == *team => "win".into(),
            Some(_) => "loss".into(),
            None => "draw".into(),
        };
        row.first_death = match (deaths.get(h), deaths.get(p)) {
            (None, None) => "none",
            (Some(_), None) => "healer",
            (None, Some(_)) => "partner",
            (Some(a), Some(b)) => {
                if a <= b {
                    "healer"
                } else {
                    "partner"
                }
            }
        }
        .into();
    }

    // The enemy focus moves off each side's healer, from the trace.
    if trace {
        let index_of: BTreeMap<u32, Entity> = sides
            .values()
            .flat_map(|(h, p)| [(h.index(), *h), (p.index(), *p)])
            .collect();
        // Per team that focuses: (frame, Some((subject, from)) | None).
        let mut changes: Vec<(usize, u8, Option<(Entity, Entity)>)> = Vec::new();
        if let Ok(f) = std::fs::File::open(&trace_path) {
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                if !(line.contains("\"situation_change\"") && line.contains("\"target_focus\"")) {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let fr = v["frame"].as_u64().unwrap_or(0) as usize;
                let team = v["team"].as_u64().unwrap_or(0) as u8;
                let ent = |k: &str| {
                    v[k].as_u64()
                        .and_then(|x| index_of.get(&(x as u32)).copied())
                };
                let state = match v["change"].as_str() {
                    Some("end") => None,
                    _ => ent("subject").zip(ent("from")),
                };
                changes.push((fr, team, state));
            }
        }
        let _ = std::fs::remove_file(&trace_path);
        for (i, (fr, team, state)) in changes.iter().enumerate() {
            let Some((subject, from)) = state else {
                continue;
            };
            let side = 3 - *team;
            let Some((h, p)) = sides.get(&side) else {
                continue;
            };
            if from != h || subject != p {
                continue;
            }
            let hist = &history[&side];
            // Trace frames count from the first update; history from the
            // first observed frame — the same clock.
            let at = fr.saturating_sub(1).min(hist.len().saturating_sub(1));
            let end = changes[i + 1..]
                .iter()
                .find(|(_, t, _)| t == team)
                .map_or(hist.len(), |(f, _, _)| f.saturating_sub(1).min(hist.len()));
            let mut causes: BTreeMap<&str, usize> = BTreeMap::new();
            for s in &hist[at.saturating_sub(HIDE_FRAMES)..=at] {
                *causes.entry(s.cause).or_default() += 1;
            }
            let top = causes
                .iter()
                .max_by_key(|(_, n)| **n)
                .map_or("none", |(c, _)| *c);
            let window = &hist[at..end.max(at)];
            let start_hp = hist[at].hp;
            let min_hp = window.iter().map(|s| s.hp).fold(start_hp, f32::min);
            let wl = window.len().max(1) as f32;
            let then = &hist[at.saturating_sub(HIDE_FRAMES)];
            // The enemy ranged DPS that was on the healer as the focus moved,
            // and the frame it last saw it: the hide's real start.
            let shooter = hist[at.saturating_sub(1)]
                .enemies
                .iter()
                .flatten()
                .find(|(_, _, t)| *t == Some(*h))
                .map(|(e, _, _)| *e);
            let pos_of = |k: usize, e: Entity| {
                hist[k]
                    .enemies
                    .iter()
                    .flatten()
                    .find(|(x, _, _)| *x == e)
                    .map(|(_, p, _)| *p)
            };
            let mut hid_by = "none";
            let mut blind_for = 0.0;
            let mut break_cause = "none";
            if let Some(d) = shooter {
                let mut k = at;
                while k > gate && pos_of(k, d).is_some_and(|dp| !sees(dp, hist[k].hpos)) {
                    k -= 1;
                }
                blind_for = (at - k) as f32 * FRAME;
                let b = k + 1;
                if k > gate && b < hist.len() {
                    break_cause = hist[b].cause;
                    let (pre, post) = (b.saturating_sub(30), (b + 30).min(at));
                    if let (Some(d0), Some(d1)) = (pos_of(pre, d), pos_of(post, d)) {
                        let hb = !sees(d0, hist[post].hpos);
                        let db = !sees(d1, hist[pre].hpos);
                        hid_by = match (hb, db) {
                            (true, false) => "healer",
                            (false, true) => "enemy",
                            (true, true) => "both",
                            (false, false) => "joint",
                        };
                    }
                } else {
                    hid_by = "never_seen";
                }
            }
            let now = &hist[at];
            let broke = if now.sight {
                "kept"
            } else if !sees(then.hpos, then.ppos) {
                "before"
            } else {
                let healer_breaks = !sees(now.hpos, then.ppos);
                let partner_breaks = !sees(then.hpos, now.ppos);
                match (healer_breaks, partner_breaks) {
                    (true, false) => "healer",
                    (false, true) => "partner",
                    (true, true) => "both",
                    (false, false) => "joint",
                }
            };
            rows.get_mut(&side).unwrap().focus.push(FocusMove {
                t: (at.saturating_sub(gate)) as f32 * FRAME,
                cause: top.into(),
                link: hist[at].link,
                pressured: hist[at].pressured,
                broke: broke.into(),
                hid_by: hid_by.into(),
                blind_for,
                break_cause: break_cause.into(),
                dist: hist[at].dist,
                sight: hist[at].sight,
                melee_near: hist[at].melee_near,
                partner_on_ranged: hist[at].partner_on_ranged,
                held_link: window.iter().filter(|s| s.link).count() as f32 / wl,
                held_pressured: window.iter().filter(|s| s.pressured).count() as f32 / wl,
                held: window.len() as f32 * FRAME,
                held_hidden: window.iter().filter(|s| s.hidden).count() as f32 * FRAME,
                partner_loss: (start_hp - min_hp).max(0.0),
                partner_died: deaths
                    .get(p)
                    .is_some_and(|d| *d > at && *d <= end.max(at) + 1),
            });
        }
    }
    let _ = std::fs::remove_file(format!("{tmp}/match_{i:06}.txt"));
    if let Some(dir) = &track_dir {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/track_{i:06}.jsonl"), track.join("\n")).unwrap();
    }
    rows.into_values().collect()
}

#[test]
#[ignore = "a measurement a human reads; run with HHX_IN / HHX_OUT / HHX_TMP set"]
fn healer_hide_exposure() {
    use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool, TaskPoolBuilder};
    ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    AsyncComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());
    IoTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(1).build());

    let input = std::env::var("HHX_IN").expect("HHX_IN: the JSONL of match configs");
    let output = std::env::var("HHX_OUT").expect("HHX_OUT: where the rows go");
    let tmp = std::env::var("HHX_TMP").expect("HHX_TMP: a scratch directory");
    let trace = std::env::var("HHX_TRACE").map_or(true, |v| v != "0");
    let jobs: usize = std::env::var("HHX_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::fs::create_dir_all(&tmp).unwrap();

    let configs: Vec<HeadlessMatchConfig> =
        std::io::BufReader::new(std::fs::File::open(&input).expect("open HHX_IN"))
            .lines()
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(&l).expect("a HeadlessMatchConfig"))
            .collect();
    let total = configs.len();
    let configs = Arc::new(configs);
    let cursor = Arc::new(AtomicUsize::new(0));
    let rows: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(vec![None; total]));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (configs, cursor, rows, tmp) = (
                Arc::clone(&configs),
                Arc::clone(&cursor),
                Arc::clone(&rows),
                tmp.clone(),
            );
            scope.spawn(move || loop {
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let out = run_one(configs[i].clone(), &tmp, i, trace);
                let text = out
                    .iter()
                    .map(|r| serde_json::to_string(r).unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                rows.lock().unwrap()[i] = Some(text);
                if (i + 1) % 500 == 0 {
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
