//! The Warlock's Demonology tree (AS-410): Master Demonologist, Demonic
//! Sacrifice and Soul Link, as `talents.ron` ships them.
//!
//! Each mechanism is pinned as a pair — the case and its control with the one
//! condition removed:
//! - the demon choice and its exclusion (Sacrifice switches Soul Link off;
//!   only Demonic Sacrifice offers it; only a Warlock makes it);
//! - the link's share, through the resolved build and the real damage sites;
//! - the sacrifice at the gates: the Felhunter dies, is no kill, and the match
//!   plays on, with Touch of Shadow on the Warlock and no pet left to command;
//! - Master Demonologist on both while the Felhunter lives, off the Warlock
//!   once it is dead, and what its aura and Touch of Shadow do to a hit.

mod common;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::{CharacterClass, WarlockDemon};
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{
    apply_damage_with_absorb, damage_dealt_multiplier, DamageFollowUps, Hit,
};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, Combatant, MatchCountdown, Pet, PetType,
};
use arenasim::states::play_match::constants::PET_SLOT_BASE;
use arenasim::states::play_match::demonology::{
    demonic_bonds, master_demonologist_aura, touch_of_shadow_aura, MASTER_DEMONOLOGIST_NAME,
    TOUCH_OF_SHADOW_NAME,
};
use arenasim::states::play_match::rendering::{
    get_aura_icon_key, talent_aura_icon_key, talent_aura_icons,
};
use arenasim::states::play_match::talent_choices::{ChoiceOption, SlotChoices};
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use common::staged::{play_seeded, Played};

use TalentId::{DemonicEmbrace, DemonicSacrifice, FelStamina, MasterDemonologist, SoulLink};

fn abilities() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

fn talents() -> &'static TalentDefinitions {
    TalentDefinitions::shipped()
}

/// The whole tree's capstone build: 2/2 Embrace, 2/2 Fel Stamina, Master
/// Demonologist, Demonic Sacrifice, Soul Link (7 points).
fn linked() -> Build {
    Build::of(&[
        (DemonicEmbrace, 2),
        (FelStamina, 2),
        (MasterDemonologist, 1),
        (DemonicSacrifice, 1),
        (SoulLink, 1),
    ])
}

/// A headless Warlock-versus-`enemy` config with the Warlock on `build`,
/// choosing `demon`.
fn config(enemy: &str, build: Build, demon: &str, seed: u64, secs: f32) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec![enemy.into()],
        random_seed: Some(seed),
        max_duration_secs: secs,
        team1_builds: vec![build],
        team1_warlock_demons: vec![demon.into()],
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// The demon choice, and the exclusion
// ---------------------------------------------------------------------------

/// Resolve team 1's slot 0 of `config` as its spawn does.
fn resolve(config: &HeadlessMatchConfig) -> Combatant {
    let match_config = config.to_match_config().expect("a legal config");
    let build = match_config.team1_builds[0].clone();
    let choices = SlotChoices::from_config(&match_config, 1, 0, CharacterClass::Warlock);
    let resolved = talents()
        .resolve_with_choices(abilities(), CharacterClass::Warlock, &build, &choices)
        .expect("resolves");
    let mut combatant = Combatant::new(1, 0, CharacterClass::Warlock);
    resolved.apply(&mut combatant);
    combatant
}

#[test]
fn the_sacrifice_switches_soul_link_off_and_the_felhunter_keeps_it() {
    let felhunter = resolve(&config("Warrior", linked(), "Felhunter", 1, 5.0));
    let mechanics = &felhunter.talent_mechanics;
    assert!((mechanics.soul_link().unwrap() - 0.12).abs() < 1e-6);
    assert!((mechanics.master_demonologist().unwrap() - 0.04).abs() < 1e-6);
    assert!((mechanics.demonic_sacrifice().unwrap() - 0.15).abs() < 1e-6);
    assert!(!felhunter.sacrifice_pending);

    let sacrifice = resolve(&config("Warrior", linked(), "Sacrifice", 1, 5.0));
    assert_eq!(sacrifice.talent_mechanics.soul_link(), None, "switched off");
    assert!(sacrifice.talent_mechanics.master_demonologist().is_some());
    assert!(sacrifice.talent_mechanics.demonic_sacrifice().is_some());
    assert!(sacrifice.sacrifice_pending);
}

