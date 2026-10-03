//! AS-197 — the healers' `Legacy` dispel walk, measured in played matches.
//!
//! The walk (Paladin AS-180, Priest AS-187) carries a healer to a teammate held
//! in urgent crowd control its dispel removes. Three rules over it, each probed
//! here in a real headless match that broke it before the card:
//!
//! 1. **No walk while the dispel cannot be cast.** A silenced or Holy-locked
//!    Paladin walked to the range edge with nothing to cast, released, and its
//!    posture stepped it back out — over and over for the whole lockout.
//! 2. **The walk holds its reach point while the dispel is owed.** It released
//!    the moment the teammate came into reach; with the GCD still running, the
//!    posture could step the healer straight back out of range before the
//!    dispel was cast.
//! 3. **A dying teammate comes first.** No walk while another living non-pet
//!    teammate is below `urgency_hp_threshold` — the rule `TeamPlan`'s
//!    `DispelGoal` keeps. The medic chase covered only a dying teammate the
//!    healer could not SEE.
//!
//! Each probe reads the match two ways at once: the decision trace, for when
//! the healer walked (`DispelChase`) and what it cast; and the read-only
//! per-frame observer, for where everyone stood, their health and their auras.

use std::collections::BTreeMap;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::play_match::{AbilityDefinitions, AbilityType, AuraType};
use arenasim::CharacterClass;
use bevy::prelude::Vec3;

/// One observed frame, reduced to what the probes read, keyed by entity index
/// (the id the decision trace carries).
struct Frame {
    t: f64,
    units: BTreeMap<u64, Unit>,
}

#[derive(Clone)]
struct Unit {
    team: u8,
    class: CharacterClass,
    is_pet: bool,
    alive: bool,
    hp: f32,
    pos: Vec3,
    auras: Vec<AuraType>,
}

impl Unit {
    fn has(&self, types: &[AuraType]) -> bool {
        self.auras.iter().any(|a| types.contains(a))
    }
}

/// Crowd control the healers' dispels free at the urgent bar
/// (`dispel_priority` >= `URGENT_DISPEL_PRIORITY`).
const URGENT_CC: &[AuraType] = &[
    AuraType::Polymorph,
    AuraType::Incapacitate,
    AuraType::Silence,
    AuraType::Fear,
];
/// What keeps the healer itself from walking (`is_ccd`, Root included).
const HEALER_HELD: &[AuraType] = &[
    AuraType::Stun,
    AuraType::Fear,
    AuraType::Polymorph,
    AuraType::Incapacitate,
    AuraType::Root,
];
/// What keeps a Paladin from casting Cleanse — every Paladin spell is Holy, so
/// any school lockout on a Paladin is a Holy lockout.
const PALADIN_CANNOT_CLEANSE: &[AuraType] = &[AuraType::Silence, AuraType::SpellSchoolLockout];

/// The healer's dispel.
fn dispel_of(class: CharacterClass) -> Option<AbilityType> {
    match class {
        CharacterClass::Paladin => Some(AbilityType::PaladinCleanse),
        CharacterClass::Priest => Some(AbilityType::DispelMagic),
        _ => None,
    }
}

/// A played match: every frame and every trace event.
struct Played {
    frames: Vec<Frame>,
    events: Vec<serde_json::Value>,
}

fn play(config: HeadlessMatchConfig) -> Played {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    let mut frames = Vec::new();
    run_headless_match_observed(
        config,
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
        |obs: &FrameObservation| {
            if !obs.gates_open {
                return;
            }
            frames.push(Frame {
                t: f64::from(obs.sim_time),
                units: obs
                    .combatants
                    .iter()
                    .map(|(e, c)| {
                        (
                            u64::from(e.index()),
                            Unit {
                                team: c.team,
                                class: c.class,
                                is_pet: c.is_pet,
                                alive: c.alive,
                                hp: c.current_health / c.max_health,
                                pos: c.position,
                                auras: c.aura_types.clone(),
                            },
                        )
                    })
                    .collect(),
            });
        },
    )
    .expect("headless match");
    // The trace's clock starts at the gates; the observer's includes the
    // countdown. Put every event on the observer's clock.
    let gates = frames.first().map_or(0.0, |f: &Frame| f.t);
    let events = std::fs::read_to_string(&path)
        .expect("read trace")
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|mut e| {
            if let Some(t) = e["sim_time"].as_f64() {
                e["sim_time"] = serde_json::json!(t + gates);
            }
            e
        })
        .collect();
    let _ = std::fs::remove_file(&path);
    Played { frames, events }
}

