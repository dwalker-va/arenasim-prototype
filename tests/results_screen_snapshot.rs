//! Offscreen visual snapshot of the post-match Results screen.
//!
//! This is the fast visual-iteration loop for `src/states/results_ui.rs`:
//! it renders the real `draw_results_screen` to a PNG via `egui_kittest`
//! (wgpu, no window, no match to play) in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render the screen; writes tests/snapshots/results_screen.new.png
//! cargo test --release --test results_screen_snapshot -- --ignored
//! # ...then open / read that PNG, tweak results_ui.rs, repeat.
//!
//! # Once it looks right, bless the baseline (test then passes as a
//! # regression guard; a future pixel change writes a .new.png + .diff.png):
//! UPDATE_SNAPSHOTS=1 cargo test --release --test results_screen_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps it out of the default `cargo test` run because it needs a
//! GPU adapter (wgpu), which CI runners may lack.

use bevy_egui::egui;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::Harness;

use arenasim::combat::log::CombatLog;
use arenasim::states::encyclopedia::EncyclopediaData;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::equipment::{load_item_definitions, ItemDefinitions};
use arenasim::states::play_match::{CombatantStats, MatchResults};
use arenasim::states::results_ui::{class_link_id, draw_results_screen};

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn results_screen_2v2() {
    let mut harness = harness(mock_results(), mock_combat_log());
    harness.run();
    harness.snapshot("results_screen");
}

/// A TOOLTIP-OPEN frame: the pointer is parked on Team 1's class cell, so the
/// linked-icon contract the Results screen borrowed from the encyclopedia is
/// what the snapshot actually shows — the class's own tooltip, built by the
/// shared builder, plus the "click to open" affordance line.
///
/// The hover is driven as real input (a `PointerMoved` event through egui's own
/// hover machinery), not by calling the tooltip renderer directly, so this
/// covers the interaction rather than the drawing of its contents. The widget's
/// rect is found by its PINNED id, so the probe does not depend on screen
/// coordinates that a layout tweak would silently invalidate.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn results_screen_class_tooltip() {
    let mut harness = harness(mock_results(), mock_combat_log());
    harness.run();

    let rect = harness
        .ctx
        .read_response(class_link_id(1, 0, CharacterClass::Rogue))
        .expect("the Team 1 Rogue row must expose a linked class cell")
        .rect;
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(rect.center()));
    harness.run();

    harness.snapshot("results_screen_class_tooltip");
}

/// The ability breakdowns EXPANDED, which is where the screen's ability icons
/// live: every bar that names a real ability carries that ability's icon and is
/// a link to its page, and the rows that name no ability (auto attacks, wands)
/// keep the reserved slot empty rather than stepping their labels in and out.
///
/// The expanders are opened by CLICKING them through the harness, so the
/// snapshot is of a state a reader can actually reach.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn results_screen_ability_links() {
    let mut harness = sized_harness(mock_results(), mock_combat_log(), [1500.0, 1000.0]);
    harness.run();

    for node in harness.get_all_by_label("Ability breakdown") {
        node.click();
    }
    harness.run();

    harness.snapshot("results_screen_ability_links");
}

/// Stress the stat-column alignment: deliberately mix value widths within and
/// across rows — em-dash (zero heal) next to "12.3k", 1-digit vs 2-digit K —
/// so any column that tracks content width instead of a fixed width visibly
/// misaligns from its header.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn results_screen_value_combos() {
    let results = MatchResults {
        winner: Some(1),
        duration_secs: 187.0,
        // Two Rogues (slots 0/1) exercise the duplicate-class case: adjacent
        // rows with the same class, distinguished only by the "#N" suffix +
        // widened name cell.
        team1_combatants: vec![
            cs(CharacterClass::Rogue, 0, 956.0, 0.0, 334.0, true),
            cs(CharacterClass::Rogue, 1, 8.0, 0.0, 5.0, true),
            cs(CharacterClass::Priest, 2, 451.0, 1820.0, 301.0, true),
        ],
        team2_combatants: vec![
            cs(CharacterClass::Warlock, 0, 1234.0, 0.0, 12345.0, false),
            cs(CharacterClass::Priest, 1, 388.0, 13400.0, 451.0, false),
            cs(CharacterClass::Hunter, 2, 0.0, 0.0, 7.0, false),
        ],
        pet_damage_links: Default::default(),
    };

    // Vary killing-blow counts so the K column spans 1- and 2-digit widths.
    // The K column reads killing-blow *Damage* events (not Death events), and
    // ids carry the #slot suffix, so seed those to match the rows above.
    let mut log = CombatLog::default();
    let rogue = "Team 1 Rogue #1".to_string();
    let warlock = "Team 2 Warlock #1".to_string();
    for _ in 0..2 {
        log.log_damage(rogue.clone(), "Team 2 Priest #2".to_string(), "Eviscerate".to_string(), 100.0, true, false, String::new());
    }
    for _ in 0..11 {
        log.log_damage(warlock.clone(), "Team 1 Rogue #2".to_string(), "Shadow Bolt".to_string(), 100.0, true, false, String::new());
    }

    let mut harness = harness(results, log);
    harness.run();
    harness.snapshot("results_screen_value_combos");
}

