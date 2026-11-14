use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::hunter_shots::{draw, ramp3, ramp_color, ClientEmitter, EmitterSprite, PALETTE_STEPS};
use super::school_impact::impact_origin;
use crate::states::play_match::components::*;

// ==============================================================================
// The bleed spray — a short spray of blood from the victim's chest
// ==============================================================================
//
// From the client data (`docs/design/2026-10-17-rogue-talent-client-data.md`
// §1): Rupture's impact kit 220 is `bloodyexplosion.m2` at the victim's chest —
// twelve blood-spurt emitters (`bloodspurtsmall01.blp`, ALPHA-blended: the
// texture carries the red) fanned round the chest, each bursting at 150/s for
// 167 ms, falling to 50/s at 333 ms and off at 500 ms, its droplets flying at
// 2.2 yd/s under gravity 6.9 for 0.5 s. Hemorrhage's impact is the same model
// (AS-367 reuses this), and so is Rend's — which ships its drips and not this.
//
// As the signed-off AS-336 bench plays it (version 1792302646-d16f,
// `bloodSpray`, committed as `docs/design/benches/2026-10-17-rogue-talent-
// bench.html`): ONE emitter standing for the twelve, launching into a cone
// 1.25 rad either side of straight up at 2.2 yd/s ±25%, its rate track the
// client's per-emitter rates times six, deep-red opaque droplets (a solid disc,
// the drips' material rather than an additive glow) darkening as they fall.
// Its tail streak is a 0.006 yd line under the droplet, and is not drawn.
//
// **Fired at the stroke's contact, and only on a landing.** The router holds
// the spray for the backhand's `impact_at` (Mortal Strike's flourish waits for
// its blade the same way), then fires it only if the victim is alive and wears
// the caster's aura — a dodged or parried strike, an immune victim, a corpse,
// or a bleed already gone draws no blood.
//
// Graphical-only: scattered by the shared deterministic hash (never
// `game_rng`), writing no sim state, registered in `states/mod.rs` only —
// headless stays byte-identical.

// ── The bench's numbers ──────────────────────────────────────────────────────

/// The spray, as one client emitter: its rate track and its droplets' life,
/// speed, gravity, colour, alpha and size. Played at the victim's chest.
pub const BLEED_SPRAY: ClientEmitter = ClientEmitter {
    name: "bloodyexplosion/spurts",
    // Unused here (the spray draws its own droplet sprite); the nearest kind.
    sprite: EmitterSprite::Glow,
    additive: false,
    rate: 0.0,
    window: ClientEmitter::tracked(&BLEED_SPRAY_RATE),
    life: 0.5,
    speed: 2.2,
    gravity: 6.9,
    area: 0.0,
    column: None,
    rise: false,
    // The bench keys (150,8,8) at birth and (110,4,4) at death; the middle key
    // is that ramp at the size and alpha ramps' 0.25.
    color: [[150, 8, 8], [140, 7, 7], [110, 4, 4]],
    alpha: [1.0, 0.69, 0.0],
    // The bench keys a droplet's RADIUS (its `sprite` draws `r = scale`), and
    // its blob fills `BLEED_DROPLET_FILL` of that.
    size: ParticleSize::radii([0.03, 0.11, 0.02]),
    mid: 0.25,
    track: Some(&BLEED_SPRAY_RATE),
};

/// The spray's rate, `(seconds, droplets per second)`: the client's per-spurt
/// 150/s held 167 ms, 50/s at 333 ms and off at 500 ms, times six (the bench's
/// one emitter for the twelve) — about 275 droplets in all.
pub const BLEED_SPRAY_RATE: [(f32, f32); 4] =
    [(0.0, 900.0), (0.167, 900.0), (0.333, 300.0), (0.5, 0.0)];
