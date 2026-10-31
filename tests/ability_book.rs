//! The per-combatant ability book (AS-407).
//!
//! With no talents a combatant's book is its class's slice of the global
//! table, so reading either gives the same answer and no match can tell them
//! apart — `tests/talent_book_audit.rs` is what keeps the sim off the table.
//! These tests build books that DIFFER from the shipped table (a table edited
//! in the test, standing in for a talent's patch) and check that each read
//! lands on the right book: the one the match's spawn resolved, and, for a
//! question about another unit, that unit's.

use bevy::prelude::*;

use arenasim::headless::runner::{run_headless_match_prepared, PreloadedConfigs};
use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_book::{AbilityBook, BookOwner};
use arenasim::states::play_match::ability_config::{AbilitiesConfig, Prevention};
use arenasim::states::play_match::class_ai::interrupts::{CastTake, EnemyCast};
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::class_ai::CombatantInfo;
use arenasim::states::play_match::components::{CastingState, Pet, PetType};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::{AbilityDefinitions, AbilityType, Combatant};

/// The shipped table with `edit` applied: what a talent's patch would make of
/// it, for the books built from it.
fn table_with(edit: impl FnOnce(&mut AbilitiesConfig)) -> AbilityDefinitions {
    let path = arenasim::paths::asset_path_str("config/abilities.ron");
    let mut config: AbilitiesConfig =
        ron::from_str(&std::fs::read_to_string(path).expect("abilities.ron")).expect("parses");
    edit(&mut config);
    AbilityDefinitions::new(config)
}

