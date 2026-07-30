//! Which AI implementation a match runs under.
//!
//! The escape hatch for evolving the AI without leaving the game in a broken
//! state. `Legacy` is the reactive, per-unit AI that every balance baseline and
//! movement probe is calibrated against; `TeamPlan` is the team-level positioning
//! layer described in `design-docs/team-level-positioning-ai.md`. Both live in the
//! same build, selected per match, so the two can be A/B'd against identical seeds.
//!
//! ## Why this exists
//!
//! `cover_seek` (healer navigation to distant cover) was first landed
//! unconditionally and immediately drifted 6 fixed-seed probes on *every* map with
//! obstacles — not because it was wrong, but because any behaviour change
//! re-rolls what a given seed exercises. Gating it made all 97 probes pass again.
//! Every subsequent AI behaviour should arrive behind this flag for the same
//! reason.
//!
//! ## The A/B is paired
//!
//! Matches are deterministic at a fixed seed, so running the same seed set and
//! comps under both profiles varies *only* the AI. That is a paired comparison,
//! not two independent samples — materially more sensitive, so real effects show
//! up with far fewer runs than an unpaired sweep would need.
//!
//! ## Adding a behaviour
//!
//! Prefer a **system-level** swap: register the `Legacy` and `TeamPlan` producers
//! as separate systems, each `.run_if(...)` on the profile, sharing the downstream
//! executor (`move_to_target` consumes `MovementDirective` without caring who
//! produced it — that component is the natural seam). A gated-off system does not
//! run at all and therefore draws no `GameRng`, which is what keeps determinism
//! intact; a branch *inside* a system that consumes RNG differently per profile
//! would not be safe.
//!
//! Value-level gates (reading [`AiProfile`] and returning early) are acceptable
//! for a single decision inside an existing system, which is how `cover_seek` is
//! gated today. They do not scale to whole subsystems.

use bevy::prelude::*;

/// The AI implementation for the current match. Inserted in BOTH modes — the
/// headless runner and the graphical stack — like every other match-scoped
/// config resource.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AiProfile {
    /// The reactive, per-unit AI. Every recorded balance baseline and every
    /// movement probe is calibrated against this, so it is the default and must
    /// stay byte-identical.
    #[default]
    Legacy,
    /// The team-level positioning layer. Opt-in while it is built out.
    TeamPlan,
}

impl AiProfile {
    /// Parse from a config string. Accepts the RON/CLI spellings.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "Legacy" | "legacy" => Ok(AiProfile::Legacy),
            "TeamPlan" | "teamplan" | "team_plan" => Ok(AiProfile::TeamPlan),
            other => Err(format!(
                "Unknown ai_profile: '{other}'. Valid profiles: Legacy, TeamPlan"
            )),
        }
    }

    /// Stable string for the decision trace and log lines. Must stay in sync with
    /// the `parse` spellings so a trace can be fed back as config.
    pub fn name(&self) -> &'static str {
        match self {
            AiProfile::Legacy => "Legacy",
            AiProfile::TeamPlan => "TeamPlan",
        }
    }

    /// Whether team-level behaviours are active.
    pub fn is_team_plan(&self) -> bool {
        *self == AiProfile::TeamPlan
    }
}

/// Run condition for gating whole systems on a profile.
///
/// ```ignore
/// .add_systems(Update, team_plan_postures.run_if(ai_profile_is(AiProfile::TeamPlan)))
/// ```
pub fn ai_profile_is(want: AiProfile) -> impl Fn(Option<Res<AiProfile>>) -> bool {
    // `Option<Res<_>>` so a scene that never inserted the resource simply
    // reports the default rather than panicking.
    move |profile: Option<Res<AiProfile>>| {
        profile.map_or(want == AiProfile::Legacy, |p| *p == want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_is_the_default() {
        assert_eq!(AiProfile::default(), AiProfile::Legacy);
        assert!(!AiProfile::default().is_team_plan());
    }

    /// `name()` must round-trip through `parse()` — the trace stamp doubles as a
    /// config value, so a drift between them would make traces unreplayable.
    #[test]
    fn name_round_trips_through_parse() {
        for p in [AiProfile::Legacy, AiProfile::TeamPlan] {
            assert_eq!(AiProfile::parse(p.name()).unwrap(), p, "{}", p.name());
        }
    }

    #[test]
    fn parses_config_spellings() {
        assert_eq!(AiProfile::parse("Legacy").unwrap(), AiProfile::Legacy);
        assert_eq!(AiProfile::parse("legacy").unwrap(), AiProfile::Legacy);
        assert_eq!(AiProfile::parse("TeamPlan").unwrap(), AiProfile::TeamPlan);
        assert_eq!(AiProfile::parse("team_plan").unwrap(), AiProfile::TeamPlan);
        let err = AiProfile::parse("Nonsense").expect_err("must reject");
        assert!(err.contains("Legacy") && err.contains("TeamPlan"), "{err}");
    }

    /// A missing resource must read as `Legacy`, so a scene that forgets to insert
    /// it cannot silently opt into experimental behaviour.
    #[test]
    fn absent_resource_defaults_to_legacy() {
        let legacy_gate = ai_profile_is(AiProfile::Legacy);
        let team_gate = ai_profile_is(AiProfile::TeamPlan);
        assert!(legacy_gate(None), "absent resource should satisfy Legacy");
        assert!(!team_gate(None), "absent resource must NOT satisfy TeamPlan");
    }
}
