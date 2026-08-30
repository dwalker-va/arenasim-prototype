//! Audits that `InstantAbilityFired::is_spawned_for` matches the real spawn
//! sites in the source.
//!
//! That list is the seam between combat code and the animation sandbox: the
//! sandbox runs neither the class AIs nor `combat_ai.rs`'s `QueuedInstantAttack`
//! drain, so it spawns the marker itself for exactly the abilities the list
//! names. The sandbox's own test asserts every LISTED ability previews.
//!
//! It cannot assert the converse. A `commands.spawn((InstantAbilityFired {..}))`
//! added to a class AI without being added to the list is invisible to any
//! runtime check — the ability simply never previews and its signature never
//! fires there, with nothing failing. This scans the source the way
//! `tests/registration_audit.rs` scans it for unregistered systems.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::InstantAbilityFired;

/// Files that legitimately spawn the marker, and are therefore scanned.
///
/// `animation_sandbox/playback.rs` is deliberately excluded: it spawns the
/// marker DERIVED from the list, so including it would make this circular.
const SPAWN_SOURCES: &[&str] = &[
    "src/states/play_match/combat_ai.rs",
    "src/states/play_match/class_ai/mage.rs",
    "src/states/play_match/class_ai/rogue.rs",
    "src/states/play_match/class_ai/paladin.rs",
    "src/states/play_match/class_ai/warrior.rs",
    "src/states/play_match/class_ai/hunter.rs",
    "src/states/play_match/class_ai/priest.rs",
    "src/states/play_match/class_ai/warlock.rs",
    "src/states/play_match/class_ai/shaman.rs",
];

/// Abilities that reach the marker through `combat_ai.rs`'s generic
/// `QueuedInstantAttack` drain rather than by name. The drain spawns one marker
/// for whatever ability was queued, so the scan cannot see these literally.
const VIA_DRAIN: &[AbilityType] = &[
    AbilityType::MortalStrike,
    AbilityType::Ambush,
    AbilityType::SinisterStrike,
];

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

fn all_abilities() -> Vec<AbilityType> {
    AbilityDefinitions::default().iter().map(|(a, _)| *a).collect()
}

fn listed() -> Vec<AbilityType> {
    all_abilities()
        .into_iter()
        .filter(|a| InstantAbilityFired::is_spawned_for(*a))
        .collect()
}

/// Resolves the ability named at a spawn site, which is rarely a literal.
///
/// Call sites bind the ability first (`let kidney_shot = AbilityType::KidneyShot;`
/// then `ability: kidney_shot`), including via field shorthand, so the scan has
/// to walk BACKWARD from the spawn for the nearest binding of that name. A
/// file-wide map would be ambiguous — `rogue.rs` binds `ability` three separate
/// times, once per ability it can use.
///
/// `None` means the name resolves to no local binding, which is the generic
/// `QueuedInstantAttack` drain forwarding a runtime value.
fn resolve_ability(src: &str, before: usize, name: &str) -> Option<String> {
    if let Some(variant) = name.strip_prefix("AbilityType::") {
        return Some(variant.trim().to_string());
    }
    let needle = format!("let {name} = AbilityType::");
    let idx = src[..before].rfind(&needle)?;
    let tail = &src[idx + needle.len()..];
    let variant: String = tail
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!variant.is_empty()).then_some(variant)
}

/// Every ability named in an `InstantAbilityFired { .. ability: X .. }` spawn.
fn scanned_spawn_sites() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for rel in SPAWN_SOURCES {
        let src = read(rel);
        for (idx, _) in src.match_indices("InstantAbilityFired {") {
            let tail = &src[idx..];
            let end = tail.find("},").unwrap_or(tail.len().min(400));
            let block = &tail[..end];
            let Some(a) = block.find("ability:") else {
                // Field shorthand (`ability,`) — the binding is named `ability`.
                if block.contains("ability,") {
                    if let Some(v) = resolve_ability(&src, idx, "ability") {
                        found.insert(v);
                    }
                }
                continue;
            };
            let expr = block[a + "ability:".len()..]
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if let Some(v) = resolve_ability(&src, idx, &expr) {
                found.insert(v);
            }
        }
    }
    found
}

#[test]
fn every_spawn_site_is_listed_in_is_spawned_for() {
    let scanned = scanned_spawn_sites();
    assert!(
        !scanned.is_empty(),
        "the scan found no spawn sites at all — it has broken, not passed"
    );
    let names: BTreeSet<String> = listed().iter().map(|a| format!("{a:?}")).collect();
    for name in &scanned {
        assert!(
            names.contains(name),
            "{name} spawns an InstantAbilityFired but is missing from \
             InstantAbilityFired::is_spawned_for, so the animation sandbox will \
             never preview it and its signature will never fire there"
        );
    }
}

#[test]
fn every_listed_ability_has_a_spawn_site() {
    // The other direction: a name left behind after its spawn site was removed
    // makes the sandbox preview something a real match never shows.
    let scanned = scanned_spawn_sites();
    for ability in listed() {
        if VIA_DRAIN.contains(&ability) {
            continue;
        }
        let name = format!("{ability:?}");
        assert!(
            scanned.contains(&name),
            "{name} is listed in is_spawned_for but no spawn site names it"
        );
    }
}

#[test]
fn the_drain_abilities_still_queue_instant_attacks() {
    // The three that reach the marker generically. If one stopped pushing a
    // QueuedInstantAttack it would drop out of the pipeline silently, and the
    // scan above cannot see it because the drain forwards a variable.
    let all = format!(
        "{}{}",
        read("src/states/play_match/class_ai/rogue.rs"),
        read("src/states/play_match/class_ai/warrior.rs")
    );
    for ability in VIA_DRAIN {
        let name = format!("{ability:?}");
        assert!(
            all.contains(&format!("AbilityType::{name}")),
            "{name} is said to reach the marker via the QueuedInstantAttack \
             drain, but no class AI mentions it"
        );
    }
    assert!(
        all.matches("instant_attacks.push").count() >= VIA_DRAIN.len(),
        "fewer instant_attacks.push sites than abilities said to use the drain"
    );
}
