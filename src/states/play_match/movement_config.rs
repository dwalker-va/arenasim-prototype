//! Data-Driven Movement Configuration (healer posture AI)
//!
//! Mirrors the `ability_config.rs` loading pattern exactly: serde structs with
//! defaults, direct `std::fs::read_to_string` + `ron::from_str` (no asset
//! server — required for headless), `validate()`, a `Resource`, and a plugin
//! that panics on failure. The plugin is registered in BOTH the headless
//! runner (`src/headless/runner.rs`, next to `AbilityConfigPlugin`) and the
//! graphical stack (`src/main.rs`) — the dual-mode registration failure class
//! is the most-burned-by bug in this repo's history.
//!
//! All scorer weights, radii, thresholds, and commitment windows for the
//! healer posture state machine (FREE/PRESSURED/ESCAPE/DIP) and the Mage
//! ENGAGE/KITE pilot live in `assets/config/movement.ron` (R9). The config is
//! consumed by the class AI (Priest/Paladin posture eval, Mage posture eval).
//!
//! ## Usage
//! ```ignore
//! fn my_system(movement: Res<MovementConfig>) {
//!     let weights = &movement.priest.weights;
//!     let radius = movement.shared.danger_radius;
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{AUTO_SHOT_RANGE, SAFE_KITING_DISTANCE};

/// Position-scorer term weights (one block per healer class in movement.ron).
///
/// Consumed by `combat_core::movement_scoring::score_directions`. A weight of
/// `0.0` disables its term (e.g., `wand_pull: 0.0` for the Paladin, which has
/// no wand). All terms here are additive *interest* terms; the hard
/// constraints (boundary, ally-anchor) are boolean masks in the scorer, not
/// weights — the retired `ally_anchor` / `boundary_penalty` penalty fields and
/// the dominance invariant that policed them are gone.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementWeights {
    /// Per visible threat: pull away, weighted by proximity (PRESSURED).
    pub threat_repulsion: f32,
    /// Pull toward the formation point behind the engaged-ally centroid
    /// (FREE, Priest only).
    pub formation_pull: f32,
    /// Graded penalty for candidate positions approaching arena corners.
    pub corner_penalty: f32,
    /// Low-weight pull toward wand range of the kill target (Priest;
    /// 0.0 disables for Paladin).
    pub wand_pull: f32,
    /// Ring-attraction toward the kill target's `[min, max]` band (Mage
    /// kiting). `0.0` disables for the healers, which have no kill-target ring.
    pub range_band: f32,
    /// Constant pull away from the nearest threat, NOT proximity-weighted —
    /// the distance-maximization "flee" a chased ranged DPS (Hunter) needs to
    /// outrun an un-impaired chaser. Unlike `threat_repulsion` (which fades
    /// with distance, right for healers), `flee` is strong at all ranges.
    /// `0.0` disables for healers and the Mage (whose KITE target is rooted).
    pub flee: f32,
    /// Bonus toward the previously committed direction, applied only AT
    /// re-evaluation while the commitment window is open (R11).
    pub commitment_bonus: f32,
}

impl Default for MovementWeights {
    fn default() -> Self {
        Self {
            threat_repulsion: 3.0,
            formation_pull: 2.0,
            // Matches the shipped Priest value in movement.ron (the Paladin
            // block explicitly overrides to 4.0). Was 4.0 here — a silent
            // divergence from the RON, now aligned (P3 residual).
            corner_penalty: 6.0,
            wand_pull: 0.5,
            range_band: 0.0,
            flee: 0.0,
            commitment_bonus: 1.5,
        }
    }
}

