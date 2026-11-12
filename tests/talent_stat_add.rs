//! AS-447 — `StatAdd`: a talent that adds points to a stat, as an item does.
//!
//! - **every stat** a `StatAdd` can name reaches exactly its own totals, in
//!   points, and two ranks add (one test per stat);
//! - **the order**: the points join the total before a `StatPercent` takes
//!   its percentage of it;
//! - **the price**: a `StatAdd` on a weighted stat is priced at the item
//!   budget's weight for its points, at any reference combatant: +1% crit
//!   chance costs 300 × 0.01 = 3;
//! - **the prose**: each stat in its own unit;
//! - **the match**: flat crit reaches the auto-attack's crit roll. A staged
//!   Rogue with +25% crit chance from a `StatAdd` crits its swings that much
//!   more often than the same Rogue without the node.

mod common;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::{build_headless_app, PreloadedConfigs};
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{passive_movement_speed, Combatant, Pet};
use arenasim::states::play_match::talents::{
    Build, StatChange, TalentDefinitions, TalentEffect, TalentId, TalentStat,
};
use arenasim::states::talent_text::effect_text;
use common::staged::play_prepared;
use TalentId::*;

fn abilities() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

fn shipped_text() -> String {
    std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads")
}

/// Demonic Embrace's effect, the node every fixture here rewrites.
const EMBRACE: &str = "per_rank: [StatPercent(MaxHealth, 0.03)],";

/// The shipped talents with Demonic Embrace (two ranks, tier 1) doing
/// `per_rank` instead, and declaring `budget` when its price is not computed.
fn embrace_doing(per_rank: &str, budget: bool) -> TalentDefinitions {
    let text = shipped_text();
    assert!(text.contains(EMBRACE));
    let to = if budget {
        format!("budget: Some(11.0), per_rank: [{per_rank}],")
    } else {
        format!("per_rank: [{per_rank}],")
    };
    TalentDefinitions::from_ron(&text.replacen(EMBRACE, &to, 1), abilities())
        .unwrap_or_else(|e| panic!("{per_rank}: {e}"))
}

/// Every total a stat talent can touch, by name.
fn totals(c: &Combatant) -> Vec<(&'static str, f32)> {
    vec![
        ("max_health", c.max_health),
        ("max_mana", c.max_mana),
        ("mana_regen", c.mana_regen),
        ("attack_power", c.attack_power),
        ("spell_power", c.spell_power),
        ("crit_chance", c.crit_chance),
        ("base_movement_speed", c.base_movement_speed),
        ("passive_speed", c.passive_speed),
        ("armor", c.armor),
        ("fire_resistance", c.fire_resistance),
        ("frost_resistance", c.frost_resistance),
        ("shadow_resistance", c.shadow_resistance),
        ("arcane_resistance", c.arcane_resistance),
        ("nature_resistance", c.nature_resistance),
        ("holy_resistance", c.holy_resistance),
        ("attack_damage", c.attack_damage),
        ("melee_damage", c.melee_damage),
        ("offhand_damage", c.offhand_damage),
        ("dodge_chance", c.dodge_chance),
        ("parry_chance", c.parry_chance),
        ("melee_attack_speed_bonus", c.melee_attack_speed_bonus),
        ("attack_speed_bonus", c.attack_speed_bonus),
    ]
}