/// A `Legacy` dispel walk traced by a healer: who walked, to whom, and when.
struct Walk {
    healer: u64,
    ally: u64,
    t: f64,
}

/// How a played match measures against the three rules.
#[derive(Default, Debug)]
struct Breaches {
    /// `DispelChase` walks a Paladin began while silenced or school-locked.
    walked_unable_to_cast: Vec<f64>,
    /// Frames on which a healer, owed a dispel it had walked to, stepped from
    /// inside the dispel's range to outside it, its teammate where it stood.
    stepped_out_while_owed: Vec<f64>,
    /// `DispelChase` walks begun while ANOTHER teammate was below half health.
    walked_off_the_dying: Vec<f64>,
}

impl Played {
    fn walks(&self) -> Vec<Walk> {
        self.events
            .iter()
            .filter(|e| e["kind"] == "movement_decision" && e["trigger"] == "DispelChase")
            .map(|e| Walk {
                healer: e["actor"]["entity_id"].as_u64().unwrap(),
                ally: e["target"]["entity_id"].as_u64().unwrap(),
                t: e["sim_time"].as_f64().unwrap(),
            })
            .collect()
    }

    /// The frame at or just after sim-time `t`.
    fn frame_at(&self, t: f64) -> Option<usize> {
        let i = self.frames.partition_point(|f| f.t < t - 1e-4);
        (i < self.frames.len()).then_some(i)
    }

    /// The healer's traced posture as of `t` (`free` before its first event).
    fn posture_at(&self, healer: u64, t: f64) -> String {
        self.events
            .iter()
            .filter(|e| {
                e["kind"] == "movement_decision"
                    && e["actor"]["entity_id"].as_u64() == Some(healer)
                    && e["sim_time"].as_f64().unwrap_or(f64::MAX) <= t
            })
            .last()
            .and_then(|e| e["posture"].as_str())
            .unwrap_or("free")
            .to_string()
    }

    fn breaches(&self, urgency_hp: f32) -> Breaches {
        let defs = AbilityDefinitions::default();
        let mut out = Breaches::default();
        for walk in self.walks() {
            let Some(i) = self.frame_at(walk.t) else {
                continue;
            };
            let healer = &self.frames[i].units[&walk.healer];
            let Some(dispel) = dispel_of(healer.class) else {
                continue;
            };
            if healer.class == CharacterClass::Paladin && healer.has(PALADIN_CANNOT_CLEANSE) {
                out.walked_unable_to_cast.push(walk.t);
            }
            if self.someone_else_dying(i, walk.healer, walk.ally, urgency_hp) {
                out.walked_off_the_dying.push(walk.t);
            }

            // From the walk on, while the dispel stays owed, the healer must
            // never step out of reach of the teammate it walked to.
            let range = defs.get(&dispel).unwrap().range;
            for j in i..self.frames.len().saturating_sub(1) {
                if !self.owed(j, walk.healer, walk.ally, urgency_hp)
                    || !self.owed(j + 1, walk.healer, walk.ally, urgency_hp)
                {
                    break;
                }
                let posture = self.posture_at(walk.healer, self.frames[j + 1].t);
                if posture == "escape" || posture == "dip" {
                    break;
                }
                let ally = self.frames[j].units[&walk.ally].pos;
                let before = self.frames[j].units[&walk.healer].pos;
                let after = self.frames[j + 1].units[&walk.healer].pos;
                if xz(before).distance(xz(ally)) <= range && xz(after).distance(xz(ally)) > range {
                    out.stepped_out_while_owed.push(self.frames[j + 1].t);
                }
            }
        }
        // Walks re-emitted within one owed window scan the same frames.
        out.stepped_out_while_owed.sort_by(f64::total_cmp);
        out.stepped_out_while_owed.dedup();
        out
    }

    /// Whether, on frame `i`, a teammate of `healer` other than `ally` and the
    /// healer itself is alive, not a pet, and below `urgency_hp`.
    fn someone_else_dying(&self, i: usize, healer: u64, ally: u64, urgency_hp: f32) -> bool {
        let team = self.frames[i].units[&healer].team;
        self.frames[i].units.iter().any(|(&id, u)| {
            id != healer
                && id != ally
                && u.team == team
                && !u.is_pet
                && u.alive
                && u.hp < urgency_hp
        })
    }