/// Radii, thresholds, and windows shared by both healer classes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedMovementConfig {
    /// PRESSURED proximity condition: a targeting enemy within this radius
    /// (or melee/pet, or closing) flips the posture (R6).
    pub danger_radius: f32,
    /// Intent bound on the trigger's melee/pet/closing branch: a targeting
    /// melee/pet/closing threat counts only within this radius. Without it
    /// a melee enemy targeting the healer from across the arena (or an
    /// enemy healer "closing" toward its own preferred range) flips
    /// PRESSURED at gates-open (R6/AE5 refinement).
    pub threat_intent_radius: f32,
    /// PRESSURED constraint: stay within this range of the anchor ally
    /// (WoW heal range).
    pub heal_range: f32,
    /// FREE formation point: offset distance behind the engaged-ally
    /// centroid (R5).
    pub formation_offset: f32,
    /// FREE formation point: bias toward arena center (0.0 = none,
    /// 1.0 = full center pull).
    pub center_bias: f32,
    /// Commitment window in seconds — re-evaluation of a committed direction
    /// happens only after this elapses (R11 anti-zigzag; plan band 0.4–0.8).
    pub commit_window: f32,
    /// Hysteresis: minimum seconds PRESSURED holds after entry before it may
    /// relax back to FREE — a threat hovering at the danger radius must not
    /// strobe the posture (R6). Exiting additionally requires the compound
    /// trigger to be false.
    pub pressured_hold: f32,
    /// Directive time-to-live in seconds — `MovementDirective.expires` is
    /// issued as `now + directive_ttl` so stale directives self-clean.
    pub directive_ttl: f32,
    /// ESCAPE windows shorter than this (seconds) are ignored — not worth
    /// deferring a heal for (R7).
    pub escape_min_window: f32,
    /// Cast-vs-move urgency: while an ESCAPE window or DIP is live,
    /// non-critical casts are deferred unless an ally is below this HP
    /// fraction (R7/R8).
    pub urgency_hp_threshold: f32,
    /// Sticky anchor selection: switching anchor allies requires the new
    /// candidate to be more injured by this HP fraction margin.
    pub anchor_switch_margin: f32,
    /// Wand-range pull target distance (Priest wand range).
    pub wand_range: f32,
}

impl Default for SharedMovementConfig {
    fn default() -> Self {
        Self {
            danger_radius: 12.0,
            threat_intent_radius: 30.0,
            heal_range: 40.0,
            formation_offset: 8.0,
            center_bias: 0.3,
            commit_window: 0.6,
            pressured_hold: 1.5,
            directive_ttl: 1.0,
            escape_min_window: 0.5,
            urgency_hp_threshold: 0.5,
            anchor_switch_margin: 0.1,
            wand_range: 30.0,
        }
    }
}

/// Priest-specific movement configuration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PriestMovementConfig {
    pub weights: MovementWeights,
    /// FREE: distance (XZ units) the formation point must move before the
    /// directive is re-targeted and a `FormationShift` trace event fires.
    pub formation_shift_threshold: f32,
    /// FREE deadzone: no Point directive is issued when the Priest is already
    /// this close to the formation point (prevents micro-shuffling).
    pub formation_deadzone: f32,
    /// FREE: refresh the standing directive when its remaining TTL drops below
    /// this — keeps a walk alive across decide ticks without re-scoring or
    /// emitting (refreshes are not decisions).
    pub directive_refresh_margin: f32,
    /// DIP duration budget in seconds — the Psychic Scream walk-stun-return
    /// cycle aborts when exceeded (U4 offensive dip; mirrors the Paladin).
    pub dip_budget: f32,
    /// Healing-heavy deferral trigger (U4): the Priest defers the offensive
    /// dip while the lowest HP fraction across living non-pet team members
    /// (self included) is below this. Observable, deterministic state.
    pub healing_heavy_hp: f32,
}

impl Default for PriestMovementConfig {
    fn default() -> Self {
        Self {
            weights: MovementWeights::default(),
            formation_shift_threshold: 3.0,
            formation_deadzone: 1.5,
            directive_refresh_margin: 0.25,
            dip_budget: 6.0,
            healing_heavy_hp: 0.6,
        }
    }
}