#[test]
fn only_demonic_sacrifice_offers_the_sacrifice() {
    let sacrifice = ChoiceOption::WarlockDemon(WarlockDemon::Sacrifice);
    let without = Build::of(&[(DemonicEmbrace, 2), (MasterDemonologist, 1)]);
    let with = Build::of(&[
        (DemonicEmbrace, 2),
        (MasterDemonologist, 1),
        (DemonicSacrifice, 1),
    ]);
    assert!(!talents().offers(&without, sacrifice));
    assert!(talents().offers(&with, sacrifice));
    assert!(talents().offers(
        &Build::default(),
        ChoiceOption::WarlockDemon(WarlockDemon::Felhunter)
    ));
    let err = config("Warrior", without, "Sacrifice", 1, 5.0)
        .to_match_config()
        .unwrap_err();
    assert!(err.contains("WarlockDemon(Sacrifice)"), "{err}");
    assert!(config("Warrior", with, "Sacrifice", 1, 5.0)
        .to_match_config()
        .is_ok());
}

#[test]
fn only_a_warlock_sacrifices() {
    let mage = HeadlessMatchConfig {
        team1: vec!["Mage".into()],
        team2: vec!["Warrior".into()],
        team1_warlock_demons: vec!["Sacrifice".into()],
        ..Default::default()
    };
    let err = mage.to_match_config().unwrap_err();
    assert!(
        err.contains("a demon sacrifice is a Warlock's choice"),
        "{err}"
    );
    let felhunter = HeadlessMatchConfig {
        team1_warlock_demons: vec!["Felhunter".into()],
        ..mage
    };
    assert!(felhunter.to_match_config().is_ok());
}

/// The Sacrifice option and its mechanic travel together on one node.
#[test]
fn the_sacrifice_option_and_its_mechanic_come_together() {
    let shipped = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/config/talents.ron"),
    )
    .unwrap();
    assert!(TalentDefinitions::from_ron(&shipped, abilities()).is_ok());
    for cut in [
        "Mechanic(DemonicSacrifice(shadow_damage: 0.15)),",
        "WidenChoice(WarlockDemon(Sacrifice)),",
    ] {
        assert!(shipped.contains(cut));
        let err =
            TalentDefinitions::from_ron(&shipped.replace(cut, ""), abilities()).expect_err(cut);
        assert!(err.contains("one node carries both"), "{err}");
    }
}

#[test]
fn the_demonology_mechanics_need_their_class() {
    let md = TalentMechanic::MasterDemonologist { reduction: 0.04 };
    assert!(md.validate(CharacterClass::Warlock, 1).is_ok());
    assert!(md.validate(CharacterClass::Mage, 1).is_err());
    let sac = TalentMechanic::DemonicSacrifice {
        shadow_damage: 0.15,
    };
    assert!(sac.validate(CharacterClass::Warlock, 1).is_ok());
    assert!(sac.validate(CharacterClass::Hunter, 1).is_err());
    assert!(sac.validate(CharacterClass::Warlock, 2).is_err());
    // Ranks add for the reduction; the sacrifice has one rank.
    assert_eq!(
        md.at_rank(2),
        TalentMechanic::MasterDemonologist { reduction: 0.08 }
    );
    assert_eq!(sac.at_rank(1), sac);
}

// ---------------------------------------------------------------------------
// What the auras do to a hit
// ---------------------------------------------------------------------------

