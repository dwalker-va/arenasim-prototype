use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use super::hunter_shots::{spawn_client_particle, ClientEmitter, EmitterAssets, EmitterSprite};
use crate::states::play_match::components::*;

// ==============================================================================
// Travel Form — the pill on all fours, and the shapeshift puff
// ==============================================================================
//
// From the AS-160 Druid client-data doc (`docs/design/2026-10-03-druid-client-data.md`,
// `druidmorph_impact_base.m2`, `tiger.m2`, ruling 5) and its signed-off bench
// (`docs/design/benches/2026-10-03-druid-visuals-bench.html`, Travel Form tab),
// whose drawing code this ports.
//
// - **The body** (ruling 5): not the client's cheetah, which clashes with pill
//   combatants. The standing capsule lies down into a horizontal pill laid
//   along the heading, with a small head pill forward and raised, two pointed
//   cat ears and a thin tail angled up and back. It keeps the class tint.
// - **The gait is a bound** (`gait.rs`, `update_travel_bound`): a bob and a
//   nose-up/nose-down rock once per stride, driven by distance travelled.
// - **The puff** (kit 3610, shared by every shapeshift) plays on shift in AND
//   out: a violet → sky-blue flash, white toon smoke, fast streaks and star
//   motes.
//
// **The form's restore slot is its own** ([`TravelFormBodyMesh`]). Fear lands
// on a shifted Druid, so the form never touches `OriginalMesh` /
// `OriginalBodyMaterial`: it takes the body's MESH away and leaves its
// MATERIAL alone, and the form's parts wear whatever material the body has
// (`dress_travel_form`). Fear swaps that material in place, so the two
// compose in either order and either can end first.
//
// Graphical only: everything here reads the sim and writes none of it, the
// particle scatter is a deterministic hash (never `game_rng`), and the systems
// are registered in `states/mod.rs` alone — headless stays byte-identical.

// ── The bench's sign-off constants (AS-160, 2026-10-03) ─────────────────────

// Travel Form — pill body + druidmorph_impact_base.m2 (kit 3610) shift puff, AS-160 bench
pub const SHIFT_PUFF_SECS: f32 = 1.70;
pub const SHIFT_PUFF_LAYERS: [bool; 4] = [true, true, true, true]; // flash, smoke, streaks, motes
pub const SHIFT_PUFF_SIZE_MUL: f32 = 1.00;
pub const SHIFT_PUFF_SPREAD_MUL: f32 = 1.00;
pub const SHIFT_PUFF_DENSITY_MUL: f32 = 1.00;
pub const TRAVEL_BODY_LENGTH: f32 = 1.70; // tip to tip, Capsule3d::new(r, len - 2r) laid along heading
pub const TRAVEL_BODY_RADIUS: f32 = 0.36;
pub const TRAVEL_BODY_HEIGHT: f32 = 0.60; // centre above ground
pub const TRAVEL_PARTS: [bool; 3] = [true, true, true]; // head, ears, tail
pub const TRAVEL_EAR_HEIGHT: f32 = 0.20; // cone ears
pub const TRAVEL_EAR_HALF_WIDTH: f32 = 0.09;
pub const TRAVEL_STRIDE: f32 = 4.40; // yd per bound
pub const TRAVEL_BOUND_BOB: f32 = 0.12;
pub const TRAVEL_BOUND_ROCK_DEG: f32 = 9.0;

// ── The body's proportions, as the bench draws them (`drawCat`) ─────────────

