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
///
/// Reads [`AiProfiles`] — the resource actually inserted by both the headless
/// runner and `setup_play_match`. It must NOT read `AiProfile`: nothing inserts
/// that any more, so an `Option<Res<AiProfile>>` gate would silently see `None`
/// and never run a `TeamPlan` system.
///
/// True when EITHER team runs `want`, because a head-to-head match needs the
/// system scheduled for the one side that wants it. A system gated this way
/// therefore still has to filter per unit — `CombatContext::ai_profile` is
/// already resolved to the acting unit's own team.
pub fn ai_profile_is(want: AiProfile) -> impl Fn(Option<Res<AiProfiles>>) -> bool {
    // `Option<Res<_>>` so a scene that never inserted the resource simply
    // reports the default rather than panicking.
    move |profile: Option<Res<AiProfiles>>| {
        profile.map_or(want == AiProfile::Legacy, |p| {
            p.team1 == want || p.team2 == want
        })
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

    /// The gate must read the resource that is actually INSERTED (`AiProfiles`),
    /// and must fire when EITHER side wants the profile — otherwise a
    /// head-to-head match would schedule nothing for the TeamPlan side.
    #[test]
    fn the_gate_reads_the_inserted_resource_and_covers_both_sides() {
        use bevy::ecs::system::RunSystemOnce;

        let fires = |profiles: AiProfiles| {
            let gate = ai_profile_is(AiProfile::TeamPlan);
            let mut world = World::new();
            world.insert_resource(profiles);
            world
                .run_system_once(move |p: Option<Res<AiProfiles>>| gate(p))
                .expect("gate system runs")
        };

        assert!(fires(AiProfiles::uniform(AiProfile::TeamPlan)), "uniform TeamPlan");
        assert!(
            fires(AiProfiles { team1: AiProfile::Legacy, team2: AiProfile::TeamPlan }),
            "one side on TeamPlan must still schedule the system"
        );
        assert!(!fires(AiProfiles::uniform(AiProfile::Legacy)), "neither side wants it");
    }
}

/// The AI profile each TEAM runs under.
///
/// **Per-team, because a match-wide profile cannot answer "is the new AI
/// better".** With one profile for the whole match, an A/B compares
/// *both-teams-Legacy* against *both-teams-TeamPlan* — two internally consistent
/// worlds. A win-rate shift then means one comp benefits more from the change
/// than the other, which is a real signal but NOT the question usually being
/// asked. Setting the two teams differently pits the implementations directly
/// against each other on the same seed.
///
/// It also stops both healers solving identical constraints and converging on
/// the same cover, which was visible in a replay as two Priests standing close
/// enough together to be free value for an AoE fear.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AiProfiles {
    pub team1: AiProfile,
    pub team2: AiProfile,
}

impl AiProfiles {
    /// Both teams on the same implementation — the historical behaviour, and
    /// what a bare `ai_profile` in a match config still means.
    pub fn uniform(profile: AiProfile) -> Self {
        Self { team1: profile, team2: profile }
    }

    /// The profile `team` runs under. Out-of-range team ids resolve to team 1
    /// rather than panicking, matching `TeamPlans::for_team`.
    pub fn for_team(&self, team: u8) -> AiProfile {
        if team == 2 { self.team2 } else { self.team1 }
    }

    /// True when the two teams differ — i.e. this match is a head-to-head of the
    /// two implementations rather than a uniform world.
    ///
    /// Currently exercised only by tests (`trace_label` matches exhaustively
    /// instead); kept as the semantic query future consumers should reach for
    /// rather than re-deriving `team1 != team2`.
    pub fn is_head_to_head(&self) -> bool {
        self.team1 != self.team2
    }

    /// Stamp for the decision trace. A uniform match keeps the bare profile name
    /// (so every existing trace consumer and `AiProfile::parse` round-trip still
    /// works); a head-to-head records `team1/team2` so it can never be misread as
    /// uniform.
    ///
    /// `&'static str` from the four literal combinations rather than a leaked
    /// `format!` — the batch runner stamps thousands of traces in one process.
    pub fn trace_label(&self) -> &'static str {
        use AiProfile::*;
        match (self.team1, self.team2) {
            (Legacy, Legacy) => "Legacy",
            (TeamPlan, TeamPlan) => "TeamPlan",
            (Legacy, TeamPlan) => "Legacy/TeamPlan",
            (TeamPlan, Legacy) => "TeamPlan/Legacy",
        }
    }
}

#[cfg(test)]
mod profiles_tests {
    use super::*;

    #[test]
    fn uniform_sets_both_teams() {
        let p = AiProfiles::uniform(AiProfile::TeamPlan);
        assert_eq!(p.for_team(1), AiProfile::TeamPlan);
        assert_eq!(p.for_team(2), AiProfile::TeamPlan);
        assert!(!p.is_head_to_head());
    }

    #[test]
    fn teams_resolve_independently() {
        let p = AiProfiles { team1: AiProfile::TeamPlan, team2: AiProfile::Legacy };
        assert_eq!(p.for_team(1), AiProfile::TeamPlan);
        assert_eq!(p.for_team(2), AiProfile::Legacy);
        assert!(p.is_head_to_head());
    }

    /// A bad team id must not panic mid-match.
    #[test]
    fn out_of_range_team_ids_clamp() {
        let p = AiProfiles { team1: AiProfile::TeamPlan, team2: AiProfile::Legacy };
        for team in [0u8, 3, 255] {
            assert_eq!(p.for_team(team), AiProfile::TeamPlan);
        }
    }

    /// A uniform stamp must stay parseable as a plain profile (existing traces
    /// and configs depend on it); a head-to-head must name both sides.
    #[test]
    fn trace_labels_distinguish_uniform_from_head_to_head() {
        for p in [AiProfile::Legacy, AiProfile::TeamPlan] {
            let label = AiProfiles::uniform(p).trace_label();
            assert_eq!(AiProfile::parse(label).unwrap(), p, "{label}");
        }
        assert_eq!(
            AiProfiles { team1: AiProfile::TeamPlan, team2: AiProfile::Legacy }.trace_label(),
            "TeamPlan/Legacy"
        );
        assert_eq!(
            AiProfiles { team1: AiProfile::Legacy, team2: AiProfile::TeamPlan }.trace_label(),
            "Legacy/TeamPlan"
        );
    }

    /// The default must stay Legacy on both sides — every recorded baseline
    /// depends on it.
    #[test]
    fn default_is_legacy_on_both_sides() {
        let p = AiProfiles::default();
        assert_eq!(p.for_team(1), AiProfile::Legacy);
        assert_eq!(p.for_team(2), AiProfile::Legacy);
    }
}