    /// Whether, on frame `i`, `healer` still owes `ally` the dispel and is free
    /// to deliver it: the teammate alive in urgent crowd control, the healer
    /// alive, neither held nor (a Paladin) unable to Cleanse, and no other
    /// teammate dying.
    fn owed(&self, i: usize, healer: u64, ally: u64, urgency_hp: f32) -> bool {
        let units = &self.frames[i].units;
        let (h, a) = (&units[&healer], &units[&ally]);
        a.alive
            && a.has(URGENT_CC)
            && h.alive
            && !h.has(HEALER_HELD)
            && !(h.class == CharacterClass::Paladin && h.has(PALADIN_CANNOT_CLEANSE))
            && !h.auras.contains(&AuraType::Silence)
            && !self.someone_else_dying(i, healer, ally, urgency_hp)
    }
}

fn xz(v: Vec3) -> bevy::prelude::Vec2 {
    bevy::prelude::Vec2::new(v.x, v.z)
}

fn urgency_hp() -> f32 {
    arenasim::states::play_match::MovementConfig::default()
        .shared
        .urgency_hp_threshold
}

fn legacy(team1: &[&str], team2: &[&str], map: &str, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.to_string(),
        max_duration_secs: 120.0,
        random_seed: Some(seed),
        ..Default::default()
    }
}

/// The probes' matches, cut off once their dispel windows are over.
fn probe(team1: &[&str], team2: &[&str], map: &str, seed: u64) -> Played {
    let mut config = legacy(team1, team2, map, seed);
    config.max_duration_secs = 45.0;
    play(config)
}

impl Played {
    /// Frames on which a free `class` healer could not cast its dispel while a
    /// teammate sat in urgent crowd control at least `min_distance` away — the
    /// moments a walk would have to be refused. The non-vacuity count for
    /// rule 1. (Out of REACH is range or sight; the observer has positions,
    /// not sightlines, so the probe bounds distance from below instead.)
    fn unable_while_owed(&self, class: CharacterClass, min_distance: f32) -> usize {
        self.frames
            .iter()
            .filter(|f| {
                f.units.values().any(|h| {
                    h.class == class
                        && !h.is_pet
                        && h.alive
                        && !h.has(HEALER_HELD)
                        && h.has(PALADIN_CANNOT_CLEANSE)
                        && f.units.values().any(|a| {
                            a.team == h.team
                                && !a.is_pet
                                && a.alive
                                && a.has(URGENT_CC)
                                && xz(a.pos).distance(xz(h.pos)) >= min_distance
                        })
                })
            })
            .count()
    }

    /// Walks a `class` healer traced that carried it into reach of the
    /// teammate while the dispel was still owed — the non-vacuity count for
    /// rule 2.
    fn walks_reaching_while_owed(&self, class: CharacterClass) -> usize {
        let range = AbilityDefinitions::default()
            .get(&dispel_of(class).unwrap())
            .unwrap()
            .range;
        let urgency = urgency_hp();
        self.walks()
            .iter()
            .filter(|w| self.frames[0].units[&w.healer].class == class)
            .filter(|w| {
                let Some(i) = self.frame_at(w.t) else {
                    return false;
                };
                (i..self.frames.len())
                    .take_while(|&j| self.owed(j, w.healer, w.ally, urgency))
                    .any(|j| {
                        let u = &self.frames[j].units;
                        xz(u[&w.healer].pos).distance(xz(u[&w.ally].pos)) <= range
                    })
            })
            .count()
    }
}

/// Rule 1, played: `Warrior+Paladin vs Warlock+Priest` on Nagrand, seed 3. The
/// Felhunter Spell Locks the Paladin's Flash of Light (Holy locked 3s, from
/// 22.75s) as the Warlock's Death Coil sends the Warrior off ~30yd round a
/// pillar; before AS-197 the Paladin walked for the Cleanse it could not cast
/// (three `DispelChase`s, 23.3-24.7s). It must not walk while it cannot cast.
#[test]
fn a_paladin_that_cannot_cleanse_does_not_walk_for_it() {
    let played = probe(
        &["Warrior", "Paladin"],
        &["Warlock", "Priest"],
        "PillaredArena",
        3,
    );
    let unable = played.unable_while_owed(CharacterClass::Paladin, 25.0);
    assert!(
        unable >= 30,
        "vacuous: the Paladin could not Cleanse a CC'd teammate 25yd+ off on only {unable} frames"
    );
    let b = played.breaches(urgency_hp());
    assert!(
        b.walked_unable_to_cast.is_empty(),
        "the Paladin walked for Cleanse while it could not cast it, at {:?}",
        b.walked_unable_to_cast
    );
}

