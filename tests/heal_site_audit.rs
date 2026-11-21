//! Heal-site audit
//!
//! Every effect that raises a combatant's health must land through
//! `combat_core::apply_healing`, the one function that applies healing
//! reduction (Mortal Strike), the Mortal Wounds tell and arena dampening. Heal
//! over time ticks once skipped the reduction because each heal site carried
//! its own copy of those rules and one copy was missing a step; this audit is
//! what keeps there being one copy.
//!
//! Four questions, each answered as SET EQUALITY over named members, so a new
//! member fails loudly instead of passing under a `>=` floor:
//!
//! 1. **Where does health go up?** Every additive write to `current_health` in
//!    non-test `src/` code, named by (file, enclosing fn). The only members are
//!    `apply_healing` itself and one declared non-heal: a max-health buff that
//!    raises current health with the maximum. A raw `current_health +=` or
//!    `current_health = (current_health + x)...` anywhere else is a heal site
//!    that bypasses the rules.
//! 2. **Who heals?** Every (file, enclosing fn) that calls `apply_healing`. A
//!    heal system that stops calling it (and so stops healing, or heals some
//!    other way) changes this set.
//! 3. **Who credits healing done?** (AS-494) Only `credit_healing_done`
//!    writes `Combatant::healing_done`, the Results HEAL total. The systems
//!    that call it are the systems that log heal rows (`CombatLog::log_healing`,
//!    the per-ability breakdown), counted per function (HEAL_CREDITORS), save
//!    the leech heals, whose rows `leech::land` logs and whose every lander
//!    credits; and every healer is one of those, or declared as crediting no
//!    one (UNCREDITED_HEALS). Holy Shock's and Holy Nova's heals once logged a
//!    row and credited nothing.
//! 4. **Is every heal line a row?** (AS-494) A heal line logged with a plain
//!    `CombatLog::log` carries no healer or ability, so it is missing from the
//!    per-ability breakdown (Guardian Spirit's save heal and Devour Magic's
//!    once were). The places that name `CombatLogEventType::Healing` outside
//!    `CombatLog` itself are a declared set, counted per function
//!    (UNSTRUCTURED_HEAL_LINES), none of them a heal with a healer.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source. It does not see a
//! health raise spelled without `+` (`current_health = max_health`, the spawn
//! and reset sites), which is why the reset sites are not heals to it — nor
//! one taken in two steps (`let r = c.current_health + x;` then
//! `c.current_health = r.min(..)`), whose write carries no `+` on its
//! right-hand side.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

/// The additive `current_health` writes allowed in `src/`, as (file, fn).
const HEALTH_RAISES: &[(&str, &str)] = &[
    // The one heal site.
    (
        "src/states/play_match/combat_core/healing.rs",
        "apply_healing",
    ),
    // Not a heal: a MaxHealthIncrease buff (Power Word: Fortitude) raises
    // current health WITH the maximum as it lands, so the buff does not read
    // as missing health. Healing reduction must not cut it.
    ("src/states/play_match/auras.rs", "apply_pending_auras"),
];

/// Every system that lands healing, as (file, fn).
const HEALERS: &[(&str, &str)] = &[
    // Direct heals (Flash Heal, Healing Touch, Swiftmend, ...).
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
    ),
    // Heal over time ticks (Rejuvenation, Lifebloom, Healing Stream).
    ("src/states/play_match/auras.rs", "process_hot_ticks"),
    // The Lifebloom bloom.
    ("src/states/play_match/effects/bloom.rs", "process_blooms"),
    // Holy Shock's heal.
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_heals",
    ),
    // Every leech heal (`combat_core/leech.rs`): Death Coil's lifesteal,
    // Siphon Life, Drain Life's heal to its channeler and Vampiric Embrace,
    // from the projectile, cast, channel and DoT tick sites alike.
    ("src/states/play_match/combat_core/leech.rs", "land"),
    // A killing-blow save's heal (Guardian Spirit's shape, AS-408).
    (
        "src/states/play_match/combat_core/follow_ups.rs",
        "process_damage_follow_ups",
    ),
    // Devour Magic's heal to the Felhunter.
    (
        "src/states/play_match/effects/dispels.rs",
        "process_dispels",
    ),
    // Area healing decided in the frame's dispatch and landed after it
    // (Holy Nova, AS-415), on every combatant (AS-488).
    (
        "src/states/play_match/instant_landings.rs",
        "land_instant_effects",
    ),
    // A heeled pet's recovery at its owner's side.
    (
        "src/states/play_match/class_ai/pet_ai.rs",
        "recover_heeled_pets",
    ),
];

