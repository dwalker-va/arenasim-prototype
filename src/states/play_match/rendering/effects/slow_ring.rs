use bevy::color::LinearRgba;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use super::gait::gait_is_moving;
use super::movement_trails::emit_along_path;
use super::school_impact::PHYSICAL_COLOR;
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::*;

// ==============================================================================
// Slow Treatment — a bind ring at the feet, only while moving
// ==============================================================================
//
// One treatment for `AuraType::MovementSpeedSlow`, keyed on the aura type and
// never on an ability, so all six sources share it: Frostbolt, Frost Armor's
// chill, Frost Shock, Concussive Shot, Crippling Poison and the Frost Trap zone.
// Research and the decisions behind every choice below:
// `docs/design/2026-09-26-slow-state-client-data.md` ("Decision", "Kit 360").
//
// The spatial grammar it extends: Root holds the feet, Stun hangs over the
// head, so a slow reads as a PARTIAL root at the feet. It is grounded in the
// client's own generic slow state, kit 360 (`slow_impact_base.m2` at the Base
// attachment): a flat ring pulsing at about 1 Hz. Two parts:
//
//   Bind ring -> one flat additive annulus per 1.0s pulse that grows from
//                1.2 to 2.4yd across, holds full alpha to 0.65 of its life,
//                then fades. The client emits ~5 rings per pulse; one reads
//                cleaner. Deliberate departure: the ring FOLLOWS the victim.
//                The client's is world space, and at running speed it is left
//                behind within a fraction of its 0.4s life, reading as
//                detached from the unit rather than as a binding on it.
//   Scuff     -> a low streak along the path, laid by distance walked the way
//                `movement_trails.rs` lays the charge trail, fading over 0.6s.
//                At a fixed lifetime a harder slow leaves a shorter scuff, so
//                its length is itself a cue.
//
// **Sized against the body, not the client's model.** At the client's own
// numbers (0.26 -> 1.53yd, a 0.07yd band, 0.395s) the ring was barely visible
// in play, and it was geometry, not colour: the capsule is 0.5yd in radius, so
// the ring spent the first half of its life INSIDE the body's footprint, and
// from the default camera (51 degrees up) the body hides the ring's far half
// at any radius under ~1yd. What was left was a 1-2px near-side arc for ~0.1s
// a second. So the ring is born with its band on the capsule's edge and grows
// to clear the body's silhouette either side; the band matches the selection
// ring's (`selection.rs`), the ground ring that already reads at play zoom;
// and a ring lives 0.7s, the span the client's own pulse is live (its 0-333ms
// burst plus one 0.395s ring life), so the 1 Hz beat still has a gap. Alpha and
// emissive gain are unchanged: the tints already add about as much luminance
// as the sand has, so contrast was never the missing part.
//
// **Only while moving.** A stationary slowed unit shows nothing — the aura icon
// carries the debuff, as it does for any stationary unit — so the treatment
// claims no sustained on-victim channel. "Moving" is the gait's own definition
// (`gait_is_moving`), not a second one.
//
// It never touches the body: no tint, no material swap, no gait change. The
// client's chill tint would contend for `OriginalBodyMaterial` with Fear and
// Polymorph, and Frostbolt into Fear is routine. The walk bob already slows
// with the unit (it is paced by distance); this sits under it and does not
// fight it.
//
// Crippling Poison's proc is the one slow with no hit of its own to carry its
// apply moment, so its fresh application gets the shared Nature landing at the
// chest — the client plays a one-shot `poison_impact_chest.m2` there.
//
// Graphical-only: registered in `states/mod.rs` alone. No `game_rng` draw, no
// sim write — the only components it inserts are its own emitter and its own
// visuals — so headless stays byte-identical by construction.

// ------------------------------------------------------------------------------
// Constants — Slow Trail Bench defaults (signed off 2026-09-26), except the
// ring's size, band and lifetime, re-sized against the body (header)
// ------------------------------------------------------------------------------

