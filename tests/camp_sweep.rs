//! Paired `Legacy` vs `TeamPlan` sweep for the Nagrand pillar-camp opener.
//!
//! Matches are deterministic, so running the same seed under both profiles is a
//! PAIRED comparison in which the AI is the only variable. `#[ignore]`d because
//! it runs 24 full matches; it is a measurement tool, not a regression gate.
//!
//! ```bash
//! cargo test --release --test camp_sweep -- --ignored --nocapture
//! ```
//!
//! Why this lives in-tree: the 2026-08-01 investigation ran its sweep from a
//! scratch script that was never committed, and the next session had to rebuild
//! it from a prose description — including re-deriving the pet-aliasing fix (the
//! decision trace reports a Felhunter's class as `"Warlock"`, so keying on
//! `(team, class)` silently pools pet and owner). Reading positions and health
//! straight off the observer sidesteps that class of bug entirely: entities are
//! keyed by `Entity`, and `is_pet` is an explicit field.

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::CharacterClass;
use bevy::prelude::{Vec2, Vec3};

/// Nagrand's four octagonal pillars (`assets/config/maps.ron`).
fn nagrand() -> Vec<ObstacleVolume> {
    [(-40.0, -20.0), (-40.0, 20.0), (40.0, -20.0), (40.0, 20.0)]
        .into_iter()
        .map(|(x, z)| ObstacleVolume::Prism {
            center_xz: Vec2::new(x, z),
            circumradius: 6.0,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        })
        .collect()
}

struct Cell {
    won: bool,
    duration: f32,
    /// Healing the Warrior received, as the sum of its positive health deltas.
    /// Measured off the observer rather than parsed out of a match log.
    heal_to_warrior: f32,
    warrior_died: bool,
    /// Share of post-gate frames where the Priest could not see the Warrior,
    /// split at first team contact — a camp is SUPPOSED to occlude before
    /// contact, so only the post-contact share is a defect.
    blocked_frac: f32,
    pre_blocked_frac: f32,
    post_blocked_frac: f32,
    /// How far the Priest was from the Warrior when the teams met.
    separation_at_contact: f32,
    /// THE STEP-3 SUCCESS CRITERION, from the design doc: occlusion-seconds per
    /// match. Sim-seconds during which the enemy Warlock had NO line to the
    /// team-1 Priest — the cover the camp is supposed to buy. Split at contact,
    /// because denying the approach is what an opener is for.
    pre_occlusion_secs: f32,
    post_occlusion_secs: f32,
    /// Mean distance from the Priest to the nearest enemy CASTER. `OccupyCover`
    /// constrains occlusion but not distance, so this is where a healer that is
    /// "in cover" can still be well inside Fear range.
    mean_caster_dist: f32,
    /// Sim-seconds the Priest spent under hard CC (fear/stun/incap/poly).
    priest_cc_secs: f32,
    /// Mean distance of the Priest from its camp pillar centre, post-contact.
    /// A released camp lets the healer follow the fight; an unreleased one pins
    /// it to the hold ring (circumradius 6 + mover 0.5 + standoff 2 = 8.5yd).
    post_ring_dist: f32,
}

