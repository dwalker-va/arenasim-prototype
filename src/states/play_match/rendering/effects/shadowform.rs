use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::hunter_shots::{draw, particle_rotation, ClientEmitter, EmitterAssets, EmitterSprite};
use super::immolate::feet_anchor;
use crate::states::play_match::components::*;

// ==============================================================================
// Shadowform — the violet, translucent body and the shroud at its feet
// ==============================================================================
//
// From the Classic client data, joined in
// `docs/design/2026-10-17-baseline-kit-client-data.md` §4 (AS-333): Shadowform
// (15473) lands with a sound and nothing drawn, and holds STATE kit 3129 for
// as long as the form is up — here the whole match, since a Shadowform Priest
// takes the form before the gates. The kit is three parts at once:
//
//   tint    -> procedural type 1, `#270042` (near-black violet) on the body;
//   alpha   -> procedural type 14, the body drawn at 0.65;
//   shroud  -> `shadow_precast_med_base.m2` at the Base attachment (the feet):
//              faint additive flames born large and shrinking as they rise,
//              small flares rising with them, and about once a second a long
//              smoke streak climbing 4-5 yd.
//
// Played as the signed-off bench plays them (AS-333 Baseline Kit Visuals Bench,
// https://claude.ai/artifact/3v4u2Xzm18H2NQocxkeBsp, version 1792202136-e9a0,
// "Shadowform", at its defaults). The client's tint mix is unresolved, so the
// bench's reading is the one shipped: the body colour lerped toward
// `tint + 0.12 × base` at full strength, which keeps the pill's shading
// readable. The user's ruling on AS-333 (2026-10-17) keeps the transparency:
// it is central to the form's look and sets it apart from the Warlock.
//
// **Body edits go IN PLACE, never through the shared restore slot.** The tint
// and the alpha are written onto the body's own material asset, as stealth
// edits it, and recorded in a `ShadowformLook` on the body. Fear's husk and
// Polymorph's wool swap the material HANDLE and store the displaced one in
// `OriginalBodyMaterial`; when they lift they put that handle back, and it
// still points at the tinted asset, so the look returns by itself. When the
// form lands while one of them holds the body, the look is written onto the
// displaced handle in that slot — the body's true material — not onto the
// husk or the wool. The look system runs ahead of both in their chain, so a
// treatment landing on the same frame stores an already-tinted handle.
//
// **Stealth.** A Shadowform Priest cannot stealth, but stealth's visual edits
// the same asset in place and infers its own state from the alpha (below 0.9
// reads as "stealthed"), so it would take the form's 0.65 for its own fade and
// "restore" it. `update_stealth_visuals` therefore leaves a body wearing the
// look alone, and is ordered against this system so the look's marker is
// applied before stealth can see its alpha. Were the two ever to meet, the
// form's look would hold while both did, and stealth's own threshold would
// take the body back up on its next change once the form had restored its
// recorded values.
//
// Graphical-only: reads `ActiveAuras`, writes no sim state, draws no
// `game_rng` (the shroud's scatter is the shared deterministic hash), and is
// registered in `states/mod.rs` only — headless stays byte-identical.

// ── The bench's knobs (signed off 2026-10-17, version 1792202136-e9a0) ──────

/// The client's tint, `#270042`.
pub const SHADOWFORM_TINT_RGB: [u8; 3] = [39, 0, 66];
/// How far the body colour moves toward the tinted colour (bench: 1.0).
pub const SHADOWFORM_TINT_STRENGTH: f32 = 1.0;
/// How much of the body's own colour shows through the tint, so the pill's
/// shading still reads: the tinted colour is `tint + this × base`.
pub const SHADOWFORM_BASE_SHOW_THROUGH: f32 = 0.12;
/// The body's alpha while the form holds.
pub const SHADOWFORM_BODY_ALPHA: f32 = 0.65;
/// Multipliers over the client's shroud (1.0 = client). Every one shipped at
/// 1.0, with all three of the shroud's layers kept.
pub const SHADOWFORM_SIZE_MUL: f32 = 1.0;
pub const SHADOWFORM_DENSITY_MUL: f32 = 1.0;
pub const SHADOWFORM_SHROUD_OPACITY_MUL: f32 = 1.0;

// ── The table ──────────────────────────────────────────────────────────────

/// The shroud's flame and flare colours: green → violet → violet.
const FLAME_COLOR: [[u8; 3]; 3] = [[0, 104, 7], [24, 0, 86], [34, 0, 119]];