/// Seconds between bind-ring pulses (client: the 1000ms Stand loop).
pub const SLOW_RING_PERIOD: f32 = 1.0;
/// Seconds one ring lives: the span the client's pulse is live in each loop
/// (a 0-333ms burst of 0.395s rings), not one client ring's life.
pub const SLOW_RING_LIFETIME: f32 = 0.7;
/// Ring diameter at birth and at death, yards. Born with its band's inner
/// edge on the 0.5yd capsule; dies clear of the body's silhouette either side.
pub const SLOW_RING_DIAMETER_START: f32 = 1.2;
pub const SLOW_RING_DIAMETER_END: f32 = 2.4;
/// Fraction of its life a ring holds full alpha before the fade begins.
pub const SLOW_RING_FADE_FROM: f32 = 0.65;
/// Width of the ring's core band, yards: the selection ring's 0.2yd tube
/// (`selection.rs`), the ground ring that already reads at play zoom. Constant
/// as the ring grows: it is a stroke, not a scaled disc.
pub const SLOW_RING_BAND_WIDTH: f32 = 0.2;
const SLOW_RING_ALPHA: f32 = 0.8;
/// The soft edge, drawn as the bench drew it: a second, wider stroke under the
/// core at a quarter of its alpha.
const SLOW_RING_HALO_WIDTH: f32 = 2.2;
const SLOW_RING_HALO_ALPHA: f32 = 0.25;
const SLOW_RING_EMISSIVE: f32 = 1.5;
const SLOW_RING_RESOLUTION: u32 = 48;

/// Scuff streak width, lifetime and opacity.
pub const SLOW_SCUFF_WIDTH: f32 = 0.22;
pub const SLOW_SCUFF_LIFETIME: f32 = 0.6;
const SLOW_SCUFF_ALPHA: f32 = 0.45;
const SLOW_SCUFF_EMISSIVE: f32 = 0.8;
/// Yards walked per scuff segment. Each segment spans exactly this, so
/// adjacent segments meet end to end without overlapping (overlap would
/// double-brighten an additive streak at every joint).
pub const SLOW_SCUFF_SPACING: f32 = 0.15;
/// A scuff narrows to this fraction of its width as it fades.
const SLOW_SCUFF_END_WIDTH: f32 = 0.5;

/// Fixed WORLD heights, not derived from the unit (the floor is y=0; a
/// combatant sims at 1.0 and a pet at 0.75). The ring sits just above the
/// scuff, and both clear the Frost Trap zone's disc.
pub const SLOW_RING_Y: f32 = 0.05;
pub const SLOW_SCUFF_Y: f32 = 0.04;

/// Uniform scale on a pet's ring and scuff. Not the `hard_cc.rs` 0.55: that
/// would bear a pet's ring inside its body. The pet is a horizontal
/// `Capsule3d::new(0.35, 0.6)` — 0.35yd to its flank, 0.65yd to its snout — so
/// 0.8 is born clear of the flank and clears the snout by mid-life.
pub const SLOW_PET_STATURE: f32 = 0.8;

// ==============================================================================
// Routing
// ==============================================================================

/// The tint family a slow is drawn in.
///
/// Routed off the aura's `ability_name`, NOT `Aura::spell_school`: that field is
/// the school for DAMAGE, and its `None` covers physical and schoolless alike,
/// so the Frost Trap zone's slow would come out hueless.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlowTint {
    /// The chill: Frostbolt, Frost Armor, Frost Shock, the Frost Trap zone.
    Frost,
    /// The daze: Concussive Shot. The client's daze is hueless.
    Physical,
    /// Crippling Poison. The client gives the poison no state colour of its
    /// own, so green comes from the Nature school.
    Nature,
}

impl SlowTint {
    /// The tint for a `MovementSpeedSlow` aura named `name`, or `None` when
    /// nothing draws it. `None` is what `tests/lands_silently_audit.rs` fails
    /// on for every slow declared in `abilities.ron`, and
    /// `tests/slow_ring_visual_probes.rs` pins the code-applied ones (Frost
    /// Armor's chill, and the Frost Trap zone's slow as `slow_zone_system`
    /// actually stamps it), so a new slow has to be given an arm here.
    pub fn for_slow(name: &str) -> Option<SlowTint> {
        match name {
            "Frostbolt" | "Frost Armor" | "Frost Shock" | "Frost Trap" => Some(SlowTint::Frost),
            "Concussive Shot" => Some(SlowTint::Physical),
            "Crippling Poison" => Some(SlowTint::Nature),
            _ => None,
        }
    }

