//! Victim hit reactions for auto-attacks (graphical-only).
//!
//! The highest-frequency visual in the game: one reaction per landed auto, on
//! every victim, for the whole match. Everything here is deliberately quiet —
//! see the BUDGET note on the constants.
//!
//! Two pieces, with different trigger sets:
//!
//! * **The flinch** fires on EVERY landed auto — melee, pet melee, wand hit and
//!   Hunter auto-shot arrival. This is the client's generic hit-react path
//!   (`CombatWound`, anim 9): no DB2 row commands the melee flinch, but the bow
//!   and thrown impact kits explicitly command anim 9, so a ranged-auto flinch
//!   is client-faithful. Research §1.1, §2.5.
//! * **The impact burst** fires on MELEE autos only (players and pets). The
//!   client's wand SpellVisuals carry zero impact rows, so a wand hit showing
//!   only the generic flinch is the faithful result and a bespoke wand impact
//!   burst is not (research §2.5, constraint 5). The Hunter's arrow likewise
//!   gets no bespoke arrival flash — its bow impact kit is sound + CombatWound
//!   and nothing else.
//!
//! **The impact burst is a WEAPON SPARK, not blood — a deliberate,
//! user-directed deviation from the client.** Classic shows `bloodspurt.m2`,
//! but red-on-victim is already Rend's bleed-drip channel here
//! (`affliction.rs`, `DripKind::Bleed` at an opaque `(0.50, 0.03, 0.03)`), and
//! at auto-attack frequency the two channels would collide. Metallic
//! warm-white sparks read as WEAPON impact and leave the bleed vocabulary
//! alone. Do not "restore faithfulness" here; the deviation is the design.
//!
//! All randomness is a self-contained visual-only hash, never the sim
//! `game_rng`, and every system here is registered in `states/mod.rs` only, so
//! headless stays byte-identical.

use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::school_impact::impact_origin;
use super::wand_attack::{spawn_wand_missile, wand_muzzle, wand_school};
use crate::states::play_match::components::*;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

// --- The blessed spec -------------------------------------------------------
//
// Constant NAMES are the AS-132 workshop's emitted names and are part of the
// spec; the values are the blessed workshop defaults (2026-09-21).
//
// BUDGET: this is the highest-frequency visual in the game, and the blessed
// values are deliberately quiet. Do not "improve" the intensities.

/// How the victim reacts: a Y-only compression of the `VisualBody` child.
///
/// Named as the spec names it. A dip is chosen over a tilt or a recoil
/// because Y is the channel the gait writers already own and rewrite
/// absolutely every frame, so a dip is cleaned up for free the frame it
/// expires — a lateral or rotational reaction would have to restore itself
/// explicitly, and a unit that is feared or polymorphed mid-reaction would be
/// left holding the offset (`gait.rs`, the panic-tremble note / plan KTD2).
pub const FLINCH_MODE: FlinchMode = FlinchMode::Dip;

/// Total duration of the dip.
///
/// A third of the client's 1000 ms `CombatWound` on the human rig. The full
/// second is an animation that also carries a torso and limbs; the residual
/// here is a single scalar, and holding one for a second reads as a limp
/// rather than a flinch.
pub const FLINCH_DURATION_SECS: f32 = 0.35;

/// Fraction of the duration spent compressing. Matches the client's 150 ms
/// blend-in over the 1000 ms wound anim: fast in, slow out.
pub const FLINCH_RISE_FRAC: f32 = 0.15;

/// Peak downward displacement, in arena units. The walk bob's amplitude is
/// 0.10 (`gait.rs`), so a hit reads as exactly one bob's worth of compression.
pub const FLINCH_DIP: f32 = 0.10;

/// Depth multiplier on a crit — the client's separate `CombatCritical` anim
/// (id 10), which falls back to `CombatWound` rather than replacing it.
pub const FLINCH_CRIT_MULT: f32 = 1.8;

/// Duration scale when the VICTIM is a pet. Wound durations are authored per
/// rig: `wolf.m2`'s `CombatWound` is 667 ms against humanmale's 1000 ms
/// (research §3), so a beast's flinch is genuinely snappier.
pub const PET_FLINCH_DURATION_SCALE: f32 = 0.67;