fn run(profile: &str, seed: u64) -> Cell {
    let volumes = nagrand();
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);

    let (mut frames, mut blocked) = (0usize, 0usize);
    let (mut pre, mut pre_b, mut post, mut post_b) = (0usize, 0usize, 0usize, 0usize);
    let mut heal = 0.0f32;
    let mut prev_hp: Option<f32> = None;
    let mut warrior_died = false;
    let mut contact = false;
    let mut sep_at_contact = f32::NAN;
    let (mut pre_occ, mut post_occ) = (0usize, 0usize);
    let mut ring_sum = 0.0f32;
    let (mut caster_dist_sum, mut caster_dist_n) = (0.0f32, 0usize);
    let mut cc_frames = 0usize;

    let result = run_headless_match_observed(
        HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Warlock".into(), "Priest".into()],
            map: "PillaredArena".to_string(),
            ai_profile: Some(profile.to_string()),
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            ..Default::default()
        },
        true,
        None,
        |f| {
            if !f.gates_open {
                return;
            }
            let (mut priest, mut warrior, mut warlock) = (None, None, None);
            let mut enemies: Vec<Vec2> = Vec::new();
            for obs in f.combatants.values() {
                if obs.team != 1 {
                    if obs.alive {
                        enemies.push(Vec2::new(obs.position.x, obs.position.z));
                        if !obs.is_pet && obs.class == CharacterClass::Warlock {
                            warlock = Some(obs.position);
                        }
                    }
                    continue;
                }
                if obs.is_pet {
                    continue;
                }
                match obs.class {
                    CharacterClass::Priest if obs.alive => priest = Some(obs.position),
                    CharacterClass::Warrior => {
                        if !obs.alive {
                            warrior_died = true;
                        } else {
                            warrior = Some((obs.position, obs.current_health));
                        }
                    }
                    _ => {}
                }
            }
            let Some((w3, hp)) = warrior else { return };
            // Positive deltas are heals; damage moves the other way. Paired
            // across profiles, so any common-mode regen cancels.
            if let Some(p) = prev_hp {
                if hp > p {
                    heal += hp - p;
                }
            }
            prev_hp = Some(hp);

            let Some(p3) = priest else { return };
            {
                let pp = Vec2::new(p3.x, p3.z);
                // `Option`, not a f32::MAX sentinel: MAX is finite, so a frame
                // with no living enemy caster silently contributed 3.4e38.
                let mut nearest: Option<f32> = None;
                for obs in f.combatants.values() {
                    if obs.team == 1 || !obs.alive || obs.class.is_melee() {
                        continue;
                    }
                    let d = pp.distance(Vec2::new(obs.position.x, obs.position.z));
                    nearest = Some(nearest.map_or(d, |n: f32| n.min(d)));
                }
                if let Some(d) = nearest {
                    caster_dist_sum += d;
                    caster_dist_n += 1;
                }
                for obs in f.combatants.values() {
                    if obs.team != 1 || obs.is_pet || obs.class != CharacterClass::Priest {
                        continue;
                    }
                    use arenasim::states::play_match::components::AuraType::*;
                    if obs.aura_types.iter().any(|a| {
                        matches!(a, Fear | Stun | Incapacitate | Polymorph | Root)
                    }) {
                        cc_frames += 1;
                    }
                }
            }
            let (p, w) = (Vec2::new(p3.x, p3.z), Vec2::new(w3.x, w3.z));

            // Same definition the planner latches on: any enemy within
            // CAMP_ENGAGE_RADIUS of any team-1 member.
            if !contact
                && enemies
                    .iter()
                    .any(|e| e.distance(p) <= 15.0 || e.distance(w) <= 15.0)
            {
                contact = true;
                sep_at_contact = p.distance(w);
            }

            frames += 1;
            let is_blocked = !has_line_of_sight(&volumes, eye(p), eye(w));
            if is_blocked {
                blocked += 1;
            }
            let denied = warlock
                .is_some_and(|wl| !has_line_of_sight(&volumes, eye(Vec2::new(wl.x, wl.z)), eye(p)));
            if contact {
                post += 1;
                post_b += is_blocked as usize;
                post_occ += denied as usize;
                ring_sum += p.distance(Vec2::new(-40.0, -20.0));
            } else {
                pre += 1;
                pre_b += is_blocked as usize;
                pre_occ += denied as usize;
            }
        },
    )
    .expect("headless match failed");

    Cell {
        won: result.winner == Some(1),
        duration: result.match_time,
        heal_to_warrior: heal,
        warrior_died,
        blocked_frac: blocked as f32 / frames.max(1) as f32,
        pre_blocked_frac: pre_b as f32 / pre.max(1) as f32,
        post_blocked_frac: post_b as f32 / post.max(1) as f32,
        separation_at_contact: sep_at_contact,
        // The observer fires once per fixed 1/60s tick.
        pre_occlusion_secs: pre_occ as f32 / 60.0,
        post_occlusion_secs: post_occ as f32 / 60.0,
        post_ring_dist: ring_sum / post.max(1) as f32,
        mean_caster_dist: caster_dist_sum / caster_dist_n.max(1) as f32,
        priest_cc_secs: cc_frames as f32 / 60.0,
    }
}

