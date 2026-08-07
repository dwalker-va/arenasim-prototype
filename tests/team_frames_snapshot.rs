//! Offscreen visual snapshot of the in-match team frames (spectator UI).
//!
//! Fast visual-iteration loop for `src/states/play_match/rendering/team_frames.rs`:
//! renders the real `draw_team_frames` to a PNG via `egui_kittest` (wgpu, no
//! window, no match to play) in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render the screen; writes tests/snapshots/team_frames.new.png
//! cargo test --release --test team_frames_snapshot -- --ignored
//! # ...then open / read that PNG, tweak team_frames.rs, repeat.
//!
//! # Once it looks right, bless the baseline:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test team_frames_snapshot -- --ignored
//! ```
//!
//! Fidelity caveats (same as the Results screen): kittest has no Bevy
//! textures, so class icons render as class-color fallback squares and aura
//! icons as gold/red fallback blocks; fonts are egui defaults. Layout,
//! spacing, and color iterate faithfully.
//!
//! The click tests below are NOT `#[ignore]`d: kittest's renderer is lazy, so
//! driving the harness and reading back `draw_team_frames`'s action needs no
//! GPU — only `snapshot()` does.

use std::cell::RefCell;
use std::rc::Rc;

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::{
    apply_call_click, column_frame_rects, draw_team_frames, CallClick, CombatantFrame, FrameAura,
    ResourceType, SpellIcons, TeamFramesData,
};

/// The harness viewport, and therefore `ctx.available_rect()` — the rect the
/// frame layout is measured against.
const SCREEN: [f32; 2] = [1500.0, 820.0];

fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(SCREEN[0], SCREEN[1]))
}

fn aura(icon_key: &str, remaining: f32, is_buff: bool, is_hard_cc: bool) -> FrameAura {
    FrameAura {
        icon_key: icon_key.to_string(),
        remaining,
        is_buff,
        is_hard_cc,
    }
}

/// A busy 2v2-with-pets scene exercising every frame element: full/hurt/dead
/// HP states, all three resource types, absorb overlay, buff + debuff rows
/// (including overflow), stealth tag, pet frames. (Cast bars live on the
/// overhead nameplate, not in the frames.)
///
/// Call markers start hidden; the tests that want them set `show_calls` and
/// the called slots themselves.
fn mock_data() -> TeamFramesData {
    TeamFramesData {
        team1_called_slot: None,
        team2_called_slot: None,
        show_calls: false,
        team1: vec![
            CombatantFrame {
                class: CharacterClass::Warrior,
                pet_label: None,
                alive: true,
                stealthed: false,
                current_health: 187.0,
                max_health: 300.0,
                absorb: 0.0,
                current_resource: 62.0,
                max_resource: 100.0,
                resource_type: ResourceType::Rage,
                buffs: vec![
                    aura("Battle Shout", 96.0, true, false),
                    aura("Berserker Rage", 7.3, true, false),
                ],
                debuffs: vec![
                    aura("Corruption", 12.0, false, false),
                    aura("Curse of Agony", 19.0, false, false),
                    aura("aura_slow", 4.2, false, false),
                ],
            },
            CombatantFrame {
                class: CharacterClass::Priest,
                pet_label: None,
                alive: true,
                stealthed: false,
                current_health: 96.0,
                max_health: 250.0,
                absorb: 60.0,
                current_resource: 41.0,
                max_resource: 150.0,
                resource_type: ResourceType::Mana,
                buffs: vec![
                    aura("Power Word: Shield", 21.0, true, false),
                    aura("Power Word: Fortitude", 412.0, true, false),
                ],
                debuffs: vec![aura("aura_fear", 5.6, false, true)],
            },
        ],
        team2: vec![
            CombatantFrame {
                class: CharacterClass::Warlock,
                pet_label: None,
                alive: true,
                stealthed: false,
                current_health: 44.0,
                max_health: 250.0,
                absorb: 0.0,
                current_resource: 118.0,
                max_resource: 200.0,
                resource_type: ResourceType::Mana,
                buffs: vec![],
                debuffs: vec![
                    aura("Rend", 9.0, false, false),
                    aura("Mortal Strike", 6.5, false, false),
                    aura("aura_dot", 3.0, false, false),
                    aura("aura_dot", 7.7, false, false),
                    aura("aura_dot", 11.0, false, false),
                    aura("aura_dot", 14.0, false, false),
                    aura("aura_dot", 17.0, false, false),
                    aura("aura_dot", 21.0, false, false),
                    aura("aura_dot", 23.0, false, false),
                    aura("aura_dot", 24.0, false, false),
                ],
            },
            CombatantFrame {
                class: CharacterClass::Rogue,
                pet_label: None,
                alive: false,
                stealthed: false,
                current_health: 0.0,
                max_health: 275.0,
                absorb: 0.0,
                current_resource: 35.0,
                max_resource: 100.0,
                resource_type: ResourceType::Energy,
                buffs: vec![],
                debuffs: vec![],
            },
            CombatantFrame {
                class: CharacterClass::Hunter,
                pet_label: Some("Spider (pet)".to_string()),
                alive: true,
                stealthed: false,
                current_health: 61.0,
                max_health: 180.0,
                absorb: 0.0,
                current_resource: 0.0,
                max_resource: 0.0,
                resource_type: ResourceType::Mana,
                buffs: vec![],
                debuffs: vec![],
            },
        ],
    }
}