/// The head pill's radius, as a share of the body's.
const HEAD_RADIUS_SHARE: f32 = 0.78;
/// The head's centre: this far beyond the body's front tip, and this far above
/// the body's centre line.
const HEAD_FORWARD: f32 = 0.05;
const HEAD_RAISE: f32 = 0.32;
/// An ear's base sits this share of the head radius out to the side and up,
/// and its tip leans out to `EAR_SPLAY` times the base's offset and back by
/// `EAR_LEAN_BACK` yards.
const EAR_SIDE_SHARE: f32 = 0.5;
const EAR_BASE_SHARE: f32 = 0.6;
const EAR_SPLAY: f32 = 1.15;
const EAR_LEAN_BACK: f32 = 0.04;
/// The tail: a thin capsule from just inside the rump, up and back.
const TAIL_RADIUS: f32 = 0.07;
const TAIL_BASE: Vec3 = Vec3::new(0.0, 0.10, 0.05); // z: inside the rear tip
const TAIL_TIP: Vec3 = Vec3::new(0.0, 0.55, -0.45); // z: beyond the rear tip
/// The bench draws the ears and the tail a shade darker than the body (×0.8 and
/// ×0.9 of the class colour), which separates them from the head and rump they
/// overlap. Applied to whatever material the body wears, so a feared Druid's
/// ears darken the husk, not the class colour.
const EAR_SHADE: f32 = 0.8;
const TAIL_SHADE: f32 = 0.9;

/// Whether a unit is drawn shifted: alive and carrying the form. A killing
/// blow leaves the aura on the corpse (`update_auras` skips the dead), so
/// death counts as an exit, as it does for the sheep and the husk.
fn is_shifted(combatant: &Combatant, auras: Option<&ActiveAuras>) -> bool {
    combatant.is_alive() && auras.is_some_and(|a| a.is_shapeshifted())
}

/// Shift a Druid's body into Travel Form and back.
///
/// IN: the [`VisualBody`]'s standing capsule mesh is taken off it into the
/// form's own [`TravelFormBodyMesh`] slot (the body stays, so everything that
/// rides it — the gait's bob, a hit's flinch, the death sink, a lift — carries
/// the form too), and a [`TravelFormRig`] child holds the lying pill, head, ears
/// and tail. OUT, by any path — shifting out, death, a purge or any other
/// removal of the aura, the component itself going — the mesh goes back and
/// the rig is despawned. Both directions play the shift puff.
///
/// `Without<PolymorphedVisual>`: a sheep that is already up keeps the body
/// (the sheep swaps the same mesh), and the polymorph system carries the
/// mirror `Without<TravelFormVisual>`. A shifted Druid cannot be polymorphed in
/// the sim, so this is the only order that can arise.
///
/// Graphical-mode only — registered in `StatesPlugin::build()`.
pub fn update_travel_form_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    combatants: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&TravelFormVisual>,
            &Children,
        ),
        Without<PolymorphedVisual>,
    >,
    bodies: Query<(
        Option<&Mesh3d>,
        &MeshMaterial3d<StandardMaterial>,
        &VisualBody,
        Option<&TravelFormBodyMesh>,
    )>,
) {
    for (entity, combatant, transform, auras, marker, children) in combatants.iter() {
        let shifted = is_shifted(combatant, auras);

        if shifted && marker.is_none() {
            // Locate the body before building anything: without one there is
            // nothing to shift, and the marker stays off so this retries.
            let Some((body_child, (Some(mesh), material, body, _))) = children
                .iter()
                .find_map(|c| bodies.get(c).ok().map(|b| (c, b)))
            else {
                continue;
            };

            // The floor in the body's local space, as the sheep derives it:
            // pets render their body off the sim entity's `y`.
            let ground_y = -(transform.translation.y + body.rest_y);
            let rig = commands
                .spawn((
                    TravelFormRig {
                        owner: entity,
                        rock: 0.0,
                        dressed_in: None,
                    },
                    Transform::from_xyz(0.0, ground_y + TRAVEL_BODY_HEIGHT, 0.0),
                    Visibility::default(),
                ))
                .id();
            spawn_travel_form_parts(&mut commands, &mut meshes, &material.0, rig);
            commands.entity(body_child).add_child(rig);
            commands
                .entity(body_child)
                .insert(TravelFormBodyMesh(mesh.0.clone()))
                .remove::<Mesh3d>();
            commands.entity(entity).insert(TravelFormVisual { rig });
            spawn_shift_puff(&mut commands, entity, transform);
        } else if !shifted {
            let Some(marker) = marker else {
                continue;
            };
            for child in children.iter() {
                if let Ok((_, _, _, Some(stored))) = bodies.get(child) {
                    commands
                        .entity(child)
                        .insert(Mesh3d(stored.0.clone()))
                        .remove::<TravelFormBodyMesh>();
                }
            }
            if let Ok(mut rig) = commands.get_entity(marker.rig) {
                rig.despawn();
            }
            commands.entity(entity).remove::<TravelFormVisual>();
            spawn_shift_puff(&mut commands, entity, transform);
        }
    }
}

