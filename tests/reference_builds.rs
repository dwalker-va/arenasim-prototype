//! Reference builds (AS-299): each class's named builds at the points cap,
//! one deep in each tree, held to their rules by the loader, and named by a
//! headless config with or without ranks moved.
//!
//! Design: `docs/design/proposals/2026-10-11-talents-and-gauntlet.md` §4.

use std::collections::BTreeSet;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::reference_builds::BuildSpec;
use arenasim::states::play_match::talents::{
    Build, TalentDefinitions, TalentId, TreeId, CAPSTONE_TIER, MAX_TALENT_POINTS,
};

use TalentId::*;

fn shipped_text() -> String {
    std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads")
}

fn defs() -> &'static TalentDefinitions {
    TalentDefinitions::shipped()
}

/// The shipped talents file with `from` replaced by `to` (which must occur
/// exactly once).
fn edited(from: &str, to: &str) -> String {
    let text = shipped_text();
    assert_eq!(
        text.matches(from).count(),
        1,
        "the shipped file has one {from:?}"
    );
    text.replacen(from, to, 1)
}

fn load_err(text: &str) -> String {
    TalentDefinitions::from_ron(text, AbilityDefinitions::shipped())
        .expect_err("the edit is rejected")
}

// ---------------------------------------------------------------------------
// The shipped builds
// ---------------------------------------------------------------------------

/// Every class's reference builds are legal at exactly the cap, each deep in
/// the tree it names (its capstone taken, and the most points there), and the
/// class's references name each of its trees exactly once.
#[test]
fn every_reference_build_is_legal_at_the_cap_and_deep_in_its_tree() {
    let defs = defs();
    let mut classes = 0;
    for (class, references) in defs.all_reference_builds() {
        classes += 1;
        for reference in references {
            let at = format!("{class:?} {:?}", reference.name);
            defs.validate_build(class, &reference.build)
                .unwrap_or_else(|e| panic!("{at}: {e}"));
            assert_eq!(reference.build.points(), MAX_TALENT_POINTS, "{at}");
            let capstone = defs
                .talents_for_class(class)
                .find(|(_, t)| t.tree == reference.tree && t.tier == CAPSTONE_TIER)
                .map(|(id, _)| id)
                .unwrap_or_else(|| panic!("{at}: its tree has no capstone"));
            assert_eq!(reference.build.rank(capstone), 1, "{at} takes {capstone:?}");
            let points = defs.tree_points(class, &reference.build);
            let deepest = points.iter().max_by_key(|(_, p)| *p).unwrap();
            let named = points.iter().find(|(t, _)| *t == reference.tree).unwrap();
            assert_eq!(named.1, deepest.1, "{at}: deepest in {:?}", reference.tree);
            assert!(
                points
                    .iter()
                    .filter(|(t, _)| *t != reference.tree)
                    .all(|(_, p)| *p < named.1),
                "{at}: no other tree ties {:?}",
                reference.tree
            );
        }
        let named: Vec<TreeId> = references.iter().map(|r| r.tree).collect();
        let trees: Vec<TreeId> = defs.trees(class).iter().map(|t| t.id).collect();
        assert_eq!(
            named.iter().copied().collect::<BTreeSet<_>>(),
            trees.iter().copied().collect::<BTreeSet<_>>(),
            "{class:?}'s references name every tree"
        );
        assert_eq!(named.len(), trees.len(), "{class:?}: one reference a tree");
    }
    assert!(classes >= 1, "the shipped table has reference builds");
}

/// The Warlock's three, by name, at design §5.1's shapes: Affliction 6/0/2,
/// Demonology 2/6/0, Destruction 0/2/6.
#[test]
fn the_warlock_reference_builds_are_as_specified() {
    let defs = defs();
    let names: Vec<&str> = defs
        .reference_builds(CharacterClass::Warlock)
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["Affliction", "Demonology", "Destruction"]);
    let expect = [
        (
            "Affliction",
            TreeId::Affliction,
            "6/0/2",
            Build::of(&[
                (ImprovedDrainLife, 2),
                (Nightfall, 2),
                (SiphonLife, 1),
                (ShadowMastery, 1),
                (Bane, 2),
            ]),
        ),
        (
            "Demonology",
            TreeId::Demonology,
            "2/6/0",
            Build::of(&[
                (ImprovedDrainLife, 2),
                (DemonicEmbrace, 1),
                (FelStamina, 2),
                (UnholyPower, 1),
                (MasterDemonologist, 1),
                (SoulLink, 1),
            ]),
        ),
        (
            "Destruction",
            TreeId::Destruction,
            "0/2/6",
            Build::of(&[
                (ImprovedShadowBolt, 2),
                (Bane, 2),
                (ImprovedImmolate, 1),
                (Conflagrate, 1),
                (FelStamina, 2),
            ]),
        ),
    ];
    for (name, tree, short, build) in expect {
        let reference = defs
            .reference_build(CharacterClass::Warlock, name)
            .unwrap_or_else(|| panic!("no Warlock reference {name:?}"));
        assert_eq!(reference.tree, tree, "{name}");
        assert_eq!(reference.build, build, "{name}");
        let spec = defs
            .spec(CharacterClass::Warlock, &reference.build)
            .unwrap();
        assert_eq!(spec.short, short, "{name}");
    }
    assert!(defs
        .reference_build(CharacterClass::Warlock, "Fire")
        .is_none());
    assert!(defs
        .reference_build(CharacterClass::Mage, "Affliction")
        .is_none());
}

