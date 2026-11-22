//! Reference builds: each class's named builds at the points cap, one deep in
//! each of its trees, and how a config names a build (AS-299).
//!
//! Design: `docs/design/proposals/2026-10-11-talents-and-gauntlet.md` §4,
//! "Sweeps measure nodes and reference builds". A reference build is what an
//! authored Gauntlet opponent carries, what the bot's default policy plays,
//! and what a milestone sweep reports a class's standing with, beside its
//! untalented baseline. `talents.ron` lists them per class under
//! `reference_builds`; [`TalentDefinitions`]'s loader holds each to the rules
//! [`validate`] names.
//!
//! A headless config names a slot's build one of three ways ([`BuildSpec`]):
//! the ranks themselves, a reference build of the slot's class by name, or a
//! reference build with ranks taken back and added — the node-arm instrument
//! of design §4, "a reference build with its last point moved onto that
//! node". [`TalentDefinitions::resolve_spec`] turns any of them into a legal
//! [`Build`] for the slot's class, or says why it cannot.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::talents::{Build, TalentDefinitions, TreeId, CAPSTONE_TIER, MAX_TALENT_POINTS};
use crate::states::match_config::CharacterClass;

/// One named build of a class: legal, at exactly [`MAX_TALENT_POINTS`], and
/// deep in `tree` (it takes that tree's capstone).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReferenceBuild {
    /// What a config and a sweep name it by, unique within its class.
    pub name: String,
    /// The tree it is deep in.
    pub tree: TreeId,
    /// Its ranks.
    pub build: Build,
}

/// A slot's build as a headless config writes it.
///
/// - `{"DemonicEmbrace": 2, "FelStamina": 2}`: the ranks themselves;
/// - `"Demonology"`: the slot's class's reference build of that name;
/// - `{"reference": "Demonology", "remove": {"SoulLink": 1}, "add":
///   {"DemonicEmbrace": 1}}`: that reference build with the ranks `remove`
///   names taken back, then the ranks `add` names learned ([`DerivedBuild`]).
///
/// Every form resolves to a [`Build`] the tree rules allow, checked against
/// the slot's class before the match ([`TalentDefinitions::resolve_spec`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum BuildSpec {
    Ranks(Build),
    Reference(String),
    Derived(DerivedBuild),
}

impl Default for BuildSpec {
    /// No talents.
    fn default() -> Self {
        BuildSpec::Ranks(Build::default())
    }
}

impl From<Build> for BuildSpec {
    fn from(build: Build) -> Self {
        BuildSpec::Ranks(build)
    }
}

/// A reference build with ranks moved: `remove` is taken back first, then
/// `add` is learned. Only the result must be legal, so a node a higher tier
/// leans on may be removed as long as what is added keeps the tier open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedBuild {
    /// The class's reference build it starts from, by name.
    pub reference: String,
    /// Ranks taken back from it.
    #[serde(default)]
    pub remove: Build,
    /// Ranks learned on top.
    #[serde(default)]
    pub add: Build,
}

impl<'de> Deserialize<'de> for BuildSpec {
    /// A string is a reference; an object with a `reference` key is derived
    /// from one; any other object is the ranks. Deserialized through a JSON
    /// value so each form reports its own error ("unknown variant `Nope`")
    /// rather than an untagged enum's "did not match any variant".
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(name) => Ok(BuildSpec::Reference(name)),
            serde_json::Value::Object(ref map) if map.contains_key("reference") => {
                serde_json::from_value(value)
                    .map(BuildSpec::Derived)
                    .map_err(D::Error::custom)
            }
            serde_json::Value::Object(_) => serde_json::from_value(value)
                .map(BuildSpec::Ranks)
                .map_err(D::Error::custom),
            other => Err(D::Error::custom(format!(
                "a build is a map of talent to rank, a reference build's name, or \
                 {{\"reference\": .., \"remove\": .., \"add\": ..}}; got {other}"
            ))),
        }
    }
}

