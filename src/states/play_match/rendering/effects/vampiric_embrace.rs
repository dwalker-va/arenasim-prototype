use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::conflagrate::{conflagrate_material, ConflagrateSprite, CONFLAGRATE_GLOW};
use super::druid_heals::{body_point, keyed, keyed_color};
use super::heal_impact::{HealMoteLook, Ramp};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Vampiric Embrace — the dust implosion on the victim, the violet heal stream
// on each ally
// ==============================================================================
//
// From the client data, `docs/design/2026-10-17-priest-talent-client-data.md`
// §7 (AS-335):
//
// - **Cast** (kit 218): `shadow_precast_low_hand.m2` on both hands. Vampiric
//   Embrace is an instant the Priest AI applies inline, with no cast state and
//   no instant marker, and no Shadow instant draws its hands today (Shadow
//   Word: Pain is the precedent), so nothing is drawn on the Priest.
// - **Landing on the victim** (kit 3109): `soothingkiss_impact_head.m2` at the
//   chest — one emitter, `tail_dust3`, additive, 80/s for 1967 ms, travelling
//   in at 1.67 yd/s, 0.5 s lives, 0.83 -> 0.42 -> 0.14 yd, (0,0,196) ->
//   (101,22,198) -> (212,5,195), alpha 0 -> 1 -> 0.78: a 2 s ball of blue to
//   magenta dust collapsing into the chest. Rides the sim's `AuraLanding`
//   marker (`apply_pending_auras`, on a fresh application or a refresh).
// - **No debuff state.** The 60 s mark draws nothing after it lands; its icon
//   carries it.
// - **The party heal** (kit 3072): `vampiricembrace_state_base.m2`, the Heal
//   landing's rig recoloured violet, on every ally a heal reaches. It is the
//   shipped heal rig (`heal_impact.rs`, `HealImpactKind::VampiricEmbrace`),
//   spawned by `combat_core::leech::land` for every heal, and played on an
//   ally at most once every [`VAMPIRIC_EMBRACE_HEAL_VISUAL_MIN_INTERVAL_SECS`]
//   (the AS-335 ruling: a Mind Flay heals once a second). The throttle is the
//   rig's (`heal_landing_interval`), keyed per ally; every heal still lands
//   and is logged.
//
// Played as the signed-off AS-335 Priest Talent Visuals Bench plays it
// (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
// 1792207490-53c9, "Vampiric Embrace" tab, both layers kept, every knob at
// its default; its source as committed with the sign-off is
// `docs/design/benches/2026-10-17-priest-talent-bench.html`). Its constants
// block, at the ruling, is the block below. Both layers draw through the
// bench's `Emitter.draw` without `blend: 'alpha'`, so both are `lighter`:
// additive.
//
// Where the research doc and the bench disagree, the bench was signed off and
// wins. The research reads the heal model as three ribbon emitters (lives
// 0.9 / 1.1 / 1.3 s) plus the dim-star emitter and one `star5a` emitter
// turning red -> gold; the bench draws ONE layer of soft violet glows (1.1 s
// lives, rising 1.4 yd/s in a 0.15 rad cone, 0.09 -> 0.05 yd, alpha 0.9 -> 0)
// and no stars, so no red-to-gold stars and no dim stars are drawn.
//
// ## Departures from the bench (each deliberate)
//
// 1. **Held in front of the body.** The bench is a 2D canvas: it paints the
//    bodies first and every particle over them. Here the body is an
//    `AlphaMode::Blend` mesh that writes no depth, and the transparent pass
//    sorts by each mesh's translation, so a particle deeper than the body's
//    centre — about half of a shell round the chest, and of a stream rising
//    through the body — is drawn first and the body paints over it. Every
//    implosion particle and every stream mote is drawn on its own view ray,
//    slid in front of its unit's body and scaled by the same ratio — the
//    shared body hold (`body_hold.rs`) — so it projects where the bench puts
//    it.
// 2. **The stream rides its ally.** The bench's heal motes stay where they
//    were born (no `follow`); the shipped heal rig carries its motes with the
//    recipient, as every heal landing does.
// 3. **The stream's cone is the rig's.** The bench draws a polar angle up to
//    0.15 rad at 1.4 yd/s; the shipped rig launches at 1.4 yd/s up plus a
//    sideways kick up to 0.15 of that — the same 0.15 rad cone, at most 1.1%
//    faster at its rim.
// 4. **The stream's rate is integrated per frame.** The rig accumulates the
//    rate at each frame's point in the window; the bench integrates its own
//    frames the same way, so the count is the track's 13.7 motes give or take
//    one.
// 5. **Follows in height too.** The bench's `follow` moves the dust with the
//    victim in x and z only; here it rides the rig at the chest.
//
// ## Lifecycle
//
// The implosion is a landing. It plays out on a victim that dies or is
// polymorphed under it (Conflagrate's precedent); a victim that is GONE
// retires it at once. Its particles ride it, as the bench's `follow` says,
// and go with it. Everything is a `PlayMatchEntity`, so a match that ends
// mid-implosion takes all of it.
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: a newborn dust mote is ~36 px across, shrinking
// to ~6 px as it reaches the chest; a stream glow is ~4 px, shrinking to ~2.
// Client size, by the ruling.
//
// **Rides the rendered body.** The rig stands at the chest of the victim's
// `VisualBody`, not its root, and the hold measures that body
// (`rendered_root`): the walk bob, the fear bob and the swing step carry the
// dust with them, as `shield.rs`, `vengeance.rs` and `shadow_dance.rs` ride
// it. So the placement runs in PostUpdate, after every writer of a body's
// transform and before propagation (`place_vampiric_embrace_implosions`).
//
// House rules: spawn / animate / age / place systems, `Res<Time>`,
// registered in `states/mod.rs` only (headless never runs any of it), no
// `game_rng` — all scatter is hash-seeded off the rig's entity index and
// emission count.