/// Build the offscreen harness over the real pure draw.
///
/// `EncyclopediaData` is the same read-only bundle the encyclopedia renders
/// from — it is Bevy-free, which is exactly why the Results screen could adopt
/// the linked-icon widget without giving up this loop. No egui textures exist
/// in kittest, so class icons stay the screen's class-color fallback squares
/// and ability icons render the widget's placeholder tile.
fn harness(results: MatchResults, log: CombatLog) -> Harness<'static> {
    sized_harness(results, log, [1500.0, 820.0])
}

/// [`harness`] at an explicit size, for the views that need more room.
fn sized_harness(results: MatchResults, log: CombatLog, size: [f32; 2]) -> Harness<'static> {
    let items: ItemDefinitions = load_item_definitions().expect("items.ron must load");
    let abilities: AbilityDefinitions =
        load_ability_definitions().expect("abilities.ron must load");

    Harness::builder()
        .with_size(size)
        .build(move |ctx| {
            let data = EncyclopediaData {
                items: &items,
                abilities: &abilities,
                item_icons: None,
                class_icons: None,
                ability_icons: None,
            };
            let _ = draw_results_screen(ctx, Some(&results), &log, &data);
        })
}

fn cs(class: CharacterClass, slot: u8, dmg: f32, heal: f32, tkn: f32, survived: bool) -> CombatantStats {
    CombatantStats {
        class,
        slot,
        damage_dealt: dmg,
        damage_taken: tkn,
        healing_done: heal,
        survived,
    }
}

/// Representative 2v2 result: Rogue+Priest beat Warlock+Priest.
fn mock_results() -> MatchResults {
    MatchResults {
        winner: Some(1),
        duration_secs: 53.0,
        team1_combatants: vec![
            CombatantStats {
                class: CharacterClass::Rogue,
                slot: 0,
                damage_dealt: 956.0,
                damage_taken: 334.0,
                healing_done: 0.0,
                survived: true,
            },
            CombatantStats {
                class: CharacterClass::Priest,
                slot: 1,
                damage_dealt: 451.0,
                damage_taken: 301.0,
                healing_done: 1820.0,
                survived: true,
            },
        ],
        team2_combatants: vec![
            CombatantStats {
                class: CharacterClass::Warlock,
                slot: 0,
                damage_dealt: 612.0,
                damage_taken: 956.0,
                healing_done: 0.0,
                survived: false,
            },
            CombatantStats {
                class: CharacterClass::Priest,
                slot: 1,
                damage_dealt: 388.0,
                damage_taken: 451.0,
                healing_done: 1340.0,
                survived: false,
            },
        ],
        pet_damage_links: Default::default(),
    }
}

/// Small but representative event log so the K column, ability-breakdown
/// expanders, and CC lines have real data.
fn mock_combat_log() -> CombatLog {
    let mut log = CombatLog::default();

    // Ids carry the 1-based slot suffix, matching mock_results()'s slots.
    let rogue = "Team 1 Rogue #1".to_string();
    let t1_priest = "Team 1 Priest #2".to_string();
    let warlock = "Team 2 Warlock #1".to_string();
    let t2_priest = "Team 2 Priest #2".to_string();

    for (ability, amount, kb) in [
        ("Sinister Strike", 50.0, false),
        ("Sinister Strike", 98.0, false),
        ("Ambush", 210.0, false),
        ("Eviscerate", 188.0, false),
        ("Sinister Strike", 64.0, true),
    ] {
        log.log_damage(rogue.clone(), warlock.clone(), ability.to_string(), amount, kb, false, String::new());
    }
    for (ability, amount, kb) in [
        ("Sinister Strike", 51.0, false),
        ("Sinister Strike", 49.0, false),
        ("Eviscerate", 156.0, true),
    ] {
        log.log_damage(rogue.clone(), t2_priest.clone(), ability.to_string(), amount, kb, false, String::new());
    }
    for (ability, amount) in [
        ("Flash Heal", 420.0),
        ("Flash Heal", 380.0),
        ("Renew", 220.0),
        ("Greater Heal", 800.0),
    ] {
        log.log_healing(t1_priest.clone(), rogue.clone(), ability.to_string(), amount, false, String::new());
    }
    log.log_crowd_control(rogue.clone(), warlock.clone(), "Kidney Shot".to_string(), 6.0, String::new());
    log.log_crowd_control(t2_priest.clone(), rogue.clone(), "Psychic Scream".to_string(), 2.0, String::new());
    log.log_death(warlock, Some(rogue.clone()), String::new());
    log.log_death(t2_priest, Some(rogue), String::new());

    log
}
