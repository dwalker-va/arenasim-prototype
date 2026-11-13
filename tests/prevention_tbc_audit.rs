//! AS-478 — every ability's `prevention`, held to the TBC client's.
//!
//! `prevention` is TBC's `SpellCategories.PreventionType` (CLAUDE.md, *Every
//! ability declares `prevention`*): it decides whether a silence stops an
//! ability and whether an interrupt takes its cast bar. A value TBC does not
//! give the ability changes both, silently.
//!
//! `tests/data/tbc_spell_categories.csv` is every TBC class or pet spell's
//! PreventionType, keyed (owner, name), generated from the client by
//! `scripts/tbc_spell_categories.py`. That script's `EXPLICIT` table adds a
//! row under the sim's own (owner, name) for an ability TBC has no spell for
//! by that name (a pet ability the client names differently, or a spell a
//! later client brings), naming the spells it is read from. Every shipped
//! ability must join a row, by its class (a pet ability's "Pet - <pet>") and
//! its RON `name`, and declare that row's value. A new ability that joins by
//! name needs nothing; one that does not fails here with the line to add.

use std::collections::{BTreeMap, BTreeSet};

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions, Prevention};

const FIXTURE: &str = "tests/data/tbc_spell_categories.csv";
const REGENERATE: &str =
    "python3 scripts/tbc_spell_categories.py > tests/data/tbc_spell_categories.csv";

/// One fixture row: a name's ranks in one client build.
#[derive(Clone, Debug)]
struct Row {
    /// `name` (read by name) or `explicit` (named in the script's EXPLICIT).
    source: String,
    build: String,
    spells: String,
    /// "Silence", "Pacify", "None", or several joined by `/` when the ranks
    /// disagree.
    prevention: String,
}

type Key = (String, String);

/// The fixture, keyed (owner, name). Columns are found by the header, so a
/// column added for another `SpellCategories` field (AS-484's
/// StartRecoveryCategory) does not move these; `name` is last and runs to the
/// end of the line.
fn parse(text: &str) -> BTreeMap<Key, Row> {
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let header: Vec<&str> = lines
        .next()
        .expect("the fixture's header")
        .split(',')
        .collect();
    assert_eq!(
        header.last(),
        Some(&"name"),
        "`name` is the fixture's last column"
    );
    let col = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .unwrap_or_else(|| panic!("the fixture has no `{name}` column"))
    };
    let (owner, source, build, spells, prevention) = (
        col("owner"),
        col("source"),
        col("build"),
        col("spells"),
        col("prevention"),
    );
    let mut rows = BTreeMap::new();
    for line in lines {
        let cols: Vec<&str> = line.splitn(header.len(), ',').collect();
        assert_eq!(cols.len(), header.len(), "a short fixture row: {line:?}");
        let key = (cols[owner].to_string(), cols[header.len() - 1].to_string());
        let row = Row {
            source: cols[source].to_string(),
            build: cols[build].to_string(),
            spells: cols[spells].to_string(),
            prevention: cols[prevention].to_string(),
        };
        assert!(
            rows.insert(key.clone(), row).is_none(),
            "{key:?} is in the fixture twice"
        );
    }
    rows
}

fn fixture() -> BTreeMap<Key, Row> {
    let path = format!("{}/{FIXTURE}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read_to_string(&path).expect(FIXTURE))
}

/// The fixture's key for an ability: its class (or its pet's skill line,
/// "Pet - Spider") and its name.
fn key(def: &AbilityConfig) -> Key {
    let owner = match def.pet {
        Some(pet) => format!("Pet - {pet:?}"),
        None => def.class.name().to_string(),
    };
    (owner, def.name.clone())
}

fn spelled(p: Prevention) -> &'static str {
    match p {
        Prevention::Silence => "Silence",
        Prevention::Pacify => "Pacify",
        Prevention::None => "None",
    }
}

/// Every way `abilities` (ability, its key, its declared prevention) fails
/// the fixture, one message each, saying what to change.
fn audit(abilities: &[(String, Key, Prevention)], rows: &BTreeMap<Key, Row>) -> Vec<String> {
    let mut failures = Vec::new();
    for (ability, (owner, name), declared) in abilities {
        let Some(row) = rows.get(&(owner.clone(), name.clone())) else {
            failures.push(format!(
                "{ability}: TBC has no {owner} spell named {name:?} ({FIXTURE}). If the \
                 client names it differently, or only a later client has it, add \
                 `({owner:?}, {name:?}): (<build>, [\"<spell id>\", ...], \"<why>\")` to \
                 EXPLICIT in scripts/tbc_spell_categories.py, naming the client spell \
                 {ability} begins, then regenerate: {REGENERATE}"
            ));
            continue;
        };
        if row.prevention.contains('/') {
            failures.push(format!(
                "{ability}: the ranks of {owner} {name:?} disagree ({}, spells {} at {}): \
                 one is not the spell {ability} begins. Name that one in EXCLUDED in \
                 scripts/tbc_spell_categories.py, then regenerate: {REGENERATE}",
                row.prevention, row.spells, row.build
            ));
        } else if row.prevention != spelled(*declared) {
            failures.push(format!(
                "{ability}: abilities.ron says `prevention: {}`, TBC says {} (spells {} at \
                 {}). Set `prevention: {}`; a value that departs from TBC changes what a \
                 silence and an interrupt do, and needs a ruling",
                spelled(*declared),
                row.prevention,
                row.spells,
                row.build,
                row.prevention
            ));
        }
    }
    let joined: BTreeSet<&Key> = abilities.iter().map(|(_, k, _)| k).collect();
    for (key, row) in rows {
        if row.source == "explicit" && !joined.contains(key) {
            failures.push(format!(
                "the EXPLICIT row {key:?} joins no ability: remove it from \
                 scripts/tbc_spell_categories.py, then regenerate: {REGENERATE}"
            ));
        }
    }
    failures
}