/// Paladin-specific movement configuration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PaladinMovementConfig {
    pub weights: MovementWeights,
    /// PRESSURED retreat range — focused (or healing-heavy) Paladins pull
    /// back to this distance instead of face-tanking at melee range (R8).
    pub fallback_range: f32,
    /// DIP duration budget in seconds — the walk-stun-return cycle aborts
    /// when exceeded (R8).
    pub dip_budget: f32,
    /// Healing-heavy PRESSURED trigger (R8): the Paladin counts as
    /// healing-heavy while the lowest HP fraction across living non-pet
    /// team members (self included) is below this. Observable, deterministic
    /// state — no cast-history bookkeeping.
    pub healing_heavy_hp: f32,
}

impl Default for PaladinMovementConfig {
    fn default() -> Self {
        Self {
            weights: MovementWeights {
                // Paladin has no wand and no backline formation point.
                wand_pull: 0.0,
                formation_pull: 0.0,
                ..MovementWeights::default()
            },
            fallback_range: 15.0,
            dip_budget: 6.0,
            healing_heavy_hp: 0.6,
        }
    }
}

/// Shaman-specific movement configuration.
///
/// Mirrors [`PriestMovementConfig`] minus the Dip fields — the Shaman has no
/// Hammer-of-Justice / Psychic-Scream dip. The defaults lean OFFENSIVE versus
/// the Priest (a ranged caster that pressures rather than a pure backline
/// healer): weaker `threat_repulsion`/`formation_pull` (less eager to flee /
/// fall back) and a stronger `wand_pull` (the Shaman repurposes the wand-pull
/// term as a pull toward Lightning Bolt range of the kill target).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShamanMovementConfig {
    pub weights: MovementWeights,
    /// FREE: distance (XZ units) the formation point must move before the
    /// directive is re-targeted and a `FormationShift` trace event fires.
    pub formation_shift_threshold: f32,
    /// FREE deadzone: no Point directive is issued when the Shaman is already
    /// this close to the formation point (prevents micro-shuffling).
    pub formation_deadzone: f32,
    /// FREE: refresh the standing directive when its remaining TTL drops below
    /// this — keeps a walk alive across decide ticks without re-scoring.
    pub directive_refresh_margin: f32,
    /// Healing-heavy deferral fraction (parity with the Priest): observable,
    /// deterministic team-HP gate kept for forward-compatibility / consistency.
    pub healing_heavy_hp: f32,
}

impl Default for ShamanMovementConfig {
    fn default() -> Self {
        Self {
            weights: MovementWeights {
                // Offense-slanted vs the Priest (3.0/2.0/6.0/0.5): less flee,
                // weaker backline pull, stronger Lightning-Bolt-range pull.
                threat_repulsion: 2.0,
                formation_pull: 1.0,
                corner_penalty: 6.0,
                wand_pull: 1.0,
                ..MovementWeights::default()
            },
            formation_shift_threshold: 3.0,
            formation_deadzone: 1.5,
            directive_refresh_margin: 0.25,
            healing_heavy_hp: 0.6,
        }
    }
}

/// Melee target-swap tuning (bucket A offensive-punish). When a melee's kill
/// target kites persistently out of reach and a softer enemy is in melee, the
/// melee swaps instead of chasing forever — gated by hysteresis to avoid
/// ping-pong. Consumed by `acquire_targets` in `combat_ai.rs`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MeleeMovementConfig {
    /// A candidate softer target must be within this range (yards) to be worth
    /// swapping to — i.e. already in (or right at) melee, so the swap buys
    /// immediate uptime the kited kill target can't.
    pub swap_range: f32,
    /// Minimum seconds between swaps for one combatant — the anti-ping-pong
    /// hysteresis. A fresh swap (or a kill-target change) resets the timer.
    pub swap_hysteresis: f32,
    /// The softer target must be at least this HP fraction below the kited kill
    /// target to justify abandoning the original focus (prevents swapping over
    /// trivial HP differences).
    pub swap_hp_margin: f32,
}

