//! Talents (AS-297): the table's validation, its prices, how a build resolves,
//! and that BOTH spawn paths carry a resolved build into the match.
//!
//! The spawn tests play a fixture table whose Demonic Embrace also PATCHES
//! Shadow Bolt, so the spawned Warlock's book differs from its class's base
//! book: a spawn that handed a combatant the base book (or the shipped one)
//! instead of its resolved build's fails them, on either path.

use std::collections::BTreeMap;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::log::CombatLog;
use arenasim::combat::CombatPlugin;
use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::{build_headless_app, PreloadedConfigs};
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::ability_config::{AbilitiesConfig, AbilityDefinitions};
use arenasim::states::play_match::components::{Combatant, Pet, PetType};
use arenasim::states::play_match::equipment::{
    load_default_loadouts, load_item_definitions, EquipmentPlugin,
};
use arenasim::states::play_match::talents::{
    AbilityField, Build, TalentDefinitions, TalentId, TalentStat, CAPSTONE_PRICE,
    MAX_TALENT_POINTS, PRICE_TOLERANCE, RANK_PRICE,
};
use arenasim::states::play_match::{
    AbilityConfigPlugin, AbilityType, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};

use TalentId::*;

fn shipped_text() -> String {
    std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads")
}

fn abilities() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

/// The shipped talents file with `from` replaced by `to` (which must occur).
fn edited(from: &str, to: &str) -> String {
    let text = shipped_text();
    assert!(text.contains(from), "the shipped file has no {from:?}");
    text.replacen(from, to, 1)
}

fn load_err(text: &str, abilities: &AbilityDefinitions) -> String {
    TalentDefinitions::from_ron(text, abilities).expect_err("the edit is rejected")
}

// The fixture: Demonic Embrace additionally shortens Shadow Bolt's cast by
// 0.25s a rank, so it declares a price.
const EMBRACE: &str = "per_rank: [StatPercent(MaxHealth, 0.03)],";
const EMBRACE_PATCHING: &str = "budget: Some(11.0), per_rank: [StatPercent(MaxHealth, 0.03), \
     Patch(on: Ability(Shadowbolt), field: CastTime, op: Add(-0.25))],";

fn patching_fixture() -> TalentDefinitions {
    TalentDefinitions::from_ron(&edited(EMBRACE, EMBRACE_PATCHING), abilities())
        .expect("the fixture validates")
}

fn demonology_build() -> Build {
    Build::of(&[(DemonicEmbrace, 2), (FelStamina, 2), (UnholyPower, 2)])
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

#[test]
fn the_shipped_table_loads_and_names_every_talent() {
    let talents = TalentDefinitions::shipped();
    let ids: Vec<TalentId> = talents.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, TalentId::all().to_vec());
    for (id, talent) in talents.iter() {
        assert!(
            arenasim::paths::asset_path(&talent.icon).is_file(),
            "{id:?}'s icon {} is missing",
            talent.icon
        );
    }
    for class in CharacterClass::all() {
        for tree in talents.trees(*class) {
            assert!(
                arenasim::paths::asset_path(&tree.icon).is_file(),
                "{class:?}'s {:?} icon is missing",
                tree.id
            );
        }
    }
}

/// The Demonology tier 1-2 proof's nodes, as the card writes them.
#[test]
fn demonology_tier_one_and_two_are_as_specified() {
    let talents = TalentDefinitions::shipped();
    let node = |id| talents.get(id).unwrap();
    let rows: Vec<(TalentId, u8, u8, u8)> = [DemonicEmbrace, FelStamina, UnholyPower]
        .into_iter()
        .map(|id| (id, node(id).tier, node(id).column, node(id).max_rank))
        .collect();
    assert_eq!(
        rows,
        vec![
            (DemonicEmbrace, 1, 0, 2),
            (FelStamina, 1, 2, 2),
            (UnholyPower, 2, 0, 2)
        ]
    );
    use arenasim::states::play_match::talents::TalentEffect::*;
    assert_eq!(
        node(DemonicEmbrace).per_rank,
        vec![StatPercent(TalentStat::MaxHealth, 0.03)]
    );
    assert_eq!(
        node(FelStamina).per_rank,
        vec![PetStatPercent(TalentStat::MaxHealth, 0.10)]
    );
    assert_eq!(
        node(UnholyPower).per_rank,
        vec![PetStatPercent(TalentStat::WeaponDamage, 0.10)]
    );
}