/// One layer of the shroud: its emitter, and the half-angle of the cone it
/// launches in about world up. The bench launches each particle at a polar
/// angle uniform in `±cone` from straight up, from a flat square of side
/// `column.0` on the floor; the shared `rise` launch fans wider than the
/// smoke's nearly straight climb, so the shroud carries its own.
#[derive(Clone, Copy, Debug)]
struct ShroudLayer {
    emitter: ClientEmitter,
    cone: f32,
}

/// `shadow_precast_med_base.m2` (fdid 166808) at the feet, as the bench plays
/// it. A size key is the sprite's RADIUS (the bench draws a key as a radius),
/// so the sizes are `ParticleSize::radii`. Every layer is additive (blend 4)
/// and always on.
const SHADOW_PRECAST_MED_BASE: [ShroudLayer; 3] = [
    // P0 `flamelicksmallblue`: born large and shrinking as it rises, faint.
    ShroudLayer {
        emitter: ClientEmitter {
            name: "shadow_precast_med_base/p0 flames",
            sprite: EmitterSprite::Glow,
            additive: true,
            rate: 50.0,
            window: None,
            life: 1.0,
            speed: 0.56,
            // Negative: the flames ACCELERATE upward at 1.39 yd/s².
            gravity: -1.39,
            area: 0.0,
            column: Some((0.69, 0.0)),
            rise: true,
            color: FLAME_COLOR,
            alpha: [0.0, 0.39, 0.0],
            size: ParticleSize::radii([1.39, 0.56, 0.28]),
            mid: 0.5,
            track: None,
        },
        cone: 0.35,
    },
    // P1 `flare`: small glints rising with the flames.
    ShroudLayer {
        emitter: ClientEmitter {
            name: "shadow_precast_med_base/p1 rising flares",
            sprite: EmitterSprite::Flare,
            additive: true,
            rate: 50.0,
            window: None,
            life: 1.0,
            speed: 1.11,
            gravity: -1.39,
            area: 0.0,
            column: Some((0.42, 0.0)),
            rise: true,
            color: FLAME_COLOR,
            alpha: [0.0, 1.0, 0.0],
            size: ParticleSize::radii([0.28, 0.28, 0.14]),
            mid: 0.5,
            track: None,
        },
        cone: 0.35,
    },
    // P2 + P3 `smoke02a` / `smoke02_flip`, merged as the bench merges them:
    // about one streak a second, climbing nearly straight up to 4-5 yd over a
    // 3 s life. The client's tails (6-7) draw a streak a tenth of a yard long
    // at this speed, so the bench draws them as soft puffs, and so does this.
    ShroudLayer {
        emitter: ClientEmitter {
            name: "shadow_precast_med_base/p2 smoke streaks",
            sprite: EmitterSprite::Glow,
            additive: true,
            rate: 1.1,
            window: None,
            life: 3.0,
            speed: 1.6,
            gravity: 0.0,
            area: 0.0,
            column: Some((0.1, 0.0)),
            rise: true,
            color: [[47, 139, 210], [45, 31, 179], [64, 6, 150]],
            alpha: [0.0, 1.0, 0.0],
            size: ParticleSize::radii([0.42, 0.58, 0.9]),
            mid: 0.5,
            track: None,
        },
        cone: 0.05,
    },
];

/// A layer at the bench's knobs.
const fn at_knobs(layer: ShroudLayer) -> ShroudLayer {
    let mut e = layer.emitter;
    e.size = e.size.scaled(SHADOWFORM_SIZE_MUL);
    e.rate *= SHADOWFORM_DENSITY_MUL;
    let k = SHADOWFORM_SHROUD_OPACITY_MUL;
    e.alpha = [e.alpha[0] * k, e.alpha[1] * k, e.alpha[2] * k];
    ShroudLayer {
        emitter: e,
        cone: layer.cone,
    }
}

/// What the shroud actually plays: the table at the bench's knobs.
static SHADOWFORM_SHROUD: [ShroudLayer; 3] = [
    at_knobs(SHADOW_PRECAST_MED_BASE[0]),
    at_knobs(SHADOW_PRECAST_MED_BASE[1]),
    at_knobs(SHADOW_PRECAST_MED_BASE[2]),
];

// ── Routing ────────────────────────────────────────────────────────────────