impl Default for MeleeMovementConfig {
    fn default() -> Self {
        Self {
            swap_range: 4.0,
            swap_hysteresis: 2.0,
            swap_hp_margin: 0.15,
        }
    }
}

/// DPS kiter movement tuning (the shared ENGAGE/KITE machine — Mage, Hunter).
/// `range_band_min`/`max` bound the kiting orbit ring; `kite_hold` is the
/// hysteresis floor; `directive_ttl` must cover the longest cast so the kiter
/// can act post-cast; `commit_window` is the anti-zigzag window.
/// `kite_entry_radius`/`kite_sustain_radius` are used only by **proximity-gated**
/// kiters (Hunter): enter KITE when a melee threat is within entry range, exit
/// when no threat remains within sustain range. Aura-gated kiters (Mage) ignore
/// them — KITE keys off the Mage's own root/slow instead.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DpsMovementConfig {
    pub weights: MovementWeights,
    /// Inner ring radius — the orbit stays outside this of the kill target
    /// (keeps the kiter out of melee, and out of the Hunter's dead zone).
    pub range_band_min: f32,
    /// Outer ring radius — the orbit stays inside this of the kill target
    /// (keeps the kill target in shot/cast range).
    pub range_band_max: f32,
    /// Hysteresis floor: minimum seconds KITE holds after entry before it may
    /// exit, even if the sustain condition lapses (anti-strobe).
    pub kite_hold: f32,
    /// MovementDirective TTL — must cover the longest cast so movement survives
    /// it (Frostbolt / Aimed Shot).
    pub directive_ttl: f32,
    /// Committed-direction window (anti-zigzag).
    pub commit_window: f32,
    /// Proximity-gated entry (Hunter): a melee threat within this radius opens
    /// KITE. Ignored by aura-gated kiters.
    pub kite_entry_radius: f32,
    /// Proximity-gated sustain (Hunter): KITE holds while a melee threat is
    /// within this radius, exits once all are kited beyond it. Slightly larger
    /// than entry for hysteresis. Ignored by aura-gated kiters.
    pub kite_sustain_radius: f32,
    /// Freezing Trap DIP budget (Hunter only): max seconds the Hunter will walk
    /// toward an out-of-range enemy healer to set a trap on it, before aborting.
    /// `0.0` disables the dip entirely (the Hunter still places opportunistic
    /// in-range traps on the healer). Mirrors the Paladin/Priest `dip_budget`.
    /// Ignored by the Mage.
    pub dip_budget: f32,
}

impl Default for DpsMovementConfig {
    fn default() -> Self {
        Self {
            weights: MovementWeights {
                // Kiter profile: strong repulsion, ring attraction on, no
                // healer terms (formation/wand) and a light corner penalty.
                // `flee` defaults off (Mage orbits a rooted target); the Hunter
                // block in movement.ron turns it up for distance-max kiting.
                threat_repulsion: 3.0,
                formation_pull: 0.0,
                flee: 0.0,
                corner_penalty: 4.0,
                wand_pull: 0.0,
                range_band: 2.0,
                commitment_bonus: 1.5,
            },
            range_band_min: 8.0,   // SAFE_KITING_DISTANCE / HUNTER_DEAD_ZONE
            range_band_max: 30.0,  // within AUTO_SHOT_RANGE
            kite_hold: 1.0,
            directive_ttl: 3.0,    // covers a Frostbolt / Aimed Shot cast
            commit_window: 0.6,
            kite_entry_radius: 20.0,   // Hunter closing-range band
            kite_sustain_radius: 24.0, // hold a touch past entry
            dip_budget: 0.0,           // Mage default off; Hunter ron turns it on
        }
    }
}