/// Each structural rule of the table, broken one at a time.
#[test]
fn the_loader_rejects_each_broken_rule() {
    let defs = abilities();
    let cases: &[(&str, &str, &str)] = &[
        // A node in a tree its class does not have.
        (
            "class: Warlock, tree: Demonology, tier: 1, column: 0",
            "class: Mage, tree: Demonology, tier: 1, column: 0",
            "is not one of Mage's",
        ),
        ("tier: 1, column: 0", "tier: 5, column: 0", "tier 5 is not 1 to 4"),
        ("tier: 1, column: 0", "tier: 1, column: 3", "column 3 is not 0 to 2"),
        ("tier: 1, column: 2", "tier: 1, column: 0", "sits at tier 1 column 0"),
        ("max_rank: 2,\n            // +3%", "max_rank: 3,\n            // +3%", "max_rank 3"),
        // A computed node declares no price; every other node does.
        (EMBRACE, "budget: Some(11.0), per_rank: [StatPercent(MaxHealth, 0.03)],", "declares a budget"),
        (
            "budget: Some(11.0),\n            // Felhunter +10% maximum health",
            "// Felhunter +10% maximum health",
            "declares no budget",
        ),
        // StatPercent on an unweighted stat has no computed price.
        (EMBRACE, "per_rank: [StatPercent(Armor, 0.03)],", "declares no budget"),
        (EMBRACE, "per_rank: [StatPercent(MaxHealth, 0.0)],", "not positive"),
        (EMBRACE, "per_rank: [],", "per_rank is empty"),
        // A prerequisite: same tree, a lower tier, a rank it has.
        (
            "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter melee",
            "max_rank: 2, requires: Some((FelStamina, 3)),\n            budget: Some(11.0),\n            // Felhunter melee",
            "requires FelStamina at rank 3",
        ),
        (
            "max_rank: 2,\n            // +3%",
            "max_rank: 2, requires: Some((UnholyPower, 1)),\n            // +3%",
            "not below its own tier",
        ),
        // A grant names a talent ability.
        (
            "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
            "per_rank: [Grant(DeathCoil)],",
            "not marked `talent: true`",
        ),
        // A patch needs a target that has its field.
        (
            "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
            "per_rank: [Patch(on: Ability(Frostbolt), field: CastTime, op: Add(-0.1))],",
            "which no Warlock can have",
        ),
        (
            "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
            "per_rank: [Patch(on: Ability(Shadowbolt), field: ChannelDuration, op: Add(1.0))],",
            "no Warlock ability there has it",
        ),
        (
            "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
            "per_rank: [Patch(on: School(Holy), field: CastTime, op: Mul(0.9))],",
            "no Warlock ability there has it",
        ),
        (
            "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
            "per_rank: [Patch(on: Ability(Shadowbolt), field: CastTime, op: Mul(0.0))],",
            "not positive",
        ),
        // Every talent has an entry.
        ("        FelStamina: (", "        FelStaminaGone: (", "parse error"),
    ];
    for (from, to, expect) in cases {
        let err = load_err(&edited(from, to), defs);
        assert!(err.contains(expect), "{to:?}: {err}");
    }

    // A PetStatPercent on a class with no pet.
    let mage = edited(
        "Warlock: [",
        "Mage: [(id: Demonology, name: \"D\", icon: \"x\")], Warlock: [",
    )
    .replacen(
        "class: Warlock, tree: Demonology, tier: 1, column: 2",
        "class: Mage, tree: Demonology, tier: 1, column: 2",
        1,
    );
    assert!(load_err(&mage, defs).contains("Mage has no pet"));

    // A talent with no entry.
    let text = shipped_text();
    let (start, end) = (
        text.find("        FelStamina: (").unwrap(),
        text.find("        UnholyPower: (").unwrap(),
    );
    let gone = format!("{}{}", &text[..start], &text[end..]);
    assert!(load_err(&gone, defs).contains("no entry for FelStamina"));

    // The trees themselves.
    let twice = edited(
        "Warlock: [",
        "Warlock: [(id: Demonology, name: \"X\", icon: \"y\"), ",
    );
    assert!(load_err(&twice, defs).contains("lists the tree Demonology twice"));
    let no_icon = edited(
        "icon: \"icons/talents/spell_shadow_metamorphosis.jpg\"),",
        "icon: \"\"),",
    );
    assert!(load_err(&no_icon, defs).contains("tree has no icon"));
    let node_no_icon = edited(
        "icon: \"icons/talents/spell_shadow_antishadow.jpg\",",
        "icon: \"\",",
    );
    assert!(load_err(&node_no_icon, defs).contains("FelStamina: has no icon"));
    let add_inf = edited(
        "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
        "per_rank: [Patch(on: Ability(Shadowbolt), field: CastTime, op: Add(inf))],",
    );
    assert!(load_err(&add_inf, defs).contains("not finite"));

    // Fel Stamina moved to a Hunter tree of the same name.
    let hunter = |per_rank: &str| {
        edited(
            "Warlock: [",
            "Hunter: [(id: Demonology, name: \"D\", icon: \"x\")], Warlock: [",
        )
        .replacen(
            "class: Warlock, tree: Demonology, tier: 1, column: 2",
            "class: Hunter, tree: Demonology, tier: 1, column: 2",
            1,
        )
        .replacen("per_rank: [PetStatPercent(MaxHealth, 0.10)],", per_rank, 1)
    };
    // A prerequisite in another class's tree of the same name.
    let across = hunter("per_rank: [PetStatPercent(MaxHealth, 0.10)],").replacen(
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter melee",
        "max_rank: 2, requires: Some((FelStamina, 1)),\n            budget: Some(11.0),\n            // Felhunter melee",
        1,
    );
    assert!(load_err(&across, defs).contains("from another tree"));
    // A cooldown category has one cooldown: no patch takes one member's.
    let trap =
        hunter("per_rank: [Patch(on: Ability(FreezingTrap), field: Cooldown, op: Add(-1.0))],");
    assert!(load_err(&trap, defs).contains("a cooldown category's member"));
    // The control: the same Hunter node patching an uncategorised cooldown.
    TalentDefinitions::from_ron(
        &hunter("per_rank: [Patch(on: Ability(AimedShot), field: Cooldown, op: Add(-1.0))],"),
        defs,
    )
    .expect("an uncategorised cooldown patches");

    // A grant of another class's talent ability.
    let frost = flagged_abilities(AbilityType::Frostbolt);
    let grant = edited(
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter +10%",
        "max_rank: 1,\n            budget: Some(11.0),\n            // Felhunter +10%",
    )
    .replacen(
        "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
        "per_rank: [Grant(Frostbolt)],",
        1,
    );
    assert!(load_err(&grant, &frost).contains("a Mage ability, to a Warlock"));
}

