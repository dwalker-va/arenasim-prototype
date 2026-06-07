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
//! healer posture state machine (FREE/PRESSURED/ESCAPE/DIP) live in
//! `assets/config/movement.ron` (R9). As of U5 the config is loaded and
//! validated but not yet consumed by any system — posture emitters arrive in
//! U6–U8.
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

/// Position-scorer term weights (one block per healer class in movement.ron).
///
/// Consumed by `combat_core::movement_scoring::score_directions`. A weight of
/// `0.0` disables its term (e.g., `wand_pull: 0.0` for the Paladin, which has
/// no wand). `ally_anchor` and `boundary_penalty` are HARD penalties — they
/// must dominate the sum of all soft terms so a violating candidate can never
/// outscore a non-violating one (enforced by `validate()`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementWeights {
    /// Per visible threat: pull away, weighted by proximity (PRESSURED).
    pub threat_repulsion: f32,
    /// Hard penalty for candidate positions outside heal range of the anchor
    /// ally (PRESSURED constraint).
    pub ally_anchor: f32,
    /// Pull toward the formation point behind the engaged-ally centroid
    /// (FREE, Priest only).
    pub formation_pull: f32,
    /// Hard penalty for candidate positions outside the arena bounds.
    pub boundary_penalty: f32,
    /// Graded penalty for candidate positions approaching arena corners.
    pub corner_penalty: f32,
    /// Low-weight pull toward wand range of the kill target (Priest;
    /// 0.0 disables for Paladin).
    pub wand_pull: f32,
    /// Bonus toward the previously committed direction, applied only AT
    /// re-evaluation while the commitment window is open (R11).
    pub commitment_bonus: f32,
}

impl Default for MovementWeights {
    fn default() -> Self {
        Self {
            threat_repulsion: 3.0,
            ally_anchor: 1000.0,
            formation_pull: 2.0,
            boundary_penalty: 1000.0,
            corner_penalty: 4.0,
            wand_pull: 0.5,
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
            heal_range: 40.0,
            formation_offset: 8.0,
            center_bias: 0.3,
            commit_window: 0.6,
            directive_ttl: 1.0,
            escape_min_window: 0.5,
            urgency_hp_threshold: 0.5,
            anchor_switch_margin: 0.1,
            wand_range: 30.0,
        }
    }
}

/// Priest-specific movement configuration.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PriestMovementConfig {
    pub weights: MovementWeights,
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
        }
    }
}

/// Resource containing all healer-movement weights and thresholds.
///
/// Loaded from `assets/config/movement.ron` at startup (both modes).
/// Access via `Res<MovementConfig>` in systems.
#[derive(Resource, Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementConfig {
    pub shared: SharedMovementConfig,
    pub priest: PriestMovementConfig,
    pub paladin: PaladinMovementConfig,
}

impl MovementConfig {
    /// Check value sanity. Returns the list of violations on failure.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut issues: Vec<String> = Vec::new();
        let s = &self.shared;

        let positives = [
            ("shared.danger_radius", s.danger_radius),
            ("shared.heal_range", s.heal_range),
            ("shared.commit_window", s.commit_window),
            ("shared.directive_ttl", s.directive_ttl),
            ("shared.wand_range", s.wand_range),
            ("paladin.fallback_range", self.paladin.fallback_range),
            ("paladin.dip_budget", self.paladin.dip_budget),
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
        ];
        for (name, value) in non_negatives {
            if value < 0.0 || !value.is_finite() {
                issues.push(format!("{} must be non-negative and finite, got {}", name, value));
            }
        }

        let fractions = [
            ("shared.center_bias", s.center_bias),
            ("shared.urgency_hp_threshold", s.urgency_hp_threshold),
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

        for (class, weights) in [("priest", &self.priest.weights), ("paladin", &self.paladin.weights)] {
            let terms = [
                ("threat_repulsion", weights.threat_repulsion),
                ("ally_anchor", weights.ally_anchor),
                ("formation_pull", weights.formation_pull),
                ("boundary_penalty", weights.boundary_penalty),
                ("corner_penalty", weights.corner_penalty),
                ("wand_pull", weights.wand_pull),
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
            // Hard penalties must dominate the soft terms so a violating
            // candidate can never outscore a non-violating one. Soft-term
            // ceiling: ~3 visible threats at dot/proximity <= 1 each, plus
            // formation/wand/commitment at dot <= 1 each.
            let soft_ceiling = weights.threat_repulsion * 3.0
                + weights.formation_pull
                + weights.wand_pull
                + weights.commitment_bonus
                + weights.corner_penalty;
            for (name, value) in [("ally_anchor", weights.ally_anchor), ("boundary_penalty", weights.boundary_penalty)] {
                if value <= soft_ceiling {
                    issues.push(format!(
                        "{}.weights.{} ({}) is a HARD penalty and must exceed the soft-term \
                         ceiling ({:.1}) to act as a constraint",
                        class, name, value, soft_ceiling
                    ));
                }
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
    fn validate_rejects_soft_hard_penalty() {
        let mut config = MovementConfig::default();
        // A "hard" penalty smaller than the soft-term ceiling is a config
        // bug: the anchor constraint would stop being a constraint.
        config.priest.weights.ally_anchor = 1.0;
        let issues = config.validate().expect_err("weak ally_anchor must fail");
        assert!(
            issues.iter().any(|i| i.contains("priest.weights.ally_anchor")),
            "issues should name the weak hard penalty: {:?}",
            issues
        );
    }

    #[test]
    fn defaults_pass_validation() {
        MovementConfig::default()
            .validate()
            .expect("built-in defaults must be internally consistent");
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
