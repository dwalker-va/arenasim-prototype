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
    /// Team 2's win, kept SEPARATE from `!won` because a draw is neither. The
    /// head-to-head sweeps score one implementation per side, so reading team
    /// 2's wins as `!won` silently credits every timeout draw to whichever
    /// implementation happened to be on team 2.
    won_t2: bool,
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
    /// Sim-seconds the Priest spent under CC (fear/stun/incap/poly/root). Root
    /// is included deliberately — it does not stop a cast, but a rooted healer
    /// cannot walk to the position the solve picked, which is what this sweep
    /// measures.
    priest_cc_secs: f32,
    /// Mean distance between the TWO Priests. Under the UNIFORM comparison both
    /// healers solve the same constraint set and can converge on the same cover
    /// — free value for an AoE like Psychic Scream. In a head-to-head run only
    /// one side is solving, so read this as a one-sided figure there.
    mean_healer_sep: f32,
    /// Mean distance of the Priest from its camp pillar centre, post-contact.
    /// A released camp lets the healer follow the fight; an unreleased one pins
    /// it to the hold ring (circumradius 6 + mover 0.5 + standoff 2 = 8.5yd).
    post_ring_dist: f32,
}

fn run(profile: &str, seed: u64) -> Cell {
    run_pair(profile, profile, seed)
}

/// Head-to-head: each team on its own implementation, same seed.
fn run_pair(t1: &str, t2: &str, seed: u64) -> Cell {
    run_comp(t1, t2, seed, &["Warrior", "Priest"], &["Warlock", "Priest"])
}

