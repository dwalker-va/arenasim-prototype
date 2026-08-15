//! What does a Fear on the enemy healer actually buy, and why does the map change it?
//!
//! The step-1 CC-targeting change measured **+10pt (z=2.56) on BasicArena and
//! +2pt (z=0.44) on PillaredArena**, with the 3v3 cell flipping to -7pt. The AI
//! makes the same decision on both maps — Fears actually cast on the enemy
//! healer went 4 -> 16 on PillaredArena, the same as 6 -> 16 on BasicArena — so
//! the extra Fears simply stop converting once there are pillars.
//!
//! The hypothesis this probe tests: **a feared healer flees, and on a pillared
//! map it may flee behind a pillar**, where it is occluded from our own team and
//! can heal unmolested. If true, the Fear that opens a kill window on open
//! ground is handing the healer cover on a pillared one — and the value model is
//! missing a *displacement cost that depends on map geometry*, which no amount
//! of better `T_eff` prediction would supply.
//!
//! Two measurements, because they answer different questions:
//!
//! 1. **Within-match, before/after.** For every Fear that lands on the enemy
//!    healer, compare its occlusion during the Fear against the equal-length
//!    window immediately before it. This isolates the Fear's own effect on
//!    position from the healer's baseline tendency to hide — which it does
//!    anyway when pressured, and which a naive feared-vs-unfeared split would
//!    mistake for causation.
//! 2. **Between-policy.** Total healer-occlusion-seconds per match under
//!    `Identity` (few Fears) versus `Priced` (many). If Fear buys cover, the
//!    policy that Fears four times as often should show more of it.
//!
//! Read-only throughout, via `run_headless_match_observed`.
//!
//! ```bash
//! cargo test --release --test feared_healer_cover_probe -- --ignored --nocapture
//! ```

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::match_config::{ArenaMap, CharacterClass};
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::map_config::load_map_geometry_config;
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use bevy::prelude::{Entity, Vec3};

const FRAME_DT: f32 = 1.0 / 60.0;

/// Load the REAL PillaredArena geometry rather than restating it, so this probe
/// cannot silently drift from `assets/config/maps.ron` the way a hardcoded copy
/// would.
fn pillared_volumes() -> Vec<ObstacleVolume> {
    load_map_geometry_config()
        .expect("maps.ron must load — tests run from the crate root")
        .active_for(ArenaMap::PillaredArena)
        .volumes
        .clone()
}

/// One frame of the healer's situation.
#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f32,
    /// Fraction of living enemy (our-team) units that CANNOT see the healer.
    occluded_frac: f32,
    /// True when no living enemy of the healer can see it at all.
    fully_hidden: bool,
    feared: bool,
    /// Total health across the healer's own team, for healing delivered.
    own_team_hp: f32,
    /// Healer position, for displacement.
    healer_pos: Vec3,
    /// Distance from the NEAREST living enemy (our team) to the healer. The
    /// healer is the called kill target here, so this is our team's ability to
    /// keep damaging the thing it is trying to kill.
    nearest_enemy_dist: f32,
    /// Healer health, for damage delivered onto it.
    healer_hp: f32,
    /// Fraction of ALL living cross-team pairs whose sightline is blocked.
    /// The context a zero healer figure needs: it separates "Fear does not buy
    /// the healer cover" from "this map and profile produce no cover for anyone".
    any_pair_occluded_frac: f32,
    /// Distance from the healer to the nearest pillar centre, on the XZ plane.
    /// If the healer never goes near a pillar, it cannot hide behind one.
    healer_dist_to_nearest_pillar: f32,
}

