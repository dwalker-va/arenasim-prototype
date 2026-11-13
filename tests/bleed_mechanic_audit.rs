//! AS-481 — the declared bleeds, held to the TBC client's.
//!
//! Bleeds ignore armor, as in TBC (user ruling, 2026-11-13). The sim knows a
//! bleed only by its declaration (`mechanic: Some(Bleed)` on an ability's
//! damage over time, `abilities.ron`), so an ability TBC calls a bleed that
//! forgets it would meet armor, and one that claims it wrongly would pass it.
//!
//! `tests/data/tbc_bleeds.csv` is every TBC class or pet spell whose damage
//! is a bleed (Mechanic 15 on a damaging effect, or a spell that triggers
//! one), keyed (class, name), generated from the client by
//! `scripts/tbc_bleeds.py` at build 2.5.6.69795. Joined to the kit by each
//! ability's class (a pet ability's pet line) and RON `name`, it must name
//! exactly the abilities that declare a bleed. Neither side is a list kept
//! by hand: a new "Garrote" or "Deep Wounds" fails here until it declares
//! the bleed, whoever writes it.

use std::collections::BTreeSet;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use arenasim::states::play_match::components::Mechanic;

/// The (class, name) rows of the client fixture.
fn tbc_bleeds() -> BTreeSet<(String, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/tbc_bleeds.csv");
    let text = std::fs::read_to_string(path).expect("tests/data/tbc_bleeds.csv");
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    assert_eq!(
        lines.next(),
        Some("class,name,spells,via"),
        "the fixture's header"
    );
    lines
        .map(|line| {
            let mut cols = line.split(',');
            let class = cols.next().expect("class").to_string();
            let name = cols.next().expect("name").to_string();
            (class, name)
        })
        .collect()
}

/// The fixture's key for an ability: its class (or its pet's skill line,
/// "Pet - Spider") and its name.
fn key(def: &AbilityConfig) -> (String, String) {
    let owner = match def.pet {
        Some(pet) => format!("Pet - {pet:?}"),
        None => def.class.name().to_string(),
    };
    (owner, def.name.clone())
}

#[test]
fn the_declared_bleeds_are_the_tbc_clients() {
    let tbc = tbc_bleeds();
    let defs = AbilityDefinitions::shipped();
    let declared: BTreeSet<AbilityType> = defs
        .iter()
        .filter(|(_, def)| {
            def.applies_aura
                .as_ref()
                .is_some_and(|a| a.mechanic == Some(Mechanic::Bleed))
        })
        .map(|(ability, _)| *ability)
        .collect();
    let in_client: BTreeSet<AbilityType> = defs
        .iter()
        .filter(|(_, def)| tbc.contains(&key(def)))
        .map(|(ability, _)| *ability)
        .collect();
    assert_eq!(
        declared, in_client,
        "abilities that declare `mechanic: Some(Bleed)` (left) against those TBC calls a \
         bleed (right, tests/data/tbc_bleeds.csv)"
    );
    // The members, named: an empty join (a renamed class, a moved fixture)
    // would make the equality above hold over nothing.
    assert_eq!(
        in_client,
        BTreeSet::from([AbilityType::Rend, AbilityType::Rupture]),
        "the kit's bleeds; edit this list when a bleed joins the kit"
    );
}

/// The fixture reads as the client wrote it: the classes the sim knows, and
/// the bleeds the card names.
#[test]
fn the_fixture_holds_the_tbc_bleeds() {
    let tbc = tbc_bleeds();
    for (class, name) in [
        ("Warrior", "Rend"),
        ("Warrior", "Deep Wounds"),
        ("Rogue", "Rupture"),
        ("Rogue", "Garrote"),
        ("Druid", "Rake"),
        ("Druid", "Pounce"),
    ] {
        assert!(
            tbc.contains(&(class.to_string(), name.to_string())),
            "{class} {name}"
        );
    }
    assert!(
        !tbc.contains(&("Rogue".to_string(), "Hemorrhage".to_string())),
        "not every Physical attack is a bleed"
    );
}