/// Rule 2, played, for each walk: a healer that walked into reach of a teammate
/// in urgent crowd control holds there while the dispel is owed — it never
/// steps from inside the dispel's range to outside it. Before AS-197 both
/// walks released on reach and the posture stepped the healer back out while
/// the GCD ran: the Priest in `Hunter+Priest vs Mage+Warlock` (BasicArena, seed
/// 1) at 23.9-24.1s, and the Paladin in `Rogue+Paladin vs Warlock+Rogue`
/// (Nagrand, seed 2) at 34.7-35.0s.
#[test]
fn a_healer_that_walked_into_dispel_reach_holds_there_while_the_dispel_is_owed() {
    for (class, team1, team2, map, seed) in [
        (
            CharacterClass::Priest,
            ["Hunter", "Priest"],
            ["Mage", "Warlock"],
            "BasicArena",
            1,
        ),
        (
            CharacterClass::Paladin,
            ["Rogue", "Paladin"],
            ["Warlock", "Rogue"],
            "PillaredArena",
            2,
        ),
    ] {
        let played = probe(&team1, &team2, map, seed);
        let reaching = played.walks_reaching_while_owed(class);
        assert!(
            reaching >= 1,
            "vacuous: no {class:?} walk reached its teammate while the dispel was owed"
        );
        let b = played.breaches(urgency_hp());
        assert!(
            b.stepped_out_while_owed.is_empty(),
            "the {class:?} stepped out of dispel range while the dispel was owed, at {:?}",
            b.stepped_out_while_owed
        );
    }
}

impl Played {
    /// Frames on which a `class` healer free to act had a teammate in urgent
    /// crowd control beyond its dispel's range while ANOTHER teammate was below
    /// `urgency_hp` — the non-vacuity count for rule 3.
    fn dying_while_owed(&self, class: CharacterClass) -> usize {
        let range = AbilityDefinitions::default()
            .get(&dispel_of(class).unwrap())
            .unwrap()
            .range;
        let urgency = urgency_hp();
        self.frames
            .iter()
            .enumerate()
            .filter(|(i, f)| {
                f.units.iter().any(|(&hid, h)| {
                    h.class == class
                        && !h.is_pet
                        && h.alive
                        && !h.has(HEALER_HELD)
                        && f.units.iter().any(|(&aid, a)| {
                            aid != hid
                                && a.team == h.team
                                && !a.is_pet
                                && a.alive
                                && a.has(URGENT_CC)
                                && xz(a.pos).distance(xz(h.pos)) > range
                                && self.someone_else_dying(*i, hid, aid, urgency)
                        })
                })
            })
            .count()
    }
}

/// Rule 3, played, for each walk: no dispel walk while another teammate is
/// dying. Before AS-197 the `Legacy` walk yielded only to a dying teammate the
/// healer could not SEE (the medic chase), so on an open map it walked off a
/// sighted one: the Priest in `Warrior+Mage+Priest vs Mage+Rogue+Priest`
/// (BasicArena, seed 4) at 21.5s and 22.1s, and the Paladin in
/// `Warrior+Warlock+Paladin vs Warlock+Rogue+Paladin` (BasicArena, seed 3) at
/// 30.8s and 31.4s.
#[test]
fn a_healer_does_not_walk_for_a_dispel_while_another_teammate_is_dying() {
    for (class, team1, team2, seed) in [
        (
            CharacterClass::Priest,
            ["Warrior", "Mage", "Priest"],
            ["Mage", "Rogue", "Priest"],
            4,
        ),
        (
            CharacterClass::Paladin,
            ["Warrior", "Warlock", "Paladin"],
            ["Warlock", "Rogue", "Paladin"],
            3,
        ),
    ] {
        let played = probe(&team1, &team2, "BasicArena", seed);
        let dying = played.dying_while_owed(class);
        assert!(
            dying >= 30,
            "vacuous: a {class:?} teammate was dying while another sat in CC beyond reach on only {dying} frames"
        );
        let b = played.breaches(urgency_hp());
        assert!(
            b.walked_off_the_dying.is_empty(),
            "the {class:?} walked for a dispel while another teammate was dying, at {:?}",
            b.walked_off_the_dying
        );
    }
}

