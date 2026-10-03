//! AS-196 — Hunter traps share one cooldown.
//!
//! In Classic every trap is in the client's `SpellCategory` 411 "Trap", whose
//! 15s `CategoryRecoveryTime` is the traps' only cooldown (`SpellCooldowns`
//! has `RecoveryTime` 0 for every rank of Freezing, Frost, Immolation and
//! Explosive Trap; client 1.15.9.69547). Throwing one trap puts every trap on
//! cooldown. The sim expresses that as `cooldown_category: Some(Trap)` on
//! Freezing Trap and Frost Trap in `abilities.ron`, applied by
//! `Combatant::start_cooldown`.
//!
//! Three things are pinned: the data (which abilities share the category, and
//! its length), the mechanism on one combatant, and the behaviour in real
//! matches — no Hunter throws any two traps less than 15s apart, and the trace
//! shows the shared cooldown actually turning a trap away.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{AbilityDefinitions, CooldownCategory};
use arenasim::states::play_match::Combatant;

/// The trap category's cooldown, from the client data.
const TRAP_COOLDOWN: f32 = 15.0;

/// Exactly the two traps the sim has are in the `Trap` category, at 15s.
#[test]
fn freezing_and_frost_trap_share_the_trap_category() {
    let defs = AbilityDefinitions::default();
    let members: BTreeSet<AbilityType> = defs
        .iter()
        .filter(|(_, def)| def.cooldown_category == Some(CooldownCategory::Trap))
        .map(|(ability, _)| *ability)
        .collect();
    assert_eq!(
        members,
        BTreeSet::from([AbilityType::FreezingTrap, AbilityType::FrostTrap])
    );
    for ability in members {
        assert_eq!(
            defs.get_unchecked(&ability).cooldown,
            TRAP_COOLDOWN,
            "{ability:?}"
        );
    }
}

/// Using either trap puts BOTH on the category's cooldown, each way round; an
/// ability outside the category starts only its own.
#[test]
fn either_trap_puts_both_on_cooldown() {
    let defs = AbilityDefinitions::default();
    for (used, other) in [
        (AbilityType::FreezingTrap, AbilityType::FrostTrap),
        (AbilityType::FrostTrap, AbilityType::FreezingTrap),
    ] {
        let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
        hunter.start_cooldown(used, &defs);
        assert_eq!(hunter.ability_cooldowns.get(&used), Some(&TRAP_COOLDOWN));
        assert_eq!(
            hunter.ability_cooldowns.get(&other),
            Some(&TRAP_COOLDOWN),
            "{used:?} did not put {other:?} on the shared cooldown"
        );
    }

    let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    hunter.start_cooldown(AbilityType::Disengage, &defs);
    assert_eq!(
        hunter.ability_cooldowns.keys().copied().collect::<Vec<_>>(),
        vec![AbilityType::Disengage]
    );
}

/// The category never SHORTENS a cooldown: a member already waiting longer
/// than the category's cooldown keeps its own time.
#[test]
fn the_category_never_shortens_a_longer_cooldown() {
    let defs = AbilityDefinitions::default();
    let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    hunter.start_cooldown(AbilityType::FreezingTrap, &defs);
    // Five seconds on, the Freezing Trap has 10s left; a Frost Trap could not
    // be thrown, but if the sim allowed it the category would put the Freezing
    // Trap back to 15s, never below what it had.
    *hunter
        .ability_cooldowns
        .get_mut(&AbilityType::FreezingTrap)
        .unwrap() = 40.0;
    hunter.start_cooldown(AbilityType::FrostTrap, &defs);
    assert_eq!(
        hunter.ability_cooldowns.get(&AbilityType::FreezingTrap),
        Some(&40.0)
    );
}