/// The cone the droplets launch into: up to this many radians off straight up,
/// in any direction round it.
pub const BLEED_SPRAY_CONE: f32 = 1.25;
/// Launch speed varies by up to this fraction either side of `speed`.
pub const BLEED_SPRAY_SPEED_VARIANCE: f32 = 0.25;
/// The droplet is a solid disc filling this fraction of its sprite's radius
/// (the bench's blob draws `r * 0.7`).
pub const BLEED_DROPLET_FILL: f32 = 0.7;

/// The droplet sprite, pixels on a side.
const DROPLET_PX: u32 = 32;
/// Keeps float drift from dropping the last droplet of an exact count.
const EMIT_EPSILON: f32 = 1e-4;

// ── Components ───────────────────────────────────────────────────────────────

/// A spray waiting for its stroke's contact. Fires after `delay` seconds if
/// `victim` is alive and wears `aura` from `caster`.
#[derive(Component)]
pub struct BleedSprayPending {
    pub victim: Entity,
    pub caster: Entity,
    /// The RON `name:` of the aura whose landing the spray shows.
    pub aura: String,
    pub delay: f32,
}

/// One spray emitting at the chest it was fired at. Its droplets are
/// world-space: they fly on after the victim moves, and outlive the rig.
#[derive(Component)]
pub struct BleedSpray {
    pub age: f32,
    /// Droplets owed but not yet emitted.
    pub carry: f32,
    pub emitted: u32,
}

/// One droplet of a [`BleedSpray`], turned to the camera each frame.
#[derive(Component)]
pub struct BleedDroplet;

// ── The pure model ───────────────────────────────────────────────────────────

/// Whether a strike that put `aura` on its victim landed: the victim is
/// alive and wears the caster's `aura` now.
pub fn bleed_landed(
    victim_alive: bool,
    victim_auras: Option<&ActiveAuras>,
    caster: Entity,
    aura: &str,
) -> bool {
    victim_alive
        && victim_auras.is_some_and(|a| {
            a.auras
                .iter()
                .any(|x| x.caster == Some(caster) && x.ability_name == aura)
        })
}

/// Droplet `seed`'s launch velocity: into the cone round straight up, at the
/// spray's speed varied by [`BLEED_SPRAY_SPEED_VARIANCE`].
pub fn bleed_droplet_velocity(seed: u32) -> Vec3 {
    let polar = (2.0 * draw(seed, 1) - 1.0) * BLEED_SPRAY_CONE;
    let around = draw(seed, 2) * TAU;
    let dir = Vec3::new(
        polar.sin() * around.cos(),
        polar.cos(),
        polar.sin() * around.sin(),
    );
    let speed =
        BLEED_SPRAY.speed * (1.0 + BLEED_SPRAY_SPEED_VARIANCE * (2.0 * draw(seed, 3) - 1.0));
    dir * speed
}