/// A reference combatant is three Gauntlet picks its class can wear.
#[test]
fn a_reference_combatant_is_three_picks_its_class_wears() {
    let items = load_item_definitions().unwrap();
    let loadouts = load_default_loadouts(&items).unwrap();
    let reference = |picks: &str| {
        let text = edited(
            "Head: ArcanistCrown,\n            Chest: ArcanistRobes,\n            Legs: ArcanistLeggings,",
            picks,
        );
        TalentDefinitions::from_ron(&text, abilities())
            .unwrap()
            .reference_combatant(CharacterClass::Warlock, abilities(), &items, &loadouts)
            .map(|c| c.max_health)
    };
    assert!(reference("Head: ArcanistCrown, Chest: ArcanistRobes,")
        .unwrap_err()
        .contains("names 2 picks"));
    assert!(
        reference("Head: MagistersCrown, Chest: ArcanistRobes, Legs: ArcanistLeggings,")
            .unwrap_err()
            .contains("starting-kit gear")
    );
    assert!(
        reference("Head: LionheartHelm, Chest: ArcanistRobes, Legs: ArcanistLeggings,")
            .unwrap_err()
            .contains("cannot equip")
    );
    assert!(TalentDefinitions::shipped()
        .reference_combatant(CharacterClass::Mage, abilities(), &items, &loadouts)
        .map(|c| c.max_health)
        .unwrap_err()
        .contains("no reference combatant"));
    // The shipped picks, worn over the kit.
    let shipped = reference(
        "Head: ArcanistCrown,\n            Chest: ArcanistRobes,\n            Legs: ArcanistLeggings,",
    )
    .unwrap();
    let mut kit = Combatant::new(1, 0, CharacterClass::Warlock);
    kit.apply_equipment(loadouts.get(CharacterClass::Warlock).unwrap(), &items);
    let gain: f32 = [
        arenasim::states::play_match::equipment::ItemId::ArcanistCrown,
        arenasim::states::play_match::equipment::ItemId::ArcanistRobes,
        arenasim::states::play_match::equipment::ItemId::ArcanistLeggings,
    ]
    .iter()
    .map(|i| items.get(i).unwrap().max_health)
    .sum::<f32>()
        - [
            arenasim::states::play_match::equipment::ItemId::MagistersCrown,
            arenasim::states::play_match::equipment::ItemId::MagistersRobes,
            arenasim::states::play_match::equipment::ItemId::MagistersLeggings,
        ]
        .iter()
        .map(|i| items.get(i).unwrap().max_health)
        .sum::<f32>();
    assert_eq!(shipped, kit.max_health + gain);
}

/// A computed price is the item weight of each stat times its percentage of
/// the reference's total: Spell Power weighs 1.5 a point.
#[test]
fn a_computed_price_uses_each_stats_item_weight() {
    let text = edited(EMBRACE, "per_rank: [StatPercent(SpellPower, 0.05)],");
    let talents = TalentDefinitions::from_ron(&text, abilities()).unwrap();
    let mut reference = Combatant::new(1, 0, CharacterClass::Warlock);
    reference.spell_power = 140.0;
    let price = talents
        .rank_price(DemonicEmbrace, Some(&reference))
        .unwrap();
    assert_eq!(price, 1.5 * 0.05 * 140.0);
}

/// A stat percentage refills the pool it raises, and a weapon percentage
/// reaches every swing.
#[test]
fn stat_percents_refill_pools_and_reach_every_weapon() {
    let text = edited(
        EMBRACE,
        "budget: Some(11.0), per_rank: [StatPercent(MaxMana, 0.1), StatPercent(WeaponDamage, 0.1)],",
    );
    let talents = TalentDefinitions::from_ron(&text, abilities()).unwrap();
    let resolved = talents
        .resolve(
            abilities(),
            CharacterClass::Warlock,
            &Build::of(&[(DemonicEmbrace, 1)]),
        )
        .unwrap();
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    c.current_mana = 1.0;
    c.attack_damage = 10.0;
    c.melee_damage = 20.0;
    c.offhand_damage = 30.0;
    let max_mana = c.max_mana;
    resolved.apply(&mut c);
    assert_eq!(c.max_mana, max_mana * 1.1);
    assert_eq!(c.current_mana, c.max_mana);
    assert_eq!(
        (c.attack_damage, c.melee_damage, c.offhand_damage),
        (10.0 * 1.1, 20.0 * 1.1, 30.0 * 1.1)
    );
}

