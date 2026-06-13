//! Movement-directive and healer-posture components (healer movement plan, U5).
//!
//! `MovementDirective` is the decision-to-execution handoff for posture-based
//! movement: class AI (Priest/Paladin posture evaluation, U6–U8) writes a
//! directive; `combat_core/movement.rs::move_to_target` executes it in the
//! movement ladder between Disengage and kiting. Casting/channeling/root/stun
//! still block execution (their early-continues sit above the directive
//! branch); only the EXPIRY check runs before them, so a directive issued
//! pre-stun is removed — never executed — on the first post-stun frame.
//!
//! `HealerPosture` is the persistent FREE/PRESSURED/ESCAPE/DIP state machine
//! state. It deliberately does NOT live on the directive: a feared/stunned/
//! casting healer's AI doesn't run, so directives can expire while the posture
//! must survive — hysteresis and trace transition events key off real posture
//! changes, never expiry artifacts.
//!
//! As of U5 nothing issues directives or postures yet — the components,
//! executor branch, scorer, and config land behavior-neutral; emitters arrive
//! in U6 (Priest), U7 (ESCAPE), and U8 (Paladin).

use bevy::prelude::*;

/// What a [`MovementDirective`] asks the executor to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MovementGoal {
    /// Move along a unit XZ direction (PRESSURED repositioning, ESCAPE
    /// separation). `Vec2.x` maps to world X, `Vec2.y` to world Z.
    Direction(Vec2),
    /// Move toward a fixed world point, stopping within a small epsilon
    /// (FREE formation anchor).
    Point(Vec3),
    /// Pursue an entity's current position (DIP target chase).
    Entity(Entity),
}

/// A movement order issued by class AI, executed by `move_to_target`.
///
/// Executes at base speed × `MovementSpeedSlow` multipliers (same slow
/// handling as the kiting branch). Entities without this component fall
/// through the existing movement ladder unchanged.
#[derive(Component, Clone, Copy, Debug)]
pub struct MovementDirective {
    pub goal: MovementGoal,
    /// ABSOLUTE sim-time deadline (`Time::elapsed_secs()`), not a countdown.
    /// Checked at the TOP of `move_to_target`'s per-combatant loop — before
    /// the casting/channeling/CC early-continues — so a stale directive is
    /// removed without executing even if the owner was CC'd past the
    /// deadline.
    pub expires: f32,
    /// Absolute sim-time until which the ISSUING AI treats the chosen
    /// direction as committed (R11 anti-zigzag window). This is the "when
    /// does re-evaluation happen" governor; the scorer's commitment-bonus
    /// term applies only AT re-evaluation — the two never stack. The
    /// executor ignores this field.
    pub committed_until: f32,
}

/// Healer movement posture. Gameplay-side mirror of
/// `decision_trace::events::Posture` (the trace enum carries the serde
/// attributes the wire format needs; conversion lives in
/// `decision_trace/events.rs`, which already depends on `components`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Posture {
    /// No credible threat: formation anchoring (Priest) / legacy melee
    /// pursuit (Paladin).
    #[default]
    Free,
    /// Targeted by a visible enemy AND a proximity/intent condition holds.
    Pressured,
    /// All proximate threats movement-impaired — converting the window into
    /// separation.
    Escape,
    /// Paladin only: committed walk to the enemy healer for Hammer of
    /// Justice.
    Dip,
    /// Mage only: holding cast position (no directive — falls through to
    /// normal pursuit to preferred range, then stands and casts).
    Engage,
    /// Mage only: kiting a melee threat impaired by the Mage's own root/slow,
    /// orbiting the kill target at `range_band` distance.
    Kite,
}

/// Persistent posture state for a healer. Survives directive expiry.
#[derive(Component, Clone, Copy, Debug)]
pub struct HealerPosture {
    /// Current posture.
    pub posture: Posture,
    /// Absolute sim-time of the last posture transition.
    pub since: f32,
    /// Hysteresis floor: absolute sim-time before which the posture may not
    /// relax (e.g., PRESSURED may not flip back to FREE) so a threat hovering
    /// at the danger radius doesn't strobe the state machine. `0.0` = no
    /// hold.
    pub hold_until: f32,
    /// Sticky anchor ally for the PRESSURED heal-range constraint. Switching
    /// requires beating the configured `anchor_switch_margin` so two
    /// similarly-injured allies don't flap the constraint region tick to
    /// tick.
    pub anchor: Option<Entity>,
    /// ESCAPE window end: absolute sim-time at which the committed escape
    /// directive (and the cast-vs-move heal deferral) expires. Set on
    /// PRESSURED → ESCAPE entry to `now + min(CC remaining over impaired
    /// proximate threats)`; the posture exits (→ PRESSURED or FREE) on the
    /// first evaluation at/after this deadline. `0.0` = no window.
    pub escape_until: f32,
    /// Last committed scorer direction (unit XZ), input to the scorer's
    /// commitment-bonus term at the next re-evaluation. `None` before the
    /// first directional decision and after posture transitions.
    pub last_direction: Option<Vec2>,
    /// Last issued FREE formation point (XZ), input to the FormationShift
    /// re-commit threshold (only re-target + emit when the point moved
    /// meaningfully). `None` before the first formation directive and after
    /// posture transitions.
    pub last_point: Option<Vec2>,
    /// DIP target (Paladin only, U8): the enemy healer the committed Hammer
    /// of Justice walk is pursuing. `None` outside DIP.
    pub dip_target: Option<Entity>,
    /// DIP budget deadline: absolute sim-time at which the walk-stun-return
    /// cycle aborts (budget exceeded). `0.0` = no live dip.
    pub dip_until: f32,
}

impl HealerPosture {
    /// Fresh posture state at sim-time `now` (FREE, no hysteresis hold).
    pub fn new(now: f32) -> Self {
        Self {
            posture: Posture::Free,
            since: now,
            hold_until: 0.0,
            anchor: None,
            escape_until: 0.0,
            last_direction: None,
            last_point: None,
            dip_target: None,
            dip_until: 0.0,
        }
    }
}

/// Persistent Mage movement posture (ENGAGE/KITE). Far simpler than
/// `HealerPosture` — the Mage has no heal-range anchor, no DIP target, and no
/// ESCAPE window, so it carries only the state the two-posture machine needs.
/// Survives directive expiry and CC, like `HealerPosture`.
#[derive(Component, Clone, Copy, Debug)]
pub struct MagePosture {
    /// Current posture (ENGAGE or KITE).
    pub posture: Posture,
    /// Absolute sim-time of the last posture transition.
    pub since: f32,
    /// Hysteresis floor: KITE may not exit before this sim-time even if the
    /// sustaining aura breaks, preventing KITE↔ENGAGE strobing on fast root
    /// breaks. `0.0` = no hold.
    pub hold_until: f32,
    /// Last committed scorer direction (unit XZ), input to the commitment
    /// bonus at the next re-evaluation. `None` before the first directional
    /// decision and after posture transitions.
    pub last_direction: Option<Vec2>,
}

impl MagePosture {
    /// Fresh posture state at sim-time `now` (ENGAGE, no hold).
    pub fn new(now: f32) -> Self {
        Self {
            posture: Posture::Engage,
            since: now,
            hold_until: 0.0,
            last_direction: None,
        }
    }
}
