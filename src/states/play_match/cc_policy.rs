//! Which CC-selection model a team runs under.
//!
//! See `design-docs/cc-value-model.md`. Deliberately a **separate axis from
//! [`AiProfile`](super::ai_profile::AiProfile)**, and the reason is worth stating
//! because it was originally planned the other way round.
//!
//! `AiProfile::Legacy` is the default, is what the graphical client runs, and is
//! what every balance baseline and movement probe is calibrated against. Gating
//! CC work behind `TeamPlan` would have left the measured defect — the Warlock's
//! healer lockout switching itself off whenever the Warlock is assigned to damage
//! the healer — unfixed in the profile that actually ships, indefinitely, waiting
//! on a build-out that is paused. It would also have inherited a dependency on
//! stance transitions that production never produces (`Stance::Withdraw` is never
//! assigned outside tests).
//!
//! So this is orthogonal: measure the CC change under `Legacy`, with no
//! positioning confound, and treat `TeamPlan × Priced` as a separate confirmation
//! cell rather than the headline.
//!
//! Per-team for the same reason `AiProfiles` is per-team: a uniform A/B compares
//! two internally consistent worlds and cannot answer "is this better".
//!
//! ## Why there are two policies, and why there will not be six
//!
//! Flags are split **per decision site, not per migration step**.
//!
//! [`CcPolicy`] governs *which enemy to crowd-control*; [`InterruptPolicy`]
//! governs *which cast to interrupt*. Those are two different decisions, taken
//! by different classes through different abilities, and the code paths never
//! consult each other — so they are independently measurable, and measurement
//! says they must be: step 1 (CC targeting) measured **+10pt, z=2.56** while
//! step 2 (interrupt targeting) measured **-5pt, z=-0.81**. One flag would force
//! shipping the loss to get the win.
//!
//! What must NOT happen is a flag per step. The remaining migration steps —
//! `D` (denial rate), `C` (cost), `E` (enabling) — are all *terms in the same
//! CC-target score*. They interact by construction, so measuring them behind
//! separate flags would be measuring nonsense, and six flags would be sixty-four
//! configurations. They land on [`CcPolicy`] and are measured incrementally
//! against it.

use bevy::prelude::*;

/// How a team decides which enemy to crowd-control.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CcPolicy {
    /// Today's behaviour: CC targets are chosen by role identity
    /// (`class.is_healer()`) and fixed guards. The default, and what the
    /// canonical baselines are measured against.
    #[default]
    Identity,
    /// CC targets are chosen by expected denial — `cc_value::predict_t_eff` and
    /// the terms around it.
    Priced,
}

impl CcPolicy {
    /// Parse from a config string. Accepts the RON/CLI spellings.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "Identity" | "identity" => Ok(CcPolicy::Identity),
            "Priced" | "priced" => Ok(CcPolicy::Priced),
            other => Err(format!(
                "Unknown cc_policy: '{other}'. Valid policies: Identity, Priced"
            )),
        }
    }

    /// Stable string for trace and log lines. Must stay in sync with `parse`.
    pub fn name(&self) -> &'static str {
        match self {
            CcPolicy::Identity => "Identity",
            CcPolicy::Priced => "Priced",
        }
    }

    pub fn is_priced(&self) -> bool {
        *self == CcPolicy::Priced
    }
}

/// The per-team CC policy for the current match. Inserted in BOTH modes, like
/// every other match-scoped config resource.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CcPolicies {
    pub team1: CcPolicy,
    pub team2: CcPolicy,
}

impl CcPolicies {
    pub fn uniform(policy: CcPolicy) -> Self {
        Self { team1: policy, team2: policy }
    }

    /// The policy `team` runs under. Out-of-range team ids resolve to team 1
    /// rather than panicking, matching `AiProfiles::for_team`.
    pub fn for_team(&self, team: u8) -> CcPolicy {
        if team == 2 { self.team2 } else { self.team1 }
    }