/// A talent ability is in no base book, and is granted by exactly one node.
#[test]
fn a_talent_ability_is_granted_by_exactly_one_node() {
    let flagged = flagged_abilities(AbilityType::DeathCoil);
    // Granted by nobody: rejected.
    let err = load_err(&shipped_text(), &flagged);
    assert!(err.contains("granted by []"), "{err}");
    // Granted by one node: accepted.
    let one = edited(
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter +10%",
        "max_rank: 1,\n            budget: Some(11.0),\n            // Felhunter +10%",
    )
    .replacen(
        "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
        "per_rank: [Grant(DeathCoil)],",
        1,
    );
    TalentDefinitions::from_ron(&one, &flagged).expect("one grant validates");
    // Granted by two: rejected.
    let two = one.replacen(
        "per_rank: [PetStatPercent(WeaponDamage, 0.10)],",
        "per_rank: [Grant(DeathCoil)],",
        1,
    );
    let two = two.replacen(
        "tier: 2, column: 0,\n            icon: \"icons/talents/spell_shadow_shadowworddominate.jpg\",\n            max_rank: 2,",
        "tier: 2, column: 0,\n            icon: \"icons/talents/spell_shadow_shadowworddominate.jpg\",\n            max_rank: 1,",
        1,
    );
    let err = load_err(&two, &flagged);
    assert!(
        err.contains("granted by [FelStamina, UnholyPower]"),
        "{err}"
    );
    // A grant takes one rank.
    let ranked = one.replacen(
        "max_rank: 1,\n            budget: Some(11.0),\n            // Felhunter +10%",
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter +10%",
        1,
    );
    assert!(load_err(&ranked, &flagged).contains("grants an ability has one rank"));
}

/// The shipped ability table with `ability` marked `talent: true`.
fn flagged_abilities(ability: AbilityType) -> AbilityDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/abilities.ron"))
        .expect("abilities.ron reads");
    let mut config: AbilitiesConfig = ron::from_str(&text).expect("abilities.ron parses");
    config.abilities.get_mut(&ability).unwrap().talent = true;
    AbilityDefinitions::new(config)
}

// ---------------------------------------------------------------------------
// Builds
// ---------------------------------------------------------------------------

#[test]
fn a_build_is_legal_only_within_the_tree_rules() {
    let talents = TalentDefinitions::shipped();
    let warlock = CharacterClass::Warlock;
    let ok = |b: Build| talents.validate_build(warlock, &b).map(|_| ());
    let err = |b: Build| talents.validate_build(warlock, &b).unwrap_err();
    assert_eq!(ok(Build::default()), Ok(()));
    assert_eq!(ok(demonology_build()), Ok(()));
    assert_eq!(
        ok(Build::of(&[
            (DemonicEmbrace, 1),
            (FelStamina, 1),
            (UnholyPower, 1)
        ])),
        Ok(())
    );
    // Tier 2 opens at two points in tier 1.
    assert!(err(Build::of(&[(DemonicEmbrace, 1), (UnholyPower, 1)])).contains("opens at 2"));
    assert!(err(Build::of(&[(UnholyPower, 2)])).contains("opens at 2"));
    // Ranks are 1..=max_rank; a rank of 0 is not written.
    assert!(err(Build::of(&[(DemonicEmbrace, 3)])).contains("ranks 1 to 2"));
    assert!(err(Build::of(&[(DemonicEmbrace, 0)])).contains("ranks 1 to 2"));
    // Another class's talents.
    assert!(talents
        .validate_build(CharacterClass::Mage, &Build::of(&[(DemonicEmbrace, 1)]))
        .unwrap_err()
        .contains("not a Mage's"));
    assert_eq!(MAX_TALENT_POINTS, 8);
}

/// A prerequisite and the points cap, on a fixture where both bite.
#[test]
fn prerequisites_and_the_points_cap_are_enforced() {
    // Unholy Power requires Fel Stamina 2/2 here.
    let text = edited(
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter melee",
        "max_rank: 2, requires: Some((FelStamina, 2)),\n            budget: Some(11.0),\n            // Felhunter melee",
    );
    let talents = TalentDefinitions::from_ron(&text, abilities()).unwrap();
    let warlock = CharacterClass::Warlock;
    assert!(talents
        .validate_build(
            warlock,
            &Build::of(&[(DemonicEmbrace, 2), (UnholyPower, 1)])
        )
        .unwrap_err()
        .contains("requires FelStamina at rank 2"));
    assert!(talents
        .validate_build(warlock, &Build::of(&[(FelStamina, 2), (UnholyPower, 1)]))
        .is_ok());
    // Six ranks exist, so no seventh point can be spent.
    assert!(TalentDefinitions::shipped()
        .legal_builds(warlock, 7)
        .is_empty());
}

