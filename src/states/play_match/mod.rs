//! Play Match Scene - 3D Combat Arena
//!
//! This module handles the active match simulation where combatants battle each other.
//! Inspired by World of Warcraft's combat mechanics, it features:
//!
//! ## Combat System
//! - **Target Acquisition**: Combatants automatically find the nearest alive enemy
//! - **Movement**: Combatants move towards targets if out of range
//! - **Range Mechanics**: Melee attacks require being in melee range (2.5 units)
//! - **Auto-Attacks**: Each combatant attacks when in range, based on attack speed
//! - **Damage & Stats**: Tracks damage dealt/taken for each combatant
//! - **Win Conditions**: Match ends when all combatants of one team are eliminated
//!
//! ## Visual Representation
//! - 3D capsule meshes represent combatants, colored by class
//! - Health bars rendered above each combatant's head using 2D overlay
//! - Combatants rotate to face their targets
//! - Simple arena floor (60x60 plane)
//! - Isometric camera view
//!
//! ## Flow
//! 1. `setup_play_match`: Spawns arena, camera, lights, and combatants from `MatchConfig`
//! 2. Systems run each frame:
//!    - `update_play_match`: Handle ESC key to exit
//!    - `acquire_targets`: Find nearest enemy for each combatant
//!    - `move_to_target`: Move combatants towards targets if out of range
//!    - `combat_auto_attack`: Process attacks when in range, based on attack speed
//!    - `check_match_end`: Detect when match is over, transition to Results
//!    - `render_health_bars`: Draw 2D health bars over 3D combatants
//! 3. `cleanup_play_match`: Despawn all entities when exiting

// Submodules
pub mod abilities;
pub mod ability_config;
pub mod movement_config;
pub mod equipment;
pub mod components;
pub mod camera;
pub mod projectiles;
pub mod rendering;
pub mod auras;
pub mod effects;
pub mod match_flow;
pub mod traps;
pub mod combat_ai;
pub mod combat_core;
pub mod shadow_sight;
pub mod systems;
pub mod utils;
pub mod class_ai;
pub mod constants;
pub mod decision_trace;
pub mod selection;

// Re-exports
pub use abilities::*;
pub use ability_config::*;
pub use movement_config::*;
pub use components::*;
pub use camera::*;
pub use projectiles::*;
pub use rendering::*;
pub use auras::*;
pub use match_flow::*;
pub use combat_ai::*;
pub use combat_core::*;
pub use shadow_sight::*;
pub use utils::*;
pub use constants::*;
pub use effects::*;
pub use traps::*;
pub use class_ai::pet_ai::pet_ai_system;
pub use selection::{
    pick_selected_combatant, sync_selection_ring, follow_selection_ring,
    reset_selection_on_exit, Selection,
};

use bevy::prelude::*;
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::CascadeShadowConfigBuilder;
use bevy::math::Affine2;
use bevy::image::{ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::render_asset::RenderAssetUsages;
use super::match_config::{self, MatchConfig};
use super::GameState;
use crate::combat::log::{CombatLog, CombatLogEventType};
use equipment::{ItemDefinitions, DefaultLoadouts, ItemSlot, ItemId, resolve_loadout, enforce_two_hand_conflicts, format_loadout};

// ============================================================================
// Helper Functions
// ============================================================================

/// Integer hash (Murmur-style finalizer) for deterministic per-texel noise.
fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Deterministic white noise in 0..1 for a texel coordinate.
fn texel_noise(x: u32, y: u32) -> f32 {
    let h = hash_u32(
        x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663),
    );
    h as f32 / u32::MAX as f32
}