// ---------------------------------------------------------------------------
// The loader
// ---------------------------------------------------------------------------

const AFFLICTION: &str = "name: \"Affliction\",\n                tree: Affliction,";
const DEMONOLOGY_NAME: &str = "name: \"Demonology\",\n                tree: Demonology,";
const AFFLICTION_RANKS: &str =
    "ImprovedDrainLife: 2, Nightfall: 2, SiphonLife: 1, ShadowMastery: 1,";
const DESTRUCTION_RANKS: &str =
    "ImprovedShadowBolt: 2, Bane: 2, ImprovedImmolate: 1, Conflagrate: 1,";

/// Each rule a reference build is held to, broken one at a time, is refused
/// with its reason.
#[test]
fn the_loader_rejects_each_broken_reference_build() {
    let cases: &[(&str, &str, &str)] = &[
        // A class key that is not a class is a parse error.
        (
            "reference_builds: {\n        Warlock: [",
            "reference_builds: {\n        Wizard: [",
            "parse error",
        ),
        // A class with no trees has none to be deep in.
        (
            "reference_builds: {\n        Warlock: [",
            "reference_builds: {\n        Mage: [",
            "Mage has reference builds but no talent trees",
        ),
        // Two references of one class by one name.
        (
            DEMONOLOGY_NAME,
            "name: \"Affliction\",\n                tree: Demonology,",
            "names the reference build \"Affliction\" twice",
        ),
        (
            AFFLICTION,
            "name: \"  \",\n                tree: Affliction,",
            "a reference build with no name",
        ),
        // A tree the class does not have.
        (
            AFFLICTION,
            "name: \"Affliction\",\n                tree: Holy,",
            "its tree Holy is not one of Warlock's",
        ),
        // An illegal build: Conflagrate without Improved Immolate.
        (
            DESTRUCTION_RANKS,
            "ImprovedShadowBolt: 2, Bane: 2, Aftermath: 1, Conflagrate: 1,",
            "Conflagrate requires ImprovedImmolate",
        ),
        // Seven points, and nine.
        (
            "ShadowMastery: 1,\n                    Bane: 2,",
            "ShadowMastery: 1,\n                    Bane: 1,",
            "spends 7 points",
        ),
        (
            AFFLICTION_RANKS,
            "ImprovedDrainLife: 2, Nightfall: 2, SiphonLife: 1, ShadowMastery: 1, AmplifyCurse: 1,",
            "spends at most 8",
        ),
        // Eight points, but not deep in the tree it names: Shadow Mastery's
        // point moved onto Curse of Exhaustion.
        (
            AFFLICTION_RANKS,
            "ImprovedDrainLife: 2, Nightfall: 2, SiphonLife: 1, CurseOfExhaustion: 1,",
            "is not deep in Affliction",
        ),
        // Deep in another tree than it names.
        (
            AFFLICTION,
            "name: \"Affliction\",\n                tree: Destruction,",
            "is not deep in Destruction",
        ),
    ];
    for (from, to, expect) in cases {
        let err = load_err(&edited(from, to));
        assert!(
            err.contains(expect),
            "{to:?}: expected {expect:?}, got {err}"
        );
    }
    TalentDefinitions::from_ron(&shipped_text(), AbilityDefinitions::shipped())
        .expect("the shipped file loads");
}

/// A file with no `reference_builds` loads, with none for any class.
#[test]
fn reference_builds_are_optional() {
    let text = shipped_text();
    let start = text.find("    reference_builds: {").expect("the block");
    let end = text.rfind(')').unwrap();
    let without = format!("{}{}", &text[..start], &text[end..]);
    let defs = TalentDefinitions::from_ron(&without, AbilityDefinitions::shipped())
        .expect("loads without reference builds");
    assert!(defs.reference_builds(CharacterClass::Warlock).is_empty());
    assert_eq!(defs.all_reference_builds().count(), 0);
}

// ---------------------------------------------------------------------------
// Naming a build from a config
// ---------------------------------------------------------------------------