/// The name of the `fn` item `code[..at]` is inside, or `<module>`.
fn enclosing_fn(code: &str, at: usize) -> String {
    let fn_item = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    fn_item
        .captures_iter(&code[..at])
        .last()
        .map_or_else(|| "<module>".to_string(), |c| c[1].to_string())
}

/// Every (file, enclosing fn) in non-test `src/` code where `pattern` matches
/// and `keep(first capture group, rest of the statement)` holds.
fn sites(pattern: &Regex, keep: impl Fn(&str, &str) -> bool) -> BTreeSet<(String, String)> {
    site_counts(pattern, keep).into_keys().collect()
}

/// [`sites`], with how many times each matched: a second match in a function
/// already named is a new member too.
fn site_counts(
    pattern: &Regex,
    keep: impl Fn(&str, &str) -> bool,
) -> BTreeMap<(String, String), usize> {
    let mut found = BTreeMap::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in pattern.captures_iter(&code) {
            let whole = m.get(0).unwrap();
            let statement_end = code[whole.end()..]
                .find(';')
                .map_or(code.len(), |i| whole.end() + i);
            let group = m.get(1).map_or("", |g| g.as_str());
            if !keep(group, &code[whole.end()..statement_end]) {
                continue;
            }
            let enclosing = enclosing_fn(&code, whole.start());
            *found.entry((file.rel_display(), enclosing)).or_insert(0) += 1;
        }
    }
    found
}

fn named(members: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    members
        .iter()
        .map(|(file, f)| (file.to_string(), f.to_string()))
        .collect()
}

fn assert_same(
    label: &str,
    found: BTreeSet<(String, String)>,
    expected: BTreeSet<(String, String)>,
) {
    let extra: Vec<_> = found.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&found).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "{label}\n  found but not declared: {extra:?}\n  declared but not found: {missing:?}"
    );
}

/// [`assert_same`] for counted members: each (file, fn) and how many times.
fn assert_same_counts(
    label: &str,
    found: BTreeMap<(String, String), usize>,
    expected: BTreeMap<(String, String), usize>,
) {
    let differ: Vec<_> = found
        .keys()
        .chain(expected.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|k| found.get(*k) != expected.get(*k))
        .map(|k| {
            format!(
                "{k:?}: found {}, declared {}",
                found.get(k).copied().unwrap_or(0),
                expected.get(k).copied().unwrap_or(0)
            )
        })
        .collect();
    assert!(differ.is_empty(), "{label}\n  {}", differ.join("\n  "));
}

/// Every non-definition call of `name(` in non-test `src/`, as (file, fn).
fn callers(name: &str) -> BTreeSet<(String, String)> {
    let call = Regex::new(&format!(r"(\bfn\s+)?\b{name}\s*\(")).unwrap();
    sites(&call, |definition, _| definition.is_empty())
}

#[test]
fn health_goes_up_only_in_apply_healing() {
    // `current_health +=`, or `current_health =` whose right-hand side adds.
    let write = Regex::new(r"\bcurrent_health\s*(\+=|=[^=])").unwrap();
    let found = sites(&write, |op, rhs| op == "+=" || rhs.contains('+'));
    assert_same(
        "Health rises outside `combat_core::apply_healing`. A heal must land through it, \
         so healing reduction, the Mortal Wounds tell and arena dampening apply; a raise \
         that is genuinely not a heal goes in HEALTH_RAISES with its reason.",
        found,
        named(HEALTH_RAISES),
    );
}

#[test]
fn every_healer_lands_through_apply_healing() {
    assert_same(
        "The set of systems that call `apply_healing` changed. A new heal names its \
         system in HEALERS; a system that stopped calling it has a heal that no longer \
         lands through the shared rules.",
        callers("apply_healing"),
        named(HEALERS),
    );
}

// ---------------------------------------------------------------------------
// Healing done: one path for the Results total and its breakdown (AS-494)
// ---------------------------------------------------------------------------