/// Resource containing all movement weights and thresholds.
///
/// Loaded from `assets/config/movement.ron` at startup (both modes).
/// Access via `Res<MovementConfig>` in systems.
#[derive(Resource, Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementConfig {
    pub shared: SharedMovementConfig,
    pub priest: PriestMovementConfig,
    pub paladin: PaladinMovementConfig,
    pub shaman: ShamanMovementConfig,
    pub melee: MeleeMovementConfig,
    pub mage: DpsMovementConfig,
    pub hunter: DpsMovementConfig,
}

impl MovementConfig {
    /// Check value sanity. Returns the list of violations on failure.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut issues: Vec<String> = Vec::new();
        let s = &self.shared;

        let positives = [
            ("shared.danger_radius", s.danger_radius),
            ("shared.threat_intent_radius", s.threat_intent_radius),
            ("shared.heal_range", s.heal_range),
            ("shared.commit_window", s.commit_window),
            ("shared.pressured_hold", s.pressured_hold),
            ("shared.directive_ttl", s.directive_ttl),
            ("shared.wand_range", s.wand_range),
            ("paladin.fallback_range", self.paladin.fallback_range),
            ("paladin.dip_budget", self.paladin.dip_budget),
            ("priest.dip_budget", self.priest.dip_budget),
        ];
        for (name, value) in positives {
            if !(value > 0.0) || !value.is_finite() {
                issues.push(format!("{} must be a positive finite number, got {}", name, value));
            }
        }

        let non_negatives = [
            ("shared.formation_offset", s.formation_offset),
            ("shared.escape_min_window", s.escape_min_window),
            ("shared.anchor_switch_margin", s.anchor_switch_margin),
            ("priest.formation_shift_threshold", self.priest.formation_shift_threshold),
            ("priest.formation_deadzone", self.priest.formation_deadzone),
            ("priest.directive_refresh_margin", self.priest.directive_refresh_margin),
            ("shaman.formation_shift_threshold", self.shaman.formation_shift_threshold),
            ("shaman.formation_deadzone", self.shaman.formation_deadzone),
            ("shaman.directive_refresh_margin", self.shaman.directive_refresh_margin),
        ];
        for (name, value) in non_negatives {
            if value < 0.0 || !value.is_finite() {
                issues.push(format!("{} must be non-negative and finite, got {}", name, value));
            }
        }

        let fractions = [
            ("shared.center_bias", s.center_bias),
            ("shared.urgency_hp_threshold", s.urgency_hp_threshold),
            ("paladin.healing_heavy_hp", self.paladin.healing_heavy_hp),
            ("priest.healing_heavy_hp", self.priest.healing_heavy_hp),
            ("shaman.healing_heavy_hp", self.shaman.healing_heavy_hp),
        ];
        for (name, value) in fractions {
            if !(0.0..=1.0).contains(&value) {
                issues.push(format!("{} must be within [0.0, 1.0], got {}", name, value));
            }
        }

        if s.wand_range > s.heal_range {
            issues.push(format!(
                "shared.wand_range ({}) must not exceed shared.heal_range ({})",
                s.wand_range, s.heal_range
            ));
        }
        if s.directive_ttl < s.commit_window {
            issues.push(format!(
                "shared.directive_ttl ({}) must cover shared.commit_window ({}) — otherwise \
                 directives expire mid-commitment and movement stutters",
                s.directive_ttl, s.commit_window
            ));
        }
        if self.priest.formation_deadzone >= self.priest.formation_shift_threshold {
            issues.push(format!(
                "priest.formation_deadzone ({}) must be below priest.formation_shift_threshold \
                 ({}) — otherwise the deadzone swallows every formation shift",
                self.priest.formation_deadzone, self.priest.formation_shift_threshold
            ));
        }
        if self.priest.directive_refresh_margin >= s.directive_ttl {
            issues.push(format!(
                "priest.directive_refresh_margin ({}) must be below shared.directive_ttl ({}) — \
                 a margin at/above the TTL refreshes the directive every tick",
                self.priest.directive_refresh_margin, s.directive_ttl
            ));
        }
        if self.shaman.formation_deadzone >= self.shaman.formation_shift_threshold {
            issues.push(format!(
                "shaman.formation_deadzone ({}) must be below shaman.formation_shift_threshold \
                 ({}) — otherwise the deadzone swallows every formation shift",
                self.shaman.formation_deadzone, self.shaman.formation_shift_threshold
            ));
        }
        if self.shaman.directive_refresh_margin >= s.directive_ttl {
            issues.push(format!(
                "shaman.directive_refresh_margin ({}) must be below shared.directive_ttl ({}) — \
                 a margin at/above the TTL refreshes the directive every tick",
                self.shaman.directive_refresh_margin, s.directive_ttl
            ));
        }