// ── Bench sign-off constants (AS-335, version 1792207490-53c9) ───────────────

// Vampiric Embrace: soothingkiss_impact_head.m2 (kit 3109) on the target; violet heal_low_base (kit 3072) on each healed ally. AS-335 bench.
pub const VAMPIRIC_EMBRACE_IMPLOSION: bool = true; // 2 s
pub const VAMPIRIC_EMBRACE_HEAL_STREAM: bool = true;
pub const VAMPIRIC_EMBRACE_HEAL_VISUAL_MIN_INTERVAL_SECS: f32 = 3.00; // per ally; client = every heal event
                                                                      // No debuff state (client has none).

// ── The implosion: soothingkiss_impact_head.m2 ───────────────────────────────

/// The bench's chest anchor, above the floor (`ANCH.chest`; attachment 34).
pub const VE_CHEST_ABOVE_FLOOR: f32 = 1.45;
/// The shell the dust is born on (the bench's `inward`): each mote at
/// 0.6-1.0 of it from the chest, travelling straight in.
pub const VE_IMPLOSION_SHELL: f32 = 1.0;
/// Inward speed, yd/s.
pub const VE_IMPLOSION_SPEED: f32 = 1.67;
pub const VE_IMPLOSION_LIFE: f32 = 0.5;
/// Motes per second, while the emission window is open.
pub const VE_IMPLOSION_RATE: f32 = 80.0;
/// The emission window, seconds from the landing (the bench's 1967 ms).
pub const VE_IMPLOSION_EMIT_SECS: f32 = 1.967;
pub const VE_IMPLOSION_COLOR: [(f32, [u8; 3]); 3] = [
    (0.0, [0, 0, 196]),
    (0.5, [101, 22, 198]),
    (1.0, [212, 5, 195]),
];
pub const VE_IMPLOSION_ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.78)];
/// Sprite RADIUS, yards.
pub const VE_IMPLOSION_SCALE: [(f32, f32); 3] = [(0.0, 0.83), (0.5, 0.42), (1.0, 0.14)];

/// The dust mote's sprite (`tail_dust3`): the bench's `dust`, whose falloff is
/// its smoke's — 0.85 at the centre, 0.5 halfway out.
pub const VE_IMPLOSION_SPRITE: ConflagrateSprite = ConflagrateSprite::Smoke;

// ── The heal stream: vampiricembrace_state_base.m2 ───────────────────────────

/// The stream's emission span, seconds after the heal (the bench's 633 ms).
pub const VE_HEAL_SPAN_SECS: f32 = 0.633;
/// Its rate: 20/s at the heal, 40/s 100 ms on, nothing at 633 ms.
pub const VE_HEAL_RATE: Ramp = Ramp::knotted(20.0, 40.0, 0.0, 0.100 / VE_HEAL_SPAN_SECS);
/// Rising speed, yd/s, within a cone this wide off straight up, radians.
pub const VE_HEAL_SPEED: f32 = 1.4;
pub const VE_HEAL_CONE: f32 = 0.15;
pub const VE_HEAL_LIFE: f32 = 1.1;
/// Side of the square at the feet the motes are born in, yards.
pub const VE_HEAL_AREA: f32 = 0.69;
pub const VE_HEAL_LOOK: HealMoteLook = HealMoteLook {
    color: &[
        (0.0, [47, 0, 179]),
        (0.5, [74, 0, 225]),
        (1.0, [138, 90, 238]),
    ],
    alpha: &[(0.0, 0.9), (1.0, 0.0)],
    scale: &[(0.0, 0.09), (1.0, 0.05)],
};