struct MatchTrace {
    samples: Vec<Sample>,
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

/// Backwards-compatible shim for the cover report, which is PillaredArena-only.
fn run(policy: &str, seed: u64) -> MatchTrace {
    run_on("PillaredArena", policy, seed)
}

fn run_on(map: &str, policy: &str, seed: u64) -> MatchTrace {
    // Occlusion is only meaningful on the pillared map; on BasicArena the empty
    // volume list makes every sightline clear, which is correct, not a bug.
    let volumes = if map == "PillaredArena" { pillared_volumes() } else { Vec::new() };
    // Pillar centres, for "did the healer ever go near one".
    let pillar_centres: Vec<Vec3> = volumes
        .iter()
        .map(|v| match v {
            ObstacleVolume::Prism { center_xz, .. } => Vec3::new(center_xz.x, 0.0, center_xz.y),
            ObstacleVolume::Cylinder { center_xz, .. } => Vec3::new(center_xz.x, 0.0, center_xz.y),
            // An axis-aligned box has no centre field — use the midpoint.
            ObstacleVolume::Aabb { min, max } => (*min + *max) * 0.5,
        })
        .collect();
    let mut samples = Vec::new();

    run_headless_match_observed(
        HeadlessMatchConfig {
            team1: vec!["Warlock".into(), "Priest".into()],
            team2: vec!["Priest".into(), "Rogue".into()],
            map: map.to_string(),
            // The healer is the called kill target — the exact condition under
            // which step 1 was measured on both maps.
            team1_kill_target: Some(0),
            cc_policy: Some(policy.to_string()),
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            ..Default::default()
        },
        true,
        None,
        |frame| {
            // The enemy healer: team 2's Priest. Team 1 is the Warlock side.
            let Some((_, healer)) = frame
                .combatants
                .iter()
                .find(|(_, c)| c.team == 2 && !c.is_pet && c.class == CharacterClass::Priest)
            else {
                return;
            };
            if !healer.alive {
                return;
            }

            let watchers: Vec<&arenasim::headless::ObservedCombatant> = frame
                .combatants
                .values()
                .filter(|c| c.team == 1 && c.alive && !c.is_pet)
                .collect();
            if watchers.is_empty() {
                return;
            }
            let blocked = watchers
                .iter()
                .filter(|w| !has_line_of_sight(&volumes, eye(w.position), eye(healer.position)))
                .count();

            // Cross-team occlusion over every living non-pet pair.
            let living: Vec<&arenasim::headless::ObservedCombatant> =
                frame.combatants.values().filter(|c| c.alive && !c.is_pet).collect();
            let (mut pairs, mut pairs_blocked) = (0usize, 0usize);
            for a in &living {
                for b in &living {
                    if a.team >= b.team {
                        continue;
                    }
                    pairs += 1;
                    if !has_line_of_sight(&volumes, eye(a.position), eye(b.position)) {
                        pairs_blocked += 1;
                    }
                }
            }

            let nearest_pillar = pillar_centres
                .iter()
                .map(|c: &Vec3| {
                    ((healer.position.x - c.x).powi(2) + (healer.position.z - c.z).powi(2)).sqrt()
                })
                .fold(f32::INFINITY, f32::min);

            samples.push(Sample {
                t: frame.sim_time,
                healer_pos: healer.position,
                nearest_enemy_dist: watchers
                    .iter()
                    .map(|w| {
                        ((w.position.x - healer.position.x).powi(2)
                            + (w.position.z - healer.position.z).powi(2))
                        .sqrt()
                    })
                    .fold(f32::INFINITY, f32::min),
                healer_hp: healer.current_health,
                any_pair_occluded_frac: pairs_blocked as f32 / pairs.max(1) as f32,
                healer_dist_to_nearest_pillar: nearest_pillar,
                occluded_frac: blocked as f32 / watchers.len() as f32,
                fully_hidden: blocked == watchers.len(),
                feared: healer.auras.iter().any(|a| a.effect_type == AuraType::Fear),
                own_team_hp: frame
                    .combatants
                    .values()
                    .filter(|c| c.team == 2 && !c.is_pet)
                    .map(|c| c.current_health)
                    .sum(),
            });
        },
    )
    .expect("match should run");

    MatchTrace { samples }
}

/// A contiguous run of feared frames, with the equal-length window before it.
struct FearWindow {
    during: (usize, usize),
    before: (usize, usize),
    /// Equal-length window AFTER the Fear ends. This is where a displacement
    /// cost shows up: the Fear denies healing while it lasts, and the bill
    /// arrives afterwards as time spent re-closing on a target that ran.
    after: (usize, usize),
}

fn fear_windows(s: &[Sample]) -> Vec<FearWindow> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if !s[i].feared {
            i += 1;
            continue;
        }
        let start = i;
        while i < s.len() && s[i].feared {
            i += 1;
        }
        let end = i; // exclusive
        let len = end - start;
        // Equal-length window immediately before. Skipped when there is not
        // enough history — a truncated control would bias the comparison.
        if start >= len && end + len <= s.len() {
            out.push(FearWindow {
                during: (start, end),
                before: (start - len, start),
                after: (end, end + len),
            });
        }
    }
    out
}