/// A Warlock with no resistance or armor, so a hit's number is the
/// pipeline's alone.
fn bare() -> Combatant {
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    c.max_health = 1000.0;
    c.current_health = 1000.0;
    c.armor = 0.0;
    c.fire_resistance = 0.0;
    c.shadow_resistance = 0.0;
    c
}

fn taken(school: SpellSchool, auras: Vec<Aura>) -> f32 {
    let mut world = World::new();
    let victim = world.spawn_empty().id();
    let mut active = ActiveAuras { auras };
    apply_damage_with_absorb(
        Hit::new(100.0, school, "Probe", None, None, victim),
        &mut bare(),
        Some(&mut active),
        &mut DamageFollowUps::default(),
        &mut CombatLog::default(),
    )
    .0
}

#[test]
fn master_demonologist_cuts_spell_damage_only() {
    let owner = Entity::PLACEHOLDER;
    let md = || vec![master_demonologist_aura(0.04, owner)];
    assert!((taken(SpellSchool::Fire, md()) - 96.0).abs() < 1e-3);
    assert!((taken(SpellSchool::Shadow, md()) - 96.0).abs() < 1e-3);
    assert!((taken(SpellSchool::Physical, md()) - 100.0).abs() < 1e-3);
    assert!((taken(SpellSchool::Fire, vec![]) - 100.0).abs() < 1e-3);
}

#[test]
fn touch_of_shadow_raises_shadow_damage_only() {
    let tos = ActiveAuras {
        auras: vec![touch_of_shadow_aura(0.15, Entity::PLACEHOLDER)],
    };
    assert!((damage_dealt_multiplier(Some(&tos), SpellSchool::Shadow) - 1.15).abs() < 1e-6);
    assert!((damage_dealt_multiplier(Some(&tos), SpellSchool::Fire) - 1.0).abs() < 1e-6);
    assert!((damage_dealt_multiplier(None, SpellSchool::Shadow) - 1.0).abs() < 1e-6);
}

/// A talent aura is keyed by its source, so it neither refuses nor is refused
/// by a same-type aura from elsewhere (Devotion Aura beside Master
/// Demonologist), and no dispel or purge takes it.
#[test]
fn a_talent_aura_leaves_the_type_slot_free_and_cannot_be_removed() {
    let md = master_demonologist_aura(0.04, Entity::PLACEHOLDER);
    let tos = touch_of_shadow_aura(0.15, Entity::PLACEHOLDER);
    for aura in [&md, &tos] {
        assert!(
            !aura.holds_type_slot(aura.effect_type),
            "{}",
            aura.ability_name
        );
        assert!(!aura.can_be_purged(), "{}", aura.ability_name);
        assert!(aura.duration > 310.0, "outlasts the match");
    }
    // Control: Devotion Aura, a plain buff of the same type, holds the slot.
    let devotion = Aura {
        effect_type: AuraType::DamageTakenReduction,
        magnitude: 0.10,
        duration: 600.0,
        ability_name: "Devotion Aura".into(),
        ..Default::default()
    };
    assert!(devotion.holds_type_slot(AuraType::DamageTakenReduction));
}

#[test]
fn the_talent_auras_wear_their_nodes_icons() {
    let registered = talent_aura_icons(talents());
    for (aura, talent, icon) in [
        (
            master_demonologist_aura(0.04, Entity::PLACEHOLDER),
            MasterDemonologist,
            "icons/talents/spell_shadow_shadowpact.jpg",
        ),
        (
            touch_of_shadow_aura(0.15, Entity::PLACEHOLDER),
            DemonicSacrifice,
            "icons/talents/spell_shadow_psychicscream.jpg",
        ),
    ] {
        assert_eq!(aura.source_talent, Some(talent), "{}", aura.ability_name);
        let key = get_aura_icon_key(&aura, abilities());
        assert_eq!(key, talent_aura_icon_key(talent));
        let path = registered
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, p)| p.as_str());
        assert_eq!(path, Some(icon), "{}", aura.ability_name);
    }
    // Control: Soul Link lands no aura of its own, so no icon is registered.
    assert!(!registered
        .iter()
        .any(|(k, _)| *k == talent_aura_icon_key(SoulLink)));
}

