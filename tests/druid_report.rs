//! The Druid's healing report — a MEASUREMENT a human reads, not an assertion.
//!
//! Plays `<X>+Druid` against `<X>+Priest` for each DPS partner X (the matched
//! comp: same partner, the healer is the only difference) over a fixed seed
//! set, and prints three numbers per partner:
//!
//! - **HoT uptime on the focused ally**: the fraction of the focused ally's
//!   alive, post-gates frames on which it carried a heal over time. The focus
//!   is the Druid-team member (pets excluded) that took the most damage — the
//!   ally the enemy actually attacked. Only the Druid applies a heal over time
//!   in these comps.
//! - **Overheal fraction**: of the healing the Druid's heals would have done
//!   onto a target with room for all of it, the share that found no missing
//!   health. Read from the match log: each Druid heal line carries what
//!   landed, and what it WOULD have landed is rebuilt from the RON values, the
//!   Druid's spell power, its Lifebloom stack at the time, a critical Swiftmend
//!   and arena dampening. Healing reduction (Mortal Strike) is NOT modelled,
//!   so a heal it cut reads as overheal: the figure is an upper bound where a
//!   Mortal Strike was up.
//! - **Seconds out of mana before dampening reaches 50%**: post-gates frames,
//!   before `DAMPENING_START_SECS + DAMPENING_RAMP_SECS / 2`, on which the
//!   healer could not afford its cheapest heal (the Druid's Lifebloom, the
//!   Priest's Flash Heal). Reported for both healers, so the Druid's number has
//!   the Priest's beside it.
//!
//! Run it explicitly, with a directory for the match logs:
//!
//! ```bash
//! DRUID_REPORT_OUT=/tmp/druid_report cargo test --release --test druid_report \
//!     -- --ignored --nocapture
//! ```
//!
//! `DRUID_REPORT_SEEDS` (default 10) sets how many seeds per partner.

use std::collections::BTreeMap;

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{AuraType, Combatant};
use arenasim::states::play_match::constants::{
    CRIT_HEALING_MULTIPLIER, DAMPENING_RAMP_SECS, DAMPENING_START_SECS,
};
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::CharacterClass;

const PARTNERS: [&str; 5] = ["Warrior", "Rogue", "Mage", "Warlock", "Hunter"];
/// The countdown before the gates; log timestamps and `sim_time` include it.
const COUNTDOWN_SECS: f32 = 10.0;

fn reduction_at(log_time: f32) -> f32 {
    let since_gates = log_time - COUNTDOWN_SECS;
    ((since_gates - DAMPENING_START_SECS) / DAMPENING_RAMP_SECS).clamp(0.0, 1.0)
}

/// The Druid's spell power with its shipped loadout on.
fn druid_spell_power() -> f32 {
    let items = load_item_definitions().expect("items.ron");
    let loadouts = load_default_loadouts(&items).expect("loadouts.ron");
    let mut druid = Combatant::new(1, 0, CharacterClass::Druid);
    druid.apply_equipment(loadouts.get(CharacterClass::Druid).unwrap(), &items);
    druid.spell_power
}

/// What each Druid heal would land on a target with room for all of it,
/// before dampening.
struct DruidHeals {
    rejuvenation_tick: f32,
    lifebloom_tick_per_stack: f32,
    lifebloom_bloom_per_stack: f32,
    swiftmend: f32,
}

impl DruidHeals {
    fn new(defs: &AbilityDefinitions, sp: f32) -> Self {
        let aura = |a| defs.get_unchecked(&a).applies_aura.clone().unwrap();
        let rejuv = aura(AbilityType::Rejuvenation);
        let lifebloom = aura(AbilityType::Lifebloom);
        let bloom = lifebloom.bloom.unwrap();
        let swiftmend = defs.get_unchecked(&AbilityType::Swiftmend);
        Self {
            rejuvenation_tick: rejuv.magnitude + sp * rejuv.magnitude_coefficient,
            lifebloom_tick_per_stack: lifebloom.magnitude + sp * lifebloom.magnitude_coefficient,
            lifebloom_bloom_per_stack: bloom.heal_base + sp * bloom.heal_coefficient,
            // The base roll's midpoint: its spread is a few points either way.
            swiftmend: (swiftmend.healing_base_min + swiftmend.healing_base_max) / 2.0
                + sp * swiftmend.healing_coefficient,
        }
    }
}