fn shipped() -> Vec<(String, Key, Prevention)> {
    let mut out: Vec<_> = AbilityDefinitions::shipped()
        .iter()
        .map(|(ability, def)| (format!("{ability:?}"), key(def), def.prevention))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_ability_declares_its_tbc_prevention() {
    let rows = fixture();
    let abilities = shipped();
    let failures = audit(&abilities, &rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The join is not empty, and reaches every kind of row: the explicit ones,
/// named, and an ability of each prevention.
#[test]
fn the_audit_joins_the_whole_book() {
    let rows = fixture();
    let abilities = shipped();
    assert_eq!(
        abilities.len(),
        AbilityDefinitions::shipped().iter().count()
    );
    assert!(
        abilities.len() > 100,
        "the book has {} abilities",
        abilities.len()
    );
    let joined: BTreeSet<String> = abilities
        .iter()
        .filter(|(_, k, _)| rows.get(k).is_some_and(|r| r.source == "explicit"))
        .map(|(a, _, _)| a.clone())
        .collect();
    let explicit: BTreeSet<String> = [
        AbilityType::SpiderWeb,
        AbilityType::BoarCharge,
        AbilityType::MastersCall,
        AbilityType::WindShear,
        AbilityType::GuardianSpirit,
        AbilityType::SacredShield,
    ]
    .iter()
    .map(|a| format!("{a:?}"))
    .collect();
    assert_eq!(
        joined, explicit,
        "the abilities read from EXPLICIT rows; edit this list with the script's EXPLICIT"
    );
    for (ability, prevention) in [
        (AbilityType::Frostbolt, "Silence"),
        (AbilityType::AimedShot, "Pacify"),
        (AbilityType::TravelForm, "None"),
        (AbilityType::PainSuppression, "Silence"),
    ] {
        let def = AbilityDefinitions::shipped()
            .iter()
            .find(|(a, _)| **a == ability)
            .map(|(_, d)| d)
            .expect("shipped");
        assert_eq!(rows[&key(def)].prevention, prevention, "{ability:?}");
    }
}

/// The audit fails on each way an ability can be wrong, with the line to add.
#[test]
fn the_audit_names_each_failure() {
    let rows = parse(
        "owner,source,build,spells,prevention,name\n\
         Mage,name,2.5.6.69795,116,Silence,Frostbolt\n\
         Priest,name,2.5.6.69795,33206 44416,None/Silence,Pain Suppression\n\
         Paladin,explicit,3.80.2.70315,53601,Silence,Sacred Shield\n\
         Paladin,explicit,3.80.2.70315,1,Silence,Retired Spell\n",
    );
    let k = |o: &str, n: &str| (o.to_string(), n.to_string());
    let failures = audit(
        &[
            (
                "Frostbolt".into(),
                k("Mage", "Frostbolt"),
                Prevention::Pacify,
            ),
            (
                "Fireball".into(),
                k("Mage", "Fireball, Rank 9"),
                Prevention::Silence,
            ),
            (
                "PainSuppression".into(),
                k("Priest", "Pain Suppression"),
                Prevention::Silence,
            ),
            (
                "SacredShield".into(),
                k("Paladin", "Sacred Shield"),
                Prevention::Silence,
            ),
        ],
        &rows,
    );
    assert_eq!(failures.len(), 4, "{failures:#?}");
    assert!(failures[0]
        .starts_with("Frostbolt: abilities.ron says `prevention: Pacify`, TBC says Silence"));
    assert!(failures[0].contains("Set `prevention: Silence`"));
    assert!(failures[1].starts_with("Fireball: TBC has no Mage spell named \"Fireball, Rank 9\""));
    assert!(failures[1].contains("`(\"Mage\", \"Fireball, Rank 9\"): (<build>"));
    assert!(failures[1].contains("EXPLICIT in scripts/tbc_spell_categories.py"));
    assert!(failures[2]
        .starts_with("PainSuppression: the ranks of Priest \"Pain Suppression\" disagree"));
    assert!(failures[2].contains("EXCLUDED"));
    assert!(failures[3]
        .starts_with("the EXPLICIT row (\"Paladin\", \"Retired Spell\") joins no ability"));
    // A name with a comma reads to the end of the line.
    let comma = parse("owner,source,build,spells,prevention,name\nMage,name,1,2,None,A, B\n");
    assert!(comma.contains_key(&k("Mage", "A, B")));
}
