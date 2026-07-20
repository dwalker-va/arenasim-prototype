//! Movement-directive and posture components (movement AI).
//!
//! `MovementDirective` is the decision-to-execution handoff for posture-based
//! movement: class AI (Priest/Paladin posture evaluation, plus the Mage/Hunter
//! ENGAGE/KITE machine) writes a directive; `combat_core/movement.rs::move_to_target`
//! executes it in the movement ladder after Disengage. The directive is now the
//! sole kiting path — the legacy `kiting_timer` branch has been deleted.
//! Casting/channeling/root/stun
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
//! `KitePosture` (with `DpsPosture`) is the simpler ENGAGE/KITE state shared by
//! the Mage and Hunter kiters — no
//! anchor, DIP, or ESCAPE window, just the two-posture machine and its
//! hysteresis hold.

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
}

/// DPS movement posture (the ENGAGE/KITE machine shared by the Mage and Hunter
/// kiters). Separate from the healer `Posture` so the two state machines can't
/// cross-pollinate variants — a kiter is never PRESSURED, a healer never KITEs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DpsPosture {
    /// Holding firing position: no directive — falls through to normal pursuit
    /// to preferred range, then stands and shoots/casts.
    #[default]
    Engage,
    /// Kiting a melee threat: orbiting the kill target at `range_band` distance
    /// while repelling the threat (arc-kiting).
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
    /// DIP target (Paladin only): the enemy healer the committed Hammer
    /// of Justice walk is pursuing. `None` outside DIP.
    pub dip_target: Option<Entity>,
    /// DIP budget deadline: absolute sim-time at which the walk-stun-return
    /// cycle aborts (budget exceeded). `0.0` = no live dip.
    pub dip_until: f32,
    /// Medic-chase target: the dying, occluded teammate the healer is currently
    /// walking toward to regain line of sight (and heal). `Some` only while the
    /// medic chase overrides the normal FREE/PRESSURED movement tick; `None`
    /// otherwise. A change here (or `None` → `Some`) forces the chase directive
    /// to re-target the ally's live position. Always `None` on obstacle-free
    /// maps (no ally is ever occluded, so the chase never arms).
    pub medic_target: Option<Entity>,
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
            medic_target: None,
        }
    }
}

/// Persistent DPS kiter posture (ENGAGE/KITE), shared by the Mage and Hunter.
/// Far simpler than `HealerPosture` — a kiter has no heal-range anchor, no DIP
/// target, and no ESCAPE window, so it carries only the two-posture machine's
/// state. Survives directive expiry and CC, like `HealerPosture`.
#[derive(Component, Clone, Copy, Debug)]
pub struct KitePosture {
    /// Current posture (ENGAGE or KITE).
    pub posture: DpsPosture,
    /// Absolute sim-time of the last posture transition.
    pub since: f32,
    /// Hysteresis floor: KITE may not exit before this sim-time even if its
    /// sustain condition lapses, preventing KITE↔ENGAGE strobing. `0.0` = no
    /// hold.
    pub hold_until: f32,
    /// Last committed scorer direction (unit XZ), input to the commitment
    /// bonus at the next re-evaluation. `None` before the first directional
    /// decision and after posture transitions.
    pub last_direction: Option<Vec2>,
    /// Occlusion-chase leaky-bucket accumulator (ENGAGE), in occlusion units.
    /// FILLS at a fixed 1.0/sec of sim time while the kiter is occluded from its
    /// kill target in shot range (the `should_seek_los` stall), and DRAINS at
    /// `seek_chase_decay`/sec while it has sight, clamped at 0. The direct chase
    /// ARMS once this reaches `seek_chase_timeout`. Because a juking target
    /// (occlude mid-cast, flash back between casts) fills faster than the
    /// sub-fill drain bleeds it, intermittent occlusion still accrues toward the
    /// threshold instead of resetting each flicker — the fix for the mid-cast
    /// juke that the old continuous-clock missed. A target under continuous
    /// occlusion fills at 1.0/sec, so the static pillar-hug arms at exactly
    /// `seek_chase_timeout` seconds, identical to the old clock. Reset to 0 on
    /// kill-target change or death (see `occluded_target`). Ticked every frame
    /// (including mid-cast) by `tick_kite_occlusion`, which OWNS this field;
    /// `evaluate_dps_posture` only reads it. Always 0.0 on obstacle-free maps
    /// (sight never breaks).
    pub occlusion_accum: f32,
    /// The kill target the `occlusion_accum` bucket is bound to. A change here
    /// (target swap) or the target's death resets the accumulator to 0 so the
    /// swapped-to target must re-earn the arm threshold. `None` when unbound
    /// (no living kill target).
    pub occluded_target: Option<Entity>,
    /// Freezing Trap DIP target (Hunter only): the enemy healer the committed
    /// trap-setup walk is pursuing. `None` outside a dip. Mirrors the Paladin
    /// `HealerPosture::dip_target`; the Mage never sets it.
    pub dip_target: Option<Entity>,
    /// Freezing Trap DIP budget deadline: absolute sim-time at which the walk
    /// aborts (budget exceeded). A live dip is `now < dip_until`. `0.0` = no
    /// live dip. While a dip is live the Hunter arm skips the ENGAGE/KITE
    /// evaluation so the dip directive owns movement.
    pub dip_until: f32,
}

/// Persistent melee "tempo reset" state (Warrior). When a melee's go is
/// stopped by a movement-impairing CC (Root/Stun/Incapacitate) and its gap
/// closer is on cooldown, it falls back toward its healer for a bounded window
/// instead of face-chasing a kited target into more CC. The window is *armed*
/// while under CC and stays available for `melee.reset_window` seconds after
/// the CC ends, so the reset actually runs once the root drops (a rooted
/// warrior can't move). Survives directive expiry, like the posture states.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MeleeResetState {
    /// Absolute sim-time until which a movement-CC keeps the reset available.
    /// Set to `now + melee.reset_window` every frame the warrior is under a
    /// movement-impairing CC. `0.0` = never armed.
    pub armed_until: f32,
    /// Whether the reset directive was issued last evaluation — the edge used
    /// to emit the `MeleeReset` trace only on activation (not every frame) and
    /// to know a fallback directive is ours to clear on deactivation.
    pub active: bool,
}

impl KitePosture {
    /// Fresh posture state at sim-time `now` (ENGAGE, no hold).
    pub fn new(now: f32) -> Self {
        Self {
            posture: DpsPosture::Engage,
            since: now,
            hold_until: 0.0,
            last_direction: None,
            occlusion_accum: 0.0,
            occluded_target: None,
            dip_target: None,
            dip_until: 0.0,
        }
    }

    /// Is a Freezing Trap dip currently live at sim-time `now`?
    pub fn dipping(&self, now: f32) -> bool {
        now < self.dip_until && self.dip_target.is_some()
    }
}