fn mean_occ(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a {
        return 0.0;
    }
    s[a..b].iter().map(|x| x.occluded_frac).sum::<f32>() / (b - a) as f32
}

fn mean_hidden(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a {
        return 0.0;
    }
    s[a..b].iter().filter(|x| x.fully_hidden).count() as f32 / (b - a) as f32
}

/// Healing delivered to the healer's own team across a window, measured as the
/// sum of positive team-HP deltas.
fn healing_in(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a + 1 {
        return 0.0;
    }
    s[a..b].windows(2).map(|w| (w[1].own_team_hp - w[0].own_team_hp).max(0.0)).sum()
}

fn mean_dist(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a {
        return 0.0;
    }
    s[a..b].iter().map(|x| x.nearest_enemy_dist).sum::<f32>() / (b - a) as f32
}

/// Damage delivered onto the healer across a window (sum of HP drops).
fn damage_on_healer(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a + 1 {
        return 0.0;
    }
    s[a..b].windows(2).map(|w| (w[0].healer_hp - w[1].healer_hp).max(0.0)).sum()
}

/// Straight-line distance the healer ended up from where the Fear caught it.
fn net_displacement(s: &[Sample], (a, b): (usize, usize)) -> f32 {
    if b <= a + 1 {
        return 0.0;
    }
    let (p, q) = (s[a].healer_pos, s[b - 1].healer_pos);
    ((p.x - q.x).powi(2) + (p.z - q.z).powi(2)).sqrt()
}

