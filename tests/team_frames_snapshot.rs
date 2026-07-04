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

use egui_kittest::Harness;

use arenasim::states::configure_match_ui::ClassIcons;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::{
    draw_team_frames, CombatantFrame, FrameAura, ResourceType, SpellIcons, TeamFramesData,
};

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
fn mock_data() -> TeamFramesData {
    TeamFramesData {
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

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn team_frames_2v2() {
    let data = mock_data();
    let class_icons = ClassIcons::default(); // no textures -> class-color fallback squares
    let spell_icons = SpellIcons::default(); // no textures -> gold/red fallback blocks

    let mut harness = Harness::builder()
        .with_size([1500.0, 820.0])
        .build(move |ctx| {
            draw_team_frames(ctx, &data, &class_icons, &spell_icons);
        });

    harness.run();
    harness.snapshot("team_frames");
}
