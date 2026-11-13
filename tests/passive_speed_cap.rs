//! AS-464 — passive movement speed is ONE capped term, as in TBC.
//!
//! Every passive source — an item's `movement_speed` (a boot enchant, a meta
//! gem), a talent's `MovementSpeed` points — is a fraction of natural speed
//! offered to one term: the strongest applies, never their sum, and never
//! above `PASSIVE_SPEED_CAP` (8%). A temporary speed aura (Travel Form) is
//! outside the cap: it multiplies the passive speed as the unit moves.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::components::PetType;
use arenasim::states::play_match::components::{
    passive_movement_speed, ActiveAuras, Aura, AuraType, Combatant,
};
use arenasim::states::play_match::equipment::{
    parse_item_definitions, ItemDefinitions, ItemId, ItemSlot, Loadout,
};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId, TalentStat};
use arenasim::states::play_match::{
    effective_movement_speed, AbilityDefinitions, AbilityType, PASSIVE_SPEED_CAP,
};

/// Two items with passive speed: boots and a helm.
fn items(boots: f32, helm: f32) -> ItemDefinitions {
    parse_item_definitions(
        &format!(
            "(items: {{ \
             SabatonsBattleBorn: (name: \"Boots\", item_level: 56, slot: Feet, \
             movement_speed: {boots:?}), \
             LionheartHelm: (name: \"Helm\", item_level: 56, slot: Head, \
             movement_speed: {helm:?}) }})"
        ),
        "fixture",
    )
    .expect("the fixture loads")
}

fn both() -> Loadout {
    [
        (ItemSlot::Feet, ItemId::SabatonsBattleBorn),
        (ItemSlot::Head, ItemId::LionheartHelm),
    ]
    .into_iter()
    .collect()
}

/// The shipped talents with Demonic Embrace (Warlock, two ranks) adding
/// `points` of movement speed a rank.
fn embrace_speed(points: f32) -> TalentDefinitions {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let health = "per_rank: [StatPercent(MaxHealth, 0.03)],";
    assert!(text.contains(health));
    TalentDefinitions::from_ron(
        &text.replacen(
            health,
            &format!("per_rank: [StatAdd(MovementSpeed, {points:?})],"),
            1,
        ),
        AbilityDefinitions::shipped(),
    )
    .expect("the fixture validates")
}

/// A Warlock wearing `items`, then given `ranks` of `talents`' Demonic
/// Embrace: its speed, as the spawn sets it.
fn warlock(items: &ItemDefinitions, talents: Option<(&TalentDefinitions, u8)>) -> Combatant {
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    c.apply_equipment(&both(), items);
    if let Some((talents, ranks)) = talents {
        talents
            .resolve(
                AbilityDefinitions::shipped(),
                CharacterClass::Warlock,
                &Build::of(&[(TalentId::DemonicEmbrace, ranks)]),
            )
            .expect("the build resolves")
            .apply(&mut c);
    }
    c
}

/// Two items, then an item and a talent: the strongest source is the term,
/// never the sum. Each case names the speed it must be and the sum it must
/// not be.
#[test]
fn two_passive_sources_give_the_strongest_never_their_sum() {
    let natural = Combatant::new(1, 0, CharacterClass::Warlock).base_movement_speed;
    assert_eq!(natural, 4.5, "the fixture's arithmetic assumes it");

    let gear = items(0.05, 0.03);
    let c = warlock(&gear, None);
    assert_eq!(c.passive_speed, 0.05);
    assert_eq!(c.base_movement_speed, passive_movement_speed(natural, 0.05));
    assert!((c.base_movement_speed - 4.725).abs() < 1e-5, "+5%, not +8%");

    // A talent weaker than the gear changes nothing (+4% beside +5%); a
    // stronger one replaces it (+4% beside +3%).
    let talents = embrace_speed(0.02);
    let weaker = warlock(&gear, Some((&talents, 2)));
    assert_eq!(weaker.base_movement_speed, c.base_movement_speed);
    let stronger = warlock(&items(0.03, 0.0), Some((&talents, 2)));
    assert!(
        (stronger.base_movement_speed - natural * 1.04).abs() < 1e-5,
        "the talent's +4%, not +7%: {}",
        stronger.base_movement_speed
    );
}

/// The term never goes above the cap, however strong the source: an item at
/// the cap with a talent above it moves at natural speed plus 8%.
#[test]
fn passive_speed_never_exceeds_the_cap() {
    let natural = Combatant::new(1, 0, CharacterClass::Warlock).base_movement_speed;
    let talents = embrace_speed(0.05);
    let c = warlock(&items(PASSIVE_SPEED_CAP, 0.0), Some((&talents, 2)));
    assert!(
        c.passive_speed > PASSIVE_SPEED_CAP,
        "the talent offers +10%"
    );
    assert_eq!(
        c.base_movement_speed,
        natural * (1.0 + PASSIVE_SPEED_CAP),
        "capped at +8%"
    );
    // The control: the cap is what held it, not the sources.
    assert!(natural * (1.0 + c.passive_speed) > c.base_movement_speed);
}

/// The control: a temporary speed aura is outside the cap. A Druid already at
/// the cap shifts into Travel Form and moves faster than natural speed plus
/// 8% — the form's own multiplier, on top of its passive speed.
#[test]
fn a_temporary_speed_aura_goes_above_the_cap() {
    let mut druid = Combatant::new(1, 0, CharacterClass::Druid);
    let natural = druid.natural_movement_speed;
    druid.apply_equipment(&both(), &items(PASSIVE_SPEED_CAP, 0.0));
    let capped = natural * (1.0 + PASSIVE_SPEED_CAP);
    assert_eq!(druid.base_movement_speed, capped);

    let form = AbilityDefinitions::shipped()
        .get(&AbilityType::TravelForm)
        .and_then(|a| a.applies_aura.as_ref())
        .expect("Travel Form applies its form")
        .magnitude;
    let auras = ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::TravelForm,
            magnitude: form,
            duration: 600.0,
            ..Default::default()
        }],
    };
    let shifted = effective_movement_speed(druid.base_movement_speed, Some(&auras));
    assert_eq!(shifted, capped * form);
    assert!(
        shifted > capped,
        "{shifted} is not above the capped {capped}"
    );
}

/// A passive source scales the unit's OWN natural speed: a pet's is its pet
/// type's, not its owner's class speed. (No source reaches a pet today; the
/// rule must hold when one does.)
#[test]
fn a_pets_passive_speed_scales_its_own_natural_speed() {
    let hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    let mut spider = Combatant::new_pet(
        1,
        10,
        PetType::Spider,
        &hunter,
        AbilityBook::shipped_pet(PetType::Spider),
    );
    let own = PetType::Spider.movement_speed();
    assert_ne!(
        own, hunter.natural_movement_speed,
        "the fixture needs them apart"
    );
    spider.offer_passive_speed(0.05);
    assert_eq!(
        spider.base_movement_speed,
        passive_movement_speed(own, 0.05)
    );
}

/// A talent's `MovementSpeed` total is the passive term's fraction, the unit
/// its points and the item weight are in, never the speed in yards.
#[test]
fn the_talent_stats_total_is_the_passive_fraction() {
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    assert_eq!(TalentStat::MovementSpeed.total(&c), 0.0);
    c.offer_passive_speed(0.05);
    assert_eq!(TalentStat::MovementSpeed.total(&c), 0.05);
}