/// Two ranks of `StatAdd(stat, points)` on a Warlock add `2 × points` to
/// exactly the totals named in `fields`, and to nothing else; the pools refill
/// to their new pre-combat levels. Movement speed's points are a passive
/// source, so its speed is the natural speed raised by them
/// (`passive_movement_speed`), not the points added in yards.
fn assert_stat_add(stat: TalentStat, points: f32, fields: &[&str]) {
    let budget = stat.item_weight().is_none();
    let talents = embrace_doing(&format!("StatAdd({stat:?}, {points:?})"), budget);
    let resolved = talents
        .resolve(
            abilities(),
            CharacterClass::Warlock,
            &Build::of(&[(DemonicEmbrace, 2)]),
        )
        .unwrap();
    assert_eq!(
        resolved.stat_changes().iter().collect::<Vec<_>>(),
        vec![(
            &stat,
            &StatChange {
                points: points * 2.0,
                fraction: 0.0
            }
        )],
        "{stat:?}: ranks add"
    );
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    // Weapons the Warlock does not carry, so a weapon add has something to
    // land beside.
    c.attack_damage = 10.0;
    c.melee_damage = 20.0;
    c.offhand_damage = 30.0;
    c.current_health = 1.0;
    c.current_mana = 1.0;
    let before = totals(&c);
    resolved.apply(&mut c);
    let after = totals(&c);
    for ((name, was), (_, is)) in before.iter().zip(&after) {
        let want = match (stat, *name) {
            (TalentStat::MovementSpeed, "base_movement_speed") => {
                passive_movement_speed(c.natural_movement_speed, points * 2.0)
            }
            _ if fields.contains(name) => *was + points * 2.0,
            _ => *was,
        };
        assert_eq!(
            is.to_bits(),
            want.to_bits(),
            "{stat:?} {name}: {was} -> {is}"
        );
    }
    assert_eq!(c.current_health, c.max_health, "{stat:?}");
    assert_eq!(c.current_mana, c.pre_combat_resource(), "{stat:?}");
}

/// One test per stat a `StatAdd` supports, and the totals it reaches.
macro_rules! stat_add_reaches {
    ($($test:ident: $stat:ident, $points:expr => [$($field:literal),+];)+) => {
        $(
            #[test]
            fn $test() {
                assert_stat_add(TalentStat::$stat, $points, &[$($field),+]);
            }
        )+

        /// The tests above name every stat: a new one fails to compile here
        /// until it has a test.
        #[test]
        fn every_stat_has_a_stat_add_test() {
            let named = [$(TalentStat::$stat),+];
            for stat in named {
                match stat {
                    $(TalentStat::$stat)|+ => {}
                }
            }
        }
    };
}

stat_add_reaches! {
    stat_add_max_health: MaxHealth, 10.0 => ["max_health"];
    stat_add_max_mana: MaxMana, 15.0 => ["max_mana"];
    stat_add_mana_regen: ManaRegen, 1.5 => ["mana_regen"];
    stat_add_attack_power: AttackPower, 7.0 => ["attack_power"];
    stat_add_spell_power: SpellPower, 7.0 => ["spell_power"];
    stat_add_crit_chance: CritChance, 0.01 => ["crit_chance"];
    stat_add_movement_speed: MovementSpeed, 0.03 => ["passive_speed", "base_movement_speed"];
    stat_add_armor: Armor, 40.0 => ["armor"];
    stat_add_fire_resistance: FireResistance, 5.0 => ["fire_resistance"];
    stat_add_frost_resistance: FrostResistance, 5.0 => ["frost_resistance"];
    stat_add_shadow_resistance: ShadowResistance, 5.0 => ["shadow_resistance"];
    stat_add_arcane_resistance: ArcaneResistance, 5.0 => ["arcane_resistance"];
    stat_add_nature_resistance: NatureResistance, 5.0 => ["nature_resistance"];
    stat_add_holy_resistance: HolyResistance, 5.0 => ["holy_resistance"];
    stat_add_weapon_damage: WeaponDamage, 3.0 => ["attack_damage", "melee_damage", "offhand_damage"];
    stat_add_offhand_damage: OffHandDamage, 3.0 => ["offhand_damage"];
    stat_add_dodge_chance: DodgeChance, 0.04 => ["dodge_chance"];
    stat_add_parry_chance: ParryChance, 0.04 => ["parry_chance"];
    stat_add_melee_attack_speed: MeleeAttackSpeed, 0.1 => ["melee_attack_speed_bonus"];
}