/// Hold every reference build in `defs` to its rules: its class has trees;
/// its name is not empty and is unique within the class; its tree is one of
/// the class's; it is a legal build of the class
/// ([`TalentDefinitions::validate_build`]) spending exactly
/// [`MAX_TALENT_POINTS`]; and it is deep in its tree, taking that tree's
/// capstone. A class key that is not a class fails the parse before this
/// runs.
pub(super) fn validate(defs: &TalentDefinitions) -> Result<(), String> {
    for (class, references) in defs.all_reference_builds() {
        if defs.trees(class).is_empty() {
            return Err(format!(
                "{class:?} has reference builds but no talent trees"
            ));
        }
        let mut names = BTreeSet::new();
        for reference in references {
            let at = |problem: String| {
                format!(
                    "{class:?}'s reference build {:?}: {problem}",
                    reference.name
                )
            };
            if reference.name.trim().is_empty() {
                return Err(format!("{class:?} has a reference build with no name"));
            }
            if !names.insert(reference.name.as_str()) {
                return Err(format!(
                    "{class:?} names the reference build {:?} twice",
                    reference.name
                ));
            }
            if !defs.trees(class).iter().any(|t| t.id == reference.tree) {
                return Err(at(format!(
                    "its tree {:?} is not one of {class:?}'s",
                    reference.tree
                )));
            }
            defs.validate_build(class, &reference.build).map_err(at)?;
            if reference.build.points() != MAX_TALENT_POINTS {
                return Err(at(format!(
                    "spends {} points; a reference build spends exactly {MAX_TALENT_POINTS}",
                    reference.build.points()
                )));
            }
            let capstones: Vec<_> = defs
                .talents_for_class(class)
                .filter(|(_, t)| t.tree == reference.tree && t.tier == CAPSTONE_TIER)
                .map(|(id, _)| id)
                .collect();
            if capstones.is_empty() {
                return Err(at(format!(
                    "its tree {:?} has no capstone to be deep in",
                    reference.tree
                )));
            }
            if !capstones.iter().any(|id| reference.build.rank(*id) > 0) {
                return Err(at(format!(
                    "is not deep in {:?}: it takes none of its capstones ({capstones:?})",
                    reference.tree
                )));
            }
        }
    }
    Ok(())
}

impl TalentDefinitions {
    /// `class`'s reference build named `name`.
    pub fn reference_build(&self, class: CharacterClass, name: &str) -> Option<&ReferenceBuild> {
        self.reference_builds(class).iter().find(|r| r.name == name)
    }

    /// The build `spec` names for a combatant of `class`, checked against the
    /// tree rules: the ranks as written, the named reference build, or that
    /// reference build with its ranks moved. `Err` names what is wrong: an
    /// unknown reference (with the class's names), a rank removed that the
    /// reference does not hold, or the illegal result.
    pub fn resolve_spec(&self, class: CharacterClass, spec: &BuildSpec) -> Result<Build, String> {
        let build = match spec {
            BuildSpec::Ranks(build) => build.clone(),
            BuildSpec::Reference(name) => self.named_reference(class, name)?.build.clone(),
            BuildSpec::Derived(derived) => {
                let mut build = self
                    .named_reference(class, &derived.reference)?
                    .build
                    .clone();
                for (id, take) in &derived.remove.ranks {
                    let held = build.rank(*id);
                    if *take > held {
                        return Err(format!(
                            "the reference build {:?} takes {id:?} at rank {held}; {take} cannot \
                             be removed",
                            derived.reference
                        ));
                    }
                    if *take == held {
                        build.ranks.remove(id);
                    } else {
                        build.ranks.insert(*id, held - take);
                    }
                }
                for (id, learn) in &derived.add.ranks {
                    let rank = build.rank(*id).saturating_add(*learn);
                    if rank > 0 {
                        build.ranks.insert(*id, rank);
                    }
                }
                build
            }
        };
        self.validate_build(class, &build)?;
        Ok(build)
    }

    fn named_reference(
        &self,
        class: CharacterClass,
        name: &str,
    ) -> Result<&ReferenceBuild, String> {
        self.reference_build(class, name).ok_or_else(|| {
            let names: Vec<&str> = self
                .reference_builds(class)
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            if names.is_empty() {
                format!("{class:?} has no reference builds, so none is named {name:?}")
            } else {
                format!(
                    "{class:?} has no reference build {name:?} (it has {})",
                    names.join(", ")
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_deserializes_each_form_and_reports_its_own_error() {
        let parse = |json: &str| serde_json::from_str::<BuildSpec>(json);
        assert_eq!(
            parse(r#""Demonology""#).unwrap(),
            BuildSpec::Reference("Demonology".into())
        );
        assert!(matches!(
            parse(r#"{"DemonicEmbrace": 2}"#).unwrap(),
            BuildSpec::Ranks(_)
        ));
        assert!(matches!(
            parse(r#"{"reference": "Demonology", "add": {"DemonicEmbrace": 1}}"#).unwrap(),
            BuildSpec::Derived(DerivedBuild { ref remove, .. }) if remove.is_empty()
        ));
        assert_eq!(parse("{}").unwrap(), BuildSpec::default());
        let nope = parse(r#"{"Nope": 1}"#).unwrap_err().to_string();
        assert!(nope.contains("Nope"), "{nope}");
        let typo = parse(r#"{"reference": "Demonology", "ad": {}}"#)
            .unwrap_err()
            .to_string();
        assert!(typo.contains("ad"), "{typo}");
        assert!(parse("3").is_err());
    }

    #[test]
    fn a_spec_serializes_in_the_form_it_was_written() {
        for json in [
            r#""Affliction""#,
            r#"{"DemonicEmbrace":2}"#,
            r#"{"reference":"Destruction","remove":{"Conflagrate":1},"add":{"Shadowburn":1}}"#,
        ] {
            let spec: BuildSpec = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&spec).unwrap(), json);
        }
    }
}