/// Whether an aura of this compound wears the Shadowform look: the form's
/// face and its rider both carry `CompoundDebuff::Shadowform`. The one
/// predicate the look and the lands-silently audit both ask.
pub fn wears_shadowform_look(compound: Option<CompoundDebuff>) -> bool {
    compound == Some(CompoundDebuff::Shadowform)
}

/// The body colour the form gives a body whose own colour is `base`: lerped
/// toward `tint + SHADOWFORM_BASE_SHOW_THROUGH × base` by
/// `SHADOWFORM_TINT_STRENGTH`, in sRGB as the bench mixes it, at
/// `SHADOWFORM_BODY_ALPHA`.
pub fn shadowform_body_color(base: Color) -> Color {
    let b = base.to_srgba();
    let mix = |c: f32, tint: u8| {
        let target = tint as f32 / 255.0 + SHADOWFORM_BASE_SHOW_THROUGH * c;
        (c + (target - c) * SHADOWFORM_TINT_STRENGTH).clamp(0.0, 1.0)
    };
    Color::srgba(
        mix(b.red, SHADOWFORM_TINT_RGB[0]),
        mix(b.green, SHADOWFORM_TINT_RGB[1]),
        mix(b.blue, SHADOWFORM_TINT_RGB[2]),
        SHADOWFORM_BODY_ALPHA,
    )
}

// ── Systems ────────────────────────────────────────────────────────────────