fn seeds() -> Vec<u64> {
    (1..=24).collect()
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

// ---------------------------------------------------------------------------
// Structural invariant (always runs)
// ---------------------------------------------------------------------------

#[test]
fn the_probe_observes_fears_and_geometry() {
    let volumes = pillared_volumes();
    assert_eq!(volumes.len(), 4, "PillaredArena should carry four pillars");
    // Prove the LoS predicate actually bites on this geometry: a line straight
    // through the (-40, -20) pillar must be blocked. This is the check that
    // separates "the probe is wired correctly" from "occlusion happens to be
    // zero", which is a FINDING and must not be asserted away.
    assert!(
        !has_line_of_sight(
            &volumes,
            Vec3::new(-60.0, EYE_HEIGHT, -20.0),
            Vec3::new(60.0, EYE_HEIGHT, -20.0)
        ),
        "a sightline straight through a pillar was not blocked — LoS is broken"
    );

    let t = run("Priced", 1);
    assert!(!t.samples.is_empty(), "no samples collected — the observer is broken");
    // Scan rather than depend on one seed: whether any PARTICULAR seed produces
    // a Fear is a tuning outcome, not an invariant. Pinning seed 1 broke when
    // step 4's cost gate legitimately declined that Fear.
    let with_fear = seeds()
        .iter()
        .filter(|s| run("Priced", **s).samples.iter().any(|x| x.feared))
        .count();
    assert!(
        with_fear > 0,
        "no Fear observed on the enemy healer across {} seeds — the probe cannot \
         test anything",
        seeds().len()
    );
    let total: usize = seeds()
        .iter()
        .map(|s| fear_windows(&run("Priced", *s).samples).len())
        .sum();
    assert!(
        total > 0,
        "no Fear landed on the enemy healer across {} seeds under Priced — the \
         probe cannot test the hypothesis",
        seeds().len()
    );
}

// ---------------------------------------------------------------------------
// The measurement
// ---------------------------------------------------------------------------

#[test]
#[ignore = "measurement: prints a table, asserts nothing. Run with --nocapture"]
fn report_feared_healer_cover() {
    println!("\n=== Does fearing a healer on PillaredArena hand it cover? ===");
    println!("Warlock+Priest vs Priest+Rogue, healer called, {} seeds\n", seeds().len());

    for policy in ["Identity", "Priced"] {
        let traces: Vec<MatchTrace> = seeds().iter().map(|s| run(policy, *s)).collect();

        // --- 1. within-match, during vs the equal window before ---
        let (mut d_occ, mut b_occ, mut d_hid, mut b_hid) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut d_heal, mut b_heal) = (Vec::new(), Vec::new());
        let mut windows = 0usize;
        for t in &traces {
            for w in fear_windows(&t.samples) {
                windows += 1;
                d_occ.push(mean_occ(&t.samples, w.during));
                b_occ.push(mean_occ(&t.samples, w.before));
                d_hid.push(mean_hidden(&t.samples, w.during));
                b_hid.push(mean_hidden(&t.samples, w.before));
                d_heal.push(healing_in(&t.samples, w.during));
                b_heal.push(healing_in(&t.samples, w.before));
            }
        }

        // --- 2. whole-match occlusion, for the between-policy comparison ---
        let match_occ: Vec<f32> = traces
            .iter()
            .map(|t| t.samples.iter().map(|s| s.occluded_frac).sum::<f32>() * FRAME_DT)
            .collect();
        let match_hidden: Vec<f32> = traces
            .iter()
            .map(|t| t.samples.iter().filter(|s| s.fully_hidden).count() as f32 * FRAME_DT)
            .collect();

        println!("--- {policy} ---");
        println!("  fear windows on the healer: {windows}");
        if windows > 0 {
            println!(
                "  occluded fraction   before {:.3}  ->  during {:.3}   ({:+.3})",
                mean(&b_occ),
                mean(&d_occ),
                mean(&d_occ) - mean(&b_occ)
            );
            println!(
                "  fully hidden        before {:.3}  ->  during {:.3}   ({:+.3})",
                mean(&b_hid),
                mean(&d_hid),
                mean(&d_hid) - mean(&b_hid)
            );
            println!(
                "  healing to its team before {:>6.1}  ->  during {:>6.1}",
                mean(&b_heal),
                mean(&d_heal)
            );
        }
        println!(
            "  whole-match healer occlusion {:>6.2}s   fully-hidden {:>6.2}s",
            mean(&match_occ),
            mean(&match_hidden)
        );

        // Context for a zero: is ANYONE ever occluded, and does the healer ever
        // even approach a pillar?
        let all_pairs: Vec<f32> = traces
            .iter()
            .map(|t| t.samples.iter().map(|s| s.any_pair_occluded_frac).sum::<f32>() * FRAME_DT)
            .collect();
        let closest: Vec<f32> = traces
            .iter()
            .map(|t| {
                t.samples
                    .iter()
                    .map(|s| s.healer_dist_to_nearest_pillar)
                    .fold(f32::INFINITY, f32::min)
            })
            .collect();
        println!(
            "  ALL cross-team pairs occluded {:>6.2}s   healer's closest approach to a pillar {:>5.1}yd\n",
            mean(&all_pairs),
            mean(&closest)
        );
    }

    println!("Reading it: the hypothesis predicts occlusion RISES during fear windows");
    println!("(within-match) and that Priced — which fears ~4x as often — accumulates");
    println!("more whole-match healer occlusion than Identity. If neither holds, the");
    println!("PillaredArena gap is NOT a fear-hands-them-cover effect.");
}


// ---------------------------------------------------------------------------
// Why does the map change what a Fear is worth?
// ---------------------------------------------------------------------------