        let m = &self.melee;
        if !(m.swap_range > 0.0) || !m.swap_range.is_finite() {
            issues.push(format!(
                "melee.swap_range must be a positive finite number, got {}",
                m.swap_range
            ));
        }
        if !(m.swap_hysteresis > 0.0) || !m.swap_hysteresis.is_finite() {
            issues.push(format!(
                "melee.swap_hysteresis must be a positive finite number, got {}",
                m.swap_hysteresis
            ));
        }
        if !(0.0..=1.0).contains(&m.swap_hp_margin) {
            issues.push(format!(
                "melee.swap_hp_margin must be within [0.0, 1.0], got {}",
                m.swap_hp_margin
            ));
        }

        for (class, weights) in [
            ("priest", &self.priest.weights),
            ("paladin", &self.paladin.weights),
            ("shaman", &self.shaman.weights),
            ("mage", &self.mage.weights),
            ("hunter", &self.hunter.weights),
        ] {
            let terms = [
                ("threat_repulsion", weights.threat_repulsion),
                ("formation_pull", weights.formation_pull),
                ("corner_penalty", weights.corner_penalty),
                ("wand_pull", weights.wand_pull),
                ("range_band", weights.range_band),
                ("flee", weights.flee),
                ("commitment_bonus", weights.commitment_bonus),
            ];
            for (name, value) in terms {
                if value < 0.0 || !value.is_finite() {
                    issues.push(format!(
                        "{}.weights.{} must be non-negative and finite, got {}",
                        class, name, value
                    ));
                }
            }
        }