/// An empty build changes nothing: the base book, no stat, and a combatant
/// it is applied to is bit-identical.
#[test]
fn an_empty_build_resolves_to_the_untalented_combatant() {
    let defs = abilities();
    let resolved = TalentDefinitions::shipped()
        .resolve(defs, CharacterClass::Warlock, &Build::default())
        .unwrap();
    let base = AbilityBook::for_class(defs, CharacterClass::Warlock);
    assert_eq!(books(resolved.book()), books(&base));
    assert!(resolved.stat_percents().is_empty() && resolved.pet_stat_percents().is_empty());
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    c.current_health = 1.0;
    let before = stat_bits(&c);
    resolved.apply(&mut c);
    resolved.apply_to_pet(&mut c);
    assert_eq!(stat_bits(&c), before);
}

/// Every stat a talent can touch, and the pools a stat talent refills, bit
/// for bit.
fn stat_bits(c: &Combatant) -> Vec<u32> {
    [
        c.max_health,
        c.current_health,
        c.max_mana,
        c.current_mana,
        c.mana_regen,
        c.attack_power,
        c.spell_power,
        c.crit_chance,
        c.base_movement_speed,
        c.armor,
        c.fire_resistance,
        c.frost_resistance,
        c.shadow_resistance,
        c.arcane_resistance,
        c.nature_resistance,
        c.holy_resistance,
        c.attack_damage,
        c.melee_damage,
        c.offhand_damage,
    ]
    .map(f32::to_bits)
    .to_vec()
}

fn books(book: &AbilityBook) -> Vec<String> {
    book.iter().map(|(a, c)| format!("{a:?} {c:?}")).collect()
}

/// Ranks add their percentages; a patch's adds land before its multiplies.
#[test]
fn a_build_resolves_its_ranks_and_patches() {
    let defs = abilities();
    let base_cast = defs.get_unchecked(&AbilityType::Shadowbolt).cast_time;
    // Embrace: Add(-0.25) a rank; Fel Stamina: Mul(0.5) on every Shadow spell.
    let text = edited(EMBRACE, EMBRACE_PATCHING).replacen(
        "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
        "per_rank: [PetStatPercent(MaxHealth, 0.10), Patch(on: School(Shadow), field: CastTime, op: Mul(0.5))],",
        1,
    );
    let talents = TalentDefinitions::from_ron(&text, defs).unwrap();
    let resolved = talents
        .resolve(
            defs,
            CharacterClass::Warlock,
            &Build::of(&[(DemonicEmbrace, 2), (FelStamina, 1)]),
        )
        .unwrap();
    let cast = resolved
        .book()
        .get_unchecked(&AbilityType::Shadowbolt)
        .cast_time;
    assert_eq!(cast, (base_cast - 0.5) * 0.5, "adds, then multiplies");
    // The school patch reaches every Shadow ability with a cast time, and
    // nothing else.
    for (ability, config) in resolved.book().iter() {
        let base = defs.get_unchecked(ability).cast_time;
        let want = if *ability == AbilityType::Shadowbolt {
            (base - 0.5) * 0.5
        } else if config.spell_school == arenasim::states::play_match::SpellSchool::Shadow {
            base * 0.5
        } else {
            base
        };
        assert_eq!(config.cast_time, want, "{ability:?}");
    }
    assert_eq!(resolved.stat_percents()[&TalentStat::MaxHealth], 0.06);
    assert_eq!(resolved.pet_stat_percents()[&TalentStat::MaxHealth], 0.10);
    // A patch that drives a field below zero is refused.
    let negative = edited(
        EMBRACE,
        "budget: Some(11.0), per_rank: [Patch(on: Ability(Shadowbolt), field: CastTime, op: Add(-1.5))],",
    );
    let talents = TalentDefinitions::from_ron(&negative, defs).unwrap();
    assert!(talents
        .resolve(
            defs,
            CharacterClass::Warlock,
            &Build::of(&[(DemonicEmbrace, 2)])
        )
        .unwrap_err()
        .contains("below zero"));
}

/// A granted ability joins the book, and a patch reaches it.
#[test]
fn a_grant_adds_its_ability_to_the_book() {
    let flagged = flagged_abilities(AbilityType::DeathCoil);
    let text = edited(
        "max_rank: 2,\n            budget: Some(11.0),\n            // Felhunter +10%",
        "max_rank: 1,\n            budget: Some(11.0),\n            // Felhunter +10%",
    )
    .replacen(
        "per_rank: [PetStatPercent(MaxHealth, 0.10)],",
        "per_rank: [Grant(DeathCoil)],",
        1,
    )
    .replacen(
        EMBRACE,
        "budget: Some(11.0), per_rank: [Patch(on: Ability(DeathCoil), field: Cooldown, op: Add(-5.0))],",
        1,
    );
    let talents = TalentDefinitions::from_ron(&text, &flagged).unwrap();
    let warlock = CharacterClass::Warlock;
    let base = talents
        .resolve(&flagged, warlock, &Build::default())
        .unwrap();
    assert!(!base.book().contains(&AbilityType::DeathCoil));
    let granted = talents
        .resolve(
            &flagged,
            warlock,
            &Build::of(&[(FelStamina, 1), (DemonicEmbrace, 1)]),
        )
        .unwrap();
    let coil = granted.book().get_unchecked(&AbilityType::DeathCoil);
    assert_eq!(
        coil.cooldown,
        flagged.get_unchecked(&AbilityType::DeathCoil).cooldown - 5.0
    );
}

// ---------------------------------------------------------------------------
// Prices
// ---------------------------------------------------------------------------

