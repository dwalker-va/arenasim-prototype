//! Every ability's casts draw a real icon on the in-match timeline.
//!
//! The timeline once drew its fallback from a hand-written table of about
//! eighteen abilities, so every cast outside it — the whole Shaman, Druid,
//! Paladin and Hunter kits — showed a grey "??" whenever its art was not
//! loaded. It now derives the icon from `abilities.ron`'s `icon` field
//! ([`timeline_icon_path`]), the same field the in-match loader registers
//! under the ability's name ([`ability_icon_paths`]). This holds that chain
//! for every ability `abilities.ron` defines: the timeline resolves the name
//! the log records to the ability, that ability has an icon on disk, and the
//! loader registers it under the key the timeline reads it back by.

use std::collections::BTreeMap;

use arenasim::paths::asset_path;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::rendering::{
    ability_icon_paths, timeline_ability, timeline_icon_path,
};

#[test]
fn every_ability_resolves_a_timeline_icon() {
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let registered: BTreeMap<String, String> = ability_icon_paths(&abilities).into_iter().collect();

    let mut checked = 0;
    let mut failures = Vec::new();
    for (ability, config) in abilities.iter() {
        checked += 1;
        // The log records a cast by the ability's display name.
        let Some(resolved) = timeline_ability(&config.name, &abilities) else {
            failures.push(format!(
                "{ability:?}: \"{}\" resolves to no ability",
                config.name
            ));
            continue;
        };
        if resolved.name != config.name {
            failures.push(format!(
                "{ability:?}: \"{}\" resolves to \"{}\"",
                config.name, resolved.name
            ));
            continue;
        }
        let Some(path) = timeline_icon_path(&config.name, &abilities) else {
            failures.push(format!("{ability:?}: no `icon` in abilities.ron"));
            continue;
        };
        if !asset_path(path).is_file() {
            failures.push(format!(
                "{ability:?}: icon {path} is not a file under assets/"
            ));
        }
        if registered.get(&config.name).map(String::as_str) != Some(path) {
            failures.push(format!(
                "{ability:?}: the loader does not register {path} under \"{}\"",
                config.name
            ));
        }
    }

    // `load_ability_definitions` refuses a file missing any `AbilityType`
    // (`validate()`), so the loop above covered every ability there is.
    assert!(
        failures.is_empty(),
        "{} of {checked} abilities draw no timeline icon:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// The name lookup has to be unambiguous, or a cast could draw another
/// ability's icon.
#[test]
fn ability_display_names_are_unique() {
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    let mut by_name: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (ability, config) in abilities.iter() {
        by_name
            .entry(config.name.as_str())
            .or_default()
            .push(format!("{ability:?}"));
    }
    let shared: Vec<String> = by_name
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(name, owners)| format!("\"{name}\": {}", owners.join(", ")))
        .collect();
    assert!(shared.is_empty(), "display names shared: {shared:?}");
}

/// Holy Shock logs which way it went; both variants draw Holy Shock's icon,
/// and a name no ability carries resolves to nothing.
#[test]
fn a_logged_variant_resolves_to_its_ability() {
    let abilities = load_ability_definitions().expect("abilities.ron loads");
    for logged in ["Holy Shock (Heal)", "Holy Shock (Damage)"] {
        let resolved = timeline_ability(logged, &abilities)
            .unwrap_or_else(|| panic!("\"{logged}\" resolves to no ability"));
        assert_eq!(resolved.name, "Holy Shock", "\"{logged}\"");
        assert!(
            timeline_icon_path(logged, &abilities).is_some(),
            "\"{logged}\""
        );
    }
    assert!(timeline_ability("Not An Ability", &abilities).is_none());
    assert!(timeline_ability("Not An Ability (Heal)", &abilities).is_none());
}