/// The cover hypothesis is refuted, so the BasicArena/PillaredArena gap is not
/// line of sight. The remaining structural difference is **size**: BasicArena is
/// a 73x43 octagon, PillaredArena a ~119yd-diameter bowl — roughly 3x the extent
/// on the short axis.
///
/// That matters here specifically because **the healer IS the called kill
/// target**. Step 1's change makes the Warlock Fear the very unit its team is
/// trying to kill, and a Fear displaces its target. In a 43yd-wide arena a
/// fleeing healer hits a wall and stays reachable; in a 119yd bowl it does not.
///
/// Prediction if that is the mechanism: net displacement and the distance our
/// team has to re-close should both be larger on PillaredArena, and the damage
/// we land on the kill target in the window AFTER the Fear should recover less.
#[test]
#[ignore = "measurement: prints a table, asserts nothing. Run with --nocapture"]
fn report_fear_displacement_cost_by_map() {
    println!("\n=== What does a Fear on the kill-target healer cost, by map? ===");
    println!("Warlock+Priest vs Priest+Rogue, healer called, {} seeds\n", seeds().len());
    println!(
        "{:<14} {:<9} {:>4} {:>8} {:>9} {:>9} {:>9} {:>8} {:>8} {:>8}",
        "map", "policy", "n", "displ", "dist_bef", "dist_dur", "dist_aft", "dmg_bef", "dmg_dur", "dmg_aft"
    );

    for map in ["BasicArena", "PillaredArena"] {
        for policy in ["Identity", "Priced"] {
            let traces: Vec<MatchTrace> = seeds().iter().map(|s| run_on(map, policy, *s)).collect();
            let (mut displ, mut db, mut dd, mut da) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let (mut hb, mut hd, mut ha) = (Vec::new(), Vec::new(), Vec::new());
            for t in &traces {
                for w in fear_windows(&t.samples) {
                    displ.push(net_displacement(&t.samples, w.during));
                    db.push(mean_dist(&t.samples, w.before));
                    dd.push(mean_dist(&t.samples, w.during));
                    da.push(mean_dist(&t.samples, w.after));
                    hb.push(damage_on_healer(&t.samples, w.before));
                    hd.push(damage_on_healer(&t.samples, w.during));
                    ha.push(damage_on_healer(&t.samples, w.after));
                }
            }
            println!(
                "{:<14} {:<9} {:>4} {:>8.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>8.1} {:>8.1}",
                map,
                policy,
                displ.len(),
                mean(&displ),
                mean(&db),
                mean(&dd),
                mean(&da),
                mean(&hb),
                mean(&hd),
                mean(&ha),
            );
        }
    }

    // Unconditional uptime: the same quantities over the WHOLE match, with no
    // reference to Fear at all. If these differ by map, the Fear windows above
    // were measuring the map's baseline rather than anything about Fear.
    println!("\n--- whole-match baseline (no reference to Fear) ---");
    println!(
        "{:<14} {:<9} {:>10} {:>12} {:>14} {:>12}",
        "map", "policy", "match_s", "mean_dist", "dmg_on_kt/s", "kt_hp_end"
    );
    for map in ["BasicArena", "PillaredArena"] {
        for policy in ["Identity", "Priced"] {
            let traces: Vec<MatchTrace> = seeds().iter().map(|s| run_on(map, policy, *s)).collect();
            let secs: Vec<f32> = traces.iter().map(|t| t.samples.len() as f32 * FRAME_DT).collect();
            let dist: Vec<f32> = traces
                .iter()
                .filter(|t| !t.samples.is_empty())
                .map(|t| {
                    t.samples.iter().map(|x| x.nearest_enemy_dist).sum::<f32>()
                        / t.samples.len() as f32
                })
                .collect();
            let dps: Vec<f32> = traces
                .iter()
                .filter(|t| t.samples.len() > 1)
                .map(|t| {
                    damage_on_healer(&t.samples, (0, t.samples.len()))
                        / (t.samples.len() as f32 * FRAME_DT)
                })
                .collect();
            let end_hp: Vec<f32> = traces
                .iter()
                .filter_map(|t| t.samples.last().map(|s| s.healer_hp))
                .collect();
            println!(
                "{:<14} {:<9} {:>10.1} {:>12.1} {:>14.2} {:>12.1}",
                map,
                policy,
                mean(&secs),
                mean(&dist),
                mean(&dps),
                mean(&end_hp)
            );
        }
    }

    println!("\ndispl    = how far the healer ended up from where the Fear caught it");
    println!("dist_*   = distance from our NEAREST unit to the healer, before/during/after");
    println!("dmg_*    = damage we landed ON the healer (our kill target) in each window");
    println!("\nThe displacement mechanism predicts: larger displ and dist on PillaredArena,");
    println!("and dmg_aft recovering less there — the Fear's bill paid in lost uptime on the");
    println!("very target we are trying to kill. If displ and dist are similar across maps,");
    println!("the size explanation fails too and the gap is somewhere else entirely.");
}