/// `[  25.08s] [HEAL] Team 1 Druid #2's Rejuvenation heals Team 1 Warrior #1 for 31`
fn parse_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

fn trailing_amount(line: &str) -> Option<f32> {
    line.rsplit(" for ").next()?.trim().parse().ok()
}

/// (overheal, would-have-healed) summed over one match log.
fn overheal_from_log(log: &str, heals: &DruidHeals, druid_id: &str) -> (f32, f32) {
    // Lifebloom stack and expiry, per target id.
    let mut lifebloom: BTreeMap<String, (u8, f32)> = BTreeMap::new();
    let mut over = 0.0;
    let mut raw_total = 0.0;
    let cast_prefix = format!("{druid_id} casts Lifebloom on ");
    let heal_prefix = format!("{druid_id}'s ");
    for line in log.lines() {
        let Some(t) = parse_time(line) else { continue };
        if let Some(rest) = line.split(&cast_prefix).nth(1) {
            let target = rest.trim().to_string();
            let entry = lifebloom.entry(target).or_insert((0, 0.0));
            entry.0 = if t <= entry.1 {
                (entry.0 + 1).min(3)
            } else {
                1
            };
            entry.1 = t + 7.0;
            continue;
        }
        if !line.contains("[HEAL]") {
            continue;
        }
        let Some(body) = line.split(&heal_prefix).nth(1) else {
            continue;
        };
        let Some(actual) = trailing_amount(line) else {
            continue;
        };
        let target_of = |verb: &str| {
            body.split(verb)
                .nth(1)
                .and_then(|r| r.rsplit_once(" for ").map(|(t, _)| t.trim().to_string()))
        };
        let damp = 1.0 - reduction_at(t);
        let raw = if body.starts_with("Rejuvenation heals ") {
            heals.rejuvenation_tick
        } else if body.starts_with("Lifebloom heals ") {
            let stacks = target_of(" heals ")
                .and_then(|t| lifebloom.get(&t).copied())
                .map_or(1, |(s, _)| s);
            heals.lifebloom_tick_per_stack * f32::from(stacks)
        } else if body.starts_with("Lifebloom blooms on ") {
            let target = target_of(" blooms on ");
            let stacks = target
                .as_ref()
                .and_then(|t| lifebloom.get(t).copied())
                .map_or(1, |(s, _)| s);
            if let Some(t) = target {
                lifebloom.remove(&t);
            }
            heals.lifebloom_bloom_per_stack * f32::from(stacks)
        } else if body.starts_with("Swiftmend ") {
            let crit = body.contains("CRITICALLY");
            heals.swiftmend * if crit { CRIT_HEALING_MULTIPLIER } else { 1.0 }
        } else {
            continue;
        } * damp;
        raw_total += raw;
        over += (raw - actual).max(0.0);
    }
    (over, raw_total)
}

#[derive(Default)]
struct Totals {
    matches: u32,
    druid_wins: u32,
    hot_frames: u32,
    focus_frames: u32,
    overheal: f32,
    raw: f32,
    druid_oom_secs: f32,
    priest_oom_secs: f32,
}