    pub fn is_head_to_head(&self) -> bool {
        self.team1 != self.team2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_the_default() {
        assert_eq!(CcPolicy::default(), CcPolicy::Identity);
        assert!(!CcPolicy::default().is_priced());
        assert_eq!(CcPolicies::default().for_team(1), CcPolicy::Identity);
        assert_eq!(CcPolicies::default().for_team(2), CcPolicy::Identity);
    }

    #[test]
    fn name_round_trips_through_parse() {
        for p in [CcPolicy::Identity, CcPolicy::Priced] {
            assert_eq!(CcPolicy::parse(p.name()).unwrap(), p, "{}", p.name());
        }
    }

    #[test]
    fn per_team_resolution_and_head_to_head() {
        let mixed = CcPolicies { team1: CcPolicy::Priced, team2: CcPolicy::Identity };
        assert_eq!(mixed.for_team(1), CcPolicy::Priced);
        assert_eq!(mixed.for_team(2), CcPolicy::Identity);
        assert!(mixed.is_head_to_head());
        // Out-of-range ids resolve to team 1, never panic.
        assert_eq!(mixed.for_team(0), CcPolicy::Priced);
        assert_eq!(mixed.for_team(7), CcPolicy::Priced);
        assert!(!CcPolicies::uniform(CcPolicy::Priced).is_head_to_head());
    }

    #[test]
    fn rejects_unknown_spellings() {
        assert!(CcPolicy::parse("Value").is_err());
        assert!(CcPolicy::parse("").is_err());
    }
}

/// How a team decides which enemy *cast* to interrupt.
///
/// A separate axis from [`CcPolicy`] because it is a separate decision — see the
/// module docs. Kept at [`InterruptPolicy::Identity`] by default and by
/// recommendation: the priced variant is implemented and measured, and it does
/// **not** beat the identity heuristic it replaces (`-5pt, z=-0.81`), because
/// the value of an interrupt is mostly its school lockout and the model prices
/// that from the cancelled cast rather than from the target's throughput. See
/// the design doc's *Step 2 result*.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InterruptPolicy {
    /// Today's behaviour: Warriors and Rogues interrupt whatever their own kill
    /// target is casting; the Shaman prefers a healer mid-cast. The default.
    #[default]
    Identity,
    /// Scan every interruptible cast in range and pick by expected denial.
    Priced,
}

impl InterruptPolicy {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "Identity" | "identity" => Ok(InterruptPolicy::Identity),
            "Priced" | "priced" => Ok(InterruptPolicy::Priced),
            other => Err(format!(
                "Unknown interrupt_policy: '{other}'. Valid policies: Identity, Priced"
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            InterruptPolicy::Identity => "Identity",
            InterruptPolicy::Priced => "Priced",
        }
    }

    pub fn is_priced(&self) -> bool {
        *self == InterruptPolicy::Priced
    }
}

/// The per-team interrupt policy for the current match. Inserted in BOTH modes.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InterruptPolicies {
    pub team1: InterruptPolicy,
    pub team2: InterruptPolicy,
}

impl InterruptPolicies {
    pub fn uniform(policy: InterruptPolicy) -> Self {
        Self { team1: policy, team2: policy }
    }

    /// The policy `team` runs under. Out-of-range team ids resolve to team 1
    /// rather than panicking, matching `AiProfiles::for_team`.
    pub fn for_team(&self, team: u8) -> InterruptPolicy {
        if team == 2 { self.team2 } else { self.team1 }
    }

    pub fn is_head_to_head(&self) -> bool {
        self.team1 != self.team2
    }
}

#[cfg(test)]
mod interrupt_policy_tests {
    use super::*;

    #[test]
    fn identity_is_the_default() {
        assert_eq!(InterruptPolicy::default(), InterruptPolicy::Identity);
        assert!(!InterruptPolicies::default().for_team(1).is_priced());
        assert!(!InterruptPolicies::default().for_team(2).is_priced());
    }

    #[test]
    fn name_round_trips_through_parse() {
        for p in [InterruptPolicy::Identity, InterruptPolicy::Priced] {
            assert_eq!(InterruptPolicy::parse(p.name()).unwrap(), p, "{}", p.name());
        }
    }

    #[test]
    fn per_team_resolution_and_head_to_head() {
        let mixed = InterruptPolicies {
            team1: InterruptPolicy::Priced,
            team2: InterruptPolicy::Identity,
        };
        assert_eq!(mixed.for_team(1), InterruptPolicy::Priced);
        assert_eq!(mixed.for_team(2), InterruptPolicy::Identity);
        assert!(mixed.is_head_to_head());
        assert_eq!(mixed.for_team(0), InterruptPolicy::Priced);
        assert!(!InterruptPolicies::uniform(InterruptPolicy::Priced).is_head_to_head());
    }

    /// The two axes must be genuinely independent — setting one must not move
    /// the other, or the split has bought nothing.
    #[test]
    fn the_two_policies_are_separate_types_with_separate_defaults() {
        let cc = CcPolicies::uniform(CcPolicy::Priced);
        let interrupts = InterruptPolicies::default();
        assert!(cc.for_team(1).is_priced());
        assert!(!interrupts.for_team(1).is_priced());
    }

    #[test]
    fn rejects_unknown_spellings() {
        assert!(InterruptPolicy::parse("Value").is_err());
        assert!(InterruptPolicy::parse("").is_err());
    }
}