/// What a melee impact throws off. Named as the spec names it; see the
/// module header for why this is not blood.
pub const MELEE_IMPACT_STYLE: MeleeImpactStyle = MeleeImpactStyle::WeaponSpark;

/// Flecks per burst. Below Mortal Strike's 14, which is the adjacency this
/// effect has to stay under (see `mortal_strike.rs`).
pub const SPARK_COUNT: u32 = 10;

/// Length of one spark streak along its own flight, in arena units. Under half
/// Mortal Strike's 0.13.
pub const SPARK_SIZE: f32 = 0.06;

/// Initial speed, in arena units per second. Roughly half Mortal Strike's 7.5,
/// so the debris stays close to the contact point instead of spraying.
pub const SPARK_BURST_SPEED: f32 = 4.0;

/// How long a fleck lives before it is gone.
pub const SPARK_LIFETIME_SECS: f32 = 0.30;

/// Downward acceleration on the flecks, in arena units per second squared.
pub const SPARK_GRAVITY: f32 = 10.0;

/// Brightness of the contact flash — the client's additive glowball/starflash
/// accents (`bloodspurt.m2` emitters 1–3), recolored metallic.
pub const SPARK_FLASH_INTENSITY: f32 = 0.7;

/// Size multiplier on a crit — the client's `[normal, large]` spurt pair
/// (research §1.3).
pub const SPARK_CRIT_SCALE: f32 = 1.5;

/// Spatial scale for a burst on a PET victim, so the flourish matches the
/// smaller body.
pub const PET_SPARK_SPATIAL_SCALE: f32 = 0.75;

// --- Derived shape knobs (not from the bench) -------------------------------

/// Cross-section of a spark streak as a fraction of its length. A fleck is a
/// BAND stretched along its velocity, never a round sprite (house amendment).
const SPARK_ASPECT: f32 = 0.28;

/// Radius and life of the contact flash.
///
/// Both must stay well under `SPARK_BURST_SPEED * SPARK_FLASH_LIFETIME_SECS`
/// (0.32), or the flecks spend their visible phase submerged in an additive
/// ball and the hit reads as a glow rather than as debris — the failure
/// `mortal_strike.rs` documents at `FLASH_RADIUS`. Both are also well under
/// Mortal Strike's 0.38 / 0.11: an auto must not out-flash a signature.
const SPARK_FLASH_RADIUS: f32 = 0.14;
const SPARK_FLASH_LIFETIME_SECS: f32 = 0.08;

/// Metallic warm-white. Clear of Rend's opaque blood red `(0.50, 0.03, 0.03)`
/// and of Mortal Strike's crimson `(0.72, 0.10, 0.06)` by being essentially
/// unsaturated.
const SPARK_BASE_COLOR: (f32, f32, f32) = (1.0, 0.94, 0.82);
const SPARK_EMISSIVE: (f32, f32, f32) = (2.2, 1.9, 1.35);

// --- The Mortal Strike adjacency guard --------------------------------------
//
// `mortal_strike.rs` already owns "a short impact flash and struck-metal
// sparks at the contact point", and the research rated an auto-attack spark a
// HIGH adjacency risk against it: an auto that reads like the signature makes
// the signature ordinary. What keeps them apart is that Mortal Strike has a
// crimson WEAPON TRAIL (its own header names the trail as the signature) and
// this has none — plus warm-white against crimson, and a burst quieter on
// every axis.
//
// That second half is enforced here rather than left to review. It is a
// COMPILE-TIME assertion, not a test, because the failure it guards is
// someone raising these values a little at a time: a compile error stops that
// at the edit, and a compile error is also the right answer to the card's
// "do not improve the intensities" instruction. Mortal Strike's constants are
// private to its module, so they are restated as the ceiling this effect must
// stay under; if that module is ever quietened below these, THIS is the
// comment to revisit.
const MS_SPARK_COUNT: u32 = 14;
const MS_SPARK_SPEED: f32 = 7.5;
const MS_SPARK_LENGTH: f32 = 0.13;
const MS_FLASH_RADIUS: f32 = 0.38;
const MS_FLASH_LIFETIME: f32 = 0.11;
const _: () = {
    assert!(SPARK_COUNT < MS_SPARK_COUNT);
    assert!(SPARK_BURST_SPEED < MS_SPARK_SPEED);
    assert!(SPARK_SIZE < MS_SPARK_LENGTH);
    assert!(SPARK_FLASH_RADIUS < MS_FLASH_RADIUS);
    assert!(SPARK_FLASH_LIFETIME_SECS < MS_FLASH_LIFETIME);
};