/// Generates a seamless, tileable surface texture procedurally — no asset
/// files. A `base` color (sRGB component space) is baked in as the average so
/// the surface's overall tone is unchanged; the texture adds low-frequency
/// blotches (weathered patches), fine grain, and — for masonry — faint
/// horizontal courses. All noise sources are periodic across the image so it
/// tiles without visible seams: the blotches use integer-frequency sinusoids,
/// the grain is independent per-texel, and the courses divide the image into a
/// whole number of bands (trivially seamless under Repeat wrapping).
///
/// - `blotch_amp` / `grain_amp`: multiplicative variation strength.
/// - `courses`: number of horizontal stone courses across the image height
///   (`0` disables — used by the floor).
fn create_surface_texture(base: [f32; 3], blotch_amp: f32, grain_amp: f32, courses: u32) -> Image {
    const SIZE: u32 = 512;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f32 / SIZE as f32;
            let v = y as f32 / SIZE as f32;
            let tau = std::f32::consts::TAU;

            // Low-frequency blotches — integer wavenumbers keep it periodic.
            let blotch = 0.50 * (tau * u).sin() * (tau * v).cos()
                + 0.30 * (tau * 2.0 * u + 1.3).sin() * (tau * 2.0 * v + 0.7).cos()
                + 0.20 * (tau * 3.0 * u + 2.1).sin() * (tau * 1.0 * v + 2.4).cos();

            let grain = texel_noise(x, y) - 0.5;

            let mut variation = 1.0 + blotch_amp * blotch + grain_amp * grain;

            // Darken mortar lines between horizontal stone courses.
            if courses > 0 {
                let cv = v * courses as f32;
                let dist_to_line = (cv - cv.round()).abs(); // 0 at a course boundary
                let line = (1.0 - (dist_to_line / 0.06)).clamp(0.0, 1.0); // ramp near line
                variation *= 1.0 - 0.35 * line;
            }

            let variation = variation.clamp(0.6, 1.3);
            let idx = ((y * SIZE + x) * 4) as usize;
            for c in 0..3 {
                data[idx + c] = ((base[c] * variation).clamp(0.0, 1.0) * 255.0) as u8;
            }
            data[idx + 3] = 255;
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Repeat wrapping so the mesh can tile the texture across the surface.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Creates an octagonal floor mesh matching the arena wall layout.
/// `uv_scale` maps world units to texture space (UV = world_pos * uv_scale),
/// giving square, uniformly-tiled texels regardless of the floor's aspect
/// ratio. Smaller values = the texture repeats more often.
fn create_octagon_mesh(length: f32, width: f32, corner_cut: f32, uv_scale: f32) -> Mesh {
    let half_length = length / 2.0;
    let half_width = width / 2.0;
    
    // Define the 8 vertices of the octagon (going counter-clockwise from top-right)
    // Y is up in 3D space, but for a floor plane we want XZ coordinates
    let vertices = vec![
        // Starting from north-east, going counter-clockwise
        [half_length - corner_cut, 0.0, half_width],           // 0: NE corner (right side of north edge)
        [-half_length + corner_cut, 0.0, half_width],          // 1: NW corner (left side of north edge)
        [-half_length, 0.0, half_width - corner_cut],          // 2: NW corner (top of west edge)
        [-half_length, 0.0, -half_width + corner_cut],         // 3: SW corner (bottom of west edge)
        [-half_length + corner_cut, 0.0, -half_width],         // 4: SW corner (left side of south edge)
        [half_length - corner_cut, 0.0, -half_width],          // 5: SE corner (right side of south edge)
        [half_length, 0.0, -half_width + corner_cut],          // 6: SE corner (bottom of east edge)
        [half_length, 0.0, half_width - corner_cut],           // 7: NE corner (top of east edge)
    ];
    
    // Create triangles by fanning from center point
    // Add center vertex
    let center = [0.0, 0.0, 0.0];
    let mut all_vertices = vertices.clone();
    all_vertices.push(center); // Index 8 is the center
    
    // Create triangle indices (center to each edge, going counter-clockwise)
    let indices = vec![
        8, 0, 1,  // North edge
        8, 1, 2,  // NW corner
        8, 2, 3,  // West edge
        8, 3, 4,  // SW corner
        8, 4, 5,  // South edge
        8, 5, 6,  // SE corner
        8, 6, 7,  // East edge
        8, 7, 0,  // NE corner
    ];
    
    // Create normals (all pointing up for a flat floor)
    let normals = vec![[0.0, 1.0, 0.0]; all_vertices.len()];
    
    // Create UVs by world-space tiling so texels stay square and uniform
    // regardless of the floor's aspect ratio (Repeat sampler handles wrap).
    let uvs: Vec<[f32; 2]> = all_vertices.iter().map(|v| {
        [v[0] * uv_scale, v[2] * uv_scale]
    }).collect();
    
    Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::render::render_asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, all_vertices)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}