// ---------------------------------------------------------------------------
// The sacrifice at the gates, in a world of its own
// ---------------------------------------------------------------------------

struct Bonds {
    world: World,
    owner: Entity,
    pet: Entity,
}

/// A Warlock with `mechanics` and its Felhunter; `pending` the sacrifice,
/// `gates` open or not.
fn bonds(mechanics: &[TalentMechanic], pending: bool, gates: bool) -> Bonds {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(MatchCountdown {
        time_remaining: if gates { 0.0 } else { 5.0 },
        gates_opened: gates,
    });
    let mut warlock = Combatant::new(1, 0, CharacterClass::Warlock);
    warlock.talent_mechanics = TalentMechanics::of(mechanics);
    warlock.sacrifice_pending = pending;
    let owner = world.spawn(warlock).id();
    let felhunter = Combatant::new(1, PET_SLOT_BASE, CharacterClass::Warlock);
    let pet = world
        .spawn((
            felhunter,
            Pet {
                owner,
                pet_type: PetType::Felhunter,
            },
        ))
        .id();
    Bonds { world, owner, pet }
}

impl Bonds {
    fn run(&mut self) {
        self.world.run_system_once(demonic_bonds).expect("ran");
    }

    fn pet_alive(&self) -> bool {
        self.world.get::<Combatant>(self.pet).unwrap().is_alive()
    }

    /// The auras queued for `target`, by name.
    fn pending_for(&mut self, target: Entity) -> Vec<String> {
        let mut names: Vec<String> = self
            .world
            .query::<&AuraPending>()
            .iter(&self.world)
            .filter(|p| p.target == target)
            .map(|p| p.aura.ability_name.clone())
            .collect();
        names.sort();
        names
    }

    /// The magnitude of the one aura named `name` queued for `target`.
    fn pending_magnitude(&mut self, target: Entity, name: &str) -> f32 {
        let found: Vec<f32> = self
            .world
            .query::<&AuraPending>()
            .iter(&self.world)
            .filter(|p| p.target == target && p.aura.ability_name == name)
            .map(|p| p.aura.magnitude)
            .collect();
        assert_eq!(found.len(), 1, "{name} queued once");
        found[0]
    }

    fn log(&self) -> &CombatLog {
        self.world.resource::<CombatLog>()
    }
}

const SACRIFICE: TalentMechanic = TalentMechanic::DemonicSacrifice {
    shadow_damage: 0.15,
};

#[test]
fn the_sacrifice_kills_the_felhunter_at_the_gates_and_grants_touch_of_shadow() {
    let mut b = bonds(&[SACRIFICE], true, true);
    b.run();
    assert!(!b.pet_alive());
    assert!(b.world.get::<Combatant>(b.pet).unwrap().is_dead);
    assert!(!b.world.get::<Combatant>(b.owner).unwrap().sacrifice_pending);
    let owner = b.owner;
    assert_eq!(b.pending_for(owner), vec![TOUCH_OF_SHADOW_NAME.to_string()]);
    assert!((b.pending_magnitude(owner, TOUCH_OF_SHADOW_NAME) - 0.15).abs() < 1e-6);
    assert!(
        b.log()
            .entries
            .iter()
            .any(|e| e.message.contains("[TALENTS]")
                && e.message.contains("sacrifices its Felhunter"))
    );
    // Once landed, never again.
    b.run();
    assert_eq!(b.pending_for(owner).len(), 1);

    // Controls: before the gates, and without the choice.
    let mut early = bonds(&[SACRIFICE], true, false);
    early.run();
    assert!(early.pet_alive());
    let owner = early.owner;
    assert!(early.pending_for(owner).is_empty());
    assert!(
        early
            .world
            .get::<Combatant>(owner)
            .unwrap()
            .sacrifice_pending
    );
    let mut kept = bonds(&[SACRIFICE], false, true);
    kept.run();
    assert!(kept.pet_alive());
    let owner = kept.owner;
    assert!(kept.pending_for(owner).is_empty());
}