/// Per-spark speed varies in `SPARK_BURST_SPEED * [MIN, MIN + SPAN]`. The floor
/// is well above zero for the same reason as Mortal Strike's: a near-stationary
/// fleck would sit inside the flash for its whole life.
const SPARK_SPEED_MIN: f32 = 0.7;
const SPARK_SPEED_SPAN: f32 = 0.6;

/// How far the burst's axis tilts UP from the horizontal bearing back toward
/// the attacker, as a rise over that horizontal run. Struck metal sprays back
/// at the striker and upward, not in a symmetric ball.
const SPARK_AXIS_UP_BIAS: f32 = 0.55;

/// Half-angle of the spray cone about that axis, in radians (~60°). Wide
/// enough to read as a burst, and — with the axis pointing AWAY from the
/// victim — narrow enough that no fleck is thrown back through the body it
/// just came off. `sparks_stay_clear_of_the_victims_silhouette` pins that.
const SPARK_CONE_HALF_ANGLE: f32 = 1.05;

// --- Spec-named modes -------------------------------------------------------

/// How a victim's body reacts to a hit. One variant today; the type exists so
/// [`FLINCH_MODE`] can name the blessed choice rather than leave it implicit
/// in the code that happens to be written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlinchMode {
    /// Y-only compression of the `VisualBody` child.
    Dip,
}

/// What a melee contact throws off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeleeImpactStyle {
    /// Metallic warm-white spark streaks plus a brief additive contact flash.
    WeaponSpark,
}

// --- Runtime components (graphical-only) ------------------------------------

/// One metallic fleck. A transient, unattached world particle with ballistic
/// motion that self-expires — the physics-lite debris recipe, shared with
/// `mortal_strike.rs`.
#[derive(Component)]
pub struct HitSpark {
    velocity: Vec3,
    lifetime: f32,
    initial_lifetime: f32,
}

/// The additive contact flash at the impact point.
#[derive(Component)]
pub struct HitFlash {
    lifetime: f32,
    initial_lifetime: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
}

// --- The flinch curve -------------------------------------------------------

/// The dip a live [`HitFlinch`] contributes right now, as a SIGNED local-Y
/// offset (negative — the body compresses downward).
///
/// Pure, so the shape is unit-testable without a world, and called from
/// `apply_gait_offset` so the composed write stays in one place. Returns
/// exactly `0.0` at `elapsed >= duration`, which means the reaction is already
/// visually finished by the time `cleanup_hit_flinch` removes the component —
/// there is no frame where removal itself moves the body.
pub fn hit_flinch_offset(flinch: &HitFlinch) -> f32 {
    if flinch.duration <= 0.0 {
        return 0.0;
    }
    let t = (flinch.elapsed / flinch.duration).clamp(0.0, 1.0);
    // Fast in over the rise fraction, slow out over the remainder. The
    // recovery is eased (squared) so the body settles rather than snapping
    // back through rest.
    let shape = if t < FLINCH_RISE_FRAC {
        t / FLINCH_RISE_FRAC.max(f32::EPSILON)
    } else {
        let k = (1.0 - t) / (1.0 - FLINCH_RISE_FRAC).max(f32::EPSILON);
        let k = k.clamp(0.0, 1.0);
        k * k
    };
    -flinch.depth * shape
}

// --- Spawn ------------------------------------------------------------------