// ── The implosion's model ────────────────────────────────────────────────────

const SPRITE_PX: u32 = 64;

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// The chest the implosion stands at, for a victim whose rendered body stands
/// as a unit at `translation` would at rest (`body_hold::rendered_root`).
pub fn ve_chest(translation: Vec3, is_pet: bool) -> Vec3 {
    body_point(VE_CHEST_ABOVE_FLOOR, translation, is_pet)
}

/// The rig's whole life: until its last mote, born as the window shuts, has
/// lived out its life.
pub fn ve_implosion_life() -> f32 {
    VE_IMPLOSION_EMIT_SECS + VE_IMPLOSION_LIFE
}

/// Whether the implosion is emitting `age` seconds after the landing.
pub fn ve_implosion_emitting(age: f32) -> bool {
    VAMPIRIC_EMBRACE_IMPLOSION && (0.0..VE_IMPLOSION_EMIT_SECS).contains(&age)
}

/// Where a mote is born, relative to the chest, and how it launches (the
/// bench's `inward` spawn, at spread 1.0), on a body of `stature`.
pub fn ve_implosion_launch(stature: f32, seed: u32) -> (Vec3, Vec3) {
    let d = Vec3::new(
        draw(seed, 1) * 2.0 - 1.0,
        draw(seed, 2) * 2.0 - 1.0,
        draw(seed, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y);
    let r = VE_IMPLOSION_SHELL * stature * (0.6 + 0.4 * draw(seed, 4));
    (d * r, -d * VE_IMPLOSION_SPEED * stature)
}

/// A mote's look `t` of the way through its life: colour, alpha and sprite
/// radius (yards, before the victim's stature).
pub fn ve_implosion_look(t: f32) -> (Color, f32, f32) {
    (
        keyed_color(&VE_IMPLOSION_COLOR, t),
        keyed(&VE_IMPLOSION_ALPHA, t),
        keyed(&VE_IMPLOSION_SCALE, t),
    )
}

// ── Components ───────────────────────────────────────────────────────────────

/// One implosion: it stands at its victim's chest and parents its motes.
#[derive(Component)]
pub struct VampiricEmbraceImplosion {
    pub target: Entity,
    pub age: f32,
    /// Fractional motes owed.
    pub carry: f32,
    pub emitted: u32,
    pub stature: f32,
    /// Whether the victim is a pet: its chest and the body the motes are
    /// held in front of are a pet's.
    pub is_pet: bool,
    quad: Handle<Mesh>,
    sprite: Handle<Image>,
}

/// One dust mote: a child of its implosion, at the point it is DRAWN.
#[derive(Component)]
pub struct VampiricEmbraceMote {
    /// Where the mote truly is, relative to the chest.
    pub offset: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub stature: f32,
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): an implosion for every Vampiric Embrace the sim
/// landed. Only this landing's markers are consumed; a victim that is gone
/// gets nothing.
pub fn spawn_vampiric_embrace_implosions(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Image>)>>,
    landings: Query<(Entity, &AuraLanding)>,
    victims: Query<(&Transform, Option<&Pet>), With<Combatant>>,
) {
    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::VampiricEmbraceImplosion {
            continue;
        }
        commands.entity(marker).despawn();
        let Ok((victim, pet)) = victims.get(landing.target) else {
            continue;
        };
        let (quad, sprite) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Rectangle::new(1.0, 1.0)),
                    images.add(disc_texture(SPRITE_PX, |r, a| {
                        VE_IMPLOSION_SPRITE.alpha_at(r, a)
                    })),
                )
            })
            .clone();
        let is_pet = pet.is_some();
        commands.spawn((
            VampiricEmbraceImplosion {
                target: landing.target,
                age: 0.0,
                carry: 0.0,
                emitted: 0,
                stature: stature(is_pet),
                is_pet,
                quad,
                sprite,
            },
            Transform::from_translation(ve_chest(victim.translation, is_pet)),
            Visibility::default(),
            PlayMatchEntity,
        ));
    }
}