/// Every class with a computed node has a reference combatant: three tier-1
/// picks it can wear, over its starting kit.
fn references() -> BTreeMap<CharacterClass, Combatant> {
    let talents = TalentDefinitions::shipped();
    let items = load_item_definitions().unwrap();
    let loadouts = load_default_loadouts(&items).unwrap();
    let needing: Vec<CharacterClass> = CharacterClass::all()
        .iter()
        .copied()
        .filter(|c| talents.talents_for_class(*c).any(|(_, t)| t.is_computed()))
        .collect();
    assert_eq!(talents.reference_classes().collect::<Vec<_>>(), needing);
    needing
        .into_iter()
        .map(|class| {
            let reference = talents
                .reference_combatant(class, abilities(), &items, &loadouts)
                .unwrap_or_else(|e| panic!("{e}"));
            (class, reference)
        })
        .collect()
}

#[test]
fn every_rank_prices_at_eleven_and_every_capstone_at_twenty_two() {
    let talents = TalentDefinitions::shipped();
    let refs = references();
    let mut off = Vec::new();
    for (id, talent) in talents.iter() {
        let price = talents.rank_price(id, refs.get(&talent.class)).unwrap();
        let target = talent.target_price();
        if (price - target).abs() > target * PRICE_TOLERANCE {
            off.push(format!("{id:?}: {price:.2} (target {target})"));
        }
    }
    assert!(off.is_empty(), "{}", off.join("\n"));
    assert_eq!((RANK_PRICE, CAPSTONE_PRICE), (11.0, 22.0));
}

/// Demonic Embrace's computed price: 3% of the reference Warlock's health at
/// the item budget's 1.0 a point.
#[test]
fn a_stat_percent_is_priced_at_the_reference_combatant() {
    let talents = TalentDefinitions::shipped();
    let refs = references();
    let warlock = &refs[&CharacterClass::Warlock];
    let starting = Combatant::new(1, 0, CharacterClass::Warlock);
    assert!(
        warlock.max_health > starting.max_health,
        "the reference wears its kit and picks"
    );
    let price = talents.rank_price(DemonicEmbrace, Some(warlock)).unwrap();
    assert_eq!(price, 0.03 * warlock.max_health);
    // A declared price ignores the reference.
    assert_eq!(talents.rank_price(FelStamina, None).unwrap(), 11.0);
    assert!(talents.rank_price(DemonicEmbrace, None).is_err());
}

/// Every legal build at each point count from 2 to 8 costs within one rank of
/// every other at that count.
#[test]
fn every_legal_build_at_a_point_count_costs_within_one_rank() {
    let talents = TalentDefinitions::shipped();
    let refs = references();
    let mut checked = 0;
    for class in CharacterClass::all() {
        let prices: BTreeMap<TalentId, f32> = talents
            .talents_for_class(*class)
            .map(|(id, _)| (id, talents.rank_price(id, refs.get(class)).unwrap()))
            .collect();
        for points in 2..=MAX_TALENT_POINTS {
            let costs: Vec<f32> = talents
                .legal_builds(*class, points)
                .iter()
                .map(|b| {
                    b.ranks
                        .iter()
                        .map(|(id, r)| prices[id] * f32::from(*r))
                        .sum()
                })
                .collect();
            if costs.is_empty() {
                continue;
            }
            checked += costs.len();
            let (lo, hi) = costs
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
            assert!(
                hi - lo <= RANK_PRICE + 1e-3,
                "{class:?} at {points} points: builds cost {lo:.2} to {hi:.2}"
            );
        }
    }
    assert!(checked > 0, "no legal build was priced");
}

/// The enumeration finds every legal build, and only legal ones: the
/// Demonology tier 1-2 nodes at each count, by set equality.
#[test]
fn legal_builds_are_enumerated_exactly() {
    let talents = TalentDefinitions::shipped();
    let names = |points| -> std::collections::BTreeSet<Vec<(TalentId, u8)>> {
        talents
            .legal_builds(CharacterClass::Warlock, points)
            .into_iter()
            .map(|b| b.ranks.into_iter().collect())
            .collect()
    };
    assert_eq!(
        names(2),
        [
            vec![(DemonicEmbrace, 2)],
            vec![(DemonicEmbrace, 1), (FelStamina, 1)],
            vec![(FelStamina, 2)],
        ]
        .into_iter()
        .collect()
    );
    // Three points: tier 2 opens only after two in tier 1.
    assert_eq!(names(3).len(), 2 + 3);
    assert_eq!(
        names(6),
        [demonology_build().ranks.into_iter().collect::<Vec<_>>()]
            .into_iter()
            .collect()
    );
    // Every one of them resolves.
    for points in 1..=MAX_TALENT_POINTS {
        for build in talents.legal_builds(CharacterClass::Warlock, points) {
            talents
                .resolve(abilities(), CharacterClass::Warlock, &build)
                .unwrap_or_else(|e| panic!("{build:?}: {e}"));
        }
    }
}