/// FixedUpdate (graphical-only): turn the sim's landed-attack markers into
/// victim reactions.
///
/// Shares [`AutoAttackSwing`] with `consume_swing_signals`, which DESPAWNS the
/// marker — so this is ordered `.before` it in `states/mod.rs`. FixedUpdate for
/// the same reason that consumer is: several ticks can fall inside one rendered
/// frame, and a reaction deferred to `Update` would collapse a focus-fire
/// flurry into a single dip.
///
/// Reads the victim's LIVE transform. The sim already resolved damage against
/// that position, and the burst is sited there rather than tracked, so a unit
/// that walks on does not drag its own sparks along.
pub fn consume_hit_reactions(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<&AutoAttackSwing>,
    victims: Query<(&Transform, Option<&Pet>, Option<&HitFlinch>), With<Combatant>>,
    attackers: Query<(&Transform, &Combatant)>,
    sockets: Query<(&WeaponSocket, &GlobalTransform)>,
) {
    for signal in signals.iter() {
        let Ok((victim_tf, victim_pet, live_flinch)) = victims.get(signal.target) else {
            continue;
        };
        let is_pet = victim_pet.is_some();

        // --- the flinch: every landed auto, whatever the weapon ------------
        let duration = FLINCH_DURATION_SECS
            * if is_pet {
                PET_FLINCH_DURATION_SCALE
            } else {
                1.0
            };
        let depth = FLINCH_DIP
            * if signal.is_crit {
                FLINCH_CRIT_MULT
            } else {
                1.0
            };
        // Refresh, never stack: a second hit inside the window restarts the
        // dip, and starting it no shallower than where the body already is
        // keeps the compression continuous. Without the floor, a normal hit
        // landing mid-crit-dip would pop the body UPWARD — the one artifact
        // focus fire would produce constantly.
        let depth = depth.max(live_flinch.map_or(0.0, |f| hit_flinch_offset(f).abs()));
        commands.entity(signal.target).insert(HitFlinch {
            elapsed: 0.0,
            duration,
            depth,
        });

        // --- the impact burst: MELEE only (client-faithful) ----------------
        if signal.kind == AutoAttackKind::Melee {
            let Ok((attacker_tf, _)) = attackers.get(signal.attacker) else {
                continue;
            };
            let (impact, outward) =
                impact_point(victim_tf.translation, attacker_tf.translation, is_pet);
            spawn_impact_burst(
                &mut commands,
                &mut meshes,
                &mut materials,
                signal.target,
                impact,
                outward,
                signal.is_crit,
                if is_pet { PET_SPARK_SPATIAL_SCALE } else { 1.0 },
            );
        }

        // --- the wand missile: a cosmetic bolt, no impact of its own -------
        if signal.kind == AutoAttackKind::Wand {
            let Ok((attacker_tf, attacker)) = attackers.get(signal.attacker) else {
                continue;
            };
            let aim = impact_origin(ImpactAnchor::Chest, victim_tf.translation, is_pet);
            spawn_wand_missile(
                &mut commands,
                &mut meshes,
                &mut materials,
                wand_school(attacker.class),
                wand_muzzle(signal.attacker, attacker_tf.translation, aim, &sockets),
                aim,
            );
        }
    }
}

/// Where a melee contact plays: the victim's chest anchor, pushed out along
/// the horizontal bearing to the ATTACKER so the burst sits on the struck side
/// of the silhouette rather than inside it.
///
/// The standoff clears `COMBATANT_BODY_RADIUS` plus a whole streak length, so
/// even a fleck thrown straight back at the attacker starts outside the body.
/// It is NOT scaled down for a pet victim: a pet's capsule is smaller, so the
/// combatant radius already clears it, and shrinking the standoff would only
/// move the burst toward the body it has to stay clear of.
fn impact_point(victim: Vec3, attacker: Vec3, is_pet: bool) -> (Vec3, Vec3) {
    let chest = impact_origin(ImpactAnchor::Chest, victim, is_pet);
    let bearing = (attacker - victim).with_y(0.0);
    // Two units at the same XZ (not reachable in the sim, but cheap to make
    // total) leave the burst on the chest anchor, spraying straight up,
    // rather than at NaN.
    let out = bearing.normalize_or_zero();
    if out == Vec3::ZERO {
        return (chest, Vec3::Y);
    }
    (chest + out * (COMBATANT_BODY_RADIUS + SPARK_SIZE), out)
}

