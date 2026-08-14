use bevy::prelude::*;
use crate::states::play_match::components::*;

/// Peak height of the walking bob above `ground_y`, in arena units.
/// Capsule height is ~2.5, so 0.10 reads as a subtle walk rather than a hop.
const WALK_BOB_AMPLITUDE: f32 = 0.10;

/// Arena units of horizontal travel per full bob cycle.
/// At base movement speed this lands near a natural walking cadence.
const WALK_STEP_LENGTH: f32 = 1.5;

/// Per-frame horizontal travel below this counts as "not moving" — the unit
/// is held flat at `ground_y` instead of accumulating phase.
const WALK_IDLE_EPSILON: f32 = 0.001;

/// Maximum phase advance per frame. Caps the cadence during Charge so the bob
/// reads as a fast walk instead of strobing when the warrior covers a large
/// XZ delta in a single frame.
const WALK_MAX_PHASE_STEP: f32 = std::f32::consts::PI;

/// Seconds without movement before a gait declares itself idle. Must stay above
/// one render frame at any refresh rate — see [`WalkAnim::idle_time`].
const WALK_IDLE_HOLD: f32 = 0.1;

/// Rate the body eases back to `rest_y` once idle, in arena units per second.
const GAIT_SETTLE_RATE: f32 = 0.6;

/// Peak height of a polymorphed unit's hop above `rest_y`, in arena units.
/// Far above the walk bob: a sheep's gait is meant to read from the camera's
/// default framing, and at the bob's 0.10 it did not.
const HOP_AMPLITUDE: f32 = 0.28;

/// Arena units of horizontal travel per hop cycle. Shorter than
/// `WALK_STEP_LENGTH` so the polymorph wander — which runs at 20% movement
/// speed — reads as quick little hops rather than one slow arc per pace.
const HOP_STEP_LENGTH: f32 = 0.9;

/// Shapes the hop arc. `sin` clipped at zero already grounds half the cycle;
/// the exponent narrows the airborne half further, so the sheep pauses on the
/// ground between hops instead of rolling smoothly between them.
const HOP_SHARPNESS: f32 = 1.6;

/// Fold this frame's horizontal travel into a gait's phase and idle clock, and
/// report whether the unit should be held at rest.
///
/// Idle is TIME-based, not frame-based: the sim moves units only on FixedUpdate
/// ticks, so at render rates above the tick rate every other frame sees zero
/// movement. Snapping to rest on those frames strobed the gait (and every
/// attached weapon) at frame rate.
fn advance_gait(
    walk: &mut WalkAnim,
    current_xz: Vec2,
    step_length: f32,
    alive: bool,
    delta_secs: f32,
) -> bool {
    let distance = (current_xz - walk.previous_xz).length();
    walk.previous_xz = current_xz;

    if distance >= WALK_IDLE_EPSILON {
        // Coming out of a real stop, restart the cycle at its zero crossing so
        // the first moving frame matches the rest height.
        if walk.idle_time > WALK_IDLE_HOLD {
            walk.phase = 0.0;
        }
        walk.idle_time = 0.0;
    } else {
        walk.idle_time += delta_secs;
    }

    let idle = !alive || walk.idle_time > WALK_IDLE_HOLD;
    if !idle {
        let step = (distance / step_length * std::f32::consts::TAU).min(WALK_MAX_PHASE_STEP);
        walk.phase = (walk.phase + step).rem_euclid(std::f32::consts::TAU);
    }
    idle
}

/// Write a gait's vertical offset to a unit's [`VisualBody`] child.
///
/// Settling into idle EASES down to rest instead of snapping — a gait can stop
/// at any height, and a one-frame drop reads as a pop (more so with weapons
/// riding the body).
fn apply_gait_offset(
    children: &Children,
    bodies: &mut Query<(&mut Transform, &VisualBody)>,
    idle: bool,
    offset: f32,
    settle_step: f32,
) {
    for child in children.iter() {
        let Ok((mut body_transform, body)) = bodies.get_mut(child) else {
            continue;
        };
        if idle {
            let err = body.rest_y - body_transform.translation.y;
            body_transform.translation.y += err.clamp(-settle_step, settle_step);
        } else {
            body_transform.translation.y = body.rest_y + offset;
        }
    }
}