/// The systems that credit a healer, as (file, fn, the `CombatLog::log_healing`
/// calls in it, whether it lands leech heals). Each credits through
/// `combat_core::credit_healing_done` and logs the breakdown row of the same
/// heal — save a leech lander, whose rows `leech::land` logs (LEECH_ROWS) and
/// which credits what `leech::land` / `leech::land_owed` returns. The row
/// count makes the row audit exact: a row that disappears, or a second one in
/// a system already named, fails it.
const HEAL_CREDITORS: &[(&str, &str, usize, bool)] = &[
    // Direct heals, and the leech heals of the casts that land with no
    // projectile.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
        1,
        true,
    ),
    // Drain Life's heal to its channeler.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_channeling",
        0,
        true,
    ),
    // Heal over time ticks.
    (
        "src/states/play_match/auras.rs",
        "process_hot_ticks",
        1,
        false,
    ),
    // Siphon Life's and Vampiric Embrace's heals from damage over time ticks.
    (
        "src/states/play_match/auras.rs",
        "process_dot_ticks",
        0,
        true,
    ),
    // Death Coil's lifesteal, and a projectile's party leech.
    (
        "src/states/play_match/projectiles.rs",
        "process_projectile_hits",
        0,
        true,
    ),
    (
        "src/states/play_match/projectiles.rs",
        "land_party_leech",
        0,
        true,
    ),
    // The Lifebloom bloom.
    (
        "src/states/play_match/effects/bloom.rs",
        "process_blooms",
        1,
        false,
    ),
    // Holy Shock's heal.
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_heals",
        1,
        false,
    ),
    // Holy Nova's area heal.
    (
        "src/states/play_match/instant_landings.rs",
        "land_instant_effects",
        1,
        false,
    ),
    // A killing-blow save's heal (Guardian Spirit), credited to its caster.
    (
        "src/states/play_match/combat_core/follow_ups.rs",
        "process_damage_follow_ups",
        1,
        false,
    ),
    // Devour Magic's heal to its Felhunter.
    (
        "src/states/play_match/effects/dispels.rs",
        "process_dispels",
        1,
        false,
    ),
];

/// The heal rows logged by a function that credits no one itself, as (file,
/// fn, rows): `leech::land` logs each leech heal's row and returns what landed
/// to its lander, a HEAL_CREDITORS system that lands leech heals.
const LEECH_ROWS: (&str, &str, usize) = ("src/states/play_match/combat_core/leech.rs", "land", 1);

/// The healers that credit no one and log no row, each with why.
const UNCREDITED_HEALS: &[(&str, &str, &str)] = &[(
    "src/states/play_match/class_ai/pet_ai.rs",
    "recover_heeled_pets",
    "a heeled pet's own recovery at its owner's side: no one cast it",
)];

fn heal_creditors() -> BTreeSet<(String, String)> {
    HEAL_CREDITORS
        .iter()
        .map(|(file, f, _, _)| (file.to_string(), f.to_string()))
        .collect()
}

/// `Combatant::healing_done` — the Results screen's HEAL total — is written in
/// one place, `credit_healing_done`, so a heal is credited the way its row is
/// logged: by what `apply_healing` returned, to the healer the row names.
#[test]
fn healing_done_is_written_only_by_credit_healing_done() {
    let write = Regex::new(r"\.\s*healing_done\s*(\+=|-=|\*=|=[^=])").unwrap();
    assert_same(
        "`healing_done` is written outside `combat_core::credit_healing_done`. Credit a \
         heal through it, beside the heal's breakdown row.",
        sites(&write, |_, _| true),
        named(&[(
            "src/states/play_match/combat_core/healing.rs",
            "credit_healing_done",
        )]),
    );
}

/// The systems that credit healing done and the systems that log heal rows
/// are one set, row for row: a heal that logs a row and credits nothing
/// (Holy Shock's and Holy Nova's once did) leaves the Results total below its
/// breakdown, and one that credits and logs no row leaves it above. The leech
/// heals' rows are `leech::land`'s, and every system that lands a leech heal
/// credits what it returned.
#[test]
fn every_heal_credit_has_its_breakdown_row() {
    assert_same(
        "The systems that call `credit_healing_done` changed. A new heal site names \
         itself in HEAL_CREDITORS.",
        callers("credit_healing_done"),
        heal_creditors(),
    );

    let row = Regex::new(r"\.\s*log_healing\s*\(").unwrap();
    let mut rows = site_counts(&row, |_, _| true);
    // `CombatLog`'s own module: the definition's tests.
    rows.retain(|(file, _), _| file != "src/combat/log.rs");
    let mut declared: BTreeMap<(String, String), usize> = HEAL_CREDITORS
        .iter()
        .filter(|(_, _, n, _)| *n > 0)
        .map(|(file, f, n, _)| ((file.to_string(), f.to_string()), *n))
        .collect();
    declared.insert(
        (LEECH_ROWS.0.to_string(), LEECH_ROWS.1.to_string()),
        LEECH_ROWS.2,
    );
    assert_same_counts(
        "The `log_healing` rows are not the rows HEAL_CREDITORS and LEECH_ROWS declare: \
         a row went missing, or a new one needs its count raised there and its heal \
         credited through `credit_healing_done`.",
        rows,
        declared,
    );

    let lander = Regex::new(r"\bleech\s*::\s*land(?:_owed)?\s*\(").unwrap();
    let mut landers = sites(&lander, |_, _| true);
    landers.retain(|(file, _)| file != LEECH_ROWS.0);
    assert_same(
        "The systems that land a leech heal are not the HEAL_CREDITORS marked as \
         landing one: `leech::land` credits no one, so its lander must credit what it \
         returns.",
        landers,
        HEAL_CREDITORS
            .iter()
            .filter(|(_, _, _, leech)| *leech)
            .map(|(file, f, _, _)| (file.to_string(), f.to_string()))
            .collect(),
    );
}

