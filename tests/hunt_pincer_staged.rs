//! AS-404 — the Hunt play's active pincer in played endgames (tier 3,
//! staged), asserted on GEOMETRY: where the hunters stand and walk round the
//! hider's pillar, never on the play's own bookkeeping.
//!
//! The bank is a set of lone-healer endgames on Nagrand, each staged on the
//! frame the gates open as a 2v1 at a pillar, played on a few seeds. Two
//! reconstruct the onsets of the 0.8 review's replays (seed 723, Mage+Priest
//! against a Priest on the south-east pillar's west face; seed 732, Hunter+
//! Priest against a Druid off the north-east pillar), at the survivors'
//! positions, health and mana there. The rest put the hunters together on
//! one side of the pillar and the hider on another. The hider plays its own
//! AI, so it runs round its pillar and steps off it; the seed governs only
//! the rolls.
//!
//! The claims, each read over the whole bank:
//!
//! 1. **Nobody stands blind.** No hunter stands still without sight of the
//!    hider for longer than a global cooldown while it is free to move: not
//!    crowd-controlled, rooted, casting or channelling.
//! 2. **Opposite sides.** Whenever both hunters stand blind on one side of
//!    the hider, they do not both close on its face from that side: one of
//!    them goes round the back.
//! 3. **The hunt holds in the shadow.** On every frame the hider stands
//!    beyond `HUNT_LEAVE` of its pillar's skin but in that pillar's shadow from
//!    a hunter, the hunt is on.
//!
//! Each claim first asserts that its situation occurred (frames counted), so
//! none passes on an empty bank. Its control is the play excepted everywhere
//! on the hunting team (`every_exception_entry`, the graduated play's
//! control): the hunt is still classified, but nobody takes a side, and the
//! claim must fail there — so the bank can see the failure it guards.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::class_ai::hunt::{
    book_reaches, in_shadow, HuntWindow, Station, HUNT_LEAVE,
};
use arenasim::states::play_match::class_ai::plays::every_exception_entry;
use arenasim::states::play_match::components::{Aura, AuraType, DispelType};
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use arenasim::states::play_match::situation::{PlayId, Situations};
use arenasim::ArenaMap;
use bevy::prelude::*;
use common::staged::{play, sim_time, Played};

/// How long after the stage each endgame is read.
const WINDOW: f32 = 30.0;

/// The most new walks a hunter may begin in one second.
const MAX_WALKS_PER_SECOND: usize = 4;

/// A global cooldown: the longest a hunter may stand blind.
const GCD: f32 = 1.5;

/// Below this speed (yd/s) a unit is standing.
const STILL: f32 = 0.3;

/// Two blind hunters may stand on one side of the hider this long while one
/// of them leaves for the other (a frame or two of overlap is noise).
const SAME_SIDE_SETTLE: f32 = 0.5;

/// A hunter that has seen the hider for this long has ended its onset walk
/// (it walked the shorter way to its side until then, as ruled).
const SEEN_ONCE: f32 = 0.25;