/// The points join the total as an item's would, so a `StatPercent` on the
/// same stat takes its percentage of them too.
#[test]
fn points_land_before_a_percentage_of_the_total() {
    let talents = embrace_doing(
        "StatAdd(SpellPower, 10.0), StatPercent(SpellPower, 0.1)",
        false,
    );
    let resolved = talents
        .resolve(
            abilities(),
            CharacterClass::Warlock,
            &Build::of(&[(DemonicEmbrace, 1)]),
        )
        .unwrap();
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    c.spell_power = 100.0;
    resolved.apply(&mut c);
    assert_eq!(c.spell_power, (100.0 + 10.0) * (1.0 + 0.1));
}

/// A `StatAdd` on a weighted stat is priced at the item budget's weight for
/// its points, whatever the reference: +1% crit chance is 300 × 0.01 = 3, a
/// quarter of a rank (11). Mixed with a `StatPercent`, the node's price is
/// the sum.
#[test]
fn a_stat_add_is_priced_at_the_item_weight_of_its_points() {
    let crit = embrace_doing("StatAdd(CritChance, 0.01)", false);
    let mut reference = Combatant::new(1, 0, CharacterClass::Warlock);
    for reference_crit in [0.0, 0.05, 0.3] {
        reference.crit_chance = reference_crit;
        let price = crit.rank_price(DemonicEmbrace, Some(&reference)).unwrap();
        assert!((price - 3.0).abs() < 1e-5, "{price}");
    }
    let health = embrace_doing("StatAdd(MaxHealth, 11.0)", false);
    assert_eq!(
        health.rank_price(DemonicEmbrace, Some(&reference)).unwrap(),
        11.0
    );

    let mixed = embrace_doing(
        "StatAdd(CritChance, 0.01), StatPercent(MaxHealth, 0.02)",
        false,
    );
    reference.max_health = 400.0;
    let price = mixed.rank_price(DemonicEmbrace, Some(&reference)).unwrap();
    assert!((price - (3.0 + 1.0 * 0.02 * 400.0)).abs() < 1e-4, "{price}");

    // A StatAdd on a stat the budget does not weigh declares its price.
    let armor = embrace_doing("StatAdd(Armor, 40.0)", true);
    assert_eq!(armor.rank_price(DemonicEmbrace, None).unwrap(), 11.0);
}

/// Each stat in its own unit, ranks multiplied in.
#[test]
fn stat_add_prose_reads_in_each_stats_unit() {
    let text =
        |stat, points, ranks| effect_text(&TalentEffect::StatAdd(stat, points), ranks, abilities());
    assert_eq!(
        text(TalentStat::CritChance, 0.01, 1),
        "+1% critical strike chance"
    );
    assert_eq!(
        text(TalentStat::CritChance, 0.01, 2),
        "+2% critical strike chance"
    );
    assert_eq!(text(TalentStat::DodgeChance, 0.04, 2), "+8% dodge chance");
    assert_eq!(
        text(TalentStat::MeleeAttackSpeed, 0.1, 1),
        "+10% melee attack speed"
    );
    assert_eq!(text(TalentStat::MaxHealth, 10.0, 2), "+20 maximum health");
    assert_eq!(text(TalentStat::SpellPower, 7.0, 1), "+7 spell power");
    assert_eq!(
        text(TalentStat::MovementSpeed, 0.04, 2),
        "+8% movement speed"
    );
}

// ---------------------------------------------------------------------------
// The match
// ---------------------------------------------------------------------------

/// Crit chance a rank in the fixture: large, so a rise reads at a glance
/// over a few hundred swings.
const FIXTURE_CRIT: f32 = 0.25;

/// The shipped configs with Vitality (Rogue Combat, tier 2) adding
/// [`FIXTURE_CRIT`] crit chance in points instead of its health.
fn crit_fixture() -> PreloadedConfigs {
    let from = "per_rank: [StatPercent(MaxHealth, 0.05)],";
    let text = shipped_text();
    assert!(text.contains(from), "Vitality's effect moved");
    let text = text.replacen(
        from,
        &format!("per_rank: [StatAdd(CritChance, {FIXTURE_CRIT:?})],"),
        1,
    );
    let mut preloaded = PreloadedConfigs::load().expect("configs");
    preloaded.talents = TalentDefinitions::from_ron(&text, abilities()).expect("the fixture");
    preloaded
}