/// Animate (graphical-only): emit each implosion's dust, integrating the rate
/// over the frame so a frame-rate change does not change the density; retire
/// it once its last mote has died, or at once when its victim is gone. Where
/// it stands is [`place_vampiric_embrace_implosions`]'s.
pub fn animate_vampiric_embrace_implosions(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut VampiricEmbraceImplosion)>,
    victims: Query<(), With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (rig_entity, mut rig) in rigs.iter_mut() {
        if victims.get(rig.target).is_err() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        if rig.age >= ve_implosion_life() {
            commands.entity(rig_entity).despawn();
            continue;
        }
        rig.age += dt;
        if !ve_implosion_emitting(rig.age) {
            continue;
        }
        rig.carry += VE_IMPLOSION_RATE * dt;
        while rig.carry >= 1.0 {
            rig.carry -= 1.0;
            let seed = rig_entity
                .index()
                .wrapping_mul(2_654_435_761)
                .wrapping_add(rig.emitted);
            rig.emitted = rig.emitted.wrapping_add(1);
            let (offset, velocity) = ve_implosion_launch(rig.stature, seed);
            let (color, alpha, _) = ve_implosion_look(0.0);
            let mote = commands
                .spawn((
                    VampiricEmbraceMote {
                        offset,
                        velocity,
                        age: 0.0,
                        life: VE_IMPLOSION_LIFE,
                        stature: rig.stature,
                    },
                    Mesh3d(rig.quad.clone()),
                    MeshMaterial3d(materials.add(conflagrate_material(
                        true,
                        color,
                        alpha,
                        Some(rig.sprite.clone()),
                    ))),
                    Transform::from_translation(offset).with_scale(Vec3::splat(1e-4)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig_entity).add_child(mote);
        }
    }
}

/// Age and move every dust mote along its straight line in, and retire it at
/// the end of its life.
pub fn age_vampiric_embrace_motes(
    mut commands: Commands,
    time: Res<Time>,
    mut motes: Query<(Entity, &mut VampiricEmbraceMote)>,
) {
    let dt = time.delta_secs();
    for (entity, mut mote) in motes.iter_mut() {
        mote.age += dt;
        if mote.age >= mote.life {
            commands.entity(entity).despawn();
            continue;
        }
        let step = mote.velocity * dt;
        mote.offset += step;
    }
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each implosion at its victim's RENDERED chest,
/// and place each mote where it is drawn — held in front of that rendered
/// body for the camera (departure 1) — turned to the camera, sized and
/// coloured along its life. The rig is unrotated, so a mote's local pose is
/// its world offset.
#[allow(clippy::type_complexity)]
pub fn place_vampiric_embrace_implosions(
    mut materials: ResMut<Assets<StandardMaterial>>,
    hold: BodyHold,
    mut rigs: Query<
        (&VampiricEmbraceImplosion, &mut Transform, &Children),
        (Without<VampiricEmbraceMote>, HoldDisjoint),
    >,
    mut motes: Query<
        (
            &VampiricEmbraceMote,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<VampiricEmbraceImplosion>, HoldDisjoint),
    >,
) {
    let view = hold.view();
    let cam = view.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let chest = ve_chest(body.root, rig.is_pet);
        rig_transform.translation = chest;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((mote, mut transform, material)) = motes.get_mut(child) else {
                continue;
            };
            let at = chest + mote.offset;
            let (drawn, k) = held_in(view, at, capsule);
            let (color, alpha, radius) = ve_implosion_look((mote.age / mote.life).clamp(0.0, 1.0));
            transform.translation = drawn - chest;
            transform.rotation = rig_transform.rotation.inverse() * cam;
            transform.scale = Vec3::splat((radius * mote.stature * k * 2.0).max(1e-4));
            if let Some(m) = materials.get_mut(&material.0) {
                let c = color.to_linear();
                m.base_color = color.with_alpha(alpha);
                m.emissive = LinearRgba::rgb(
                    c.red * CONFLAGRATE_GLOW,
                    c.green * CONFLAGRATE_GLOW,
                    c.blue * CONFLAGRATE_GLOW,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_lives_until_its_last_mote_dies() {
        assert!((ve_implosion_life() - 2.467).abs() < 1e-6);
        assert!(ve_implosion_emitting(0.0));
        assert!(ve_implosion_emitting(1.966));
        assert!(!ve_implosion_emitting(1.967));
    }

    #[test]
    fn a_mote_is_born_on_the_shell_and_travels_in() {
        for seed in 0..64 {
            let (at, v) = ve_implosion_launch(1.0, seed);
            assert!((0.6 - 1e-5..=1.0 + 1e-5).contains(&at.length()), "{at}");
            assert!((v.length() - VE_IMPLOSION_SPEED).abs() < 1e-4);
            assert!(v.normalize().dot(-at.normalize()) > 0.9999);
        }
    }
}