/// One staged endgame: the hunting team (team 1 here or team 2), its two
/// units and the hider, each `(x, z, health, mana)`.
#[derive(Clone)]
pub struct Endgame {
    pub name: &'static str,
    pub hunters: [&'static str; 2],
    pub hider: &'static str,
    /// The hunting team's number: the reconstructions keep their replay's
    /// sides.
    pub hunting_team: u8,
    pub at: [(f32, f32, f32, f32); 3],
    pub seeds: &'static [u64],
}

/// The bank (module docs).
pub fn bank() -> Vec<Endgame> {
    vec![
        Endgame {
            name: "723 onset",
            hunters: ["Mage", "Priest"],
            hider: "Priest",
            hunting_team: 1,
            at: [
                (39.20, -8.94, 0.871, 0.285),
                (40.36, -13.92, 1.0, 0.02),
                (33.63, -17.46, 0.925, 0.391),
            ],
            seeds: &[723, 1, 2, 3],
        },
        Endgame {
            name: "732 onset",
            hunters: ["Hunter", "Priest"],
            hider: "Druid",
            hunting_team: 2,
            at: [
                (47.44, 20.65, 0.994, 0.3),
                (44.36, 27.85, 1.0, 0.092),
                (45.26, 9.37, 0.514, 0.055),
            ],
            seeds: &[732, 1, 2, 3],
        },
        Endgame {
            name: "one side, Mage+Priest v Priest",
            hunters: ["Mage", "Priest"],
            hider: "Priest",
            hunting_team: 1,
            // North of the south-east pillar, the hider on its south face.
            at: [
                (36.0, -9.0, 0.9, 0.6),
                (44.0, -10.0, 0.9, 0.6),
                (40.0, -27.0, 0.8, 0.6),
            ],
            seeds: &[1, 2, 3, 4],
        },
        Endgame {
            name: "far sight, Hunter+Priest v Druid",
            hunters: ["Hunter", "Priest"],
            hider: "Druid",
            hunting_team: 1,
            // The hider on the south-east pillar's west face, the Hunter
            // 44yd west of it in plain sight (beyond its 35yd shots), the
            // Priest east of the pillar, out of its sight.
            at: [
                (-10.0, -20.0, 0.9, 0.6),
                (55.0, -24.0, 0.9, 0.6),
                (33.5, -20.0, 0.8, 0.6),
            ],
            seeds: &[1, 2, 3, 4],
        },
        Endgame {
            name: "one side, Warlock+Priest v Priest",
            hunters: ["Warlock", "Priest"],
            hider: "Priest",
            hunting_team: 1,
            // West of the south-west pillar, the hider on its east face.
            at: [
                (-52.0, -22.0, 0.9, 0.6),
                (-53.0, -16.0, 0.9, 0.6),
                (-33.0, -20.0, 0.8, 0.6),
            ],
            seeds: &[1, 2, 3, 4],
        },
    ]
}

fn config(e: &Endgame, seed: u64, hunt: bool) -> HeadlessMatchConfig {
    let hunters: Vec<String> = e.hunters.iter().map(|s| s.to_string()).collect();
    let hider = vec![e.hider.to_string()];
    let (team1, team2) = if e.hunting_team == 1 {
        (hunters, hider)
    } else {
        (hider, hunters)
    };
    let excepted = || [("Hunt".into(), every_exception_entry(PlayId::Hunt))].into();
    HeadlessMatchConfig {
        team1,
        team2,
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".into()),
        team1_exceptions: if !hunt && e.hunting_team == 1 {
            excepted()
        } else {
            Default::default()
        },
        team2_exceptions: if !hunt && e.hunting_team == 2 {
            excepted()
        } else {
            Default::default()
        },
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    }
}

fn hider_unit(e: &Endgame) -> Unit {
    Unit::new(3 - e.hunting_team, 0)
}

fn hunter_units(e: &Endgame) -> [Unit; 2] {
    [Unit::new(e.hunting_team, 0), Unit::new(e.hunting_team, 1)]
}

fn stage(e: &Endgame) -> Stage {
    let [h0, h1] = hunter_units(e);
    let units = [h0, h1, hider_unit(e)];
    units
        .into_iter()
        .zip(e.at)
        .fold(Stage::new(), |s, (who, (x, z, hp, mana))| {
            s.place(who, x, z).health(who, hp).mana(who, mana)
        })
}

fn class_of(name: &str) -> CharacterClass {
    match name {
        "Mage" => CharacterClass::Mage,
        "Priest" => CharacterClass::Priest,
        "Hunter" => CharacterClass::Hunter,
        "Warlock" => CharacterClass::Warlock,
        other => panic!("no hunter class {other}"),
    }
}

fn xz(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// The signed angle from `a` to `b` round a centre, in (-180°, 180°].
fn signed_deg(center: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = ((b - center).to_angle() - (a - center).to_angle() + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    d.to_degrees()
}

const HELD: [AuraType; 6] = [
    AuraType::Stun,
    AuraType::Fear,
    AuraType::Polymorph,
    AuraType::Incapacitate,
    AuraType::Root,
    AuraType::Cyclone,
];

/// One played endgame, read frame by frame.
#[derive(Debug, Default, Clone)]
pub struct Reading {
    /// Frames the hunt was on.
    pub hunt_frames: u32,
    /// The longest a hunter stood blind and free to move, during the hunt.
    pub longest_blind_still: f32,
    /// Seconds a hunter saw the hider from beyond its reach (the situation),
    /// and the longest it stood there free to move.
    pub out_of_reach: f32,
    pub longest_beyond_reach_still: f32,
    /// Seconds both hunters stood blind, after both had first seen the hider
    /// (their onset walks over), and of those the seconds they had stood
    /// together on one side of it for longer than [`SAME_SIDE_SETTLE`], and
    /// of those the seconds in which BOTH walked toward its face.
    pub both_blind: f32,
    pub same_side_blind: f32,
    pub both_closing: f32,
    /// The most new walks (`MovementTrigger::Hunt`) one hunter began in any
    /// one second: the walk is re-aimed when its point moves on, not every
    /// frame its point drifts.
    pub walks_per_second: usize,
}

/// The hunt's windows on the app clock, and the pillar of each.
fn hunt_windows(played: &Played, team: u8) -> Vec<(f32, f32, usize)> {
    let landed = played.landed();
    let offset = landed.app - landed.combat;
    let mut out = Vec::new();
    let mut open: Option<(f32, usize)> = None;
    for e in played.staged_trace() {
        if e["kind"] != "situation_change" || e["name"] != "hunt" || e["team"] != team {
            continue;
        }
        let t = sim_time(e) + offset;
        let pillar = e["pillar"].as_u64().unwrap_or(0) as usize;
        if let Some((from, p)) = open.take() {
            out.push((from, t, p));
        }
        if e["change"] == "onset" {
            open = Some((t, pillar));
        }
    }
    if let Some((from, p)) = open {
        out.push((from, f32::INFINITY, p));
    }
    out
}

pub fn read(e: &Endgame, played: &Played, obstacles: &[ObstacleVolume]) -> Reading {
    let start = played.landed().app;
    let windows = hunt_windows(played, e.hunting_team);
    let hunting_at = |t: f32| windows.iter().find(|w| t >= w.0 && t < w.1).map(|w| w.2);
    let find = |f: &arenasim::headless::FrameObservation, u: Unit| {
        f.combatants
            .values()
            .find(|c| c.team == u.team && c.slot == u.slot && !c.is_pet)
            .cloned()
    };
    let hunters = hunter_units(e);
    let mut r = Reading::default();
    let mut blind_still = [0.0f32; 2];
    let mut beyond_still = [0.0f32; 2];
    let books = e.hunters.map(|class| AbilityBook::shipped(class_of(class)));
    let mut together = 0.0f32;
    let mut sighted_for = [0.0f32; 2];
    let mut seen_once = [false; 2];
    let mut prev: Option<(f32, [Vec2; 2], Vec2)> = None;
    for f in played.staged_frames() {
        let t = f.sim_time;
        if t - start > WINDOW {
            break;
        }
        let Some(hider) = find(f, hider_unit(e)).filter(|h| h.alive) else {
            break;
        };
        let units = hunters.map(|u| find(f, u).expect("a hunter"));
        if units.iter().any(|u| !u.alive) {
            break;
        }
        let hp = xz(hider.position);
        let pos = units.each_ref().map(|u| xz(u.position));
        let dt = prev.map_or(0.0, |p| t - p.0);
        let speed = |i: usize| {
            prev.map_or(f32::INFINITY, |p| {
                if dt > 0.0 {
                    pos[i].distance(p.1[i]) / dt
                } else {
                    f32::INFINITY
                }
            })
        };
        let sees = units
            .each_ref()
            .map(|u| has_line_of_sight(obstacles, u.position, hider.position));
        for i in 0..2 {
            sighted_for[i] = if sees[i] { sighted_for[i] + dt } else { 0.0 };
            seen_once[i] |= sighted_for[i] >= SEEN_ONCE;
        }
        let hunt = hunting_at(t);
        // The play stands down while the hider is held (its lock is the
        // Chain play's to extend): those frames are not the hunt's.
        let locked = hider
            .aura_types
            .iter()
            .any(|a| HELD.contains(a) && *a != AuraType::Root);
        let Some(pillar) = hunt.filter(|_| !locked) else {
            prev = Some((t, pos, hp));
            blind_still = [0.0; 2];
            beyond_still = [0.0; 2];
            together = 0.0;
            continue;
        };
        let (center, _) = obstacles[pillar].footprint_disc();
        r.hunt_frames += 1;

        // 1. Nobody stands blind.
        for i in 0..2 {
            let u = &units[i];
            let free = !u.aura_types.iter().any(|a| HELD.contains(a))
                && u.casting.is_none()
                && u.channeling.is_none();
            if !sees[i] && free && speed(i) < STILL {
                blind_still[i] += dt;
                r.longest_blind_still = r.longest_blind_still.max(blind_still[i]);
            } else {
                blind_still[i] = 0.0;
            }
            // ...and nobody stands in sight beyond its reach.
            let out_of_reach =
                sees[i] && !book_reaches(&books[i], u.position.distance(hider.position), 0.0);
            if out_of_reach {
                r.out_of_reach += dt;
            }
            if out_of_reach && free && speed(i) < STILL {
                beyond_still[i] += dt;
                r.longest_beyond_reach_still = r.longest_beyond_reach_still.max(beyond_still[i]);
            } else {
                beyond_still[i] = 0.0;
            }
        }

        // 2. Opposite sides: both blind, after their onset walks.
        let off = pos.map(|p| signed_deg(center, hp, p));
        let after_onset = seen_once[0] && seen_once[1];
        if after_onset && !sees[0] && !sees[1] {
            r.both_blind += dt;
            let one_side = off[0].signum() == off[1].signum()
                && off.iter().all(|o| o.abs() > 5.0 && o.abs() < 175.0);
            together = if one_side { together + dt } else { 0.0 };
            if together > SAME_SIDE_SETTLE {
                r.same_side_blind += dt;
                if let Some((_, before, hider_before)) = prev {
                    // Each hunter's own walk, measured against where the
                    // hider stood a frame ago so that its run does not count.
                    let was = before.map(|p| signed_deg(center, hider_before, p).abs());
                    let now = pos.map(|p| signed_deg(center, hider_before, p).abs());
                    if now[0] < was[0] - 1e-3 && now[1] < was[1] - 1e-3 {
                        r.both_closing += dt;
                    }
                }
            }
        } else {
            together = 0.0;
        }
        prev = Some((t, pos, hp));
    }
    let landed = played.landed();
    let mut walks: std::collections::BTreeMap<u64, Vec<f32>> = Default::default();
    for ev in played.staged_trace() {
        if ev["kind"] == "movement_decision"
            && ev["trigger"] == "Hunt"
            && ev["actor"]["team"] == e.hunting_team
        {
            let id = ev["actor"]["entity_id"].as_u64().unwrap_or(0);
            walks
                .entry(id)
                .or_default()
                .push(sim_time(ev) - landed.combat);
        }
    }
    r.walks_per_second = walks
        .values()
        .map(|ts| {
            ts.iter()
                .map(|&a| ts.iter().filter(|&&b| b >= a && b < a + 1.0).count())
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);
    r
}

pub fn obstacles() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("maps.ron")
        .active_for(ArenaMap::PillaredArena)
        .volumes
        .clone()
}

/// Every endgame of the bank on every seed, with the play on (`hunt`) or
/// excepted everywhere.
pub fn readings(hunt: bool) -> Vec<(String, Reading)> {
    let obstacles = obstacles();
    let mut out = Vec::new();
    for e in bank() {
        for &seed in e.seeds {
            let played = play(config(&e, seed, hunt), stage(&e));
            out.push((
                format!("{} seed {seed}", e.name),
                read(&e, &played, &obstacles),
            ));
        }
    }
    out
}

#[test]
#[ignore = "exploration: prints the bank's readings"]
fn print_the_bank() {
    for hunt in [true, false] {
        for (name, r) in readings(hunt) {
            println!("hunt {hunt} {name}: {r:?}");
        }
    }
}

/// Claim 1: no hunter stands blind and free to move for longer than a GCD.
/// The control (the play excepted): a hunter on its ordinary movement does.
#[test]
fn no_hunter_stands_still_without_sight() {
    let on = readings(true);
    let hunted: u32 = on.iter().map(|(_, r)| r.hunt_frames).sum();
    assert!(hunted > 5_000, "the bank hunted: {hunted} frames");
    for (name, r) in &on {
        assert!(
            r.longest_blind_still <= GCD,
            "{name}: a hunter stood blind for {:.2}s",
            r.longest_blind_still
        );
    }
    let off = readings(false);
    let worst = off
        .iter()
        .map(|(_, r)| r.longest_blind_still)
        .fold(0.0f32, f32::max);
    assert!(
        worst > GCD,
        "the control: excepted, some hunter stands blind ({worst:.2}s at most)"
    );
}

/// Claim 1b: no hunter stands in sight of the hider from beyond its own
/// reach (its book's damaging abilities' range bands) for more than a GCD:
/// it walks into reach instead. The bank's "far sight" endgame starts a
/// Hunter 44yd from the hider in plain sight, beyond its 35yd shots.
#[test]
fn no_hunter_stands_in_sight_beyond_its_reach() {
    let on = readings(true);
    let beyond: f32 = on.iter().map(|(_, r)| r.out_of_reach).sum();
    assert!(
        beyond > 1.0,
        "the situation: in sight beyond reach ({beyond:.2}s)"
    );
    for (name, r) in &on {
        assert!(
            r.longest_beyond_reach_still <= GCD,
            "{name}: a hunter stood in sight beyond its reach for {:.2}s",
            r.longest_beyond_reach_still
        );
    }
}

/// The walk is re-aimed when its point moves on — a carrot round the back
/// half a step behind, or a walk point that lost the hider — never every few
/// frames as the hider moves: no hunter begins more than [`MAX_WALKS_PER_SECOND`] new
/// walks in any one second. (Its controls are mutations: dropping the kept
/// walk point or the kept carrot breaks it.)
#[test]
fn a_walk_is_re_aimed_only_when_its_point_moves_on() {
    let on = readings(true);
    assert!(
        on.iter().any(|(_, r)| r.walks_per_second >= 2),
        "the situation: hunters re-aimed their walks"
    );
    for (name, r) in &on {
        assert!(
            r.walks_per_second <= MAX_WALKS_PER_SECOND,
            "{name}: {} new walks in one second",
            r.walks_per_second
        );
    }
}

/// Claim 2: once both hunters have seen the hider, two blind hunters on one
/// side of it never both close on its face: one of them is on its way round
/// the back, so the two come at it from opposite sides. The control
/// (excepted): the two trail it round one side.
#[test]
fn blind_hunters_close_from_opposite_sides() {
    let on = readings(true);
    let same_side: f32 = on.iter().map(|(_, r)| r.same_side_blind).sum();
    assert!(
        same_side > 0.5,
        "the situation: two blind hunters on one side ({same_side:.2}s)"
    );
    for (name, r) in &on {
        assert!(
            r.both_closing == 0.0,
            "{name}: both closed on the face from one side for {:.2}s",
            r.both_closing
        );
    }
    let off: f32 = readings(false).iter().map(|(_, r)| r.both_closing).sum();
    assert!(
        off > 1.0,
        "the control: excepted, they trail it ({off:.1}s)"
    );
}

/// The south-east pillar (index 2 in `maps.ron`) and the south-west one
/// (index 0).
const SE: (usize, Vec2) = (2, Vec2::new(40.0, -20.0));
const SW: (usize, Vec2) = (0, Vec2::new(-40.0, -20.0));

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const HIDER: Unit = Unit::new(2, 0);

fn root() -> Aura {
    Aura {
        effect_type: AuraType::Root,
        duration: 60.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Entangling Roots".into(),
        // Not magic: nothing the hider casts takes it off.
        dispel_type: DispelType::Physical,
        ..Default::default()
    }
}

fn two_v_one(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Priest".into()],
        map: "PillaredArena".to_string(),
        ai_profile: Some("TeamPlan".into()),
        random_seed: Some(seed),
        max_duration_secs: 60.0,
        ..Default::default()
    }
}

/// The point at `bearing` (degrees) and `r` yards round `center`.
fn round(center: Vec2, bearing: f32, r: f32) -> (f32, f32) {
    let p = center + Vec2::from_angle(bearing.to_radians()) * r;
    (p.x, p.y)
}

/// A hunt already running round `pillar` on the hider, our Mage on side 0
/// (counter-clockwise of the hider) and our Priest on side 1: the state a
/// hunt is in partway through, staged so the claim's situation holds by
/// construction. `onset_walk_done` says whether both have already seen the
/// hider since taking their sides.
fn hunt_under_way(pillar: usize, axis: f32, onset_walk_done: bool) -> impl FnOnce(&mut World) {
    move |w: &mut World| {
        let (subject, mage, priest) = (HIDER.entity(w), MAGE.entity(w), PRIEST.entity(w));
        let side = |unit: Entity, slot: u8| Station {
            unit,
            slot,
            point: Vec2::ZERO,
            hold: true,
            sighted: false,
            onset_walk_done,
            onset_round_back: false,
            aimed_at: 0.0,
            round_back: false,
        };
        w.resource_mut::<Situations>().teams[0].hunt = Some(HuntWindow {
            subject,
            pillar,
            axis: axis.to_radians(),
            since: 0.0,
            owed: false,
            stations: [side(mage, 0), side(priest, 1)].into_iter().collect(),
        });
    }
}

struct Track {
    /// Seconds after the stage, our Mage's and Priest's signed bearing from
    /// the hider round the pillar (degrees), and whether each sees it.
    frames: Vec<(f32, [f32; 2], [bool; 2], [Vec2; 2], Vec2)>,
}

fn track(played: &Played, center: Vec2, window: f32) -> Track {
    let obstacles = obstacles();
    let start = played.landed().app;
    let find = |f: &arenasim::headless::FrameObservation, u: Unit| {
        f.combatants
            .values()
            .find(|c| c.team == u.team && c.slot == u.slot && !c.is_pet)
            .cloned()
            .expect("the unit")
    };
    let mut frames = Vec::new();
    for f in played.staged_frames() {
        let t = f.sim_time - start;
        let (m, p, h) = (find(f, MAGE), find(f, PRIEST), find(f, HIDER));
        if t > window || !m.alive || !p.alive || !h.alive {
            break;
        }
        let hp = xz(h.position);
        let pos = [xz(m.position), xz(p.position)];
        frames.push((
            t,
            pos.map(|q| signed_deg(center, hp, q)),
            [&m, &p].map(|u| has_line_of_sight(&obstacles, u.position, h.position)),
            pos,
            hp,
        ));
    }
    Track { frames }
}

/// The angle at the hider between its two hunters, in degrees.
fn separation_at(hider: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a - hider).angle_to(b - hider).abs().to_degrees()
}

/// Claim 4, with claim 2's geometry: a hunter on the wrong side of the hider
/// whose onset walk is over goes round the BACK of the pillar to its own
/// side, never toward the hider's face, and the two end on opposite sides.
///
/// The hider is rooted on the south-east pillar's east face (bearing 0°).
/// Both hunters stand south of it, out of its sight: the Priest on its own
/// (clockwise) side at -145°, the Mage at -115°, on the Priest's side
/// though its own is the counter-clockwise one. Its short way to its side
/// is across the hider's face; its long way, round the back.
///
/// The control: the same stage with the onset walks not yet over. As ruled,
/// the Mage's first walk takes the short way, toward the face, and stops to
/// cast once it sees the hider — on the Priest's side.
#[test]
fn a_hunter_on_the_wrong_side_goes_round_the_back() {
    let (pillar, center) = SE;
    let stage = |done: bool| {
        let (mx, mz) = round(center, -115.0, 14.0);
        let (px, pz) = round(center, -145.0, 14.0);
        Stage::new()
            .place(HIDER, 47.0, -20.0)
            .aura(HIDER, root())
            .place(MAGE, mx, mz)
            .target(MAGE, HIDER)
            .place(PRIEST, px, pz)
            .target(PRIEST, HIDER)
            .target(HIDER, MAGE)
            .world(hunt_under_way(pillar, 0.0, done))
    };
    for seed in [1, 2, 3] {
        let on = track(&play(two_v_one(seed), stage(true)), center, 25.0);
        let first = on.frames.first().expect("frames");
        assert!(
            first.1[0] < -100.0 && first.1[1] < -100.0 && !first.2[0] && !first.2[1],
            "seed {seed}: the situation: both blind on the clockwise side ({:?})",
            first
        );
        // Never toward the face while on the wrong side: its distance in
        // bearing from the face only grows until it crosses the back.
        let start = first.1[0].abs();
        let mut closest = start;
        let mut crossed_at = None;
        for &(t, off, ..) in &on.frames {
            if off[0] > 0.0 {
                crossed_at = Some((t, off[0]));
                break;
            }
            closest = closest.min(off[0].abs());
        }
        assert!(
            closest > start - 3.0,
            "seed {seed}: the Mage walked toward the face, to {closest:.0}° of it"
        );
        let (t, at) = crossed_at
            .unwrap_or_else(|| panic!("seed {seed}: the Mage never reached its own side"));
        assert!(
            at > 90.0,
            "seed {seed}: it crossed at {at:.0}°, not round the back"
        );
        // And the two then close from opposite sides.
        let opposite = on
            .frames
            .iter()
            .filter(|f| f.0 >= t && f.2[0] && f.2[1])
            .map(|f| separation_at(f.4, f.3[0], f.3[1]))
            .next()
            .unwrap_or_else(|| panic!("seed {seed}: the two never both saw the hider"));
        assert!(
            opposite > 90.0,
            "seed {seed}: both see it from {opposite:.0}° apart, one side"
        );

        // The control: the onset walk takes the short way, across the face.
        let off = track(&play(two_v_one(seed), stage(false)), center, 25.0);
        let start = off.frames[0].1[0].abs();
        let closest = off
            .frames
            .iter()
            .take_while(|f| f.1[0] < 0.0)
            .map(|f| f.1[0].abs())
            .fold(start, f32::min);
        assert!(
            closest < start - 3.0,
            "seed {seed}: the control: on its onset walk the Mage closes on the face \
             ({start:.0}° -> {closest:.0}°)"
        );
    }
}

/// Claim 3: a hunt holds while the hider stands beyond `HUNT_LEAVE` in its
/// pillar's shadow from a hunter, and lets go only once neither hunter has it
/// in its widened shadow.
///
/// The hider is rooted 12yd off the south-west pillar's skin, to its south;
/// our Mage and Priest stand north of the pillar, the hider in both their
/// shadows, with the hunt under way. The control: the same hider with both
/// hunters standing where they see it clear — the hunt ends at once.
#[test]
fn the_hunt_holds_while_the_hider_stands_in_the_shadow() {
    let (pillar, center) = SW;
    let obstacles = obstacles();
    let hider = Vec2::new(-40.0, -38.0);
    let (_, radius) = obstacles[pillar].footprint_disc();
    assert!(
        hider.distance(center) - radius > HUNT_LEAVE,
        "beyond the leave radius"
    );
    let stage = |mage: (f32, f32), priest: (f32, f32)| {
        Stage::new()
            .place(HIDER, hider.x, hider.y)
            .aura(HIDER, root())
            .place(MAGE, mage.0, mage.1)
            .target(MAGE, HIDER)
            .place(PRIEST, priest.0, priest.1)
            .target(PRIEST, HIDER)
            .world(hunt_under_way(pillar, -90.0, false))
    };
    let end_of = |played: &Played| {
        let landed = played.landed();
        played
            .staged_trace()
            .find(|e| {
                e["kind"] == "situation_change"
                    && e["name"] == "hunt"
                    && e["team"] == 1
                    && e["change"] == "end"
            })
            .map(|e| sim_time(e) - landed.combat)
    };
    for seed in [1, 2, 3] {
        let played = play(two_v_one(seed), stage((-43.0, -1.0), (-37.0, -1.0)));
        let t = track(&played, center, 60.0);
        let first = &t.frames[0];
        assert!(
            first
                .3
                .iter()
                .all(|&p| in_shadow(&obstacles, pillar, p, first.4)),
            "seed {seed}: the situation: in both hunters' shadow"
        );
        let first_sight = t
            .frames
            .iter()
            .find(|f| f.2[0] || f.2[1])
            .map(|f| f.0)
            .unwrap_or_else(|| panic!("seed {seed}: no hunter ever saw the hider"));
        let end = end_of(&played);
        assert!(
            end.is_none_or(|e| e >= first_sight),
            "seed {seed}: the hunt ended at {end:?}s, before anyone saw the hider \
             ({first_sight:.2}s)"
        );
        if let Some(e) = end {
            // It lets go only out of everyone's widened shadow.
            let f = t
                .frames
                .iter()
                .find(|f| f.0 >= e)
                .expect("a frame at the end");
            assert!(
                f.3.iter().all(|&p| !in_shadow(&obstacles, pillar, p, f.4)),
                "seed {seed}: the hunt ended at {e:.2}s with the hider still in a shadow"
            );
        }
        // The control: hunters that see the hider clear.
        let played = play(two_v_one(seed), stage((-28.0, -40.0), (-30.0, -30.0)));
        let end = end_of(&played);
        assert!(
            end.is_some_and(|e| e < 0.1),
            "seed {seed}: the control: in the open, the hunt ends at once ({end:?})"
        );
    }
}

/// The onset walk of the far hunter keeps the short route to its side's
/// onset point (the far station before AS-404), and here that route is
/// round the back: from -120° the onset point at +90° is 150° away behind
/// the pillar and 210° across the hider's face, though the nearest spot that
/// sees the hider on its side is the shorter walk across the face. The hunt
/// begins as played (no staged hunt): our Priest at -110° takes the near
/// (-90°) side, our Mage the far one.
///
/// The control: the play excepted, the Mage's own walk for sight closes on
/// the hider's face from the Priest's side.
#[test]
fn the_far_hunters_onset_walk_takes_the_short_route_to_its_side() {
    let (_, center) = SE;
    let stage = |hunt: bool| {
        let (mx, mz) = round(center, -120.0, 14.0);
        let (px, pz) = round(center, -110.0, 12.0);
        let config = HeadlessMatchConfig {
            team1_exceptions: if hunt {
                Default::default()
            } else {
                [("Hunt".into(), every_exception_entry(PlayId::Hunt))].into()
            },
            ..two_v_one(0)
        };
        let stage = Stage::new()
            .place(HIDER, 47.0, -20.0)
            .aura(HIDER, root())
            .place(MAGE, mx, mz)
            .target(MAGE, HIDER)
            .place(PRIEST, px, pz)
            .target(PRIEST, HIDER)
            .target(HIDER, PRIEST);
        (config, stage)
    };
    let closes_on_the_face = |seed: u64, hunt: bool| {
        let (config, stage) = stage(hunt);
        let t = track(
            &play(
                HeadlessMatchConfig {
                    random_seed: Some(seed),
                    ..config
                },
                stage,
            ),
            center,
            25.0,
        );
        let first = t.frames[0];
        assert!(
            first.1[0] < -110.0 && !first.2[0],
            "seed {seed}: the situation: the Mage blind at {:.0}°",
            first.1[0]
        );
        let start = first.1[0].abs();
        let closest = t
            .frames
            .iter()
            .take_while(|f| f.1[0] < 0.0)
            .map(|f| f.1[0].abs())
            .fold(start, f32::min);
        let reached = t.frames.iter().any(|f| f.1[0] > 0.0);
        (closest < start - 3.0, reached, closest, start)
    };
    for seed in [1, 2, 3] {
        let (closed, reached, closest, start) = closes_on_the_face(seed, true);
        assert!(
            !closed,
            "seed {seed}: the Mage walked toward the face ({start:.0}° -> {closest:.0}°)"
        );
        assert!(reached, "seed {seed}: the Mage never reached its own side");
        let (closed, ..) = closes_on_the_face(seed, false);
        assert!(
            closed,
            "seed {seed}: the control: excepted, it closes on the face"
        );
    }
}

/// The hunt holds while the pillar blocks a hunter's plain sight of the
/// hider, past the widened shadow's near-obstacle limit (seed 758, AS-404
/// round 3: the hunt ended with the healer 16.07yd from the pillar's centre
/// and the Warlock blind 15.42yd out, and started again with the sides
/// dealt afresh).
///
/// The hider is rooted 16.5yd west of the south-east pillar's centre. Our
/// Mage stands 15.4yd east of it, blind: the hider is farther from the
/// pillar than the Mage and more than `REPAIR_FULL` off its footprint, so it
/// is in no widened shadow of the Mage's. Our Priest sees it clear. The hunt
/// holds until the Mage sees the hider. The control: the Mage standing where
/// it sees the hider too, and the hunt ends at once.
#[test]
fn the_hunt_holds_while_the_pillar_hides_the_healer_past_the_shadow_limit() {
    let (pillar, center) = SE;
    let obstacles = obstacles();
    let hider = Vec2::new(23.5, -20.0);
    let stage = |mage: (f32, f32)| {
        Stage::new()
            .place(HIDER, hider.x, hider.y)
            .aura(HIDER, root())
            .place(MAGE, mage.0, mage.1)
            .target(MAGE, HIDER)
            .place(PRIEST, 10.0, -20.0)
            .target(PRIEST, HIDER)
            .world(hunt_under_way(pillar, 180.0, false))
    };
    let end_of = |played: &Played| {
        let landed = played.landed();
        played
            .staged_trace()
            .find(|e| {
                e["kind"] == "situation_change"
                    && e["name"] == "hunt"
                    && e["team"] == 1
                    && e["change"] == "end"
            })
            .map(|e| sim_time(e) - landed.combat)
    };
    let mage = Vec2::new(55.4, -20.0);
    assert!(
        !in_shadow(&obstacles, pillar, mage, hider),
        "the situation: beyond the widened shadow's limit"
    );
    let eye = |q: Vec2| Vec3::new(q.x, 1.0, q.y);
    assert!(!has_line_of_sight(&obstacles, eye(mage), eye(hider)));
    for seed in [1, 2, 3] {
        let played = play(two_v_one(seed), stage((mage.x, mage.y)));
        let t = track(&played, center, 25.0);
        let first_sight = t
            .frames
            .iter()
            .find(|f| f.2[0])
            .map(|f| f.0)
            .unwrap_or_else(|| panic!("seed {seed}: the Mage never saw the hider"));
        let end = end_of(&played);
        assert!(
            end.is_none_or(|e| e >= first_sight),
            "seed {seed}: the hunt ended at {end:?}s, before the Mage saw the hider \
             ({first_sight:.2}s)"
        );
        let played = play(two_v_one(seed), stage((35.0, -36.0)));
        let end = end_of(&played);
        assert!(
            end.is_some_and(|e| e < 0.1),
            "seed {seed}: the control: both in sight, the hunt ends at once ({end:?})"
        );
    }
}