/// Build the form's primitives as children of `rig`, whose origin is the lying
/// pill's centre and whose +Z is the heading. Each wears `material` for now;
/// [`dress_travel_form`] shades the ears and tail the same frame.
fn spawn_travel_form_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    rig: Entity,
) {
    let [head, ears, tail] = TRAVEL_PARTS;
    let mut part = |mesh: Mesh, shade: f32, transform: Transform| {
        let child = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                transform,
                TravelFormPart { shade },
            ))
            .id();
        commands.entity(rig).add_child(child);
    };

    // The lying pill: a capsule's axis is its local Y, laid along +Z. These
    // are the body, not an effect, so they cast a shadow as the standing
    // capsule they replace does.
    let half = TRAVEL_BODY_LENGTH / 2.0;
    part(
        Capsule3d::new(
            TRAVEL_BODY_RADIUS,
            TRAVEL_BODY_LENGTH - 2.0 * TRAVEL_BODY_RADIUS,
        )
        .into(),
        1.0,
        Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2)),
    );

    let head_radius = TRAVEL_BODY_RADIUS * HEAD_RADIUS_SHARE;
    let head_centre = Vec3::new(0.0, HEAD_RAISE, half + HEAD_FORWARD);
    if head {
        part(
            Sphere::new(head_radius).mesh().uv(20, 12),
            1.0,
            Transform::from_translation(head_centre),
        );
        if ears {
            for side in [-1.0f32, 1.0] {
                let out = side * head_radius * EAR_SIDE_SHARE;
                let base = head_centre + Vec3::new(out, head_radius * EAR_BASE_SHARE, 0.0);
                let tip = head_centre
                    + Vec3::new(
                        out * EAR_SPLAY,
                        head_radius * EAR_BASE_SHARE + TRAVEL_EAR_HEIGHT,
                        -EAR_LEAN_BACK,
                    );
                part(
                    Cone::new(TRAVEL_EAR_HALF_WIDTH, base.distance(tip)).into(),
                    EAR_SHADE,
                    segment_transform(base, tip),
                );
            }
        }
    }
    if tail {
        let base = TAIL_BASE - Vec3::Z * half;
        let tip = TAIL_TIP - Vec3::Z * half;
        part(
            Capsule3d::new(TAIL_RADIUS, base.distance(tip)).into(),
            TAIL_SHADE,
            segment_transform(base, tip),
        );
    }
}

/// The transform that lays a Y-axis primitive centred on its origin (a capsule,
/// a cone) from `from` to `to`.
fn segment_transform(from: Vec3, to: Vec3) -> Transform {
    Transform::from_translation((from + to) / 2.0)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, (to - from).normalize()))
}

/// Dress the form's parts in the body's current material, and keep the form
/// out of the way of what still expects the standing body.
///
/// The body keeps its material while shifted — only its mesh is gone — so a
/// Fear's husk tint swaps onto it exactly as onto a standing unit, and the form
/// follows here: the lying pill and the head wear the body's material handle,
/// the ears and tail a shaded copy of it. Re-dressed only when the handle
/// changes.
///
/// A Fear's fitted shroud is shaped to the STANDING capsule and rides the body,
/// so it would stand up around the lying pill; it is hidden while its unit is
/// shifted and shown again when the form ends (the husk tint and the rising
/// motes carry the fear meanwhile).
pub fn dress_travel_form(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(&mut TravelFormRig, &ChildOf, &Children)>,
    bodies: Query<&MeshMaterial3d<StandardMaterial>, (With<VisualBody>, Without<TravelFormPart>)>,
    mut parts: Query<(&TravelFormPart, &mut MeshMaterial3d<StandardMaterial>), Without<VisualBody>>,
    shifted: Query<(), With<TravelFormVisual>>,
    mut shrouds: Query<(&FearShroud, &mut Visibility)>,
) {
    for (mut rig, parent, children) in rigs.iter_mut() {
        let Ok(body_material) = bodies.get(parent.parent()) else {
            continue;
        };
        if rig.dressed_in.as_ref() == Some(&body_material.0) {
            continue;
        }
        let source = body_material.0.clone();
        let base = materials.get(&source).cloned();
        for child in children.iter() {
            let Ok((part, mut material)) = parts.get_mut(child) else {
                continue;
            };
            material.0 = match &base {
                Some(base) if part.shade < 1.0 => {
                    let mut shaded = base.clone();
                    let c = shaded.base_color.to_srgba();
                    shaded.base_color = Color::srgba(
                        c.red * part.shade,
                        c.green * part.shade,
                        c.blue * part.shade,
                        c.alpha,
                    );
                    materials.add(shaded)
                }
                _ => source.clone(),
            };
        }
        rig.dressed_in = Some(source);
    }

    for (shroud, mut visibility) in shrouds.iter_mut() {
        let wanted = if shifted.contains(shroud.owner) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        visibility.set_if_neq(wanted);
    }
}

