//! The Spider's Web in flight (graphical-only).
//!
//! `web_missile.m2` is a flat disc 1.2 yd across and 0.15 thick, one additive
//! material over `spiderwebs01.blp` — a spinning web, not the thin cuboid the
//! generic projectile drew. It has no landing: the only kit on arrival is
//! `web_state.m2`, the root STATE that `hard_cc.rs` already draws. See
//! docs/design/2026-09-26-interrupts-and-one-offs-client-data.md.
//!
//! The disc is a child of the sim's projectile, so it flies at the sim's own
//! projectile speed and dies with it. The projectile's local +Z is its flight
//! direction (`move_projectiles`), and the disc faces along it, spinning about
//! it. Nothing here writes sim state or draws `game_rng`; registered in
//! `states/mod.rs` only.

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::f32::consts::TAU;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;

/// The disc's diameter, yards (the client's 1.2).
pub const WEB_DISC_DIAMETER: f32 = 1.2;
/// Spin about the flight axis, revolutions per second (bench sign-off).
pub const WEB_DISC_SPIN_REV_PER_SEC: f32 = 1.5;

/// Generated web sprite, pixels on a side.
const WEB_PX: u32 = 128;
/// Fallback colour when the RON gives the web no `projectile_visuals`.
const WEB_FALLBACK_COLOR: [f32; 3] = [0.95, 0.95, 0.9];
const WEB_FALLBACK_EMISSIVE: [f32; 3] = [1.5, 1.5, 1.4];

/// Whether an ability's projectile is the web disc.
pub fn is_web_missile(ability: AbilityType) -> bool {
    ability == AbilityType::SpiderWeb
}

/// The spinning disc riding a Web projectile.
#[derive(Component)]
pub struct WebDisc {
    pub age: f32,
}

/// A web, white, with the whole shape in the alpha channel: eight spokes and
/// three octagonal rings, as `spiderwebs01` reads at this size.
fn web_texture() -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = WEB_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    let sector = TAU / 8.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - centre) / centre;
            let dy = (y as f32 - centre) / centre;
            let r = (dx * dx + dy * dy).sqrt();
            let a = dy.atan2(dx);
            // Distance to the nearest spoke, across it.
            let off = (a / sector).round() * sector - a;
            let spoke = if r <= 0.98 {
                (1.0 - (off * r).abs() / 0.03).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // An octagon's radius at this angle: the rings are straight
            // strands between spokes, not circles.
            let half = sector * 0.5;
            let octagon = half.cos() / ((a.rem_euclid(sector)) - half).cos();
            let strand = [0.35_f32, 0.62, 0.9]
                .iter()
                .map(|f| (1.0 - (r - f * octagon).abs() / 0.025).clamp(0.0, 1.0))
                .fold(0.0_f32, f32::max);
            let alpha = spoke.max(strand);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Mesh and material every web shares, built once.
pub struct WebAssets {
    quad: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// FixedUpdate (graphical-only): hang a spinning web disc on each new Web
/// projectile. Same slot as `spawn_projectile_visuals` (which skips the web),
/// so a web that spawns and lands inside one rendered frame is still drawn.
pub fn spawn_web_missile_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<WebAssets>>,
    abilities: Res<AbilityDefinitions>,
    new_projectiles: Query<(Entity, &Projectile), Added<Projectile>>,
) {
    for (entity, projectile) in new_projectiles.iter() {
        if !is_web_missile(projectile.ability) {
            continue;
        }
        let assets = assets.get_or_insert_with(|| {
            let (color, emissive) = abilities
                .get(&AbilityType::SpiderWeb)
                .and_then(|def| def.projectile_visuals.as_ref())
                .map(|v| (v.color, v.emissive))
                .unwrap_or((WEB_FALLBACK_COLOR, WEB_FALLBACK_EMISSIVE));
            let texture = images.add(web_texture());
            WebAssets {
                quad: meshes.add(Rectangle::new(1.0, 1.0)),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(color[0], color[1], color[2]),
                    base_color_texture: Some(texture.clone()),
                    emissive: LinearRgba::rgb(emissive[0], emissive[1], emissive[2]),
                    emissive_texture: Some(texture),
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                }),
            }
        });
        let disc = commands
            .spawn((
                WebDisc { age: 0.0 },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_scale(Vec3::splat(WEB_DISC_DIAMETER)),
                NotShadowCaster,
            ))
            .id();
        commands
            .entity(entity)
            .insert(Visibility::default())
            .add_child(disc);
    }
}

/// The disc's spin about the flight axis after `age` seconds.
pub fn web_disc_rotation(age: f32) -> Quat {
    Quat::from_rotation_z(age * WEB_DISC_SPIN_REV_PER_SEC * TAU)
}

/// Update (graphical-only): spin each web about its flight axis.
pub fn spin_web_discs(time: Res<Time>, mut discs: Query<(&mut WebDisc, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut disc, mut transform) in discs.iter_mut() {
        disc.age += dt;
        transform.rotation = web_disc_rotation(disc.age);
    }
}