    /// The colour, through the school authority. Physical takes the
    /// school-impact tier's bone-white rather than its own tan, which sits
    /// within a few percent of the arena floor (`school_impact.rs`,
    /// divergence 1).
    pub fn color(self) -> Color {
        match self {
            SlowTint::Frost => SpellSchool::Frost.color(),
            SlowTint::Physical => PHYSICAL_COLOR,
            SlowTint::Nature => SpellSchool::Nature.color(),
        }
    }
}

/// The aura name of Crippling Poison's slow — the one slow with a proc flash.
const CRIPPLING_POISON_SLOW: &str = "Crippling Poison";

/// The routed slow a unit is drawn with: the hardest one it carries (the
/// lowest speed multiplier), first in aura order on a tie.
pub fn drawn_slow(auras: &ActiveAuras) -> Option<(SlowTint, &Aura)> {
    auras
        .auras
        .iter()
        .filter(|a| a.effect_type == AuraType::MovementSpeedSlow)
        .filter_map(|a| SlowTint::for_slow(&a.ability_name).map(|t| (t, a)))
        .fold(None, |best: Option<(SlowTint, &Aura)>, cand| match best {
            Some(b) if b.1.magnitude <= cand.1.magnitude => Some(b),
            _ => Some(cand),
        })
}

// ==============================================================================
// Pure seams
// ==============================================================================

/// The ring's diameter at `age`, before stature: linear from start to end
/// across its life.
pub fn slow_ring_diameter(age: f32) -> f32 {
    let k = (age / SLOW_RING_LIFETIME).clamp(0.0, 1.0);
    SLOW_RING_DIAMETER_START + (SLOW_RING_DIAMETER_END - SLOW_RING_DIAMETER_START) * k
}

/// The ring's alpha envelope at `age`: full to [`SLOW_RING_FADE_FROM`], then a
/// linear fade to nothing at the end of its life.
pub fn slow_ring_fade(age: f32) -> f32 {
    let k = (age / SLOW_RING_LIFETIME).clamp(0.0, 1.0);
    if k < SLOW_RING_FADE_FROM {
        1.0
    } else {
        1.0 - (k - SLOW_RING_FADE_FROM) / (1.0 - SLOW_RING_FADE_FROM)
    }
}

/// Inner and outer radius of one stroke of the ring at `age`.
fn ring_stroke(age: f32, stature: f32, halo: bool) -> (f32, f32) {
    let r = 0.5 * slow_ring_diameter(age) * stature;
    let width = if halo {
        SLOW_RING_BAND_WIDTH * SLOW_RING_HALO_WIDTH
    } else {
        SLOW_RING_BAND_WIDTH
    };
    ((r - 0.5 * width).max(0.0), r + 0.5 * width)
}

fn ring_mesh(inner: f32, outer: f32) -> Mesh {
    Annulus::new(inner, outer)
        .mesh()
        .resolution(SLOW_RING_RESOLUTION)
        .build()
}

fn glow(tint: Color, alpha: f32, gain: f32) -> (Color, LinearRgba) {
    let lin = tint.to_linear();
    let k = gain * alpha;
    (
        tint.with_alpha(alpha),
        LinearRgba::new(lin.red * k, lin.green * k, lin.blue * k, 1.0),
    )
}

fn ring_alpha(halo: bool) -> f32 {
    if halo {
        SLOW_RING_ALPHA * SLOW_RING_HALO_ALPHA
    } else {
        SLOW_RING_ALPHA
    }
}

// ==============================================================================
// Spawning
// ==============================================================================

/// 2D primitives mesh into XY facing +Z; -90 degrees about X lays them flat
/// with their normal up.
fn lay_flat() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