// ── The shift puff: druidmorph_impact_base.m2 (kit 3610) ────────────────────

/// The four emitters, transcribed as the bench carries them. Sizes are
/// diameters (the bench draws a particle at twice its M2 scale).
const DRUIDMORPH_IMPACT_BASE: [ClientEmitter; 4] = [
    // P0 genericglow2c: the swelling violet → sky-blue flash.
    ClientEmitter {
        name: "druidmorph_impact_base/p0 flash",
        sprite: EmitterSprite::Glow,
        additive: true,
        rate: 5.0,
        window: Some((0.0, 1.4)),
        life: 1.32,
        speed: 0.064,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: true,
        color: [[69, 0, 210], [106, 180, 243], [69, 0, 210]],
        alpha: [0.22, 0.36, 0.0],
        size: [0.306, 1.488, 3.688],
        mid: 0.5,
        track: None,
    },
    // P1 + P2 toonsmoke16: the white smoke, alpha-blended, 74/s in all.
    ClientEmitter {
        name: "druidmorph_impact_base/p1+p2 smoke",
        sprite: EmitterSprite::Glow,
        additive: false,
        rate: 74.0,
        window: Some((0.0, 1.7)),
        life: 1.25,
        speed: 1.25,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[255, 255, 255]; 3],
        alpha: [0.7, 1.0, 0.0],
        size: [1.1, 1.0, 0.52],
        mid: 0.5,
        track: None,
    },
    // P3 gradient64flipa: fast streaks, tail 5.0.
    ClientEmitter {
        name: "druidmorph_impact_base/p3 streaks",
        sprite: EmitterSprite::Streak,
        additive: true,
        rate: 20.0,
        window: Some((0.0, 1.4)),
        life: 0.8,
        speed: 3.889,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[115, 121, 255], [112, 210, 255], [244, 252, 255]],
        alpha: [0.0, 0.59, 0.0],
        size: [0.006, 0.556, 0.056],
        mid: 0.5,
        track: None,
    },
    // P4 stardust: violet → blue → white motes.
    ClientEmitter {
        name: "druidmorph_impact_base/p4 motes",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 20.0,
        window: Some((0.0, 1.4)),
        life: 1.0,
        speed: 0.556,
        gravity: 0.0,
        area: 0.0,
        column: None,
        rise: false,
        color: [[61, 0, 187], [119, 196, 255], [255, 255, 255]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.388, 0.388, 0.388],
        mid: 0.5,
        track: None,
    },
];

/// Where each emitter sits above the Druid's feet, `(forward, up)` in yards.
const DRUIDMORPH_ORIGINS: [(f32, f32); 4] = [(0.22, 0.47), (0.05, 0.30), (0.0, 0.53), (0.0, 0.51)];

/// The client's own puff length, which [`SHIFT_PUFF_SECS`] stretches.
const DRUIDMORPH_CLIENT_SECS: f32 = 1.70;