/// Drive `draw_team_frames` in a kittest harness, returning every [`CallClick`]
/// it reported. Only `snapshot()` needs a GPU, so this is a plain test.
fn run_clicks(data: TeamFramesData, pointer: Option<egui::Pos2>) -> Vec<CallClick> {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&clicks);
    let class_icons = ClassIcons::default();
    let spell_icons = SpellIcons::default();

    let mut harness = Harness::builder().with_size(SCREEN).build(move |ctx| {
        if let Some(click) = draw_team_frames(ctx, &data, &class_icons, &spell_icons) {
            sink.borrow_mut().push(click);
        }
    });

    // egui hit-tests against the previous pass's widget rects, so the frames
    // must be laid out once before the pointer arrives, and press and release
    // need separate passes.
    harness.run();
    if let Some(pos) = pointer {
        harness.input_mut().events.push(egui::Event::PointerMoved(pos));
        harness.run();
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
            harness.run();
        }
    }

    let out = clicks.borrow().clone();
    out
}

/// Guard the click tests against a silent miss: if a scenario's index no
/// longer points at the frame it means to, the test should fail loudly rather
/// than pass because nothing was hit.
fn assert_is_pet(frames: &[CombatantFrame], index: usize, expected: bool) {
    assert_eq!(
        frames[index].pet_label.is_some(),
        expected,
        "frame {index} is not the kind of frame this test targets"
    );
}

#[test]
fn clicking_an_enemy_frame_reports_its_column_and_slot() {
    let mut data = mock_data();
    data.show_calls = true;
    let target = column_frame_rects(&data.team2, 2, screen_rect())[0].center();

    assert_eq!(
        run_clicks(data, Some(target)),
        vec![CallClick {
            clicked_team: 2,
            slot: 0
        }]
    );
}

#[test]
fn clicking_a_pet_sub_frame_is_a_no_op() {
    let mut data = mock_data();
    data.show_calls = true;
    // Team 2's third frame is the Hunter's pet.
    assert_is_pet(&data.team2, 2, true);
    let target = column_frame_rects(&data.team2, 2, screen_rect())[2].center();

    assert!(
        run_clicks(data, Some(target)).is_empty(),
        "a pet sub-frame must not be callable"
    );
}

#[test]
fn a_pet_does_not_shift_the_slot_of_the_primary_below_it() {
    // The pet sits last in the mock, so re-order it above the Rogue and check
    // the Rogue still answers to slot 1.
    let mut data = mock_data();
    data.show_calls = true;
    let pet = data.team2.remove(2);
    data.team2.insert(1, pet);
    let target = column_frame_rects(&data.team2, 2, screen_rect())[2].center();

    assert_eq!(
        run_clicks(data, Some(target)),
        vec![CallClick {
            clicked_team: 2,
            slot: 1
        }]
    );
}

#[test]
fn clicks_are_ignored_while_the_affordance_is_hidden() {
    let mut data = mock_data();
    data.show_calls = false;
    data.team2_called_slot = Some(0);
    let target = column_frame_rects(&data.team2, 2, screen_rect())[0].center();

    let clicks = run_clicks(data, Some(target));
    assert!(clicks.is_empty(), "a hidden affordance senses nothing");

    // ...and the stored calls stay exactly as they were.
    let mut config = MatchConfig::default();
    config.team1_kill_target = Some(0);
    config.team2_kill_target = Some(1);
    for click in &clicks {
        apply_call_click(&mut config, *click);
    }
    assert_eq!(config.team1_kill_target, Some(0));
    assert_eq!(config.team2_kill_target, Some(1));
}

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn team_frames_2v2() {
    let data = mock_data();
    let class_icons = ClassIcons::default(); // no textures -> class-color fallback squares
    let spell_icons = SpellIcons::default(); // no textures -> gold/red fallback blocks

    let mut harness = Harness::builder()
        .with_size(SCREEN)
        .build(move |ctx| {
            draw_team_frames(ctx, &data, &class_icons, &spell_icons);
        });

    harness.run();
    harness.snapshot("team_frames");
}

/// The same scene with the call affordance on and one combatant called per
/// column — Team 2 calling the Team 1 Priest, Team 1 calling the (dead) Team 2
/// Rogue, which also exercises the reticle sharing the header with a DEAD tag.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn team_frames_with_calls() {
    let mut data = mock_data();
    data.show_calls = true;
    data.team1_called_slot = Some(1);
    data.team2_called_slot = Some(1);
    let class_icons = ClassIcons::default();
    let spell_icons = SpellIcons::default();

    let mut harness = Harness::builder()
        .with_size(SCREEN)
        .build(move |ctx| {
            draw_team_frames(ctx, &data, &class_icons, &spell_icons);
        });

    harness.run();
    harness.snapshot("team_frames_with_calls");
}
