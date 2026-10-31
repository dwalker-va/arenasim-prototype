//! Talent book audit
//!
//! Every combatant carries its own [`AbilityBook`]: the configs of the
//! abilities it has, resolved once at spawn. Talents (AS-297) resolve a build
//! into that book — granted abilities added, patches applied — so a sim path
//! that reads an ability's config from the GLOBAL table (`AbilityDefinitions`)
//! instead of the book of the combatant casting it reads the unpatched ability
//! and silently ignores the talent. This audit keeps the global table out of
//! the sim paths.
//!
//! **The question:** which items in the sim's source can reach the global
//! table at all? To call `AbilityDefinitions::get` a site needs a value of the
//! type, and every way to get one names it — a `Res<AbilityDefinitions>`, a
//! `&AbilityDefinitions` parameter or field, `AbilityDefinitions::shipped()`,
//! `world.resource::<AbilityDefinitions>()` — or calls
//! `load_ability_definitions()`. Two calls reach the shipped table without
//! naming it, and count as reads too: `AbilityBook::shipped*` (a book built
//! from it), and the bare `Combatant::new` (a combatant holding one). So the
//! audit finds every (file, enclosing
//! item) in non-test code under `src/states/play_match/` (less the graphical
//! `rendering/`, and the table's and the book's own modules) and
//! `src/headless/` that names either, and asserts the set
//! EQUALS the declared list below, each entry with why it reads the table and
//! not a book. A new reader fails until it is declared; a reader that moved to
//! a book fails until its entry is struck.
//!
//! [`AbilityBook`]: arenasim::states::play_match::ability_book::AbilityBook
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source, and skips `use`
//! lines (an import reads nothing).

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

/// The table itself and the book: where the table is defined and loaded, and
/// where a book is built from it. Not scanned — reading the table is their job.
const TABLE_AND_BOOK: &[&str] = &[
    "src/states/play_match/ability_config.rs",
    "src/states/play_match/ability_book.rs",
];

/// The sim items that read the global table, as (file, item), each with why.
const GLOBAL_TABLE_READERS: &[(&str, &str, &str)] = &[
    // ---- Spawn: where every combatant's book is resolved ----
    (
        "src/states/play_match/mod.rs",
        "setup_play_match",
        "the graphical spawn: hands the table to the two spawns below",
    ),
    (
        "src/states/play_match/mod.rs",
        "spawn_combatant",
        "resolves a combatant's book at spawn (graphical, and the sandbox's staging)",
    ),
    (
        "src/states/play_match/mod.rs",
        "spawn_pet",
        "resolves a pet's book at spawn (graphical, and the sandbox's staging)",
    ),
    (
        "src/headless/runner.rs",
        "headless_setup_match",
        "the headless spawn: resolves each combatant's and pet's book",
    ),
    (
        "src/headless/runner.rs",
        "PreloadedConfigs",
        "the parsed configs a batch shares across its matches, the table among them",
    ),
    (
        "src/headless/runner.rs",
        "load",
        "parses the table once for a batch",
    ),
    (
        "src/states/play_match/class_ai/scene.rs",
        "Scene",
        "a scene's own table, which its units' books are built from",
    ),
    (
        "src/states/play_match/class_ai/scene.rs",
        "new",
        "a scene loads the shipped table",
    ),
    (
        "src/states/play_match/class_ai/scene.rs",
        "with_config",
        "a scene under a given table",
    ),
    // ---- Readers with no combatant to hand ----
    (
        "src/headless/stage.rs",
        "land",
        "a staged aura whose named source need not have its ability (a test prop): \
         read from the caster's book when it does, else from the table",
    ),
    (
        "src/states/play_match/components/combatant.rs",
        "new",
        "the bare constructor unit tests build a combatant with (the shipped table's \
         book); no sim path calls it, which this audit also pins — a spawn names its book",
    ),
    (
        "src/states/play_match/projectiles.rs",
        "spawn_projectile_visuals",
        "a projectile's colours (graphical only)",
    ),
];

/// Every (file, enclosing item) in the sim's non-test source that names the
/// global table.
fn readers() -> BTreeSet<(String, String)> {
    let names = Regex::new(
        r"\b(AbilityDefinitions|load_ability_definitions|AbilityBook::shipped\w*|Combatant::new)\b",
    )
    .unwrap();
    let item = Regex::new(r"\b(?:fn|struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut found = BTreeSet::new();
    for file in load_sources(&["src/states/play_match", "src/headless"]).expect("read src/") {
        let rel = file.rel_display();
        if rel.starts_with("src/states/play_match/rendering/")
            || TABLE_AND_BOOK.contains(&rel.as_str())
        {
            continue;
        }
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in names.find_iter(&code) {
            let line_start = code[..m.start()].rfind('\n').map_or(0, |i| i + 1);
            if code[line_start..].trim_start().starts_with("use ")
                || code[line_start..].trim_start().starts_with("pub use ")
            {
                continue;
            }
            // A name inside a multi-line `use` group: the nearest `use` or `;`
            // before it decides.
            let before = &code[..m.start()];
            let last_use = before.rfind("use ");
            let last_semi = before.rfind(';');
            if let (Some(u), semi) = (last_use, last_semi) {
                if semi.is_none_or(|s| s < u) && before[u..].contains('{') {
                    continue;
                }
            }
            let enclosing = item
                .captures_iter(before)
                .last()
                .map_or_else(|| "<module>".to_string(), |c| c[1].to_string());
            found.insert((rel.clone(), enclosing));
        }
    }
    found
}

#[test]
fn the_global_ability_table_stays_out_of_the_sim() {
    let found = readers();
    let declared: BTreeSet<(String, String)> = GLOBAL_TABLE_READERS
        .iter()
        .map(|(file, item, _)| (file.to_string(), item.to_string()))
        .collect();
    let extra: Vec<_> = found.difference(&declared).collect();
    let missing: Vec<_> = declared.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "The set of sim items that read the global ability table changed.\n\
         A sim path with a combatant to hand reads its configs from that combatant's \
         book (`Combatant::book`, `CombatantInfo::book`), never from \
         `AbilityDefinitions`: a talent patches the book, so a table read ignores it. \
         A reader with no combatant to hand goes in GLOBAL_TABLE_READERS with its \
         reason; a reader that moved to a book has its entry struck.\n  \
         found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
}

/// Every declared reason is written, and no item is declared twice.
#[test]
fn every_global_table_reader_says_why() {
    let mut seen = BTreeSet::new();
    for (file, item, why) in GLOBAL_TABLE_READERS {
        assert!(!why.trim().is_empty(), "{file} {item}: no reason given");
        assert!(seen.insert((file, item)), "{file} {item} is declared twice");
    }
}