// ============================================================================
// Setup & Cleanup Systems
// ============================================================================

/// Setup system: Spawns the 3D arena, camera, lighting, and combatants.
/// 
/// This runs once when entering the PlayMatch state.
/// Reads the `MatchConfig` resource to determine team compositions.
pub fn setup_play_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut combat_log: ResMut<CombatLog>,
    config: Res<MatchConfig>,
    game_settings: Res<crate::settings::GameSettings>,
    item_defs: Res<ItemDefinitions>,
    default_loadouts: Res<DefaultLoadouts>,
) {
    info!("Setting up Play Match scene with config: {:?}", *config);

    // Clear combat log for new match
    combat_log.clear();
    combat_log.log(CombatLogEventType::MatchEvent, "Match started!".to_string());

    // Initialize combat panel view (for tabbed Combat Log / Timeline UI)
    commands.insert_resource(CombatPanelView::default());

    // Initialize spell icons resources (for ability timeline)
    commands.insert_resource(SpellIcons::default());
    commands.insert_resource(SpellIconHandles::default());

    // Spawn 3D camera with isometric-ish view.
    // HDR + tonemapping + bloom let the pre-scaled emissive effects (shields,
    // heal columns, traps, drain beams — all authored at 2-4x) actually glow
    // instead of clipping to flat white.
    commands.spawn((
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Transform::from_xyz(0.0, 40.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        ArenaCamera,
        PlayMatchEntity,
    ));

    // Add directional light (sun-like) - warm golden sunlight.
    // Shadows grounded to the ~76-unit arena via a 2-cascade config so units
    // cast contact shadows that anchor them to the floor.
    commands.spawn((
        DirectionalLight {
            illuminance: 25000.0,
            color: Color::srgb(1.0, 0.95, 0.85), // Warm golden sunlight
            shadows_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            maximum_distance: 120.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        PlayMatchEntity,
    ));

    // Add ambient light for overall scene brightness - warm atmospheric glow.
    // Kept low so the directional light + shadows carry the contrast and the
    // emissive effects pop under bloom (was 400.0, which flattened everything).
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7), // Warm peachy ambient light
        brightness: 250.0,
        affects_lightmapped_meshes: true,
    });

    // Deep cool background so the warm sandy arena reads against a cohesive
    // backdrop instead of Bevy's default flat gray.
    commands.insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.09)));
    
    // Initialize simulation speed control
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    
    // Initialize camera controller
    commands.insert_resource(CameraController::default());
    
    // Initialize match countdown (10 seconds before gates open)
    commands.insert_resource(MatchCountdown::default());

    // Initialize Shadow Sight state (for stealth stalemate breaking)
    commands.insert_resource(ShadowSightState::default());

    // Initialize random number generator (non-deterministic for graphical mode)
    commands.insert_resource(GameRng::default());

    // Initialize display settings from game settings
    commands.insert_resource(DisplaySettings {
        show_aura_icons: game_settings.show_aura_icons,
    });

    // Spawn arena floor - octagonal shape matching the wall boundary
    // Warm sandy/dirt battleground
    let arena_length = 76.0;
    let arena_width = 46.0;
    let corner_cut = 10.0;
    
    // Create custom octagonal mesh. UV scale tiles the procedural dirt texture
    // ~every 12 world units (square texels), giving the floor grain/variation
    // without an external asset.
    let octagon_mesh = create_octagon_mesh(arena_length, arena_width, corner_cut, 1.0 / 12.0);
    // Sandy dirt: blotches + grain, no courses.
    let floor_texture = images.add(create_surface_texture([0.79, 0.66, 0.46], 0.12, 0.06, 0));

    commands.spawn((
        Mesh3d(meshes.add(octagon_mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            // Sandy tone is baked into the texture, so the tint stays white to
            // avoid double-darkening the baked color.
            base_color: Color::WHITE,
            base_color_texture: Some(floor_texture),
            perceptual_roughness: 0.95, // Matte dirt/sand texture
            cull_mode: None, // Render both sides
            ..default()
        })),
        PlayMatchEntity,
    ));

    // Spawn rectangular arena walls with chamfered corners (simplified stadium shape)
    let wall_height = 4.0;
    let wall_thickness = 1.0;
    
    // Procedural weathered-stone texture for the walls (#8b7355 baked in),
    // with faint horizontal courses so it reads as stacked masonry.
    let wall_texture = images.add(create_surface_texture([0.54, 0.45, 0.33], 0.10, 0.05, 6));

    // Arena dimensions: elongated octagon
    let corner_cut = 10.0; // How much to cut off each corner
    let half_length = arena_length / 2.0; // 38.0
    let half_width = arena_width / 2.0;   // 23.0

    // Calculate wall dimensions
    let long_wall_length = arena_length - corner_cut * 2.0; // North/South walls
    let short_wall_length = arena_width - corner_cut * 2.0; // East/West walls
    let corner_wall_length = corner_cut * 1.414; // Diagonal length (45° angle)

    // One stone material per wall size. uv_transform scales the shared texture
    // to each wall's length × height so texels stay square (~6 world units per
    // tile) instead of smearing across the long faces. The Repeat sampler wraps
    // the resulting >1 UVs.
    let wall_tile = 6.0;
    let stone_material = |length: f32, materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color: Color::WHITE, // tone baked into the texture
            base_color_texture: Some(wall_texture.clone()),
            perceptual_roughness: 0.9,
            uv_transform: Affine2::from_scale(Vec2::new(
                length / wall_tile,
                wall_height / wall_tile,
            )),
            ..default()
        })
    };
    let long_wall_material = stone_material(long_wall_length, &mut materials);
    let short_wall_material = stone_material(short_wall_length, &mut materials);
    let corner_wall_material = stone_material(corner_wall_length, &mut materials);
    
    // North wall (positive Z) - main long side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(long_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(long_wall_material.clone()),
        Transform::from_xyz(0.0, wall_height / 2.0, half_width),
        PlayMatchEntity,
    ));

    // South wall (negative Z) - main long side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(long_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(long_wall_material.clone()),
        Transform::from_xyz(0.0, wall_height / 2.0, -half_width),
        PlayMatchEntity,
    ));

    // East wall (positive X) - short side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_thickness, wall_height, short_wall_length))),
        MeshMaterial3d(short_wall_material.clone()),
        Transform::from_xyz(half_length, wall_height / 2.0, 0.0),
        PlayMatchEntity,
    ));

    // West wall (negative X) - short side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_thickness, wall_height, short_wall_length))),
        MeshMaterial3d(short_wall_material.clone()),
        Transform::from_xyz(-half_length, wall_height / 2.0, 0.0),
        PlayMatchEntity,
    ));
    
    // Add angled corner pieces to connect the walls (45-degree angles)
    // Each corner wall connects the end of one straight wall to the end of another
    let corner_offset_x = half_length - corner_cut / 2.0;
    let corner_offset_z = half_width - corner_cut / 2.0;
    
    // Northeast corner (connects North wall to East wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(corner_wall_material.clone()),
        Transform::from_xyz(corner_offset_x, wall_height / 2.0, corner_offset_z)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Southeast corner (connects South wall to East wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(corner_wall_material.clone()),
        Transform::from_xyz(corner_offset_x, wall_height / 2.0, -corner_offset_z)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Northwest corner (connects North wall to West wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(corner_wall_material.clone()),
        Transform::from_xyz(-corner_offset_x, wall_height / 2.0, corner_offset_z)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Southwest corner (connects South wall to West wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(corner_wall_material.clone()),
        Transform::from_xyz(-corner_offset_x, wall_height / 2.0, -corner_offset_z)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));

    // Count class occurrences per team to apply darkening to duplicates
    use std::collections::HashMap;
    let mut team1_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();
    let mut team2_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();

    // Spawn Team 1 combatants (left side of arena, in starting pen)
    // Teams start further back (-35/+35) and will move forward when gates open
    let team1_spawn_x = -35.0;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team1_class_counts.get(character).unwrap_or(&0);
            *team1_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(1, *character));

            // Get rogue opener preference for this slot
            let rogue_opener = config.team1_rogue_openers.get(i).copied().unwrap_or_default();

            // Get warlock curse preferences for this slot (empty vec if none configured)
            let warlock_curse_prefs = config.team1_warlock_curse_prefs.get(i).cloned().unwrap_or_default();

            // Get class-specific strategic option preferences
            let warrior_shout = config.team1_warrior_shouts.get(i).copied().unwrap_or_default();
            let mage_armor = config.team1_mage_armors.get(i).copied().unwrap_or_default();
            let paladin_aura = config.team1_paladin_auras.get(i).copied().unwrap_or_default();

            // Resolve equipment loadout (defaults + overrides), enforcing 2H constraints
            let equipment_overrides = config.team1_equipment.get(i).cloned().unwrap_or_default();
            let mut loadout = resolve_loadout(*character, &default_loadouts, &equipment_overrides);
            enforce_two_hand_conflicts(&mut loadout, &item_defs);

            let position = Vec3::new(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0);
            let (entity, combatant) = spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                1,
                i as u8, // slot index
                *character,
                position,
                count,
                rogue_opener,
                warlock_curse_prefs,
                warrior_shout,
                mage_armor,
                paladin_aura,
                &loadout,
                &item_defs,
            );

            // Log equipment loadout
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!("[EQUIPMENT] {}: {}", combatant_id(1, *character), format_loadout(&loadout, &item_defs)),
            );

            // Spawn Felhunter pet for Warlocks
            if *character == match_config::CharacterClass::Warlock {
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    PetType::Felhunter,
                );
            }

            // Spawn pet for Hunters (based on configured pet type)
            if *character == match_config::CharacterClass::Hunter {
                let pet_type_pref = config.team1_hunter_pet_types.get(i).copied().unwrap_or_default();
                let pet_type = match pet_type_pref {
                    match_config::HunterPetType::Spider => PetType::Spider,
                    match_config::HunterPetType::Boar => PetType::Boar,
                    match_config::HunterPetType::Bird => PetType::Bird,
                };
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    pet_type,
                );
            }
        } else {
            warn!("Team 1 slot {} is empty — skipping spawn", i);
        }
    }

    // Spawn Team 2 combatants (right side of arena, in starting pen)
    let team2_spawn_x = 35.0;
    for (i, character_opt) in config.team2.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team2_class_counts.get(character).unwrap_or(&0);
            *team2_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(2, *character));

            // Get rogue opener preference for this slot
            let rogue_opener = config.team2_rogue_openers.get(i).copied().unwrap_or_default();

            // Get warlock curse preferences for this slot (empty vec if none configured)
            let warlock_curse_prefs = config.team2_warlock_curse_prefs.get(i).cloned().unwrap_or_default();

            // Get class-specific strategic option preferences
            let warrior_shout = config.team2_warrior_shouts.get(i).copied().unwrap_or_default();
            let mage_armor = config.team2_mage_armors.get(i).copied().unwrap_or_default();
            let paladin_aura = config.team2_paladin_auras.get(i).copied().unwrap_or_default();

            // Resolve equipment loadout (defaults + overrides), enforcing 2H constraints
            let equipment_overrides = config.team2_equipment.get(i).cloned().unwrap_or_default();
            let mut loadout = resolve_loadout(*character, &default_loadouts, &equipment_overrides);
            enforce_two_hand_conflicts(&mut loadout, &item_defs);

            let position = Vec3::new(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0);
            let (entity, combatant) = spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                2,
                i as u8, // slot index
                *character,
                position,
                count,
                rogue_opener,
                warlock_curse_prefs,
                warrior_shout,
                mage_armor,
                paladin_aura,
                &loadout,
                &item_defs,
            );

            // Log equipment loadout
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!("[EQUIPMENT] {}: {}", combatant_id(2, *character), format_loadout(&loadout, &item_defs)),
            );

            // Spawn Felhunter pet for Warlocks
            if *character == match_config::CharacterClass::Warlock {
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    PetType::Felhunter,
                );
            }

            // Spawn pet for Hunters (based on configured pet type)
            if *character == match_config::CharacterClass::Hunter {
                let pet_type_pref = config.team2_hunter_pet_types.get(i).copied().unwrap_or_default();
                let pet_type = match pet_type_pref {
                    match_config::HunterPetType::Spider => PetType::Spider,
                    match_config::HunterPetType::Boar => PetType::Boar,
                    match_config::HunterPetType::Bird => PetType::Bird,
                };
                spawn_pet(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut combat_log,
                    entity,
                    &combatant,
                    position,
                    pet_type,
                );
            }
        } else {
            warn!("Team 2 slot {} is empty — skipping spawn", i);
        }
    }
    
    // Spawn starting gate bars for both teams
    spawn_gate_bars(&mut commands, &mut meshes, &mut materials, team1_spawn_x, team2_spawn_x);
}