fn spawn_bind_ring(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    owner: Entity,
    at: Vec3,
    stature: f32,
    tint: Color,
) {
    // Halo first, then the core over it — both additive, as the bench drew them.
    for halo in [true, false] {
        let (inner, outer) = ring_stroke(0.0, stature, halo);
        let (base_color, emissive) = glow(tint, ring_alpha(halo), SLOW_RING_EMISSIVE);
        commands.spawn((
            Mesh3d(meshes.add(ring_mesh(inner, outer))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color,
                emissive,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(Vec3::new(at.x, SLOW_RING_Y, at.z))
                .with_rotation(lay_flat()),
            SlowBindRing {
                owner,
                age: 0.0,
                stature,
                tint,
                halo,
            },
            PlayMatchEntity,
        ));
    }
}

fn spawn_scuff(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mid: Vec3,
    dir: Vec3,
    stature: f32,
    tint: Color,
) {
    let (base_color, emissive) = glow(tint, SLOW_SCUFF_ALPHA, SLOW_SCUFF_EMISSIVE);
    // Yaw local +X onto the travel direction, then lay the quad flat: its
    // length runs along the path and its width across it.
    let yaw = (-dir.z).atan2(dir.x);
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        })),
        Transform {
            translation: Vec3::new(mid.x, SLOW_SCUFF_Y, mid.z),
            rotation: Quat::from_rotation_y(yaw) * lay_flat(),
            scale: Vec3::new(1.0, stature, 1.0),
        },
        SlowScuff {
            age: 0.0,
            stature,
            tint,
        },
        PlayMatchEntity,
    ));
}

// ==============================================================================
// Systems
// ==============================================================================

/// Owns each slowed victim's [`SlowTrailEmitter`]: pulses the bind ring and
/// lays the scuff while the victim moves, and fires Crippling Poison's proc
/// flash on a fresh application.
///
/// `ActiveAuras` is OPTIONAL because `update_auras` removes the component once
/// the last aura expires, and a query requiring it would never see the slow
/// end. `is_alive()` is folded in because a killing blow leaves the aura
/// ticking out on the corpse. `Without<Celebrating>` because the winners'
/// bounce owns the gait there and leaves its idle clock stale.
pub fn update_slow_treatment(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Every scuff segment is the same flat quad; its fade lives in the
    // material, so one mesh serves them all.
    mut scuff_mesh: Local<Option<Handle<Mesh>>>,
    mut victims: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            &WalkAnim,
            Option<&ActiveAuras>,
            Option<&mut SlowTrailEmitter>,
            Option<&Pet>,
        ),
        Without<Celebrating>,
    >,
    positions: Query<&Transform>,
) {
    let dt = time.delta_secs();
    let scuff_mesh = scuff_mesh
        .get_or_insert_with(|| meshes.add(Rectangle::new(SLOW_SCUFF_SPACING, SLOW_SCUFF_WIDTH)))
        .clone();

    for (entity, combatant, transform, walk, auras, emitter, pet) in victims.iter_mut() {
        let drawn = auras.filter(|_| combatant.is_alive()).and_then(drawn_slow);
        let Some((tint, _)) = drawn else {
            if emitter.is_some() {
                commands.entity(entity).remove::<SlowTrailEmitter>();
            }
            continue;
        };
        let tint = tint.color();
        let crippling = auras.and_then(|a| {
            a.auras.iter().find(|au| {
                au.effect_type == AuraType::MovementSpeedSlow
                    && au.ability_name == CRIPPLING_POISON_SLOW
            })
        });

        let ground = Vec3::new(transform.translation.x, 0.0, transform.translation.z);
        let stature = if pet.is_some() { SLOW_PET_STATURE } else { 1.0 };

        let mut fresh = SlowTrailEmitter {
            last_emit: ground,
            // Armed, so a unit already on the move pulses on its first frame.
            since_pulse: SLOW_RING_PERIOD,
            crippled: false,
        };
        let is_new = emitter.is_none();
        let state: &mut SlowTrailEmitter = match emitter {
            Some(em) => em.into_inner(),
            None => &mut fresh,
        };

        // ---- Crippling Poison: the proc flash, on the edge only ----
        if let Some(aura) = crippling {
            if !state.crippled {
                let from = aura
                    .caster
                    .and_then(|c| positions.get(c).ok())
                    .map(|c| c.translation - transform.translation)
                    .and_then(|d| Vec3::new(d.x, 0.0, d.z).try_normalize())
                    .unwrap_or(Vec3::Z);
                commands.spawn((
                    SchoolImpact {
                        target: entity,
                        ability: AbilityType::CripplingPoison,
                        school: SpellSchool::Nature,
                        anchor: ImpactAnchor::Chest,
                        from,
                        magnitude: 0.0,
                        is_crit: false,
                        age: 0.0,
                    },
                    PlayMatchEntity,
                ));
            }
        }
        state.crippled = crippling.is_some();

        // ---- Only while moving ----
        state.since_pulse += dt;
        if gait_is_moving(walk) {
            if state.since_pulse >= SLOW_RING_PERIOD {
                state.since_pulse = 0.0;
                spawn_bind_ring(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    entity,
                    ground,
                    stature,
                    tint,
                );
            }
            emit_along_path(
                &mut state.last_emit,
                ground,
                SLOW_SCUFF_SPACING * stature,
                |mid, dir| {
                    spawn_scuff(
                        &mut commands,
                        scuff_mesh.clone(),
                        &mut materials,
                        mid,
                        dir,
                        stature,
                        tint,
                    );
                },
            );
        } else {
            // A stop never bridges: the next step lays from where it resumed.
            state.last_emit = ground;
        }

        if is_new {
            commands.entity(entity).try_insert(fresh);
        }
    }
}