/// The sacrifice is no kill: no structured death record (which the Results
/// screen's deaths and `combatant_survived` read), no damage event and so no
/// killing blow, and no killer.
#[test]
fn the_sacrifice_is_no_kill() {
    let mut b = bonds(&[SACRIFICE], true, true);
    b.run();
    assert!(!b.pet_alive(), "the case is not vacuous");
    let log = b.log();
    assert!(
        log.entries.iter().all(|e| e.structured_data.is_none()),
        "{:?}",
        log.entries.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    assert!(log.combatant_survived("Team 1 Felhunter #1"));
    assert!(log
        .entries
        .iter()
        .any(|e| e.message == "Team 1 Felhunter #1 is sacrificed by Team 1 Warlock #1"));
    assert!(!log
        .entries
        .iter()
        .any(|e| matches!(e.structured_data, Some(StructuredEventData::Damage { .. }))));
}

// ---------------------------------------------------------------------------
// Master Demonologist, in a world of its own
// ---------------------------------------------------------------------------

const MD: TalentMechanic = TalentMechanic::MasterDemonologist { reduction: 0.04 };

#[test]
fn master_demonologist_lands_on_both_while_the_felhunter_lives() {
    let mut b = bonds(&[MD], false, false);
    b.run();
    let (owner, pet) = (b.owner, b.pet);
    assert_eq!(
        b.pending_for(owner),
        vec![MASTER_DEMONOLOGIST_NAME.to_string()]
    );
    assert_eq!(
        b.pending_for(pet),
        vec![MASTER_DEMONOLOGIST_NAME.to_string()]
    );
    // At the talent's own numbers, on both.
    for who in [owner, pet] {
        assert!((b.pending_magnitude(who, MASTER_DEMONOLOGIST_NAME) - 0.04).abs() < 1e-6);
    }
    // A dead Warlock's bonds do nothing.
    let mut dead = bonds(&[MD], true, true);
    let (owner, pet) = (dead.owner, dead.pet);
    dead.world
        .get_mut::<Combatant>(owner)
        .unwrap()
        .current_health = 0.0;
    dead.run();
    assert!(dead.pending_for(owner).is_empty() && dead.pending_for(pet).is_empty());
    assert!(dead.pet_alive());
    // Control: no talent, nothing queued.
    let mut none = bonds(&[], false, false);
    none.run();
    let (owner, pet) = (none.owner, none.pet);
    assert!(none.pending_for(owner).is_empty());
    assert!(none.pending_for(pet).is_empty());
}

#[test]
fn master_demonologist_leaves_the_warlock_when_the_felhunter_dies() {
    let mut b = bonds(&[MD], false, true);
    let owner = b.owner;
    b.world.entity_mut(owner).insert(ActiveAuras {
        auras: vec![master_demonologist_aura(0.04, owner)],
    });
    b.world.get_mut::<Combatant>(b.pet).unwrap().current_health = 0.0;
    b.run();
    assert!(b.world.get::<ActiveAuras>(owner).unwrap().auras.is_empty());
    assert!(b.pending_for(owner).is_empty(), "not put back");
    // Control: the Felhunter lives, and the Warlock keeps it.
    let mut alive = bonds(&[MD], false, true);
    let owner = alive.owner;
    alive.world.entity_mut(owner).insert(ActiveAuras {
        auras: vec![master_demonologist_aura(0.04, owner)],
    });
    alive.run();
    assert_eq!(
        alive.world.get::<ActiveAuras>(owner).unwrap().auras.len(),
        1
    );
    assert!(alive.pending_for(owner).is_empty(), "not doubled");
}

// ---------------------------------------------------------------------------
// End to end: real matches
// ---------------------------------------------------------------------------

/// The observed Warlock (slot 0) and its Felhunter on each frame.
fn warlock_frames(played: &Played) -> Vec<(bool, bool, bool, Vec<AuraType>, Vec<AuraType>)> {
    played
        .frames
        .iter()
        .map(|f| {
            let warlock = f
                .combatants
                .values()
                .find(|c| c.team == 1 && !c.is_pet)
                .unwrap();
            let pet = f
                .combatants
                .values()
                .find(|c| c.team == 1 && c.is_pet)
                .expect("the Felhunter spawns either way");
            (
                f.gates_open,
                warlock.alive,
                pet.alive,
                warlock.aura_types.clone(),
                pet.aura_types.clone(),
            )
        })
        .collect()
}

fn sacrifice_build() -> Build {
    Build::of(&[
        (DemonicEmbrace, 2),
        (FelStamina, 1),
        (MasterDemonologist, 1),
        (DemonicSacrifice, 1),
    ])
}

#[test]
fn a_sacrificed_felhunter_dies_at_the_gates_and_the_match_plays_on() {
    let played = play_seeded(config("Priest", sacrifice_build(), "Sacrifice", 11, 8.0));
    let frames = warlock_frames(&played);
    let gates = frames.iter().position(|f| f.0).expect("the gates open");
    // Before the gates: alive, both under Master Demonologist.
    let before = &frames[gates - 1];
    assert!(before.2, "alive until the gates");
    assert!(before.3.contains(&AuraType::DamageTakenReduction));
    assert!(before.4.contains(&AuraType::DamageTakenReduction));
    assert!(!before.3.contains(&AuraType::DamageDealtIncrease));
    // From the gates: dead, Touch of Shadow on the Warlock, Master
    // Demonologist gone with the Felhunter.
    let after = &frames[gates];
    assert!(!after.2, "dead at the gates");
    assert!(after.3.contains(&AuraType::DamageDealtIncrease));
    assert!(frames[gates + 1..]
        .iter()
        .all(|f| !f.2 && !f.3.contains(&AuraType::DamageTakenReduction)));
    // The 1v1 plays on: the match runs its eight seconds with the Warlock up.
    assert!(played.result.match_time > 7.0, "{:?}", played.result);
    assert!(frames.last().unwrap().1);
    assert!(!played.log.contains("Felhunter #1 has been eliminated"));
    assert!(played
        .log
        .contains("[DEATH] Team 1 Felhunter #1 is sacrificed"));

    // No dead Felhunter is commanded, and the Warlock still acts. The
    // trace's clock is the combat clock: zero until the gates open.
    let after_gates = |played: &Played, kind: &str| -> Vec<serde_json::Value> {
        played
            .trace
            .iter()
            .filter(|e| e["kind"] == kind && common::staged::sim_time(e) > 0.0)
            .cloned()
            .collect()
    };
    assert_eq!(
        after_gates(&played, "pet_decision").len(),
        0,
        "a dead Felhunter decides nothing"
    );
    let warlock_casts = after_gates(&played, "ability_decision")
        .iter()
        .filter(|e| e["actor"]["class"] == "Warlock" && e["outcome"]["type"] == "action_taken")
        .count();
    assert!(warlock_casts > 0, "the Warlock fights on without its pet");

    // Control: the same build keeping its Felhunter.
    let kept = play_seeded(config("Priest", sacrifice_build(), "Felhunter", 11, 8.0));
    let frames = warlock_frames(&kept);
    assert!(frames.iter().all(|f| f.2), "alive throughout");
    assert!(frames
        .iter()
        .skip(1)
        .all(|f| f.3.contains(&AuraType::DamageTakenReduction)
            && f.4.contains(&AuraType::DamageTakenReduction)
            && !f.3.contains(&AuraType::DamageDealtIncrease)));
    assert!(
        !after_gates(&kept, "pet_decision").is_empty(),
        "a living Felhunter decides"
    );
}

#[test]
fn the_link_moves_its_share_to_the_felhunter_unless_it_was_sacrificed() {
    let transfers = |played: &Played| {
        played
            .log
            .lines()
            .filter(|l| l.contains("[TRANSFER]") && l.contains("(Soul Link)"))
            .count()
    };
    let linked_match = play_seeded(config("Warrior", linked(), "Felhunter", 7, 20.0));
    assert!(transfers(&linked_match) > 0, "{}", linked_match.log);
    let sacrificed = play_seeded(config("Warrior", linked(), "Sacrifice", 7, 20.0));
    assert!(sacrificed.log.contains("is sacrificed"));
    assert_eq!(transfers(&sacrificed), 0);
    // Control: Fel Stamina 2/2 without the link.
    let unlinked = Build::of(&[
        (DemonicEmbrace, 2),
        (FelStamina, 2),
        (MasterDemonologist, 1),
    ]);
    let control = play_seeded(config("Warrior", unlinked, "Felhunter", 7, 20.0));
    assert_eq!(transfers(&control), 0);
}

// ---------------------------------------------------------------------------
// Soul Link with a dead Felhunter
// ---------------------------------------------------------------------------

/// A Soul Link whose Felhunter is dead takes no share, so a Warlock behind an
/// absorb loses no health and nothing is transferred. Control: with the
/// Felhunter alive the link still passes shares.
#[test]
fn a_dead_felhunter_takes_no_share_and_the_absorb_holds() {
    use arenasim::headless::{Stage, Unit};
    use arenasim::states::play_match::components::AuraDamageMods;
    let warlock = Unit::new(1, 0);
    let warrior = Unit::new(2, 0);
    let felhunter = Unit::pet_of(1, 0);
    let stage = |pet_dead: bool| {
        let shield = Aura {
            effect_type: AuraType::Absorb,
            duration: 600.0,
            magnitude: 100_000.0,
            break_on_damage_threshold: -1.0,
            ability_name: "Staged Shield".into(),
            damage_mods: AuraDamageMods::default(),
            ..Default::default()
        };
        let s = Stage::new()
            .place(warlock, 0.0, 0.0)
            .place(felhunter, -6.0, 0.0)
            .place(warrior, 2.0, 0.0)
            .target(warrior, warlock)
            .aura(warlock, shield);
        if pet_dead {
            s.combatant(felhunter, |c| {
                c.current_health = 0.0;
                c.is_dead = true;
            })
        } else {
            s
        }
    };
    let transfers = |played: &Played| {
        played
            .log
            .lines()
            .filter(|l| l.contains("[TRANSFER]"))
            .count()
    };
    let dead = common::staged::play(
        config("Warrior", linked(), "Felhunter", 7, 10.0),
        stage(true),
    );
    let full = dead
        .staged_frames()
        .filter_map(|f| f.combatants.values().find(|c| c.team == 1 && !c.is_pet))
        .all(|w| w.current_health == w.max_health);
    assert!(full, "the absorb holds every hit");
    assert!(dead
        .log
        .contains("[TALENTS] Team 1 Warlock #1's Soul Link ends"));
    assert_eq!(transfers(&dead), 0, "{}", dead.log);
    // Not vacuous: the Warrior hit the shielded Warlock.
    assert!(dead
        .log
        .contains("Team 2 Warrior #1's Auto Attack hits Team 1 Warlock #1"));

    let alive = common::staged::play(
        config("Warrior", linked(), "Felhunter", 7, 10.0),
        stage(false),
    );
    assert!(transfers(&alive) > 0, "{}", alive.log);
}