/// Spawn visual gate bars that lower when countdown ends
fn spawn_gate_bars(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team1_x: f32,
    team2_x: f32,
) {
    let gate_height = 6.0;
    let bar_width = 0.5;
    let bar_depth = 0.5;
    let num_bars = 7; // Number of vertical bars per gate
    let spacing = 2.5; // Space between bars
    
    // Dark metal material for the bars
    let bar_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2), // Dark gray/metal
        metallic: 0.8,
        perceptual_roughness: 0.3,
        ..default()
    });
    
    // Team 1 gate (left side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 / 2.0)) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(team1_x + 3.0, gate_height / 2.0, z_offset),
            GateBar {
                team: 1,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
    
    // Team 2 gate (right side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 / 2.0)) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(team2_x - 3.0, gate_height / 2.0, z_offset),
            GateBar {
                team: 2,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
}

/// Deterministic per-entity walk-animation phase offset derived from the
/// spawn XZ position. Two units at the same Z separated in X get different
/// phases, so a 3v3 team that starts walking in lockstep does not bob in unison.
fn walk_phase_seed(xz: Vec2) -> f32 {
    (xz.x * 7.314 + xz.y * 11.927).rem_euclid(std::f32::consts::TAU)
}

/// Helper function to spawn a single combatant entity.
///
/// Creates a capsule mesh colored by class, with darker shades for duplicates.
/// The `duplicate_index` parameter determines how much to darken (0 = base color, 1+ = darkened).
fn spawn_combatant(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team: u8,
    slot: u8,
    class: match_config::CharacterClass,
    position: Vec3,
    duplicate_index: usize,
    rogue_opener: match_config::RogueOpener,
    warlock_curse_prefs: Vec<match_config::WarlockCurse>,
    warrior_shout: match_config::WarriorShout,
    mage_armor: match_config::MageArmor,
    paladin_aura: match_config::PaladinAura,
    equipment_loadout: &std::collections::HashMap<ItemSlot, ItemId>,
    item_defs: &ItemDefinitions,
) -> (Entity, Combatant) {
    // Get vibrant class colors for 3D visibility
    let base_color = match class {
        match_config::CharacterClass::Warrior => Color::srgb(0.9, 0.6, 0.3), // Orange/brown
        match_config::CharacterClass::Mage => Color::srgb(0.3, 0.6, 1.0),    // Bright blue
        match_config::CharacterClass::Rogue => Color::srgb(1.0, 0.9, 0.2),   // Bright yellow
        match_config::CharacterClass::Priest => Color::srgb(0.95, 0.95, 0.95), // White
        match_config::CharacterClass::Warlock => Color::srgb(0.58, 0.41, 0.93), // Purple
        match_config::CharacterClass::Paladin => Color::srgb(0.96, 0.55, 0.73), // Pink (WoW Paladin)
        match_config::CharacterClass::Hunter => Color::srgb(0.67, 0.83, 0.45), // Green (WoW Hunter)
    };
    
    // Apply darkening for duplicate classes (0.65 multiplier per duplicate)
    let darken_factor = 0.65f32.powi(duplicate_index as i32);
    let combatant_color = Color::srgb(
        base_color.to_srgba().red * darken_factor,
        base_color.to_srgba().green * darken_factor,
        base_color.to_srgba().blue * darken_factor,
    );

    // Create combatant mesh (capsule represents the body)
    let mesh_handle = meshes.add(Capsule3d::new(0.5, 1.5));
    let material = materials.add(StandardMaterial {
        base_color: combatant_color,
        perceptual_roughness: 0.5, // More reflective for better color visibility
        metallic: 0.2, // Slight metallic sheen for color pop
        // Enable alpha mode for stealth transparency
        alpha_mode: bevy::prelude::AlphaMode::Blend,
        ..default()
    });

    let mut combatant = Combatant::new_with_curse_prefs(team, slot, class, rogue_opener, warlock_curse_prefs);
    combatant.warrior_shout = warrior_shout;
    combatant.mage_armor = mage_armor;
    combatant.paladin_aura = paladin_aura;
    combatant.apply_equipment(equipment_loadout, item_defs);
    let combatant_clone = combatant.clone();

    let entity = commands.spawn((
        Mesh3d(mesh_handle.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(position),
        combatant,
        DRTracker::default(),
        FloatingTextState {
            next_pattern_index: 0,
        },
        OriginalMesh(mesh_handle),
        PlayMatchEntity,
        WalkAnim {
            ground_y: position.y,
            phase: walk_phase_seed(position.xz()),
            previous_xz: position.xz(),
        },
    )).id();

    (entity, combatant_clone)
}

/// Helper function to spawn a pet entity for a Warlock combatant.
fn spawn_pet(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    combat_log: &mut CombatLog,
    owner_entity: Entity,
    owner_combatant: &Combatant,
    owner_position: Vec3,
    pet_type: PetType,
) {
    let pet_slot = PET_SLOT_BASE + owner_combatant.slot;
    let pet_combatant = Combatant::new_pet(owner_combatant.team, pet_slot, pet_type, owner_combatant);
    let pet_position = owner_position + Vec3::new(-2.0, 0.3, 1.5);

    let pet_color = pet_type.color();
    // Stocky capsule for quadruped (tilted horizontal by apply_pet_mesh_tilt system)
    let mesh_handle = meshes.add(Capsule3d::new(0.35, 0.6));
    let material = materials.add(StandardMaterial {
        base_color: pet_color,
        perceptual_roughness: 0.5,
        metallic: 0.2,
        alpha_mode: bevy::prelude::AlphaMode::Blend,
        ..default()
    });

    // Face toward arena center so tilt system has a valid initial facing
    let initial_facing = if owner_combatant.team == 1 {
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) // Face right (+X)
    } else {
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) // Face left (-X)
    };

    commands.spawn((
        Mesh3d(mesh_handle.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(pet_position).with_rotation(initial_facing),
        pet_combatant,
        DRTracker::default(),
        Pet {
            owner: owner_entity,
            pet_type,
        },
        FloatingTextState {
            next_pattern_index: 0,
        },
        OriginalMesh(mesh_handle),
        PlayMatchEntity,
        WalkAnim {
            ground_y: pet_position.y,
            phase: walk_phase_seed(pet_position.xz()),
            previous_xz: pet_position.xz(),
        },
    ));

    // Register pet with combat log
    combat_log.register_combatant(format!("Team {} {}", owner_combatant.team, pet_type.name()));
}

/// Handle camera input for mode switching, zoom, rotation, and drag

/// Cleanup system: Despawns all Play Match entities when exiting the state.
pub fn cleanup_play_match(
    mut commands: Commands,
    query: Query<Entity, With<PlayMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
    
    // Remove resources
    commands.remove_resource::<AmbientLight>();
    commands.remove_resource::<SimulationSpeed>();
    commands.remove_resource::<MatchCountdown>();
    commands.remove_resource::<ShadowSightState>();
    commands.remove_resource::<DisplaySettings>();
    // Remove optional resources (may not exist if match didn't finish)
    commands.remove_resource::<VictoryCelebration>();
}

// ============================================================================
// Update & Input Systems
// ============================================================================

/// Countdown system: Manage pre-combat countdown and gate opening.
/// 
/// During countdown (10 seconds):
/// - Mana is restored to 100% every second (encourages pre-buffing)
/// - Combatants can cast buffs but cannot move or attack
/// - Countdown timer ticks down
/// 
/// When countdown reaches 0:
/// - Gates open (sets gates_opened flag)
/// - Combat begins normally

/// Render time control UI panel in the top-right corner.
/// 
/// Shows current speed and clickable buttons for speed control.
/// Handle player input during the match.
/// Currently only handles ESC key to return to main menu.
pub fn update_play_match(
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    use crate::keybindings::GameAction;
    
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        next_state.set(GameState::MainMenu);
    }
}

// ============================================================================
// Combat Systems (see submodules: combat_ai, combat_core, auras, projectiles)
// ============================================================================