/// Exploratory: every healer comp below against every CC comp, on two maps,
/// over a few seeds — prints each match that breaks a rule. How the probes'
/// matches were found.
#[test]
#[ignore]
fn scan_dispel_walk_breaches() {
    let healers: &[&[&str]] = &[
        &["Warrior", "Paladin"],
        &["Rogue", "Paladin"],
        &["Mage", "Paladin"],
        &["Hunter", "Paladin"],
        &["Warrior", "Priest"],
        &["Rogue", "Priest"],
        &["Hunter", "Priest"],
        &["Warlock", "Priest"],
    ];
    let cc: &[&[&str]] = &[
        &["Hunter", "Priest"],
        &["Mage", "Rogue"],
        &["Warlock", "Priest"],
        &["Mage", "Warlock"],
        &["Hunter", "Rogue"],
        &["Warlock", "Rogue"],
        &["Warrior", "Mage"],
    ];
    // The dying-first rule needs a third teammate (the healer, the CC'd one and
    // the dying one), so it only ever shows in 3v3: `SCAN_SET=3v3`.
    let healers3: &[&[&str]] = &[
        &["Warrior", "Rogue", "Paladin"],
        &["Warrior", "Mage", "Priest"],
        &["Rogue", "Hunter", "Priest"],
        &["Warrior", "Warlock", "Paladin"],
    ];
    let cc3: &[&[&str]] = &[
        &["Hunter", "Warlock", "Priest"],
        &["Mage", "Rogue", "Priest"],
        &["Warlock", "Rogue", "Paladin"],
        &["Hunter", "Mage", "Warrior"],
    ];
    let (healers, cc) = if std::env::var("SCAN_SET").as_deref() == Ok("3v3") {
        (healers3, cc3)
    } else {
        (healers, cc)
    };
    let mut jobs = Vec::new();
    for map in ["BasicArena", "PillaredArena"] {
        for h in healers {
            for c in cc {
                for seed in 1..=4u64 {
                    jobs.push((h.to_vec(), c.to_vec(), map, seed));
                }
            }
        }
    }
    let jobs = std::sync::Mutex::new(jobs);
    let threads: usize = std::env::var("SCAN_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let Some((h, c, map, seed)) = jobs.lock().unwrap().pop() else {
                    break;
                };
                let b = play(legacy(&h, &c, map, seed)).breaches(urgency_hp());
                if !b.walked_unable_to_cast.is_empty()
                    || !b.stepped_out_while_owed.is_empty()
                    || !b.walked_off_the_dying.is_empty()
                {
                    println!(
                        "SCAN {}v{} {map} seed {seed}: unable={:?} stepped_out={:?} dying={:?}",
                        h.join("+"),
                        c.join("+"),
                        b.walked_unable_to_cast,
                        b.stepped_out_while_owed,
                        b.walked_off_the_dying
                    );
                }
            });
        }
    });
}

/// Exploratory: print one match's healer movement events, dispels and per-frame
/// positions over a window. `DEBUG_MATCH="Rogue,Paladin;Warlock,Priest;PillaredArena;4;22;26"`.
#[test]
#[ignore]
fn debug_dispel_walk_match() {
    let spec = std::env::var("DEBUG_MATCH").expect("DEBUG_MATCH");
    let p: Vec<&str> = spec.split(';').collect();
    let t1: Vec<&str> = p[0].split(',').collect();
    let t2: Vec<&str> = p[1].split(',').collect();
    let (from, to): (f64, f64) = (p[4].parse().unwrap(), p[5].parse().unwrap());
    let played = play(legacy(&t1, &t2, p[2], p[3].parse().unwrap()));
    for e in &played.events {
        let t = e["sim_time"].as_f64().unwrap_or(0.0);
        if t < from || t > to {
            continue;
        }
        let class = e["actor"]["class"].as_str().unwrap_or("");
        if class != "Paladin" && class != "Priest" {
            continue;
        }
        if e["kind"] == "movement_decision" {
            println!(
                "{t:.3} MOVE {} #{} {} {} target={} at={}",
                class,
                e["actor"]["entity_id"],
                e["posture"],
                e["trigger"],
                e["target"]["entity_id"],
                e["actor"]["position"]
            );
        } else if e["outcome"]["ability"].is_string() {
            println!(
                "{t:.3} CAST {} #{} {} -> {}",
                class, e["actor"]["entity_id"], e["outcome"]["ability"], e["outcome"]["target_id"]
            );
        }
    }
    for f in &played.frames {
        if f.t < from || f.t > to {
            continue;
        }
        let line: Vec<String> = f
            .units
            .iter()
            .filter(|(_, u)| !u.is_pet)
            .map(|(id, u)| {
                format!(
                    "#{id}{:?}/{} ({:.1},{:.1}) hp{:.2} {:?}",
                    u.class, u.team, u.pos.x, u.pos.z, u.hp, u.auras
                )
            })
            .collect();
        println!("{:.3} {}", f.t, line.join(" | "));
    }
}