/// Put the Shadowform look on every living unit holding the form, and take it
/// off when the form ends or its holder dies.
///
/// The look is the tint and alpha written in place on the body's true
/// material (see the module note) plus the feet shroud rig. Its marker is the
/// [`ShadowformLook`] on the body, so the look lands once and lifts once.
/// Death counts as an end: a killing blow leaves the aura on the corpse.
pub fn update_shadowform_visuals(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    combatants: Query<(Entity, &Combatant, Option<&ActiveAuras>, &Children)>,
    bodies: Query<
        (
            &MeshMaterial3d<StandardMaterial>,
            Option<&OriginalBodyMaterial>,
            Option<&ShadowformLook>,
        ),
        With<VisualBody>,
    >,
    shrouds: Query<(Entity, &ShadowformShroud)>,
) {
    for (entity, combatant, auras, children) in combatants.iter() {
        let held = combatant.is_alive()
            && auras.is_some_and(|a| a.auras.iter().any(|au| wears_shadowform_look(au.compound)));
        let Some(body) = children.iter().find(|&c| bodies.contains(c)) else {
            continue;
        };
        let Ok((worn, displaced, look)) = bodies.get(body) else {
            continue;
        };

        match (held, look) {
            (true, None) => {
                // The body's own material: the handle Fear or Polymorph
                // displaced, if one holds the body, or the one it wears.
                let own = displaced.map_or(&worn.0, |d| &d.0).clone();
                let Some(material) = materials.get_mut(&own) else {
                    continue;
                };
                let record = ShadowformLook {
                    material: own,
                    base_color: material.base_color,
                    alpha_mode: material.alpha_mode,
                };
                material.base_color = shadowform_body_color(record.base_color);
                material.alpha_mode = AlphaMode::Blend;
                commands.entity(body).try_insert(record);

                if !shrouds.iter().any(|(_, s)| s.owner == entity) {
                    commands.spawn((
                        ShadowformShroud {
                            owner: entity,
                            carry: [0.0; 3],
                            emitted: 0,
                        },
                        Transform::default(),
                        Visibility::default(),
                        PlayMatchEntity,
                    ));
                }
            }
            (false, Some(look)) => {
                if let Some(material) = materials.get_mut(&look.material) {
                    material.base_color = look.base_color;
                    material.alpha_mode = look.alpha_mode;
                }
                commands.entity(body).remove::<ShadowformLook>();
                // Owner-scoped: another Priest's shroud stays.
                for (rig, shroud) in shrouds.iter() {
                    if shroud.owner == entity {
                        commands.entity(rig).despawn();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Where a shroud particle is born, relative to the feet, and how it launches:
/// from a flat square on the floor, at a polar angle uniform in the layer's
/// cone about world up and a uniform azimuth.
fn shroud_launch(layer: &ShroudLayer, seed: u32) -> (Vec3, Vec3) {
    let e = &layer.emitter;
    let side = e.column.map_or(e.area, |(w, _)| w);
    let offset = Vec3::new(
        (draw(seed, 1) - 0.5) * side,
        0.0,
        (draw(seed, 3) - 0.5) * side,
    );
    let polar = (draw(seed, 4) * 2.0 - 1.0) * layer.cone;
    let azimuth = draw(seed, 5) * TAU;
    let dir = Vec3::new(
        polar.sin() * azimuth.cos(),
        polar.cos(),
        polar.sin() * azimuth.sin(),
    );
    (offset, dir * e.speed)
}

/// One shroud particle, a child of its rig (so it rides the Priest), turned to
/// the camera by [`billboard_shadowform_shrouds`].
fn spawn_shroud_particle(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    layer: &ShroudLayer,
    palette: &Arc<[Handle<StandardMaterial>]>,
    seed: u32,
) -> Entity {
    let e = &layer.emitter;
    let (offset, velocity) = shroud_launch(layer, seed);
    commands
        .spawn((
            ClientParticle {
                age: 0.0,
                life: e.life,
                velocity,
                gravity: e.gravity,
                size: e.size,
                mid: e.mid,
                palette: palette.clone(),
                step: 0,
                facing: ParticleFacing::Camera,
                owned: true,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(palette[0].clone()),
            Transform::from_translation(offset),
            NotShadowCaster,
        ))
        .id()
}

/// Follow each shroud's Priest at its feet, retire its spent particles and
/// emit what its layers owe this frame. A shroud whose Priest is gone goes
/// with it.
///
/// The rig lives as long as the form, so a spent particle is despawned here
/// rather than left collapsed under it (`animate_client_particles` only
/// collapses an owned one).
pub fn update_shadowform_shrouds(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<EmitterAssets>>,
    mut rigs: Query<(
        Entity,
        &mut ShadowformShroud,
        &mut Transform,
        Option<&Children>,
    )>,
    owners: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<ShadowformShroud>)>,
    particles: Query<&ClientParticle>,
) {
    let dt = time.delta_secs();
    for (entity, mut shroud, mut transform, children) in rigs.iter_mut() {
        let Ok((owner, pet)) = owners.get(shroud.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = feet_anchor(owner.translation, pet.is_some());
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if particles.get(child).is_ok_and(|p| p.age >= p.life) {
                commands.entity(child).despawn();
            }
        }

        let assets = assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        let quad = assets.quad();
        for (i, layer) in SHADOWFORM_SHROUD.iter().enumerate() {
            shroud.carry[i] += layer.emitter.rate * dt;
            if shroud.carry[i] < 1.0 {
                continue;
            }
            let palette = assets.palette(&mut materials, &layer.emitter);
            while shroud.carry[i] >= 1.0 {
                shroud.carry[i] -= 1.0;
                let seed = shroud
                    .owner
                    .index()
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(shroud.emitted.wrapping_mul(0x85EB_CA6B));
                shroud.emitted = shroud.emitted.wrapping_add(1);
                let particle = spawn_shroud_particle(&mut commands, &quad, layer, &palette, seed);
                commands.entity(entity).add_child(particle);
            }
        }
    }
}

/// Turn every shroud particle to face the camera.
pub fn billboard_shadowform_shrouds(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<ShadowformShroud>,
            Without<ClientParticle>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<ShadowformShroud>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (Without<Camera3d>, Without<ShadowformShroud>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        for child in children.iter() {
            if let Ok((particle, mut part)) = particles.get_mut(child) {
                part.rotation = particle_rotation(particle, rig.rotation, cam.rotation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bench's Priest (235 grey) at its defaults: `#270042 + 0.12 × base`.
    #[test]
    fn the_body_colour_is_the_benchs_mix() {
        let tinted = shadowform_body_color(Color::srgb_u8(235, 235, 235)).to_srgba();
        let expect = [39.0 + 0.12 * 235.0, 0.12 * 235.0, 66.0 + 0.12 * 235.0];
        for (got, want) in [tinted.red, tinted.green, tinted.blue]
            .into_iter()
            .zip(expect)
        {
            assert!((got * 255.0 - want).abs() < 1e-3, "{got} vs {want}");
        }
        assert_eq!(tinted.alpha, 0.65);
    }

    /// One layer as the signed-off bench carries it.
    struct Bench {
        name: &'static str,
        sprite: EmitterSprite,
        rate: f32,
        life: f32,
        speed: f32,
        gravity: f32,
        side: f32,
        cone: f32,
        color: [[u8; 3]; 3],
        radius: [f32; 3],
        alpha: [f32; 3],
    }

    /// Every layer against the signed-off bench (version 1792202136-e9a0,
    /// "Shadowform", at its defaults): sprite, rate, life, speed, rise, area,
    /// cone, colour keys, radii and alpha, with every multiplier at 1.0.
    #[test]
    fn the_layers_are_the_benchs_values() {
        let bench = [
            Bench {
                name: "flames",
                sprite: EmitterSprite::Glow,
                rate: 50.0,
                life: 1.0,
                speed: 0.56,
                gravity: -1.39,
                side: 0.69,
                cone: 0.35,
                color: [[0, 104, 7], [24, 0, 86], [34, 0, 119]],
                radius: [1.39, 0.56, 0.28],
                alpha: [0.0, 0.39, 0.0],
            },
            Bench {
                name: "rising flares",
                sprite: EmitterSprite::Flare,
                rate: 50.0,
                life: 1.0,
                speed: 1.11,
                gravity: -1.39,
                side: 0.42,
                cone: 0.35,
                color: [[0, 104, 7], [24, 0, 86], [34, 0, 119]],
                radius: [0.28, 0.28, 0.14],
                alpha: [0.0, 1.0, 0.0],
            },
            Bench {
                name: "smoke streaks",
                sprite: EmitterSprite::Glow,
                rate: 1.1,
                life: 3.0,
                speed: 1.6,
                gravity: 0.0,
                side: 0.1,
                cone: 0.05,
                color: [[47, 139, 210], [45, 31, 179], [64, 6, 150]],
                radius: [0.42, 0.58, 0.9],
                alpha: [0.0, 1.0, 0.0],
            },
        ];
        for (layer, b) in SHADOWFORM_SHROUD.iter().zip(bench) {
            let (e, name) = (&layer.emitter, b.name);
            assert!(e.name.ends_with(name), "{} is not the {name}", e.name);
            assert!(e.additive, "{name} is additive");
            assert_eq!(e.sprite, b.sprite, "{name} sprite");
            assert_eq!(e.rate, b.rate, "{name} rate");
            assert_eq!(e.life, b.life, "{name} life");
            assert_eq!(e.speed, b.speed, "{name} speed");
            assert_eq!(e.gravity, b.gravity, "{name} gravity");
            assert_eq!(e.column, Some((b.side, 0.0)), "{name} area");
            assert_eq!(layer.cone, b.cone, "{name} cone");
            assert_eq!(e.color, b.color, "{name} colour keys");
            for k in 0..3 {
                assert!(
                    (e.size.diameter_keys()[k] - 2.0 * b.radius[k]).abs() < 1e-6,
                    "{name} size"
                );
                assert!((e.alpha[k] - b.alpha[k]).abs() < 1e-6, "{name} alpha");
            }
        }
    }

    /// A launch stays inside its square on the floor and inside its cone, and
    /// climbs at the layer's speed.
    #[test]
    fn a_launch_rises_inside_its_cone_from_its_square() {
        for layer in &SHADOWFORM_SHROUD {
            let side = layer.emitter.column.unwrap().0;
            for seed in 0..500 {
                let (offset, velocity) = shroud_launch(layer, seed);
                assert_eq!(offset.y, 0.0);
                assert!(offset.x.abs() <= side / 2.0 && offset.z.abs() <= side / 2.0);
                assert!((velocity.length() - layer.emitter.speed).abs() < 1e-4);
                let off_up = velocity.normalize().angle_between(Vec3::Y);
                assert!(off_up <= layer.cone + 1e-4, "{off_up} > {}", layer.cone);
            }
        }
    }

    /// A smoke streak climbs to 4-5 yd in its life (the client's reading).
    #[test]
    fn a_smoke_streak_climbs_four_to_five_yards() {
        let e = &SHADOWFORM_SHROUD[2].emitter;
        let climb = e.speed * e.life;
        assert!((4.0..=5.0).contains(&climb), "{climb}");
    }

    #[test]
    fn only_the_shadowform_compound_wears_the_look() {
        assert!(wears_shadowform_look(Some(CompoundDebuff::Shadowform)));
        assert!(!wears_shadowform_look(None));
        for other in CompoundDebuff::ALL {
            if other != CompoundDebuff::Shadowform {
                assert!(!wears_shadowform_look(Some(other)), "{other:?}");
            }
        }
    }
}