/// A unit direction `theta` off `axis`, rotated `phi` about it.
///
/// `u` and `v` must be unit vectors spanning the plane perpendicular to
/// `axis`; the caller builds them once per burst.
fn cone_direction(axis: Vec3, u: Vec3, v: Vec3, theta: f32, phi: f32) -> Vec3 {
    (axis * theta.cos() + (u * phi.cos() + v * phi.sin()) * theta.sin()).normalize_or_zero()
}

/// Cheap deterministic jitter in `[0, 1)` from a seed. Mirrors
/// `mortal_strike::spark_jitter` — visual-only, so it never perturbs the sim's
/// seeded `GameRng` and headless stays byte-identical.
fn spark_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// The contact beat: a tight additive flash plus metallic streaks.
///
/// `seed_source` only varies the jitter, so two attackers striking the same
/// victim on the same tick do not throw identical debris.
#[allow(clippy::too_many_arguments)]
fn spawn_impact_burst(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    seed_source: Entity,
    impact: Vec3,
    outward: Vec3,
    is_crit: bool,
    spatial_scale: f32,
) {
    let scale = spatial_scale * if is_crit { SPARK_CRIT_SCALE } else { 1.0 };
    let (br, bg, bb) = SPARK_BASE_COLOR;
    let (er, eg, eb) = SPARK_EMISSIVE;

    // --- contact flash -----------------------------------------------------
    let flash_material = materials.add(StandardMaterial {
        base_color: Color::srgb(br, bg, bb),
        emissive: LinearRgba::new(
            er * SPARK_FLASH_INTENSITY,
            eg * SPARK_FLASH_INTENSITY,
            eb * SPARK_FLASH_INTENSITY,
            1.0,
        ),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    commands.spawn((
        HitFlash {
            lifetime: SPARK_FLASH_LIFETIME_SECS,
            initial_lifetime: SPARK_FLASH_LIFETIME_SECS,
            radius: SPARK_FLASH_RADIUS * scale,
            material: flash_material.clone(),
        },
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(flash_material),
        Transform::from_translation(impact).with_scale(Vec3::splat(0.01)),
        NotShadowCaster,
        PlayMatchEntity,
    ));

    // --- streaks -----------------------------------------------------------
    let spark_mesh = meshes.add(Cuboid::new(
        SPARK_SIZE * SPARK_ASPECT,
        SPARK_SIZE * SPARK_ASPECT,
        SPARK_SIZE,
    ));
    let spark_material = materials.add(StandardMaterial {
        base_color: Color::srgb(br, bg, bb),
        emissive: LinearRgba::new(er, eg, eb, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    // The spray axis and an orthonormal pair spanning the plane across it.
    let axis = (outward + Vec3::Y * SPARK_AXIS_UP_BIAS).normalize_or_zero();
    let axis = if axis == Vec3::ZERO { Vec3::Y } else { axis };
    let basis_u = axis.any_orthonormal_vector();
    let basis_v = axis.cross(basis_u).normalize();

    // A crit throws the same flecks BIGGER and FASTER, not more of them: the
    // count is what sets the concurrency budget under focus fire, and letting
    // crits raise it is how a quiet effect becomes mush at three attackers.
    for i in 0..SPARK_COUNT {
        let seed = seed_source
            .index()
            .wrapping_mul(31)
            .wrapping_add(i.wrapping_mul(2_654_435_761));
        let j1 = spark_jitter(seed);
        let j2 = spark_jitter(seed.wrapping_add(7));
        let j3 = spark_jitter(seed.wrapping_add(19));
        // A cone about the axis, which points back at the striker and tilts
        // up: struck metal sprays OFF the body, never through it. `sqrt` on
        // the polar draw spreads the flecks evenly over the cone's cap rather
        // than bunching them on its axis.
        let theta = SPARK_CONE_HALF_ANGLE * j1.sqrt();
        let phi = j2 * std::f32::consts::TAU;
        let dir = cone_direction(axis, basis_u, basis_v, theta, phi);
        let speed = SPARK_BURST_SPEED * (SPARK_SPEED_MIN + SPARK_SPEED_SPAN * j3) * scale;
        let velocity = dir * speed;
        let life = SPARK_LIFETIME_SECS * (0.7 + 0.6 * j3);
        commands.spawn((
            HitSpark {
                velocity,
                lifetime: life,
                initial_lifetime: life,
            },
            Mesh3d(spark_mesh.clone()),
            MeshMaterial3d(spark_material.clone()),
            // Oriented along the flight from frame ONE, so a fleck never
            // renders axis-aligned for the tick before the updater turns it.
            Transform::from_translation(impact)
                .with_rotation(Quat::from_rotation_arc(
                    Vec3::Z,
                    velocity.normalize_or_zero(),
                ))
                .with_scale(Vec3::splat(scale)),
            NotShadowCaster,
            PlayMatchEntity,
        ));
    }
}

// --- Update -----------------------------------------------------------------

/// Update (graphical-only): advance every live flinch's clock.
///
/// Writes no geometry: `apply_gait_offset` reads the clock and composes the
/// dip onto the gait channel, so the body's local Y has exactly one writer
/// (see `gait.rs`). Ordered before the gaits in `states/mod.rs` so the dip is
/// rendered on the frame it is advanced, not one behind.
pub fn tick_hit_flinch(time: Res<Time>, mut flinches: Query<&mut HitFlinch>) {
    let dt = time.delta_secs();
    for mut flinch in flinches.iter_mut() {
        flinch.elapsed += dt;
    }
}

/// Update (graphical-only): integrate the flecks' ballistics and taper them.
pub fn update_hit_sparks(time: Res<Time>, mut sparks: Query<(&mut HitSpark, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut spark, mut transform) in sparks.iter_mut() {
        spark.lifetime -= dt;
        spark.velocity.y -= SPARK_GRAVITY * dt;
        let velocity = spark.velocity;
        transform.translation += velocity * dt;
        if velocity.length_squared() > 1e-6 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, velocity.normalize());
        }
        let k = (spark.lifetime / spark.initial_lifetime).clamp(0.0, 1.0);
        // Thins and shortens out rather than blinking off.
        let base = transform.scale.max_element();
        transform.scale = Vec3::new(
            base * k.max(0.15),
            base * k.max(0.15),
            base * (0.4 + 0.6 * k),
        );
    }
}

/// Update (graphical-only): pop the contact flash open, then collapse it.
///
/// Largest while brightest and shrinking out of the flecks' way — an envelope
/// that expanded while fading would cover its own debris at exactly the wrong
/// moment (`mortal_strike.rs`, `update_mortal_strike_flash`).
pub fn update_hit_flashes(
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(&mut HitFlash, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut flash, mut transform) in flashes.iter_mut() {
        flash.lifetime -= dt;
        let k = (flash.lifetime / flash.initial_lifetime).clamp(0.0, 1.0);
        transform.scale = Vec3::splat((0.3 + 0.7 * k) * flash.radius);
        if let Some(material) = materials.get_mut(&flash.material) {
            material.base_color = material.base_color.with_alpha(k);
            let (er, eg, eb) = SPARK_EMISSIVE;
            let i = SPARK_FLASH_INTENSITY * k;
            material.emissive = LinearRgba::new(er * i, eg * i, eb * i, 1.0);
        }
    }
}

// --- Cleanup ----------------------------------------------------------------

/// Update (graphical-only): drop spent flinches.
///
/// Removal is visually silent — [`hit_flinch_offset`] already returns `0.0` at
/// `elapsed >= duration`, so the body is back at its gait height by the time
/// the component goes.
pub fn cleanup_hit_flinch(mut commands: Commands, flinches: Query<(Entity, &HitFlinch)>) {
    for (entity, flinch) in flinches.iter() {
        if flinch.elapsed >= flinch.duration {
            commands.entity(entity).remove::<HitFlinch>();
        }
    }
}

/// Update (graphical-only): despawn spent flecks and flashes.
pub fn cleanup_hit_reactions(
    mut commands: Commands,
    sparks: Query<(Entity, &HitSpark)>,
    flashes: Query<(Entity, &HitFlash)>,
) {
    for (entity, spark) in sparks.iter() {
        if spark.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, flash) in flashes.iter() {
        if flash.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flinch(elapsed: f32) -> HitFlinch {
        HitFlinch {
            elapsed,
            duration: FLINCH_DURATION_SECS,
            depth: FLINCH_DIP,
        }
    }

    #[test]
    fn the_dip_is_downward_and_peaks_at_the_rise_fraction() {
        let peak = FLINCH_DURATION_SECS * FLINCH_RISE_FRAC;
        let at_peak = hit_flinch_offset(&flinch(peak));
        assert!(at_peak < 0.0, "a flinch compresses DOWNWARD, got {at_peak}");
        assert!((at_peak + FLINCH_DIP).abs() < 1e-5, "peak is FLINCH_DIP");
        // Strictly shallower on both sides of the peak.
        assert!(hit_flinch_offset(&flinch(peak * 0.5)) > at_peak);
        assert!(hit_flinch_offset(&flinch(peak + 0.05)) > at_peak);
    }

    #[test]
    fn the_dip_is_exactly_zero_once_spent() {
        // Load-bearing: `cleanup_hit_flinch` removes the component a frame or
        // more after this point, and removal must not itself move the body.
        assert_eq!(hit_flinch_offset(&flinch(FLINCH_DURATION_SECS)), 0.0);
        assert_eq!(hit_flinch_offset(&flinch(FLINCH_DURATION_SECS + 1.0)), 0.0);
        assert_eq!(hit_flinch_offset(&flinch(0.0)), 0.0);
    }

    #[test]
    fn a_zero_duration_flinch_is_inert_rather_than_nan() {
        let degenerate = HitFlinch {
            elapsed: 0.1,
            duration: 0.0,
            depth: FLINCH_DIP,
        };
        assert_eq!(hit_flinch_offset(&degenerate), 0.0);
    }

    #[test]
    fn a_crit_dips_deeper_than_a_normal_hit() {
        let peak = FLINCH_DURATION_SECS * FLINCH_RISE_FRAC;
        let normal = hit_flinch_offset(&flinch(peak)).abs();
        let crit = hit_flinch_offset(&HitFlinch {
            elapsed: peak,
            duration: FLINCH_DURATION_SECS,
            depth: FLINCH_DIP * FLINCH_CRIT_MULT,
        })
        .abs();
        assert!((crit / normal - FLINCH_CRIT_MULT).abs() < 1e-4);
    }

    #[test]
    fn the_flash_cannot_swallow_the_sparks() {
        // The `mortal_strike.rs` invariant, restated for this effect: the
        // slowest fleck must clear the flash's radius well inside the flash's
        // own life, or the burst reads as a plain glowing ball.
        let slowest = SPARK_BURST_SPEED * SPARK_SPEED_MIN * SPARK_CRIT_SCALE;
        let travel = slowest * SPARK_FLASH_LIFETIME_SECS;
        let radius = SPARK_FLASH_RADIUS * SPARK_CRIT_SCALE;
        assert!(
            travel > radius * 1.5,
            "slowest crit fleck travels {travel} in the flash's life, flash radius {radius}"
        );
    }

    #[test]
    fn the_impact_point_clears_the_body_on_the_attackers_side() {
        let victim = Vec3::new(10.0, 1.0, 4.0);
        let attacker = Vec3::new(12.0, 1.0, 4.0);
        let (p, out) = impact_point(victim, attacker, false);
        assert!(
            (out.length() - 1.0).abs() < 1e-5,
            "outward is a unit bearing"
        );
        assert!(out.y.abs() < 1e-6, "outward is horizontal");
        let offset = (p - victim).with_y(0.0);
        assert!(offset.length() >= COMBATANT_BODY_RADIUS + SPARK_SIZE - 1e-5);
        assert!(offset.x > 0.0, "the burst sits toward the attacker");
    }

    #[test]
    fn coincident_units_do_not_produce_a_nan_impact_point() {
        let (p, out) = impact_point(Vec3::new(1.0, 0.0, 2.0), Vec3::new(1.0, 0.0, 2.0), false);
        assert!(p.is_finite());
        assert_eq!(out, Vec3::Y, "a degenerate bearing sprays straight up");
    }
}