#[test]
#[ignore = "a measurement a human reads; run with DRUID_REPORT_OUT set"]
fn druid_versus_priest_report() {
    let out = std::env::var("DRUID_REPORT_OUT")
        .expect("set DRUID_REPORT_OUT to the directory the match logs go in");
    std::fs::create_dir_all(&out).unwrap();
    let seeds: u64 = std::env::var("DRUID_REPORT_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let defs = AbilityDefinitions::default();
    let sp = druid_spell_power();
    let heals = DruidHeals::new(&defs, sp);
    let druid_floor = defs.get_unchecked(&AbilityType::Lifebloom).mana_cost;
    let priest_floor = defs.get_unchecked(&AbilityType::FlashHeal).mana_cost;
    let horizon = COUNTDOWN_SECS + DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5;
    const FRAME: f32 = 1.0 / 60.0;

    println!("Druid spell power {sp}; per-heal (undampened): {:.1} Rejuv tick, {:.1}/stack Lifebloom tick, {:.1}/stack bloom, {:.1} Swiftmend",
        heals.rejuvenation_tick, heals.lifebloom_tick_per_stack, heals.lifebloom_bloom_per_stack, heals.swiftmend);
    println!(
        "{:<8} {:>7} {:>9} {:>11} {:>10} {:>16} {:>17}",
        "partner",
        "matches",
        "Druid won",
        "HoT uptime",
        "overheal",
        "Druid OOM s/match",
        "Priest OOM s/match"
    );
    for partner in PARTNERS {
        let mut totals = Totals::default();
        for seed in 1..=seeds {
            let path = format!("{out}/{partner}_seed{seed}.txt");
            let config = HeadlessMatchConfig {
                team1: vec![partner.to_string(), "Druid".to_string()],
                team2: vec![partner.to_string(), "Priest".to_string()],
                random_seed: Some(seed),
                max_duration_secs: 300.0,
                output_path: Some(path.clone()),
                ..Default::default()
            };
            // Per-frame: does each team-1 member carry a heal over time, and
            // is each healer below its cheapest heal?
            let mut hot: BTreeMap<u8, (u32, u32)> = BTreeMap::new();
            let (mut druid_oom, mut priest_oom) = (0.0f32, 0.0f32);
            let result = run_headless_match_observed(config, false, None, |frame| {
                if !frame.gates_open {
                    return;
                }
                for c in frame.combatants.values().filter(|c| c.alive && !c.is_pet) {
                    if c.team == 1 {
                        let entry = hot.entry(c.slot).or_default();
                        entry.1 += 1;
                        if c.aura_types.contains(&AuraType::HealingOverTime) {
                            entry.0 += 1;
                        }
                    }
                    if frame.sim_time < horizon {
                        if c.class == CharacterClass::Druid && c.current_mana < druid_floor {
                            druid_oom += FRAME;
                        }
                        if c.class == CharacterClass::Priest && c.current_mana < priest_floor {
                            priest_oom += FRAME;
                        }
                    }
                }
            })
            .expect("match runs");

            // The focus: the team-1 member that took the most damage.
            let focus_slot = result
                .team1_combatants
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.damage_taken.partial_cmp(&b.1.damage_taken).unwrap())
                .map(|(i, _)| i as u8)
                .unwrap();
            let (with_hot, alive) = hot.get(&focus_slot).copied().unwrap_or_default();
            let log = std::fs::read_to_string(&path).expect("match log written");
            let (over, raw) = overheal_from_log(&log, &heals, "Team 1 Druid #2");

            totals.matches += 1;
            totals.druid_wins += u32::from(result.winner == Some(1));
            totals.hot_frames += with_hot;
            totals.focus_frames += alive;
            totals.overheal += over;
            totals.raw += raw;
            totals.druid_oom_secs += druid_oom;
            totals.priest_oom_secs += priest_oom;
        }
        let n = totals.matches as f32;
        println!(
            "{:<8} {:>7} {:>9} {:>10.0}% {:>9.0}% {:>16.1} {:>17.1}",
            partner,
            totals.matches,
            totals.druid_wins,
            100.0 * totals.hot_frames as f32 / totals.focus_frames.max(1) as f32,
            100.0 * totals.overheal / totals.raw.max(1.0),
            totals.druid_oom_secs / n,
            totals.priest_oom_secs / n,
        );
    }
}