fn parse(json: &str) -> HeadlessMatchConfig {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn team1_build(json: &str) -> Result<Build, String> {
    parse(json)
        .to_match_config()
        .map(|c| c.team1_builds[0].clone())
}

/// What `--batch` says about a one-line batch of `json`: it validates every
/// line before it plays any match.
fn batch_err(json: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("batch.jsonl");
    std::fs::write(&input, json.replace('\n', " ")).unwrap();
    arenasim::headless::batch::run_batch(input, dir.path().join("out.csv"), Some(1))
        .expect_err("the batch is refused at load")
}

fn reference(name: &str) -> Build {
    defs()
        .reference_build(CharacterClass::Warlock, name)
        .unwrap()
        .build
        .clone()
}

/// A config names a reference build by its name, and a reference build with
/// ranks moved, and each resolves to the build it names.
#[test]
fn a_config_names_a_reference_build_and_moves_its_ranks() {
    for name in ["Affliction", "Demonology", "Destruction"] {
        let json = format!(
            r#"{{"team1":["Warlock","Priest"],"team2":["Warlock"],
                "team1_builds":["{name}"],"team2_builds":["{name}"]}}"#
        );
        let config = parse(&json).to_match_config().unwrap();
        assert_eq!(config.team1_builds, vec![reference(name), Build::default()]);
        assert_eq!(config.team2_builds, vec![reference(name)]);
    }

    // Soul Link's point moved onto Demonic Embrace.
    let moved = team1_build(
        r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Demonology",
            "remove":{"SoulLink":1},"add":{"DemonicEmbrace":1}}]}"#,
    )
    .unwrap();
    let mut expect = reference("Demonology");
    expect.ranks.remove(&SoulLink);
    expect.ranks.insert(DemonicEmbrace, 2);
    assert_eq!(moved, expect);

    // A rank taken back from a two-rank node leaves the other.
    let one_less = team1_build(
        r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Affliction",
            "remove":{"Bane":1}}]}"#,
    )
    .unwrap();
    assert_eq!(one_less.rank(Bane), 1);
    assert_eq!(one_less.points(), MAX_TALENT_POINTS - 1);

    // Removing a node a higher tier leans on is legal when what is added
    // keeps the tier open: Master Demonologist's point onto Demonic Embrace.
    let regated = team1_build(
        r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Demonology",
            "remove":{"MasterDemonologist":1},"add":{"DemonicEmbrace":1}}]}"#,
    )
    .unwrap();
    assert_eq!(regated.rank(MasterDemonologist), 0);
    assert_eq!(regated.rank(SoulLink), 1);

    // The plain map is unchanged.
    assert_eq!(
        team1_build(r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"FelStamina":2}]}"#)
            .unwrap(),
        Build::of(&[(FelStamina, 2)])
    );
}

/// Each way a named build can be wrong is refused before the match, with its
/// slot and reason, and by `validate` — so a batch fails at load, not one
/// match at a time.
#[test]
fn a_bad_named_build_is_refused_with_its_slot() {
    let cases: &[(&str, &str)] = &[
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":["Fire"]}"#,
            "team1_builds slot 0 (Warlock): Warlock has no reference build \"Fire\" (it has \
             Affliction, Demonology, Destruction)",
        ),
        (
            r#"{"team1":["Mage"],"team2":["Mage"],"team1_builds":["Affliction"]}"#,
            "Mage has no reference builds",
        ),
        (
            r#"{"team1":["Warlock"],"team2":["Mage","Warlock"],"team2_builds":[{},"Demonology",{}]}"#,
            "team2_builds has 3 entries but team2 has only 2 members",
        ),
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Destruction",
                "remove":{"SoulLink":1}}]}"#,
            "takes SoulLink at rank 0; 1 cannot be removed",
        ),
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Destruction",
                "remove":{"ImprovedImmolate":1},"add":{"Shadowburn":1}}]}"#,
            "Conflagrate",
        ),
        (
            r#"{"team1":["Warlock"],"team2":["Mage"],"team1_builds":[{"reference":"Destruction",
                "add":{"Shadowburn":1}}]}"#,
            "spends at most 8",
        ),
        (
            r#"{"team1":["Mage","Warlock"],"team2":["Mage"],"team1_builds":[{},{"reference":"Nope"}]}"#,
            "team1_builds slot 1 (Warlock): Warlock has no reference build \"Nope\"",
        ),
    ];
    for (json, expect) in cases {
        let config = parse(json);
        let err = config.to_match_config().unwrap_err();
        assert!(
            err.contains(expect),
            "{json}: expected {expect:?}, got {err}"
        );
        let batch = batch_err(json);
        assert!(batch.contains(expect), "batch: {json}: {batch}");
    }
}

/// A spec's resolution through the table, as the spawn's own check sees it:
/// every form is checked against the class.
#[test]
fn resolve_spec_checks_every_form_against_the_class() {
    let defs = defs();
    let warlock = CharacterClass::Warlock;
    assert_eq!(
        defs.resolve_spec(warlock, &BuildSpec::Reference("Destruction".into())),
        Ok(reference("Destruction"))
    );
    assert!(defs
        .resolve_spec(
            CharacterClass::Priest,
            &BuildSpec::Ranks(reference("Destruction"))
        )
        .unwrap_err()
        .contains("not a Priest's"));
    assert!(defs
        .resolve_spec(warlock, &BuildSpec::Ranks(Build::of(&[(SoulLink, 1)])))
        .unwrap_err()
        .contains("opens at 5"));
    assert_eq!(
        defs.resolve_spec(warlock, &BuildSpec::default()),
        Ok(Build::default())
    );
}