/// As `run_pair`, with the comps named explicitly.
fn run_comp(t1: &str, t2: &str, seed: u64, team1: &[&str], team2: &[&str]) -> Cell {
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
    let (mut sep_sum, mut sep_n) = (0.0f32, 0usize);

    let result = run_headless_match_observed(
        HeadlessMatchConfig {
            team1: team1.iter().map(|s| s.to_string()).collect(),
            team2: team2.iter().map(|s| s.to_string()).collect(),
            map: "PillaredArena".to_string(),
            team1_ai_profile: Some(t1.to_string()),
            team2_ai_profile: Some(t2.to_string()),
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
                {
                    let mut enemy_priest: Option<Vec2> = None;
                    for obs in f.combatants.values() {
                        if obs.team == 2 && !obs.is_pet && obs.class == CharacterClass::Priest && obs.alive {
                            enemy_priest = Some(Vec2::new(obs.position.x, obs.position.z));
                        }
                    }
                    if let Some(ep) = enemy_priest {
                        sep_sum += pp.distance(ep);
                        sep_n += 1;
                    }
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
        won_t2: result.winner == Some(2),
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
        mean_healer_sep: sep_sum / sep_n.max(1) as f32,
    }
}

/// THE HEAD-TO-HEAD. `run`'s uniform comparison asks "does this matchup play out
/// differently when BOTH teams change implementation" — a real question, but not
/// "is the new AI better". Only differing profiles answer that, and only a
/// per-team profile makes it expressible.
///
/// Run BOTH assignments: team 1 on TeamPlan against team 2 on Legacy, and the
/// mirror. The comps are asymmetric (`Warrior+Priest` vs `Warlock+Priest`), so a
/// single assignment would confound the AI with the comp.
#[test]
#[ignore]
fn head_to_head_team_plan_vs_legacy() {
    let seeds: Vec<u64> = (1..=12).collect();
    let (mut tp_as_t1, mut tp_as_t2) = (0usize, 0usize);
    // Draws are counted, never silently credited to a side: `!won` is not a
    // team-2 win, and at the 300s cap a Nagrand camp can genuinely draw.
    let mut draws = 0usize;
    println!("\nHead-to-head, same seed, one implementation per side");
    println!("{:>4}  {:>22}  {:>22}", "seed", "T1=TeamPlan T2=Legacy", "T1=Legacy T2=TeamPlan");
    let label = |team_plan_won: bool, drawn: bool| {
        if drawn {
            "draw"
        } else if team_plan_won {
            "TeamPlan"
        } else {
            "Legacy"
        }
    };
    for &seed in &seeds {
        // Team 1 wins => TeamPlan won this one.
        let a = run_pair("TeamPlan", "Legacy", seed);
        // Team 2 wins => TeamPlan won this one.
        let b = run_pair("Legacy", "TeamPlan", seed);
        tp_as_t1 += a.won as usize;
        tp_as_t2 += b.won_t2 as usize;
        draws += (!a.won && !a.won_t2) as usize + (!b.won && !b.won_t2) as usize;
        println!(
            "{:>4}  {:>22}  {:>22}",
            seed,
            label(a.won, !a.won && !a.won_t2),
            label(b.won_t2, !b.won && !b.won_t2),
        );
    }
    // The two comps are NOT equally strong, so raw head-to-head counts cannot be
    // compared to each other. Measure each side's GAIN against the uniform
    // Legacy-vs-Legacy baseline instead — that is comp-independent.
    let baseline: Vec<Cell> = seeds.iter().map(|&s| run("Legacy", s)).collect();
    let base_t1 = baseline.iter().filter(|c| c.won).count();
    let base_t2 = baseline.iter().filter(|c| c.won_t2).count();
    let n = seeds.len();
    let pct = |k: usize| 100.0 * k as f32 / n as f32;
    if draws > 0 {
        println!("\n({draws} of {} head-to-head runs DREW — excluded from both win counts)", 2 * n);
    }
    println!(
        "\nLegacy vs Legacy: team 1 wins {}/{} ({:.0}%) — the comps are not even, \n\
         so compare each side's GAIN, not the raw counts.\n\
         \n\
         team 1 on TeamPlan: {}/{} ({:.0}%)  -> {:+.0}pt for team 1\n\
         team 2 on TeamPlan: {}/{} ({:.0}%)  -> {:+.0}pt for team 2",
        base_t1, n, pct(base_t1),
        tp_as_t1, n, pct(tp_as_t1), pct(tp_as_t1) - pct(base_t1),
        tp_as_t2, n, pct(tp_as_t2), pct(tp_as_t2) - pct(base_t2),
    );
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
    let (mut lhs, mut ths) = (0.0f32, 0.0f32);
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
        lhs += l.mean_healer_sep;
        ths += t.mean_healer_sep;
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
         {:<28} {:>11.1}y {:>11.1}y\n\
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
        "distance BETWEEN the healers", lhs / n, ths / n,
        "Warrior died", format!("{ld}/{}", seeds.len()), format!("{td}/{}", seeds.len()),
        "losses with ZERO heal", zero_heal_losses.0, zero_heal_losses.1,
    );
    println!(
        "discordant pairs: Legacy-only {only_legacy}, TeamPlan-only {only_team_plan} \
         (n={} — below ~12 discordant this is not conclusive on its own)",
        only_legacy + only_team_plan
    );
}

/// STEP 4c: the kiter comps. The pillar-camp sweep above contains no unit on the
/// ENGAGE/KITE machine at all — the Warlock was deliberately taken off it — so it
/// cannot see the DPS half of the solve. Mage and Hunter are the two classes that
/// are on it.
///
/// Reports each side's GAIN against its own Legacy-vs-Legacy baseline, for the
/// same reason the pillar sweep does: the comps are not evenly matched, so raw
/// head-to-head counts are not comparable to each other.
#[test]
#[ignore]
fn head_to_head_kiter_comps() {
    let seeds: Vec<u64> = (1..=12).collect();
    let comps: [(&str, &[&str], &[&str]); 2] = [
        ("Mage+Priest vs Warrior+Priest", &["Mage", "Priest"], &["Warrior", "Priest"]),
        ("Hunter+Priest vs Rogue+Priest", &["Hunter", "Priest"], &["Rogue", "Priest"]),
    ];
    for (label, t1, t2) in comps {
        // Team 2's column is `won_t2`, NOT `!won` — a draw belongs to neither
        // side, and crediting it to team 2 would fake a TeamPlan gain.
        let baseline: Vec<Cell> = seeds.iter().map(|&s| run_comp("Legacy", "Legacy", s, t1, t2)).collect();
        let base = baseline.iter().filter(|c| c.won).count();
        let base_t2 = baseline.iter().filter(|c| c.won_t2).count();
        let with_tp = seeds.iter().filter(|&&s| run_comp("TeamPlan", "Legacy", s, t1, t2).won).count();
        let vs_tp = seeds
            .iter()
            .filter(|&&s| run_comp("Legacy", "TeamPlan", s, t1, t2).won_t2)
            .count();
        let n = seeds.len();
        let pct = |k: usize| 100.0 * k as f32 / n as f32;
        println!(
            "\n{label}\n  Legacy vs Legacy        team 1 {}/{} ({:.0}%), team 2 {}/{} ({:.0}%)\n\
               team 1 on TeamPlan      team 1 {}/{} ({:.0}%)  -> {:+.0}pt for the kiter side\n\
               team 2 on TeamPlan      team 2 {}/{} ({:.0}%)  -> {:+.0}pt for the other side",
            base, n, pct(base), base_t2, n, pct(base_t2),
            with_tp, n, pct(with_tp), pct(with_tp) - pct(base),
            vs_tp, n, pct(vs_tp), pct(vs_tp) - pct(base_t2),
        );
    }
}
