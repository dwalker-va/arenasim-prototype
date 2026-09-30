use crate::states::play_match::components::*;
use bevy::color::LinearRgba;
use bevy::prelude::*;

// ==============================================================================
// Flare Visual (placeholder: lit ground disc + a hanging flare)
// ==============================================================================
//
// A placeholder, not client-derived: the disc is the gameplay radius exactly,
// so a Rogue that walks past its edge unrevealed reads as a missed guess
// rather than a bug. The bespoke look (Classic's flare missile and its
// lingering light) is its own card, researched from the client data first.

/// Warm amber light of a Flare.
const FLARE_RGB: (f32, f32, f32) = (1.0, 0.75, 0.35);
/// Height the flare itself hangs above its lit area.
const FLARE_MOTE_HEIGHT: f32 = 3.5;
/// Seconds over which a burning-out flare fades.
const FLARE_FADE_SECS: f32 = 2.0;

/// The burning flare hanging over a [`FlareZone`]'s lit area. A separate
/// entity rather than a child so the zone's plain `despawn` takes nothing with
/// it that could be orphaned; it follows its zone and goes when the zone does.
#[derive(Component)]
pub struct FlareMote {
    pub flare: Entity,
}

/// Fade factor for a flare with `remaining` seconds left: 1 until the last
/// [`FLARE_FADE_SECS`], then linearly to 0.
fn flare_fade(remaining: f32) -> f32 {
    (remaining / FLARE_FADE_SECS).clamp(0.0, 1.0)
}

/// Light a newly fired Flare: the lit disc on the zone itself, and the flare
/// hanging over it.
pub fn spawn_flare_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_flares: Query<(Entity, &FlareZone, &Transform), (Added<FlareZone>, Without<Mesh3d>)>,
) {
    let (r, g, b) = FLARE_RGB;
    for (flare_entity, flare, transform) in new_flares.iter() {
        let disc = materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, 0.12),
            emissive: LinearRgba::new(0.8, 0.55, 0.2, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        commands.entity(flare_entity).try_insert((
            Mesh3d(meshes.add(Cylinder::new(flare.radius, 0.03))),
            MeshMaterial3d(disc),
        ));

        let mote = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.95, 0.8, 1.0),
            emissive: LinearRgba::new(6.0, 4.0, 1.5, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(0.35))),
            MeshMaterial3d(mote),
            Transform::from_translation(transform.translation + Vec3::Y * FLARE_MOTE_HEIGHT),
            FlareMote {
                flare: flare_entity,
            },
            PlayMatchEntity,
        ));
    }
}

/// Flicker the hanging flare, keep the disc steady, and fade both over the
/// flare's last seconds. A mote whose flare has burnt out goes with it.
pub fn update_flare_visuals(
    mut commands: Commands,
    time: Res<Time>,
    flares: Query<(&FlareZone, &MeshMaterial3d<StandardMaterial>)>,
    motes: Query<(Entity, &FlareMote, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    let (r, g, b) = FLARE_RGB;
    for (flare, material_handle) in flares.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color =
                Color::srgba(r, g, b, 0.12 * flare_fade(flare.duration_remaining));
        }
    }
    for (mote_entity, mote, material_handle) in motes.iter() {
        let Ok((flare, _)) = flares.get(mote.flare) else {
            commands.entity(mote_entity).despawn();
            continue;
        };
        let Some(material) = materials.get_mut(&material_handle.0) else {
            continue;
        };
        // Two incommensurate sines: a sputter, not a pulse.
        let flicker = 0.85 + 0.1 * (t * 13.0).sin() + 0.05 * (t * 31.0).sin();
        let k = flicker * flare_fade(flare.duration_remaining);
        material.emissive = LinearRgba::new(6.0 * k, 4.0 * k, 1.5 * k, 1.0);
    }
}