#[test]
#[ignore]
fn paired_legacy_vs_team_plan() {
    let seeds: Vec<u64> = (1..=12).collect();
    println!(
        "\nWarrior+Priest vs Warlock+Priest on PillaredArena, seeds 1-12\n\
         {:>4}  {:>26}   {:>26}",
        "", "--------- Legacy ---------", "-------- TeamPlan --------"
    );
    println!(
        "{:>4}  {:>3} {:>6} {:>7} {:>6}   {:>3} {:>6} {:>7} {:>6}",
        "seed", "win", "secs", "heal", "block", "win", "secs", "heal", "block"
    );

    let (mut lw, mut tw) = (0usize, 0usize);
    let (mut ld, mut td) = (0usize, 0usize);
    let (mut lh, mut th) = (0.0f32, 0.0f32);
    let (mut lb, mut tb) = (0.0f32, 0.0f32);
    let (mut lpre, mut tpre) = (0.0f32, 0.0f32);
    let (mut lpost, mut tpost) = (0.0f32, 0.0f32);
    let (mut lsep, mut tsep) = (0.0f32, 0.0f32);
    let (mut lpo, mut tpo) = (0.0f32, 0.0f32);
    let (mut lqo, mut tqo) = (0.0f32, 0.0f32);
    let (mut lrd, mut trd) = (0.0f32, 0.0f32);
    let (mut lcd, mut tcd) = (0.0f32, 0.0f32);
    let (mut lcc, mut tcc) = (0.0f32, 0.0f32);
    let (mut lt, mut tt) = (0.0f32, 0.0f32);
    // McNemar discordant pairs.
    let (mut only_legacy, mut only_team_plan) = (0usize, 0usize);
    let mut zero_heal_losses = (0usize, 0usize);

    for &seed in &seeds {
        let l = run("Legacy", seed);
        let t = run("TeamPlan", seed);
        println!(
            "{:>4}  {:>3} {:>6.1} {:>7.0} {:>5.0}%   {:>3} {:>6.1} {:>7.0} {:>5.0}%",
            seed,
            if l.won { "W" } else { "L" },
            l.duration,
            l.heal_to_warrior,
            100.0 * l.blocked_frac,
            if t.won { "W" } else { "L" },
            t.duration,
            t.heal_to_warrior,
            100.0 * t.blocked_frac,
        );
        lw += l.won as usize;
        tw += t.won as usize;
        ld += l.warrior_died as usize;
        td += t.warrior_died as usize;
        lh += l.heal_to_warrior;
        th += t.heal_to_warrior;
        lb += l.blocked_frac;
        tb += t.blocked_frac;
        lpre += l.pre_blocked_frac;
        tpre += t.pre_blocked_frac;
        lpost += l.post_blocked_frac;
        tpost += t.post_blocked_frac;
        lsep += l.separation_at_contact;
        tsep += t.separation_at_contact;
        lpo += l.pre_occlusion_secs;
        tpo += t.pre_occlusion_secs;
        lqo += l.post_occlusion_secs;
        tqo += t.post_occlusion_secs;
        lrd += l.post_ring_dist;
        trd += t.post_ring_dist;
        lcd += l.mean_caster_dist;
        tcd += t.mean_caster_dist;
        lcc += l.priest_cc_secs;
        tcc += t.priest_cc_secs;
        lt += l.duration;
        tt += t.duration;
        match (l.won, t.won) {
            (true, false) => only_legacy += 1,
            (false, true) => only_team_plan += 1,
            _ => {}
        }
        if !l.won && l.heal_to_warrior == 0.0 {
            zero_heal_losses.0 += 1;
        }
        if !t.won && t.heal_to_warrior == 0.0 {
            zero_heal_losses.1 += 1;
        }
    }

    let n = seeds.len() as f32;
    println!(
        "\n{:<28} {:>12} {:>12}\n\
         {:<28} {:>12} {:>12}\n\
         {:<28} {:>11.1}s {:>11.1}s\n\
         {:<28} {:>12.0} {:>12.0}\n\
         {:<28} {:>11.0}% {:>11.0}%\n\
         {:<28} {:>11.0}% {:>11.0}%\n\
         {:<28} {:>11.0}% {:>11.0}%\n\
         {:<28} {:>11.1}y {:>11.1}y\n\
         {:<28} {:>11.1}s {:>11.1}s\n\
         {:<28} {:>11.1}s {:>11.1}s\n\
         {:<28} {:>11.1}y {:>11.1}y\n\
         {:<28} {:>11.1}y {:>11.1}y\n\
         {:<28} {:>11.1}s {:>11.1}s\n\
         {:<28} {:>12} {:>12}\n\
         {:<28} {:>12} {:>12}",
        "", "Legacy", "TeamPlan",
        "team-1 wins", format!("{lw}/{}", seeds.len()), format!("{tw}/{}", seeds.len()),
        "mean duration", lt / n, tt / n,
        "mean heal to Warrior", lh / n, th / n,
        "mean heal line occluded", 100.0 * lb / n, 100.0 * tb / n,
        "  ...before contact", 100.0 * lpre / n, 100.0 * tpre / n,
        "  ...after contact", 100.0 * lpost / n, 100.0 * tpost / n,
        "separation at contact", lsep / n, tsep / n,
        "Warlock denied Priest, pre", lpo / n, tpo / n,
        "Warlock denied Priest, post", lqo / n, tqo / n,
        "Priest dist from camp pillar", lrd / n, trd / n,
        "Priest dist to nearest caster", lcd / n, tcd / n,
        "Priest time hard-CC'd", lcc / n, tcc / n,
        "Warrior died", format!("{ld}/{}", seeds.len()), format!("{td}/{}", seeds.len()),
        "losses with ZERO heal", zero_heal_losses.0, zero_heal_losses.1,
    );
    println!(
        "discordant pairs: Legacy-only {only_legacy}, TeamPlan-only {only_team_plan} \
         (n={} — below ~12 discordant this is not conclusive on its own)",
        only_legacy + only_team_plan
    );
}