/// A white solid disc filling [`BLEED_DROPLET_FILL`] of the sprite, its rim
/// one texel soft so it does not shimmer.
pub fn bleed_droplet_texture() -> Image {
    use bevy::render::render_asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let size = DROPLET_PX;
    let mut data = vec![0u8; (size * size * 4) as usize];
    let centre = (size as f32 - 1.0) / 2.0;
    let edge = BLEED_DROPLET_FILL * (size as f32 / 2.0);
    for y in 0..size {
        for x in 0..size {
            let r = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            let a = (edge - r + 0.5).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0).round() as u8;
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

/// The material a droplet `t` of the way through its life wears: the colour
/// and alpha ramps sampled there, alpha-BLENDED and unlit, so the spray can
/// darken the sand as the client's blend-2 spurts do.
pub fn bleed_droplet_material(t: f32, texture: Handle<Image>) -> StandardMaterial {
    let color = ramp_color(BLEED_SPRAY.color, BLEED_SPRAY.mid, t);
    let alpha = ramp3(BLEED_SPRAY.alpha, BLEED_SPRAY.mid, t);
    StandardMaterial {
        base_color: color.with_alpha(alpha),
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// The quad and the palette every droplet shares, built once.
pub struct BleedSprayAssets {
    quad: Handle<Mesh>,
    palette: Arc<[Handle<StandardMaterial>]>,
}

impl BleedSprayAssets {
    pub fn build(
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let texture = images.add(bleed_droplet_texture());
        let palette: Arc<[Handle<StandardMaterial>]> = (0..PALETTE_STEPS)
            .map(|i| {
                let t = (i as f32 + 0.5) / PALETTE_STEPS as f32;
                materials.add(bleed_droplet_material(t, texture.clone()))
            })
            .collect();
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            palette,
        }
    }
}

// ── Spawn ────────────────────────────────────────────────────────────────────

/// Hold a spray for `delay` seconds — the stroke's contact — then fire it on
/// `victim`'s chest if the strike landed `aura` there.
pub fn spawn_bleed_spray_pending(
    commands: &mut Commands,
    victim: Entity,
    caster: Entity,
    aura: String,
    delay: f32,
) {
    commands.spawn((
        BleedSprayPending {
            victim,
            caster,
            aura,
            delay,
        },
        PlayMatchEntity,
    ));
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): count each held spray down, and at its contact fire
/// it on the victim's chest if the strike landed — or drop it.
pub fn fire_bleed_sprays(
    mut commands: Commands,
    time: Res<Time>,
    mut pending: Query<(Entity, &mut BleedSprayPending)>,
    victims: Query<(&Combatant, &Transform, Option<&ActiveAuras>, Option<&Pet>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut held) in pending.iter_mut() {
        held.delay -= dt;
        if held.delay > 0.0 {
            continue;
        }
        commands.entity(entity).despawn();
        let Ok((victim, transform, auras, pet)) = victims.get(held.victim) else {
            continue;
        };
        if !bleed_landed(victim.is_alive(), auras, held.caster, &held.aura) {
            continue;
        }
        commands.spawn((
            BleedSpray {
                age: 0.0,
                carry: 0.0,
                emitted: 0,
            },
            Transform::from_translation(impact_origin(
                ImpactAnchor::Chest,
                transform.translation,
                pet.is_some(),
            )),
            PlayMatchEntity,
        ));
    }
}

/// Update (graphical-only): emit what each spray's rate track owes this frame
/// as world-space droplets, and retire the spray when its track ends. The
/// droplets fly, fall, shrink and darken under `animate_client_particles`, and
/// despawn themselves at the end of their life.
pub fn update_bleed_sprays(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<BleedSprayAssets>>,
    mut sprays: Query<(Entity, &mut BleedSpray, &Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut spray, transform) in sprays.iter_mut() {
        let (from, to) = (spray.age, spray.age + dt);
        spray.age = to;
        spray.carry += BLEED_SPRAY.count_between(from, to);
        let assets = assets.get_or_insert_with(|| {
            BleedSprayAssets::build(&mut meshes, &mut images, &mut materials)
        });
        while spray.carry >= 1.0 - EMIT_EPSILON {
            spray.carry -= 1.0;
            let seed = entity
                .index()
                .wrapping_add(spray.emitted.wrapping_mul(0x85EB_CA6B));
            spray.emitted = spray.emitted.wrapping_add(1);
            commands.spawn((
                ClientParticle {
                    age: 0.0,
                    life: BLEED_SPRAY.life,
                    velocity: bleed_droplet_velocity(seed),
                    gravity: BLEED_SPRAY.gravity,
                    size: BLEED_SPRAY.size,
                    mid: BLEED_SPRAY.mid,
                    palette: assets.palette.clone(),
                    step: 0,
                    facing: ParticleFacing::Camera,
                    owned: false,
                },
                BleedDroplet,
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(assets.palette[0].clone()),
                Transform::from_translation(transform.translation),
                NotShadowCaster,
            ));
        }
        if to >= BLEED_SPRAY.end() {
            commands.entity(entity).despawn();
        }
    }
}

/// Update (graphical-only): turn every droplet to face the camera.
pub fn billboard_bleed_droplets(
    camera: Query<&Transform, (With<Camera3d>, Without<BleedDroplet>)>,
    mut droplets: Query<&mut Transform, With<BleedDroplet>>,
) {
    let Some(camera) = camera.iter().next() else {
        return;
    };
    for mut droplet in droplets.iter_mut() {
        droplet.rotation = camera.rotation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spray_is_about_two_hundred_and_seventy_five_droplets_in_half_a_second() {
        let all = BLEED_SPRAY.count_between(0.0, 10.0);
        assert!((all - 274.95).abs() < 0.01, "{all}");
        assert_eq!(BLEED_SPRAY.end(), 0.5);
        // The first 167 ms is the burst: 900/s.
        assert!((BLEED_SPRAY.count_between(0.0, 0.167) - 150.3).abs() < 0.05);
        assert_eq!(BLEED_SPRAY.count_between(0.5, 1.0), 0.0);
    }

    #[test]
    fn a_droplet_launches_into_the_cone_at_the_sprays_speed() {
        for seed in 0..2000 {
            let v = bleed_droplet_velocity(seed);
            let speed = v.length();
            assert!(
                (2.2 * 0.75 - 1e-4..=2.2 * 1.25 + 1e-4).contains(&speed),
                "{seed}: {speed}"
            );
            let off_up = (v.y / speed).clamp(-1.0, 1.0).acos();
            assert!(off_up <= 1.25 + 1e-4, "{seed}: {off_up}");
        }
    }

    #[test]
    fn the_droplets_spread_round_the_whole_cone() {
        // Not a fountain on one side: the cone is filled round and out.
        let vs: Vec<Vec3> = (0..2000).map(bleed_droplet_velocity).collect();
        let quadrants = |f: fn(&Vec3) -> bool| vs.iter().filter(|v| f(v)).count();
        for (name, n) in [
            ("+x", quadrants(|v| v.x > 0.3)),
            ("-x", quadrants(|v| v.x < -0.3)),
            ("+z", quadrants(|v| v.z > 0.3)),
            ("-z", quadrants(|v| v.z < -0.3)),
        ] {
            assert!(n > 300, "{name}: {n}");
        }
        let steep = vs.iter().filter(|v| v.y / v.length() > 0.95).count();
        assert!(steep > 100, "some droplets go nearly straight up: {steep}");
        let wide = vs
            .iter()
            .filter(|v| (v.y / v.length()).acos() > 1.0)
            .count();
        assert!(wide > 200, "some go out wide: {wide}");
    }

    #[test]
    fn the_droplet_is_a_solid_disc_seven_tenths_of_its_sprite() {
        let image = bleed_droplet_texture();
        let data = image.data.as_ref().unwrap();
        let px = DROPLET_PX as usize;
        let alpha = |x: usize, y: usize| data[(y * px + x) * 4 + 3];
        // Solid at the centre and well inside the fill.
        assert_eq!(alpha(px / 2, px / 2), 255);
        assert_eq!(alpha(px / 2 + 9, px / 2), 255);
        // Clear outside it, and at the corner.
        assert_eq!(alpha(px / 2 + 13, px / 2), 0);
        assert_eq!(alpha(0, 0), 0);
    }

    #[test]
    fn the_palette_darkens_and_fades() {
        let tex = Handle::<Image>::default();
        let birth = bleed_droplet_material(0.0, tex.clone());
        let mid = bleed_droplet_material(0.25, tex.clone());
        let death = bleed_droplet_material(1.0, tex);
        let s = |m: &StandardMaterial| m.base_color.to_srgba();
        assert!((s(&birth).red - 150.0 / 255.0).abs() < 1e-5);
        assert!((s(&birth).alpha - 1.0).abs() < 1e-6);
        assert!((s(&mid).alpha - 0.69).abs() < 1e-6);
        assert!((s(&death).red - 110.0 / 255.0).abs() < 1e-5);
        assert_eq!(s(&death).alpha, 0.0);
        assert_eq!(birth.alpha_mode, AlphaMode::Blend);
        assert!(birth.unlit);
    }
}