/// The puff's emitters at the signed-off knobs.
fn shift_puff_emitter(i: usize) -> ClientEmitter {
    let mut e = DRUIDMORPH_IMPACT_BASE[i].scaled(
        SHIFT_PUFF_SIZE_MUL,
        SHIFT_PUFF_SPREAD_MUL,
        SHIFT_PUFF_SECS / DRUIDMORPH_CLIENT_SECS,
    );
    e.rate *= SHIFT_PUFF_DENSITY_MUL;
    e
}

/// The puff's emission origin: the unit's feet. The floor is world y 0, as the
/// sheep and the form's rig take it.
fn feet_of(transform: &Transform) -> Vec3 {
    Vec3::new(transform.translation.x, 0.0, transform.translation.z)
}

fn spawn_shift_puff(commands: &mut Commands, owner: Entity, transform: &Transform) {
    commands.spawn((
        ShiftPuff {
            owner,
            origin: feet_of(transform),
            facing: transform.rotation,
            age: 0.0,
            carry: [0.0; 4],
            emitted: 0,
        },
        PlayMatchEntity,
    ));
}

/// Play every live shift puff: follow its Druid while it emits, emit each
/// layer's particles over its keyed window, and retire the puff when the
/// windows close. The particles are world-space client particles, aged,
/// turned and despawned by `animate_client_particles` and
/// `billboard_hunter_shots`; each carries `NotShadowCaster`.
pub fn update_shift_puffs(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<EmitterAssets>>,
    mut puffs: Query<(Entity, &mut ShiftPuff)>,
    owners: Query<&Transform, With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut puff) in puffs.iter_mut() {
        if puff.age >= SHIFT_PUFF_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        if let Ok(owner) = owners.get(puff.owner) {
            puff.origin = feet_of(owner);
            puff.facing = owner.rotation;
        }
        let (from, to) = (puff.age, puff.age + dt);
        puff.age = to;

        let assets = assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        let quad = assets.quad();
        for (i, on) in SHIFT_PUFF_LAYERS.into_iter().enumerate() {
            if !on {
                continue;
            }
            let e = shift_puff_emitter(i);
            puff.carry[i] += e.count_between(from, to);
            if puff.carry[i] < 1.0 {
                continue;
            }
            let palette = assets.palette(&mut materials, &DRUIDMORPH_IMPACT_BASE[i]);
            let (forward, up) = DRUIDMORPH_ORIGINS[i];
            let at = puff.origin
                + puff.facing * Vec3::Z * (forward * SHIFT_PUFF_SPREAD_MUL)
                + Vec3::Y * up;
            while puff.carry[i] >= 1.0 {
                puff.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_add(puff.emitted.wrapping_mul(0x85EB_CA6B));
                puff.emitted = puff.emitted.wrapping_add(1);
                spawn_client_particle(&mut commands, &quad, &e, &palette, at, seed, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_puff_emits_its_keyed_counts() {
        // Rate × window: 7 flash glows, 125.8 smoke, 28 streaks, 28 motes.
        let counts: Vec<f32> = (0..4)
            .map(|i| shift_puff_emitter(i).count_between(0.0, 10.0))
            .collect();
        for (got, want) in counts.iter().zip([7.0, 125.8, 28.0, 28.0]) {
            assert!((got - want).abs() < 1e-3, "{counts:?}");
        }
    }

    #[test]
    fn the_puff_lasts_as_long_as_its_longest_window() {
        let last = (0..4)
            .map(|i| shift_puff_emitter(i).end())
            .fold(0.0f32, f32::max);
        assert!((last - SHIFT_PUFF_SECS).abs() < 1e-6);
    }

    #[test]
    fn the_ears_are_the_signed_off_height() {
        // The cone's height is the base-to-tip distance: the signed-off ear
        // height plus the lean's small contribution.
        let head_radius = TRAVEL_BODY_RADIUS * HEAD_RADIUS_SHARE;
        let out = head_radius * EAR_SIDE_SHARE;
        let rise = TRAVEL_EAR_HEIGHT;
        let len = Vec3::new(out * (EAR_SPLAY - 1.0), rise, -EAR_LEAN_BACK).length();
        assert!(len >= TRAVEL_EAR_HEIGHT && len < TRAVEL_EAR_HEIGHT * 1.05);
    }
}