/// Every numeric field of an ability is either patchable or deliberately not.
/// The destructure has no rest pattern: a new field fails to compile here
/// until it is placed.
#[test]
fn every_numeric_ability_field_is_decided() {
    let defs = abilities();
    let config = defs.get_unchecked(&AbilityType::Shadowbolt).clone();
    let arenasim::states::play_match::ability_config::AbilityConfig {
        // Patchable, as `AbilityField`:
        cast_time: _,
        range: _,
        min_range: _,
        mana_cost: _,
        cooldown: _,
        damage_base_min: _,
        damage_base_max: _,
        damage_coefficient: _,
        healing_base_min: _,
        healing_base_max: _,
        healing_coefficient: _,
        applies_aura: _,
        application_chance: _,
        projectile_speed: _,
        lockout_duration: _,
        channel_duration: _,
        channel_tick_interval: _,
        channel_healing_per_tick: _,
        mana_burn_amount: _,
        // Not numbers, or not patchable yet (a tree card adds the field it needs):
        name: _,
        class: _,
        pet: _,
        talent: _,
        icon: _,
        description: _,
        prevention: _,
        cooldown_category: _,
        damage_scales_with: _,
        consumes_aura: _,
        projectile_visuals: _,
        spell_school: _,
        is_interrupt: _,
        requires_stealth: _,
        is_charge: _,
        is_dispel: _,
        dispel_backlash: _,
        health_to_mana: _,
    } = config;
    // And every patchable field reads the field it names.
    let mut c = defs.get_unchecked(&AbilityType::Shadowbolt).clone();
    *AbilityField::CastTime.of(&mut c).unwrap() = 9.0;
    *AbilityField::Range.of(&mut c).unwrap() = 8.0;
    *AbilityField::ProjectileSpeed.of(&mut c).unwrap() = 7.0;
    assert_eq!(
        (c.cast_time, c.range, c.projectile_speed),
        (9.0, 8.0, Some(7.0))
    );
    assert!(AbilityField::AuraDuration.of(&mut c).is_none());
}

// ---------------------------------------------------------------------------
// The spawn paths
// ---------------------------------------------------------------------------

/// What a spawn made of the Warlock and its Felhunter.
struct Spawned {
    warlock: Combatant,
    felhunter: Combatant,
    log: Vec<String>,
}

fn spawned(world: &mut World) -> Spawned {
    let mut warlock = None;
    let mut felhunter = None;
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    for (c, pet) in q.iter(world) {
        match pet {
            Some(p) if p.pet_type == PetType::Felhunter && c.team == 1 => {
                felhunter = Some(c.clone())
            }
            None if c.team == 1 && c.class == CharacterClass::Warlock => warlock = Some(c.clone()),
            _ => {}
        }
    }
    let log = world
        .resource::<CombatLog>()
        .entries
        .iter()
        .map(|e| e.message.clone())
        .collect();
    Spawned {
        warlock: warlock.expect("a Warlock spawned"),
        felhunter: felhunter.expect("its Felhunter spawned"),
        log,
    }
}

/// The untalented Warlock and Felhunter both spawn paths produce.
fn untalented() -> (Combatant, Combatant) {
    let s = headless_spawn(&Build::default(), None);
    (s.warlock, s.felhunter)
}

/// What the build and fixture predict, against the untalented spawn.
fn assert_talented(s: &Spawned, base: &(Combatant, Combatant), patched: bool) {
    let (warlock, felhunter) = base;
    assert_eq!(
        s.warlock.max_health,
        warlock.max_health * (1.0 + 0.03 * 2.0)
    );
    assert_eq!(s.warlock.current_health, s.warlock.max_health);
    // The Felhunter's 0.45 share of the talented Warlock, then its own +20%.
    assert_eq!(
        s.felhunter.max_health,
        s.warlock.max_health * 0.45 * (1.0 + 0.1 * 2.0)
    );
    assert_eq!(s.felhunter.current_health, s.felhunter.max_health);
    assert_eq!(
        s.felhunter.attack_damage,
        felhunter.attack_damage * (1.0 + 0.1 * 2.0)
    );
    assert!(s.warlock.max_health > warlock.max_health);
    assert!(s.felhunter.attack_damage > felhunter.attack_damage);
    // Nothing else moved.
    assert_eq!(s.warlock.spell_power, warlock.spell_power);
    assert_eq!(s.felhunter.spell_power, felhunter.spell_power);
    let cast = s
        .warlock
        .book()
        .get_unchecked(&AbilityType::Shadowbolt)
        .cast_time;
    let base_cast = warlock
        .book()
        .get_unchecked(&AbilityType::Shadowbolt)
        .cast_time;
    if patched {
        assert_eq!(
            cast,
            base_cast - 0.5,
            "the spawned book is the resolved build's"
        );
    } else {
        assert_eq!(cast, base_cast);
    }
    let talents_line = "[TALENTS] Team 1 Warlock #1: Demonic Embrace 2/2, Fel Stamina 2/2, \
                        Unholy Power 2/2 (Demonology 6)"
        .to_string();
    assert!(
        s.log.contains(&talents_line),
        "no {talents_line:?} in {:?}",
        s.log
    );
}

fn headless_config(build: &Build) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(7),
        team1_builds: vec![build.clone()],
        ..Default::default()
    }
}