/// Vitality, and the two points below it that open it.
fn with_vitality() -> Build {
    Build::of(&[(LightningReflexes, 2), (Vitality, 1)])
}

/// The control: the same build without Vitality.
fn without_vitality() -> Build {
    Build::of(&[(LightningReflexes, 2)])
}

fn config(build: Build, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(seed),
        max_duration_secs: 150.0,
        team1_builds: vec![build],
        ..Default::default()
    }
}

/// The Rogue a match with `build` spawns, on the fixture.
fn spawned_rogue(preloaded: &PreloadedConfigs, build: Build) -> Combatant {
    let mut app = build_headless_app(&config(build, 1), true, Some(preloaded));
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let rogue = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 1 && c.class == CharacterClass::Rogue)
        .map(|(c, _)| c.clone());
    rogue.expect("a Rogue spawned")
}

/// The points reach the spawned Rogue exactly, gear included beneath them.
#[test]
fn flat_crit_reaches_the_spawned_combatant() {
    let preloaded = crit_fixture();
    let base = spawned_rogue(&preloaded, without_vitality());
    let talented = spawned_rogue(&preloaded, with_vitality());
    assert!(
        base.crit_chance > 0.0,
        "the Rogue's gear and base give it crit"
    );
    assert_eq!(talented.crit_chance, base.crit_chance + FIXTURE_CRIT);
    assert_eq!(
        talented.max_health, base.max_health,
        "the fixture adds no health"
    );
}

/// Units that cannot die, so the match runs its whole span.
fn unkillable() -> Stage {
    let tough = |c: &mut Combatant| {
        c.max_health = 1_000_000.0;
        c.current_health = 1_000_000.0;
    };
    Stage::new()
        .combatant(Unit::new(1, 0), tough)
        .combatant(Unit::new(2, 0), tough)
}

/// The Rogue's auto-attacks over a staged match: (crits, swings that landed).
fn rogue_auto_attacks(preloaded: &PreloadedConfigs, build: Build) -> (usize, usize) {
    let mut crits = 0;
    let mut landed = 0;
    for seed in [3, 4] {
        let played = play_prepared(config(build.clone(), seed), preloaded, unkillable());
        for line in played.log.lines() {
            let Some((_, rest)) = line.split_once("Team 1 Rogue #1's Auto Attack ") else {
                continue;
            };
            landed += 1;
            if rest.starts_with("CRITS") {
                crits += 1;
            } else {
                assert!(rest.starts_with("hits"), "{line}");
            }
        }
    }
    (crits, landed)
}

/// A Rogue with +25% crit chance from a `StatAdd` crits its auto-attacks
/// about 25 points more often than the same Rogue without it.
#[test]
fn flat_crit_raises_the_auto_attack_crit_rate() {
    let preloaded = crit_fixture();
    let (base_crits, base_swings) = rogue_auto_attacks(&preloaded, without_vitality());
    let (crits, swings) = rogue_auto_attacks(&preloaded, with_vitality());
    assert!(
        base_swings >= 150 && swings >= 150,
        "{base_swings} / {swings} swings"
    );
    let base_rate = base_crits as f32 / base_swings as f32;
    let rate = crits as f32 / swings as f32;
    assert!(
        rate - base_rate > FIXTURE_CRIT * 0.6,
        "{crits}/{swings} = {rate:.3} against {base_crits}/{base_swings} = {base_rate:.3}"
    );
    assert!(
        rate - base_rate < FIXTURE_CRIT * 1.4,
        "{crits}/{swings} = {rate:.3} against {base_crits}/{base_swings} = {base_rate:.3}"
    );
}