/// Grows, fades and follows each bind ring; despawns it at the end of its life.
///
/// The band is rebuilt each frame rather than scaled, because scaling an
/// annulus widens its band with it and the band is a constant-width stroke.
/// `Without<Combatant>` keeps the two `Transform` queries disjoint at Bevy's
/// static access check (the `update_cc_rigs` idiom).
pub fn update_slow_rings(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rings: Query<
        (
            Entity,
            &mut SlowBindRing,
            &mut Transform,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<Combatant>,
    >,
    owners: Query<&Transform, With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut ring, mut transform, mesh, material) in rings.iter_mut() {
        ring.age += dt;
        if ring.age >= SLOW_RING_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        // Follows the victim; if it is gone, the ring finishes where it was.
        if let Ok(owner) = owners.get(ring.owner) {
            transform.translation.x = owner.translation.x;
            transform.translation.z = owner.translation.z;
        }
        let (inner, outer) = ring_stroke(ring.age, ring.stature, ring.halo);
        if let Some(m) = meshes.get_mut(&mesh.0) {
            *m = ring_mesh(inner, outer);
        }
        if let Some(m) = materials.get_mut(&material.0) {
            let (base_color, emissive) = glow(
                ring.tint,
                ring_alpha(ring.halo) * slow_ring_fade(ring.age),
                SLOW_RING_EMISSIVE,
            );
            m.base_color = base_color;
            m.emissive = emissive;
        }
    }
}

/// Fades and narrows each scuff segment; despawns it at the end of its life.
pub fn update_slow_scuffs(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scuffs: Query<(
        Entity,
        &mut SlowScuff,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut scuff, mut transform, material) in scuffs.iter_mut() {
        scuff.age += dt;
        if scuff.age >= SLOW_SCUFF_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let k = scuff.age / SLOW_SCUFF_LIFETIME;
        // Narrow across the path only: local Y, after the lay-flat.
        transform.scale.y = scuff.stature * (1.0 - (1.0 - SLOW_SCUFF_END_WIDTH) * k);
        if let Some(m) = materials.get_mut(&material.0) {
            let (base_color, emissive) = glow(
                scuff.tint,
                SLOW_SCUFF_ALPHA * (1.0 - k),
                SLOW_SCUFF_EMISSIVE,
            );
            m.base_color = base_color;
            m.emissive = emissive;
        }
    }
}