/// One headless match on `table` (the shipped one when `None`), its log.
fn play(table: Option<AbilityDefinitions>, config: HeadlessMatchConfig) -> String {
    let mut configs = PreloadedConfigs::load().expect("configs");
    if let Some(table) = table {
        configs.abilities = table;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("match.txt");
    let config = HeadlessMatchConfig {
        output_path: Some(log.to_string_lossy().to_string()),
        ..config
    };
    run_headless_match_prepared(config, &configs, false, None).expect("match runs");
    std::fs::read_to_string(&log).expect("match log")
}

/// The amount of every log line containing `needle`: the number after " for ".
fn amounts(log: &str, needle: &str) -> Vec<f32> {
    log.lines()
        .filter(|l| l.contains(needle))
        .filter_map(|l| {
            let rest = &l[l.find(" for ")? + 5..];
            rest.split_whitespace().next()?.parse().ok()
        })
        .collect()
}

/// A match casts from the books its spawn resolved from the MATCH's table:
/// Frostbolt, a cast whose projectile lands, hits for what the match's table
/// says, which here is not what the shipped file says. The control plays the
/// same seed on the shipped table.
#[test]
fn a_match_casts_from_the_books_its_spawn_resolved() {
    const BOLT: f32 = 400.0;
    let config = || HeadlessMatchConfig {
        team1: vec!["Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(3),
        max_duration_secs: 60.0,
        ..Default::default()
    };
    let patched = table_with(|c| {
        let bolt = c.abilities.get_mut(&AbilityType::Frostbolt).unwrap();
        bolt.damage_base_min = BOLT;
        bolt.damage_base_max = BOLT;
        bolt.damage_coefficient = 0.0;
    });
    let max = |v: &[f32]| v.iter().copied().fold(0.0f32, f32::max);
    let bolts = amounts(&play(Some(patched), config()), "'s Frostbolt hits");
    assert!(
        max(&bolts) >= BOLT * 0.6,
        "Frostbolt hit for the match table's damage: {bolts:?}"
    );
    let bolts = amounts(&play(None, config()), "'s Frostbolt hits");
    assert!(
        !bolts.is_empty() && max(&bolts) < BOLT * 0.5,
        "control: the shipped table's damage: {bolts:?}"
    );
}

/// A channel ticks from its channeller's book: Drain Life ticks for what the
/// match's table says. The control plays the same seed on the shipped table.
#[test]
fn a_match_channels_from_the_books_its_spawn_resolved() {
    const TICK: f32 = 77.0;
    let config = || HeadlessMatchConfig {
        team1: vec!["Warlock".into(), "Paladin".into()],
        team2: vec!["Hunter".into(), "Druid".into()],
        team2_hunter_pet_types: vec!["Spider".into(); 2],
        random_seed: Some(12),
        max_duration_secs: 90.0,
        ..Default::default()
    };
    let patched = table_with(|c| {
        c.abilities
            .get_mut(&AbilityType::DrainLife)
            .unwrap()
            .damage_base_min = TICK;
    });
    let ticks = amounts(&play(Some(patched), config()), "'s Drain Life ticks");
    assert!(
        !ticks.is_empty() && ticks.iter().all(|&t| t == TICK),
        "Drain Life ticked for the match table's damage: {ticks:?}"
    );
    let ticks = amounts(&play(None, config()), "'s Drain Life ticks");
    assert!(
        !ticks.is_empty() && !ticks.contains(&TICK),
        "control: the shipped table's damage: {ticks:?}"
    );
}

/// An interrupter reads a cast from its CASTER's book: whether it is a heal
/// (Spell Lock takes one first) and whether it can be taken. A Hunter whose
/// book makes Aimed Shot interruptible is taken, though the shipped table
/// makes it uninterruptible; the shipped Hunter's is passed over.
#[test]
fn a_cast_is_read_from_its_casters_book() {
    let target = Entity::from_raw(9);
    let cast = |class: CharacterClass, book: Option<AbilityBook>, ability: AbilityType| {
        let mut combatant = Combatant::new(2, 0, class);
        if let Some(book) = book {
            combatant = combatant.with_book(book);
        }
        EnemyCast::casting(
            Entity::from_raw(1),
            &combatant,
            Vec3::ZERO,
            &CastingState::new(ability, target, 2.0),
        )
    };
    let heal = cast(CharacterClass::Priest, None, AbilityType::FlashHeal);
    assert!(heal.heal && heal.take == CastTake::Takes, "{heal:?}");
    let bolt = cast(CharacterClass::Mage, None, AbilityType::Frostbolt);
    assert!(!bolt.heal && bolt.take == CastTake::Takes, "{bolt:?}");

    let shipped = cast(CharacterClass::Hunter, None, AbilityType::AimedShot);
    assert_eq!(shipped.take, CastTake::Uninterruptible);
    let interruptible = table_with(|c| {
        c.abilities
            .get_mut(&AbilityType::AimedShot)
            .unwrap()
            .prevention = Prevention::Silence;
    });
    let patched = cast(
        CharacterClass::Hunter,
        Some(AbilityBook::for_class(
            &interruptible,
            CharacterClass::Hunter,
        )),
        AbilityType::AimedShot,
    );
    assert_eq!(
        patched.take,
        CastTake::Takes,
        "the Hunter's own book decides"
    );
}

/// The Rogue's Kidney Shot plan reads its target's cast from the TARGET's
/// book: against a Mage casting a Frostbolt Kick can take, it holds the stun
/// for the Kick. The pair: the same Mage, whose book makes that Frostbolt
/// uninterruptible — Kick cannot take it, so the stun is not held for it.
#[test]
fn the_rogue_reads_its_targets_cast_from_the_targets_book() {
    const ROGUE: &str = "rogue";
    const MAGE: &str = "mage";
    let held_for_kick = |view: &arenasim::states::play_match::class_ai::scene::DecisionView| {
        matches!(
            view.rejected(AbilityType::KidneyShot),
            Some(RejectionReason::PreconditionUnmet { note })
                if note == "Kidney held: Kick will interrupt this cast"
        )
    };
    let scene = |mage_book: Option<AbilityBook>| {
        let mut scene = Scene::new()
            .unit(ROGUE, 1, CharacterClass::Rogue)
            .at(0.0, 0.0)
            .targeting(MAGE)
            .unit(MAGE, 2, CharacterClass::Mage)
            .at(2.0, 0.0)
            .casting(MAGE, AbilityType::Frostbolt);
        scene.combatant_mut(ROGUE).stealthed = false;
        if let Some(book) = mage_book {
            let mage = scene.combatant_mut(MAGE);
            *mage = mage.clone().with_book(book);
        }
        scene.decide(ROGUE)
    };
    let shipped = scene(None);
    assert!(
        held_for_kick(&shipped),
        "held for the Kick: {:?} {:?}",
        shipped.chose(),
        shipped.rejected(AbilityType::KidneyShot)
    );
    let uninterruptible = table_with(|c| {
        c.abilities
            .get_mut(&AbilityType::Frostbolt)
            .unwrap()
            .prevention = Prevention::Pacify;
    });
    let patched = scene(Some(AbilityBook::for_class(
        &uninterruptible,
        CharacterClass::Mage,
    )));
    assert!(
        !held_for_kick(&patched),
        "a cast Kick cannot take is not held for: {:?} {:?}",
        patched.chose(),
        patched.rejected(AbilityType::KidneyShot)
    );
}

/// A snapshot row carries its combatant's own book — a pet's is its pet
/// type's, which is where the Hunter's dispatch and every other reader of a
/// pet's kit look.
#[test]
fn a_snapshot_row_carries_its_combatants_book() {
    let hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    let spider = Combatant::new_pet(
        1,
        10,
        PetType::Spider,
        &hunter,
        AbilityBook::shipped_pet(PetType::Spider),
    );
    let owner = Entity::from_raw(1);
    let row = CombatantInfo::from_combatant(
        Entity::from_raw(2),
        &spider,
        &Transform::default(),
        None,
        Some(&Pet {
            owner,
            pet_type: PetType::Spider,
        }),
        None,
    );
    assert_eq!(row.book.owner(), BookOwner::Pet(PetType::Spider));
    assert!(row.book.contains(&AbilityType::SpiderWeb));
    let row =
        CombatantInfo::from_combatant(owner, &hunter, &Transform::default(), None, None, None);
    assert_eq!(row.book.owner(), BookOwner::Class(CharacterClass::Hunter));
    assert!(!row.book.contains(&AbilityType::SpiderWeb));
}

/// Mana Burn's landing carries the school of the caster's book: a Priest
/// whose book makes Mana Burn Fire queues a Fire burn. The control: the
/// shipped Priest's is Shadow.
#[test]
fn a_mana_burn_lands_in_its_casters_school() {
    use arenasim::states::play_match::abilities::SpellSchool;
    use arenasim::states::play_match::combat_core::process_casting;
    use arenasim::states::play_match::components::{
        ArenaDampening, FloatingTextState, GameRng, ManaBurnPending,
    };
    use arenasim::states::play_match::map_config::ActiveMapGeometry;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    let burn_school = |book: AbilityBook| -> SpellSchool {
        let mut world = World::new();
        world.insert_resource(arenasim::combat::log::CombatLog::default());
        world.insert_resource(ArenaDampening::default());
        world.insert_resource(GameRng::from_seed(7));
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
        world.insert_resource(time);
        world.insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        });
        let target = world
            .spawn((
                Combatant::new(2, 0, CharacterClass::Priest),
                Transform::from_xyz(10.0, 0.0, 0.0),
                FloatingTextState {
                    next_pattern_index: 0,
                },
            ))
            .id();
        world.spawn((
            Combatant::new_with_book(1, 0, CharacterClass::Priest, book),
            Transform::default(),
            CastingState::new(AbilityType::ManaBurn, target, 0.0),
        ));
        world.run_system_once(process_casting).unwrap();
        let mut burns = world.query::<&ManaBurnPending>();
        let schools: Vec<SpellSchool> = burns.iter(&world).map(|b| b.school).collect();
        assert_eq!(schools.len(), 1, "one burn queued: {schools:?}");
        schools[0]
    };
    assert_eq!(
        burn_school(AbilityBook::shipped(CharacterClass::Priest)),
        SpellSchool::Shadow,
        "control: the shipped Mana Burn is Shadow"
    );
    let fire = table_with(|c| {
        c.abilities
            .get_mut(&AbilityType::ManaBurn)
            .unwrap()
            .spell_school = SpellSchool::Fire;
    });
    assert_eq!(
        burn_school(AbilityBook::for_class(&fire, CharacterClass::Priest)),
        SpellSchool::Fire
    );
}