/// Spawn the headless match (its first frame) for `build`, under `talents`
/// in place of the shipped table when given.
fn headless_spawn(build: &Build, talents: Option<TalentDefinitions>) -> Spawned {
    let config = headless_config(build);
    let mut app = match talents {
        Some(talents) => {
            let mut preloaded = PreloadedConfigs::load().unwrap();
            preloaded.talents = talents;
            build_headless_app(&config, true, Some(&preloaded))
        }
        None => build_headless_app(&config, true, None),
    };
    app.update();
    spawned(app.world_mut())
}

#[test]
fn the_headless_spawn_carries_the_resolved_build() {
    let base = untalented();
    assert_talented(&headless_spawn(&demonology_build(), None), &base, false);
    assert_talented(
        &headless_spawn(&demonology_build(), Some(patching_fixture())),
        &base,
        true,
    );
    // No build, no line.
    let plain = headless_spawn(&Build::default(), None);
    assert!(plain.log.iter().all(|l| !l.contains("[TALENTS]")));
    assert!(plain.log.iter().any(|l| l.contains("[EQUIPMENT]")));
}

/// The real `StatesPlugin` schedule, minus the window and renderer (the shape
/// of `tests/client_ai_profile.rs`).
fn boot_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .init_state::<GameState>()
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    app
}

/// Enter the graphical match for `build` under `talents`.
fn graphical_spawn(build: &Build, talents: TalentDefinitions) -> Spawned {
    let mut app = boot_app();
    let config: MatchConfig = headless_config(build).to_match_config().unwrap();
    app.insert_resource(config).insert_resource(talents);
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::PlayMatch);
    for _ in 0..3 {
        app.update();
    }
    spawned(app.world_mut())
}

/// The graphical spawn (`setup_play_match`, `spawn_combatant`, `spawn_pet`)
/// carries the match's resolved build: its book, its stats and its pet's.
#[test]
fn the_graphical_spawn_carries_the_resolved_build() {
    let base = untalented();
    let plain = graphical_spawn(&Build::default(), TalentDefinitions::shipped().clone());
    assert_eq!(
        plain.warlock.max_health, base.0.max_health,
        "both paths agree untalented"
    );
    assert_eq!(plain.felhunter.max_health, base.1.max_health);
    assert_talented(
        &graphical_spawn(&demonology_build(), patching_fixture()),
        &base,
        true,
    );
}

// ---------------------------------------------------------------------------
// The configs and the trace
// ---------------------------------------------------------------------------

#[test]
fn a_headless_config_carries_and_checks_its_builds() {
    let parse = |json: &str| serde_json::from_str::<HeadlessMatchConfig>(json).unwrap();
    let config = parse(
        r#"{"team1":["Warlock","Priest"],"team2":["Mage"],
            "team1_builds":[{"DemonicEmbrace":2,"FelStamina":2}]}"#,
    )
    .to_match_config()
    .unwrap();
    assert_eq!(
        config.team1_builds,
        vec![
            Build::of(&[(DemonicEmbrace, 2), (FelStamina, 2)]),
            Build::default()
        ]
    );
    assert_eq!(config.team2_builds, vec![Build::default()]);
    for (json, expect) in [
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"UnholyPower":1}]}"#,
            "opens at 2",
        ),
        (
            r#"{"team1":["Mage"],"team2":["Mage"],"team1_builds":[{"DemonicEmbrace":1}]}"#,
            "not a Mage's",
        ),
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team2_builds":[{},{}]}"#,
            "only 1 members",
        ),
    ] {
        let err = parse(json).to_match_config().unwrap_err();
        assert!(err.contains(expect), "{json}: {err}");
    }
    assert!(serde_json::from_str::<HeadlessMatchConfig>(
        r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"Nope":1}]}"#
    )
    .is_err());
}

/// A slot's build belongs to its class: changing the class drops it.
#[test]
fn changing_a_slots_class_drops_its_build() {
    let mut config = MatchConfig::default();
    config.set_class(1, 0, CharacterClass::Warlock);
    config.team1_builds[0] = demonology_build();
    config.set_class(1, 0, CharacterClass::Warlock);
    assert_eq!(
        config.team1_builds[0],
        demonology_build(),
        "same class keeps it"
    );
    config.set_class(1, 0, CharacterClass::Mage);
    assert!(config.team1_builds[0].is_empty());
    config.set_team1_size(3);
    assert_eq!(config.team1_builds.len(), 3);
}

/// A talented match's trace opens with its builds; an untalented one's opens
/// as it always did.
#[test]
fn the_trace_header_records_the_builds() {
    use arenasim::headless::runner::{run_headless_match_with, TraceConfig};
    let first_line = |build: &Build| -> serde_json::Value {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);
        let mut config = headless_config(build);
        config.max_duration_secs = 12.0;
        run_headless_match_with(
            config,
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
        )
        .expect("match runs");
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        serde_json::from_str(text.lines().next().unwrap()).unwrap()
    };
    let talented = first_line(&demonology_build());
    assert_eq!(talented["kind"], "match_header");
    assert_eq!(
        talented["team1_builds"],
        serde_json::json!([{"DemonicEmbrace": 2, "FelStamina": 2, "UnholyPower": 2}])
    );
    assert_eq!(talented["team2_builds"], serde_json::json!([{}]));
    assert_ne!(first_line(&Build::default())["kind"], "match_header");
}