/// Drive the walking bob on combatant and pet capsules.
///
/// Reads each unit's post-movement XZ, advances phase by the horizontal
/// distance traveled this frame, and writes the bob to that unit's
/// [`VisualBody`] child as `local.y = rest_y + sin(phase) * amplitude`. Idle
/// units (and any unit whose `Combatant::is_alive()` returns true but whose XZ
/// delta is below `WALK_IDLE_EPSILON`) snap to `rest_y` so they stand still.
///
/// **The bob must never touch the parent's `Transform`.** Gameplay range checks
/// use `Vec3::distance`, so a ±0.10 bob on the sim entity perturbed real range
/// checks — that is why a seed stopped reproducing between the client and
/// headless. See [`VisualBody`].
///
/// `Without<DeathAnimation>` and `Without<Celebrating>` cede the Y axis to
/// `animate_death` (corpse sink) and `update_victory_celebration` (winner
/// bounce). All three now write the same child's local Y and run in the same
/// post-`CombatResolution` window, so excluding their drivers is still the
/// cleanest way to avoid the last-writer-wins race. `Without<PolymorphedVisual>`
/// does the same for [`update_sheep_hop`], which owns the gait while a unit is
/// a sheep.
///
/// Graphical-mode only — registered in `StatesPlugin::build()`, never in
/// `add_core_combat_systems`. Visual-only; touches no gameplay state.
pub fn update_walk_animation(
    time: Res<Time>,
    mut movers: Query<
        (&Transform, &mut WalkAnim, &Combatant, &Children),
        (
            Without<DeathAnimation>,
            Without<Celebrating>,
            Without<VisualBody>,
            Without<PolymorphedVisual>,
        ),
    >,
    mut bodies: Query<(&mut Transform, &VisualBody)>,
) {
    for (transform, mut walk, combatant, children) in movers.iter_mut() {
        // Read the sim entity's XZ, but write only the child's local Y.
        let idle = advance_gait(
            &mut walk,
            transform.translation.xz(),
            WALK_STEP_LENGTH,
            combatant.is_alive(),
            time.delta_secs(),
        );
        apply_gait_offset(
            children,
            &mut bodies,
            idle,
            walk.phase.sin() * WALK_BOB_AMPLITUDE,
            GAIT_SETTLE_RATE * time.delta_secs(),
        );
    }
}

/// Drive the hopping gait on polymorphed units, replacing the walk bob.
///
/// Distance-driven exactly like the bob, so the polymorph wander's 20% movement
/// speed reads as slow hopping and a sheep that is not moving holds still. Only
/// the waveform differs: `sin` clipped at zero and sharpened, which grounds the
/// sheep between hops instead of easing it through a continuous sine.
///
/// Shares [`WalkAnim`] with the bob rather than carrying its own state, so the
/// bob's baseline (`previous_xz`, `idle_time`) stays live through the sheep form
/// and does not resume on a stale per-frame delta when the polymorph breaks.
///
/// Query filters mirror [`update_walk_animation`]: death and celebration own the
/// Y axis when present, and `Without<VisualBody>` keeps the mover query disjoint
/// from the body query it writes through.
///
/// Graphical-mode only — registered in `StatesPlugin::build()`, never in
/// `add_core_combat_systems`. Visual-only; touches no gameplay state.
pub fn update_sheep_hop(
    time: Res<Time>,
    mut movers: Query<
        (&Transform, &mut WalkAnim, &Combatant, &Children),
        (
            With<PolymorphedVisual>,
            Without<DeathAnimation>,
            Without<Celebrating>,
            Without<VisualBody>,
        ),
    >,
    mut bodies: Query<(&mut Transform, &VisualBody)>,
) {
    for (transform, mut walk, combatant, children) in movers.iter_mut() {
        let idle = advance_gait(
            &mut walk,
            transform.translation.xz(),
            HOP_STEP_LENGTH,
            combatant.is_alive(),
            time.delta_secs(),
        );
        let lift = walk.phase.sin().max(0.0).powf(HOP_SHARPNESS) * HOP_AMPLITUDE;
        apply_gait_offset(
            children,
            &mut bodies,
            idle,
            lift,
            GAIT_SETTLE_RATE * time.delta_secs(),
        );
    }
}