        // DPS kiter ENGAGE/KITE blocks (Mage, Hunter).
        for (class, m) in [("mage", &self.mage), ("hunter", &self.hunter)] {
            if !(m.range_band_min < m.range_band_max) {
                issues.push(format!(
                    "{class}.range_band_min ({}) must be strictly less than range_band_max ({})",
                    m.range_band_min, m.range_band_max
                ));
            }
            if m.range_band_min < SAFE_KITING_DISTANCE {
                issues.push(format!(
                    "{class}.range_band_min ({}) must be >= SAFE_KITING_DISTANCE ({}) so the orbit \
                     stays out of melee of a kill target that is also a threat",
                    m.range_band_min, SAFE_KITING_DISTANCE
                ));
            }
            if m.range_band_max > AUTO_SHOT_RANGE {
                issues.push(format!(
                    "{class}.range_band_max ({}) must be <= AUTO_SHOT_RANGE ({}) — a ring beyond \
                     shot range is config noise",
                    m.range_band_max, AUTO_SHOT_RANGE
                ));
            }
            if m.kite_hold <= 0.0 || !m.kite_hold.is_finite() {
                issues.push(format!("{class}.kite_hold must be a positive finite number, got {}", m.kite_hold));
            }
            if m.directive_ttl < m.commit_window {
                issues.push(format!(
                    "{class}.directive_ttl ({}) must be >= commit_window ({}) so a committed \
                     direction does not outlive its directive",
                    m.directive_ttl, m.commit_window
                ));
            }
            if m.commit_window <= 0.0 || !m.commit_window.is_finite() {
                issues.push(format!("{class}.commit_window must be a positive finite number, got {}", m.commit_window));
            }
            // Proximity-gate sanity (Hunter): sustain radius must not be smaller
            // than entry, or KITE would exit the instant it enters.
            if m.kite_sustain_radius < m.kite_entry_radius {
                issues.push(format!(
                    "{class}.kite_sustain_radius ({}) must be >= kite_entry_radius ({})",
                    m.kite_sustain_radius, m.kite_entry_radius
                ));
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

/// Parse a movement config from RON text. `source` names the origin for
/// error messages (a path, or "inline" in tests).
pub fn parse_movement_config(contents: &str, source: &str) -> Result<MovementConfig, String> {
    let config: MovementConfig = ron::from_str(contents)
        .map_err(|e| format!("Failed to parse {}: {}", source, e))?;

    config
        .validate()
        .map_err(|issues| format!("Invalid movement config in {}:\n  {}", source, issues.join("\n  ")))?;

    Ok(config)
}

/// Load and validate a movement config from a RON file path.
pub fn load_movement_config_from(path: &str) -> Result<MovementConfig, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    parse_movement_config(&contents, path)
}

/// Load movement configuration from assets/config/movement.ron
pub fn load_movement_config() -> Result<MovementConfig, String> {
    let config_path = "assets/config/movement.ron";
    let config = load_movement_config_from(config_path)?;
    info!("Loaded movement configuration from {}", config_path);
    Ok(config)
}

/// Bevy plugin for movement configuration loading.
///
/// Must be registered in BOTH `src/headless/runner.rs` (next to
/// `AbilityConfigPlugin`) and `src/main.rs` (graphical plugin tuple).
pub struct MovementConfigPlugin;

impl Plugin for MovementConfigPlugin {
    fn build(&self, app: &mut App) {
        match load_movement_config() {
            Ok(config) => {
                app.insert_resource(config);
            }
            Err(e) => {
                // Panic to ensure the config is always valid at startup —
                // same policy as AbilityConfigPlugin.
                panic!("Failed to load movement configuration: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped movement.ron loads, parses, and validates (also pins the
    /// two load-bearing plan values: heal range 40, wand range 30).
    #[test]
    fn shipped_movement_ron_loads_and_validates() {
        let config = load_movement_config().expect("assets/config/movement.ron must load");
        assert_eq!(config.shared.heal_range, 40.0);
        assert_eq!(config.shared.wand_range, 30.0);
        assert_eq!(
            config.paladin.weights.wand_pull, 0.0,
            "Paladin has no wand — wand_pull must be disabled"
        );
        assert!(
            (0.4..=0.8).contains(&config.shared.commit_window),
            "commit_window outside the plan's 0.4-0.8 band: {}",
            config.shared.commit_window
        );
    }

    /// Missing file → loader error with a clear message. The plugin panics
    /// with this exact string, so testing the loader covers the panic path
    /// without aborting the test binary.
    #[test]
    fn missing_file_yields_clear_error() {
        let err = load_movement_config_from("assets/config/does_not_exist.ron")
            .expect_err("missing file must fail");
        assert!(
            err.contains("Failed to read assets/config/does_not_exist.ron"),
            "error should name the missing path: {}",
            err
        );
    }

    #[test]
    fn malformed_ron_yields_parse_error() {
        let err = parse_movement_config("(shared: (danger_radius: \"not a number\"))", "inline")
            .expect_err("malformed RON must fail");
        assert!(err.contains("Failed to parse inline"), "got: {}", err);
    }

    #[test]
    fn validate_rejects_nonpositive_heal_range() {
        let mut config = MovementConfig::default();
        config.shared.heal_range = 0.0;
        let issues = config.validate().expect_err("heal_range 0 must fail validation");
        assert!(
            issues.iter().any(|i| i.contains("shared.heal_range")),
            "issues should name heal_range: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_out_of_range_center_bias() {
        // center_bias is a [0.0, 1.0] fraction — a value above 1.0 is a config
        // bug (the FREE formation point would over-pull past arena center).
        let mut config = MovementConfig::default();
        config.shared.center_bias = 1.5;
        let issues = config
            .validate()
            .expect_err("out-of-range center_bias must fail");
        assert!(
            issues.iter().any(|i| i.contains("shared.center_bias")),
            "issues should name center_bias: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_wand_range_exceeding_heal_range() {
        // The Priest can only wand-pull toward targets it can still heal — a
        // wand_range beyond heal_range would pull it out of healing range.
        let mut config = MovementConfig::default();
        config.shared.wand_range = config.shared.heal_range + 5.0;
        let issues = config
            .validate()
            .expect_err("wand_range > heal_range must fail");
        assert!(
            issues.iter().any(|i| i.contains("shared.wand_range")),
            "issues should name wand_range: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_directive_ttl_below_commit_window() {
        // A directive that expires before its commitment window closes would
        // strand the healer mid-commitment and stutter movement.
        let mut config = MovementConfig::default();
        config.shared.commit_window = 0.6;
        config.shared.directive_ttl = 0.3;
        let issues = config
            .validate()
            .expect_err("directive_ttl < commit_window must fail");
        assert!(
            issues.iter().any(|i| i.contains("shared.directive_ttl")),
            "issues should name directive_ttl: {:?}",
            issues
        );
    }

    #[test]
    fn defaults_pass_validation() {
        MovementConfig::default()
            .validate()
            .expect("built-in defaults must be internally consistent");
    }

    #[test]
    fn validate_rejects_inverted_mage_range_band() {
        let mut config = MovementConfig::default();
        config.mage.range_band_min = 30.0;
        config.mage.range_band_max = 8.0;
        let issues = config.validate().expect_err("min >= max must fail");
        assert!(
            issues.iter().any(|i| i.contains("mage.range_band_min")),
            "issues should name range_band_min: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_mage_range_band_max_beyond_shot_range() {
        let mut config = MovementConfig::default();
        config.mage.range_band_max = AUTO_SHOT_RANGE + 5.0;
        let issues = config.validate().expect_err("max > AUTO_SHOT_RANGE must fail");
        assert!(
            issues.iter().any(|i| i.contains("mage.range_band_max")),
            "issues should name range_band_max: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_mage_range_band_min_below_safe_distance() {
        let mut config = MovementConfig::default();
        config.mage.range_band_min = SAFE_KITING_DISTANCE - 1.0;
        let issues = config.validate().expect_err("min < SAFE_KITING_DISTANCE must fail");
        assert!(
            issues.iter().any(|i| i.contains("mage.range_band_min")),
            "issues should name range_band_min: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_nonpositive_mage_kite_hold() {
        let mut config = MovementConfig::default();
        config.mage.kite_hold = 0.0;
        let issues = config.validate().expect_err("kite_hold 0 must fail");
        assert!(
            issues.iter().any(|i| i.contains("mage.kite_hold")),
            "issues should name kite_hold: {:?}",
            issues
        );
    }

    #[test]
    fn validate_rejects_mage_directive_ttl_below_commit_window() {
        let mut config = MovementConfig::default();
        config.mage.commit_window = 0.6;
        config.mage.directive_ttl = 0.3;
        let issues = config.validate().expect_err("ttl < commit_window must fail");
        assert!(
            issues.iter().any(|i| i.contains("mage.directive_ttl")),
            "issues should name mage.directive_ttl: {:?}",
            issues
        );
    }

    /// Partial RON files fill missing fields from the struct defaults
    /// (serde(default) at container level) — balance tweaks can override one
    /// value without restating the whole file.
    #[test]
    fn partial_ron_uses_defaults() {
        let config = parse_movement_config("(shared: (danger_radius: 15.0))", "inline")
            .expect("partial config must parse");
        assert_eq!(config.shared.danger_radius, 15.0);
        assert_eq!(config.shared.heal_range, 40.0, "unspecified fields use defaults");
    }
}
