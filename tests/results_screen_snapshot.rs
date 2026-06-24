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

use egui_kittest::Harness;

use arenasim::combat::log::CombatLog;
use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::{CombatantStats, MatchResults};
use arenasim::states::results_ui::draw_results_screen;

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn results_screen_2v2() {
    let results = mock_results();
    let log = mock_combat_log();
    let icons = ClassIcons::default(); // no textures -> class-color fallback squares

    let mut harness = Harness::builder()
        .with_size([1500.0, 820.0])
        .build(move |ctx| {
            draw_results_screen(ctx, Some(&results), &log, &icons);
        });

    harness.run();
    harness.snapshot("results_screen");
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
        team1_combatants: vec![
            cs(CharacterClass::Rogue, 956.0, 0.0, 334.0, true),
            cs(CharacterClass::Mage, 8.0, 0.0, 5.0, true),
            cs(CharacterClass::Priest, 451.0, 1820.0, 301.0, true),
        ],
        team2_combatants: vec![
            cs(CharacterClass::Warlock, 1234.0, 0.0, 12345.0, false),
            cs(CharacterClass::Priest, 388.0, 13400.0, 451.0, false),
            cs(CharacterClass::Hunter, 0.0, 0.0, 7.0, false),
        ],
    };

    // Vary killing-blow counts so the K column spans 1- and 2-digit widths.
    let mut log = CombatLog::default();
    let rogue = "Team 1 Rogue".to_string();
    let warlock = "Team 2 Warlock".to_string();
    for _ in 0..2 {
        log.log_death("Team 2 Priest".to_string(), Some(rogue.clone()), String::new());
    }
    for _ in 0..11 {
        log.log_death("Team 1 Mage".to_string(), Some(warlock.clone()), String::new());
    }

    let icons = ClassIcons::default();
    let mut harness = Harness::builder()
        .with_size([1500.0, 820.0])
        .build(move |ctx| {
            draw_results_screen(ctx, Some(&results), &log, &icons);
        });
    harness.run();
    harness.snapshot("results_screen_value_combos");
}

fn cs(class: CharacterClass, dmg: f32, heal: f32, tkn: f32, survived: bool) -> CombatantStats {
    CombatantStats {
        class,
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
                damage_dealt: 956.0,
                damage_taken: 334.0,
                healing_done: 0.0,
                survived: true,
            },
            CombatantStats {
                class: CharacterClass::Priest,
                damage_dealt: 451.0,
                damage_taken: 301.0,
                healing_done: 1820.0,
                survived: true,
            },
        ],
        team2_combatants: vec![
            CombatantStats {
                class: CharacterClass::Warlock,
                damage_dealt: 612.0,
                damage_taken: 956.0,
                healing_done: 0.0,
                survived: false,
            },
            CombatantStats {
                class: CharacterClass::Priest,
                damage_dealt: 388.0,
                damage_taken: 451.0,
                healing_done: 1340.0,
                survived: false,
            },
        ],
    }
}

/// Small but representative event log so the K column, ability-breakdown
/// expanders, and CC lines have real data.
fn mock_combat_log() -> CombatLog {
    let mut log = CombatLog::default();

    let rogue = "Team 1 Rogue".to_string();
    let t1_priest = "Team 1 Priest".to_string();
    let warlock = "Team 2 Warlock".to_string();
    let t2_priest = "Team 2 Priest".to_string();

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