fn run_trace(team1: &[&str], team2: &[&str], seed: u64) -> Vec<serde_json::Value> {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path: PathBuf = tmp.path().to_path_buf();
    drop(tmp);
    let cfg = HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        max_duration_secs: 180.0,
        random_seed: Some(seed),
        ..Default::default()
    };
    run_headless_match_with(
        cfg,
        true,
        Some(TraceConfig {
            output_path: path.clone(),
        }),
    )
    .expect("headless match");
    std::fs::read_to_string(&path)
        .expect("read trace")
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn is_trap(ability: &serde_json::Value) -> bool {
    ability == "FreezingTrap" || ability == "FrostTrap"
}

/// In real matches no Hunter throws a trap within 15s of its last one,
/// whichever trap each was — and the trace shows the shared cooldown turning a
/// trap away: a trap rejected `OnCooldown` when its own last throw was 15s or
/// more ago (or never happened), so only the other trap can be holding it.
///
/// Comps where a melee comes onto the Hunter (the Frost Trap peel) while there
/// is a healer to freeze, so both traps are wanted in the same fight. The
/// floors are on totals across the set, so one seed shifting does not empty
/// the probe; each counts something the claim needs: throws of each trap, and
/// lock-outs by the other trap.
#[test]
fn no_hunter_throws_two_traps_within_the_shared_cooldown() {
    let comps: [(&[&str], &[&str]); 3] = [
        (&["Hunter", "Priest"], &["Warrior", "Priest"]),
        (&["Hunter", "Priest"], &["Rogue", "Priest"]),
        (&["Hunter", "Shaman"], &["Warrior", "Priest"]),
    ];
    let mut throws: BTreeMap<String, usize> = BTreeMap::new();
    let mut locked_by_other = 0usize;
    for (team1, team2) in comps {
        for seed in 0u64..4 {
            let events = run_trace(team1, team2, seed);
            // Per Hunter entity: the last trap it threw, and when.
            let mut last: BTreeMap<u64, (f64, String)> = BTreeMap::new();
            // Per (Hunter, trap): when it last threw that trap.
            let mut last_own: BTreeMap<(u64, String), f64> = BTreeMap::new();
            for e in &events {
                if e["kind"] != "ability_decision" || e["actor"]["class"] != "Hunter" {
                    continue;
                }
                let hunter = e["actor"]["entity_id"].as_u64().unwrap();
                let t = e["sim_time"].as_f64().unwrap();
                for c in e["candidates"].as_array().into_iter().flatten() {
                    if !is_trap(&c["ability"])
                        || c["status"] != "rejected"
                        || c["reason"].get("OnCooldown").is_none()
                    {
                        continue;
                    }
                    // Its own cooldown is over (or it was never thrown), so
                    // only the other trap's throw can be holding it.
                    let own = last_own.get(&(hunter, c["ability"].as_str().unwrap().to_string()));
                    // (A tick's margin, so a cooldown expiring this frame is
                    // not miscounted.)
                    if own.is_none_or(|own_t| t - own_t > f64::from(TRAP_COOLDOWN) + 0.1) {
                        locked_by_other += 1;
                    }
                }
                let ability = &e["outcome"]["ability"];
                if !is_trap(ability) {
                    continue;
                }
                let ability = ability.as_str().unwrap().to_string();
                if let Some((prev_t, prev)) = last.get(&hunter) {
                    assert!(
                        t - prev_t >= f64::from(TRAP_COOLDOWN) - 0.05,
                        "{team1:?} v {team2:?} seed {seed}: {ability} at {t:.2}s, {:.2}s \
                         after {prev} — inside the shared trap cooldown",
                        t - prev_t
                    );
                }
                *throws.entry(ability.clone()).or_default() += 1;
                last_own.insert((hunter, ability.clone()), t);
                last.insert(hunter, (t, ability));
            }
        }
    }
    for trap in ["FreezingTrap", "FrostTrap"] {
        assert!(
            throws.get(trap).copied().unwrap_or(0) >= 4,
            "only {throws:?} trap throws across the set: the probe went vacuous"
        );
    }
    assert!(
        locked_by_other >= 1,
        "no trap was ever turned away by the other trap's cooldown: the probe says \
         nothing about the SHARED cooldown ({throws:?})"
    );
}