/// Every system that lands a heal logs its row and credits it — directly
/// (a HEAL_CREDITORS row-logger), through the leech lander, or is declared as
/// crediting no one. A new heal that logs and credits nothing is missing from
/// both Results numbers.
#[test]
fn every_healer_credits_its_heals() {
    let mut accounted: BTreeSet<(String, String)> = HEAL_CREDITORS
        .iter()
        .filter(|(_, _, n, _)| *n > 0)
        .map(|(file, f, _, _)| (file.to_string(), f.to_string()))
        .collect();
    accounted.insert((LEECH_ROWS.0.to_string(), LEECH_ROWS.1.to_string()));
    accounted.extend(
        UNCREDITED_HEALS
            .iter()
            .map(|(file, f, _)| (file.to_string(), f.to_string())),
    );
    assert_same(
        "A healer neither logs its heal's row and credits it, nor is declared in \
         UNCREDITED_HEALS: log the heal with `CombatLog::log_healing` and credit it \
         with `credit_healing_done`.",
        named(HEALERS),
        accounted,
    );
}

// ---------------------------------------------------------------------------
// Every heal line is a row (AS-494)
// ---------------------------------------------------------------------------

/// The places outside `CombatLog` that name `CombatLogEventType::Healing`, as
/// (file, fn, how many times), each with why it is not a heal that owes a
/// row. Counted, so a second line in a function already named fails too. A
/// heal with a healer logs through `CombatLog::log_healing`, which carries the
/// healer and the ability; a plain `CombatLog::log` line carries neither, so
/// its heal is missing from the Results breakdown (Guardian Spirit's save heal
/// and Devour Magic's once were).
const UNSTRUCTURED_HEAL_LINES: &[(&str, &str, usize, &str)] = &[
    (
        "src/states/play_match/combat_core/follow_ups.rs",
        "process_damage_follow_ups",
        1,
        "a save's heal whose caster is not in the match: no one to credit",
    ),
    (
        "src/states/play_match/rendering/combat_log.rs",
        "render_combat_log_content",
        1,
        "the HUD log's colour for a heal line: a read, not a log",
    ),
];

#[test]
fn every_heal_line_with_a_healer_is_a_row() {
    // A glob or renamed import, or a type alias (`type Ev = CombatLogEventType;`
    // then `Ev::Healing`), would let a `Healing` past the pattern.
    let aliased = Regex::new(
        r"\bCombatLogEventType\s*(::\s*[*{]|as\b)|\btype\s+[A-Za-z_][A-Za-z0-9_]*\s*=[^;]*\bCombatLogEventType\b",
    )
    .unwrap();
    assert_same(
        "`CombatLogEventType` is imported by glob or alias, or named by a type alias, so \
         this audit cannot see its `Healing`. Name it in full.",
        sites(&aliased, |_, _| true),
        BTreeSet::new(),
    );
    let healing = Regex::new(r"\bCombatLogEventType\s*::\s*Healing\b").unwrap();
    let mut found = site_counts(&healing, |_, _| true);
    // `CombatLog`'s own module: `log_healing` itself, and the queries.
    found.retain(|(file, _), _| file != "src/combat/log.rs");
    let declared = UNSTRUCTURED_HEAL_LINES
        .iter()
        .map(|(file, f, n, _)| ((file.to_string(), f.to_string()), *n))
        .collect();
    assert_same_counts(
        "A heal line is logged without `CombatLog::log_healing`, so its heal is missing \
         from the Results breakdown. Log a heal with a healer through `log_healing` \
         (credited to the healer's ability, and through `credit_healing_done`); a line \
         that is genuinely no one's heal goes in UNSTRUCTURED_HEAL_LINES with its \
         reason.",
        found,
        declared,
    );
}
