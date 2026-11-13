//! Damage-site audit
//!
//! A combatant that reaches 0 health must leave a `[DEATH]` line in the log.
//! There is no single death funnel: each damage site marks its victim dead
//! (`Combatant::is_dead`, the dedup flag), cancels its cast and logs the death
//! with its killer itself. The Unstable Affliction backlash was the one site
//! that did not, so a Priest killed by dispelling it sat in the match report
//! at 0 HP with no `[DEATH]` line (AS-437). This audit keeps every site doing
//! it.
//!
//! Four questions, each answered as SET EQUALITY over named members, so a new
//! member fails loudly instead of passing under a `>=` floor:
//!
//! 1. **Where does health go down?** Every subtractive or zeroing write to
//!    `current_health` in non-test `src/` code, named by (file, enclosing fn).
//!    Damage lands through `combat_core::apply_damage_with_absorb`; the other
//!    members are declared with why they are not damage.
//! 2. **Who deals damage, and does each log its kills?** Every (file,
//!    enclosing fn) that calls `apply_damage_with_absorb` is named in
//!    DAMAGERS, and every one of them also calls `CombatLog::log_death`.
//! 3. **Who credits damage dealt, and under which rule?** (AS-466) Only
//!    `credit_damage_dealt` writes `Combatant::damage_dealt`; the systems that
//!    call it are the systems that log damage rows (DAMAGE_CREDITORS); and
//!    every row's amount is `credited_damage(..)`, the rule the credit
//!    applies. So the Results total and its breakdown cannot part.
//! 4. **Is every damage line a row?** (AS-439) A damage line logged with a
//!    plain `CombatLog::log` carries no dealer, ability or killing blow, so it
//!    is missing from the per-ability breakdown and the K column. The places
//!    that name `CombatLogEventType::Damage` outside `CombatLog` itself are a
//!    declared set (UNSTRUCTURED_DAMAGE_LINES), none of them a hit with a
//!    dealer; and every row hands `log_damage` its kill as a computed flag,
//!    never a literal.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`: it
//! reads comment-, string- and test-module-blanked source. It does not see a
//! health drop spelled without `-` or a zero literal (the clamp when a max
//! health buff ends, `current_health.min(max_health)`, which cannot reach 0),
//! nor one taken in two steps.

mod common;

use common::source_audit::{blank_comments_and_strings, blank_test_modules, load_sources};
use regex::Regex;
use std::collections::BTreeSet;

/// The subtractive or zeroing `current_health` writes allowed in `src/`, as
/// (file, fn).
const HEALTH_DROPS: &[(&str, &str)] = &[
    // The one damage site.
    (
        "src/states/play_match/combat_core/damage.rs",
        "apply_damage_with_absorb",
    ),
    // Not damage: a pet whose owner died despawns, and logs its own `[DEATH]`
    // line ("despawns (owner died)") as it does.
    (
        "src/states/play_match/combat_core/death.rs",
        "despawn_pets_of_dead_owners",
    ),
    // Not damage: a Demonic Sacrifice kills the Warlock's Felhunter at the
    // gates, and logs its own `[DEATH]` line ("is sacrificed by"), with no
    // killer — no kill.
    ("src/states/play_match/demonology.rs", "demonic_bonds"),
    // Not damage: Life Tap's health-for-mana conversion, floored at 1 health
    // so it never kills.
    (
        "src/states/play_match/class_ai/warlock.rs",
        "convert_health_to_mana",
    ),
];

/// Every system that deals damage through `apply_damage_with_absorb`, as
/// (file, fn). Each must also log the deaths it causes.
const DAMAGERS: &[(&str, &str)] = &[
    // Instant strikes and area damage (Frost Nova, Holy Nova), queued by the
    // class AI and landed right after the dispatch, on every combatant.
    (
        "src/states/play_match/instant_landings.rs",
        "land_instant_effects",
    ),
    // Damage over time ticks.
    ("src/states/play_match/auras.rs", "process_dot_ticks"),
    // Projectile hits (Frostbolt, Shadow Bolt, ...).
    (
        "src/states/play_match/projectiles.rs",
        "process_projectile_hits",
    ),
    // Completed casts with no projectile.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
    ),
    // Drain Life's channel ticks.
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_channeling",
    ),
    // Melee swings and Auto Shot.
    (
        "src/states/play_match/combat_core/auto_attack.rs",
        "combat_auto_attack",
    ),
    // Holy Shock's damage.
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_damage",
    ),
    // What a hit sets off on other units: transferred shares, reflected
    // damage (AS-408).
    ("src/states/play_match/combat_core/follow_ups.rs", "land"),
    // The Unstable Affliction dispel backlash (AS-437).
    (
        "src/states/play_match/effects/backlash.rs",
        "process_backlash",
    ),
];

/// Every (file, enclosing fn) in non-test `src/` code where `pattern` matches
/// and `keep(first capture group, rest of the statement)` holds.
fn sites(pattern: &Regex, keep: impl Fn(&str, &str) -> bool) -> BTreeSet<(String, String)> {
    let fn_item = Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut found = BTreeSet::new();
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
            let enclosing = fn_item
                .captures_iter(&code[..whole.start()])
                .last()
                .map_or_else(|| "<module>".to_string(), |c| c[1].to_string());
            found.insert((file.rel_display(), enclosing));
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

/// Every non-definition call of `name(` in non-test `src/`, as (file, fn).
fn callers(name: &str) -> BTreeSet<(String, String)> {
    let call = Regex::new(&format!(r"(\bfn\s+)?\b{name}\s*\(")).unwrap();
    sites(&call, |definition, _| definition.is_empty())
}

#[test]
fn health_goes_down_only_in_apply_damage_with_absorb() {
    // `current_health -=`, or `current_health =` whose right-hand side
    // subtracts or is a zero literal. The right-hand side runs to the next
    // `;`, which for a closure body is past the closure, so a `->` there is
    // not a subtraction.
    let write = Regex::new(r"\bcurrent_health\s*(-=|=[^=])").unwrap();
    let zero = Regex::new(r"^\s*0(\.0*)?\s*$").unwrap();
    let found = sites(&write, |op, rhs| {
        op == "-=" || rhs.replace("->", "").contains('-') || zero.is_match(rhs)
    });
    assert_same(
        "Health falls outside `combat_core::apply_damage_with_absorb`. Damage must land \
         through it, and its caller must log the death it causes; a drop that is \
         genuinely not damage goes in HEALTH_DROPS with its reason.",
        found,
        named(HEALTH_DROPS),
    );
}

#[test]
fn every_damager_is_declared() {
    assert_same(
        "The set of systems that call `apply_damage_with_absorb` changed. A new damage \
         site names its system in DAMAGERS, and logs the deaths it causes.",
        callers("apply_damage_with_absorb"),
        named(DAMAGERS),
    );
}

#[test]
fn every_damager_logs_the_deaths_it_causes() {
    let loggers = callers("log_death");
    let silent: Vec<_> = named(DAMAGERS).difference(&loggers).cloned().collect();
    assert!(
        silent.is_empty(),
        "These systems deal damage but never call `CombatLog::log_death`, so a unit \
         they kill sits at 0 HP with no [DEATH] line (AS-437). Mark the victim \
         `is_dead`, cancel its cast and log the death with its killer, as \
         `process_dot_ticks` does: {silent:?}"
    );
}

/// The damagers whose hits come from no attacker's own damage, so no
/// damage-dealt increase applies: each with why.
const NO_DEALT_MULTIPLIER: &[(&str, &str, &str)] = &[
    (
        "src/states/play_match/effects/backlash.rs",
        "process_backlash",
        "a snapshotted backlash, fixed when the Unstable Affliction landed",
    ),
    (
        "src/states/play_match/combat_core/follow_ups.rs",
        "land",
        "secondary damage: a share of a hit already raised, or a reflect",
    ),
];

/// Every damager raises its hits by the attacker's damage-dealt increase
/// (`Hit::with_dealt`, or `Hit::new` handed the attacker's auras) and spawns
/// the follow-ups its hits set off (`DamageFollowUps::spawn`) — or is the one
/// that lands them. A site that drops either silently loses Touch of Shadow,
/// Shadowform, a transfer, a reflect or a save's heal (AS-408). Lexical: it
/// reads the system's body, so it vouches the call is there, not its value.
#[test]
fn every_damager_raises_by_the_attacker_and_spawns_its_follow_ups() {
    let files = load_sources(&["src"]).expect("read src/");
    let exempt: BTreeSet<(String, String)> = NO_DEALT_MULTIPLIER
        .iter()
        .map(|(f, n, _)| (f.to_string(), n.to_string()))
        .collect();
    let mut bad = Vec::new();
    for (file, name) in named(DAMAGERS) {
        let src = files
            .iter()
            .find(|f| f.rel_display() == file)
            .unwrap_or_else(|| panic!("{file} is not in src/"));
        let code = blank_test_modules(&blank_comments_and_strings(&src.raw));
        let body = common::source_audit::find_fn_body(&code, &name)
            .unwrap_or_else(|| panic!("no fn {name} in {file}"));
        if !exempt.contains(&(file.clone(), name.clone())) && !body.contains(".with_dealt(") {
            bad.push(format!("{file} {name}: no `.with_dealt(`"));
        }
        let lands_them = name == "land";
        if !lands_them && !body.contains("follow_ups.spawn(") {
            bad.push(format!("{file} {name}: no `follow_ups.spawn(`"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// ---------------------------------------------------------------------------
// Damage dealt: one rule for the Results total and its breakdown (AS-466)
// ---------------------------------------------------------------------------

/// The systems that credit a dealer with damage, as (file, fn). Each credits
/// through `combat_core::credit_damage_dealt` and logs the breakdown row
/// (`CombatLog::log_damage`) of the same hit.
const DAMAGE_CREDITORS: &[(&str, &str)] = &[
    (
        "src/states/play_match/instant_landings.rs",
        "land_instant_effects",
    ),
    ("src/states/play_match/auras.rs", "process_dot_ticks"),
    (
        "src/states/play_match/projectiles.rs",
        "process_projectile_hits",
    ),
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_casting",
    ),
    (
        "src/states/play_match/combat_core/casting.rs",
        "process_channeling",
    ),
    (
        "src/states/play_match/combat_core/auto_attack.rs",
        "combat_auto_attack",
    ),
    (
        "src/states/play_match/effects/holy_shock.rs",
        "process_holy_shock_damage",
    ),
    // Transferred shares and reflects, landed by `land`.
    ("src/states/play_match/combat_core/follow_ups.rs", "record"),
    (
        "src/states/play_match/effects/backlash.rs",
        "process_backlash",
    ),
];

/// `Combatant::damage_dealt` — the Results screen's DMG total — is written in
/// one place, `credit_damage_dealt`, which applies the one rule
/// (`credited_damage`). A site that wrote it directly could pick its own
/// rule, as direct hits and damage-over-time ticks once did.
#[test]
fn damage_dealt_is_written_only_by_credit_damage_dealt() {
    let write = Regex::new(r"\.\s*damage_dealt\s*(\+=|-=|\*=|=[^=])").unwrap();
    assert_same(
        "`damage_dealt` is written outside `combat_core::credit_damage_dealt`. Credit a \
         hit through it, so its total keeps the rule its breakdown row is logged under.",
        sites(&write, |_, _| true),
        named(&[(
            "src/states/play_match/combat_core/damage.rs",
            "credit_damage_dealt",
        )]),
    );
}

/// The systems that credit damage dealt and the systems that log damage rows
/// are one set: a site that credits a hit and logs no row (the backlash,
/// reflects and transfers once did) leaves its total above its breakdown,
/// and one that logs a row and credits nothing (Holy Shock once did) leaves
/// it below.
#[test]
fn every_damage_credit_has_its_breakdown_row() {
    assert_same(
        "The systems that call `credit_damage_dealt` changed. A new damage site names \
         itself in DAMAGE_CREDITORS.",
        callers("credit_damage_dealt"),
        named(DAMAGE_CREDITORS),
    );
    let mut loggers = callers("log_damage");
    // The definition's own module: `CombatLog`'s methods and their tests.
    loggers.retain(|(file, _)| file != "src/combat/log.rs");
    assert_same(
        "The systems that log a damage row are not the systems that credit damage \
         dealt: a credit with no row leaves the Results total above its breakdown, a \
         row with no credit leaves it below.",
        loggers,
        named(DAMAGE_CREDITORS),
    );
}

/// Every damage row's amount is `credited_damage(landed, absorbed)`, the rule
/// `credit_damage_dealt` applies to the total: a row that counted only what
/// landed would agree with a total that did the same, and still break the
/// rule (`tests/results_damage_totals.rs` plays the rows).
#[test]
fn every_damage_row_is_logged_under_the_rule() {
    let call = Regex::new(r"\.\s*log_damage\s*\(").unwrap();
    let rule = Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*credited_damage\s*\(").unwrap();
    let mut rows = 0;
    let mut bad = Vec::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in call.find_iter(&code) {
            let args =
                common::source_audit::param_list(&code, m.end() - 1).expect("a balanced call");
            let args = common::source_audit::split_params(&args);
            rows += 1;
            let amount = args.get(3).map(|a| a.trim()).unwrap_or("");
            if !rule.is_match(amount) {
                let line = code[..m.start()].lines().count();
                bad.push(format!("{}:{line}: amount `{amount}`", file.rel_display()));
            }
        }
    }
    assert!(
        rows >= DAMAGE_CREDITORS.len(),
        "only {rows} damage rows found"
    );
    assert!(
        bad.is_empty(),
        "A damage row's amount is not `credited_damage(landed, absorbed)`:\n{}",
        bad.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Every damage line is a row (AS-439)
// ---------------------------------------------------------------------------

/// The places outside `CombatLog` that name `CombatLogEventType::Damage`, as
/// (file, fn), each with why it is not a hit that owes a row. A hit with a
/// dealer logs through `CombatLog::log_damage`, which carries the dealer, the
/// ability and the killing blow; a plain `CombatLog::log` line carries none of
/// them, so its damage is missing from the Results breakdown and its kill from
/// the K column (the Unstable Affliction backlash once was, AS-439).
const UNSTRUCTURED_DAMAGE_LINES: &[(&str, &str, &str)] = &[
    (
        "src/states/play_match/combat_core/avoidance.rs",
        "resolve_melee_avoidances",
        "a dodged or parried swing: no damage landed, so there is no row",
    ),
    (
        "src/states/play_match/combat_core/follow_ups.rs",
        "record",
        "a reflect or transferred share whose dealer is gone: no one to credit",
    ),
    (
        "src/states/play_match/effects/backlash.rs",
        "process_backlash",
        "a backlash whose Warlock is gone: no one to credit",
    ),
    (
        "src/states/play_match/rendering/combat_log.rs",
        "render_combat_log_content",
        "the HUD log's colour for a damage line: a read, not a log",
    ),
];

#[test]
fn every_damage_line_with_a_dealer_is_a_row() {
    // A glob or renamed import would let a bare `Damage` past the pattern.
    let aliased = Regex::new(r"\bCombatLogEventType\s*(::\s*[*{]|as\b)").unwrap();
    assert_same(
        "`CombatLogEventType` is imported by glob or alias, so this audit cannot see \
         its `Damage`. Name it in full.",
        sites(&aliased, |_, _| true),
        BTreeSet::new(),
    );
    let damage = Regex::new(r"\bCombatLogEventType\s*::\s*Damage\b").unwrap();
    let mut found = sites(&damage, |_, _| true);
    // `CombatLog`'s own module: `log_damage` itself, and the queries.
    found.retain(|(file, _)| file != "src/combat/log.rs");
    let declared: BTreeSet<(String, String)> = UNSTRUCTURED_DAMAGE_LINES
        .iter()
        .map(|(file, f, _)| (file.to_string(), f.to_string()))
        .collect();
    assert_same(
        "A damage line is logged without `CombatLog::log_damage`, so its damage is \
         missing from the Results breakdown and its kill from the K column. Log a hit \
         with a dealer through `log_damage` (credited to the dealer's ability, with its \
         killing blow); a line that is genuinely no one's hit goes in \
         UNSTRUCTURED_DAMAGE_LINES with its reason.",
        found,
        declared,
    );
}

/// Every damage row hands `log_damage` its killing blow as a computed flag
/// (`is_killing_blow`, `is_first_death`, `landed.killed`), never a literal: a
/// row that passed `false` would drop a lethal hit's kill from the K column,
/// and one that passed `true` would credit kills that never happened.
#[test]
fn every_damage_row_computes_its_killing_blow() {
    let call = Regex::new(r"\.\s*log_damage\s*\(").unwrap();
    let literal = Regex::new(r"^(true|false)$").unwrap();
    let mut rows = 0;
    let mut bad = Vec::new();
    for file in load_sources(&["src"]).expect("read src/") {
        let code = blank_test_modules(&blank_comments_and_strings(&file.raw));
        for m in call.find_iter(&code) {
            let args =
                common::source_audit::param_list(&code, m.end() - 1).expect("a balanced call");
            let args = common::source_audit::split_params(&args);
            rows += 1;
            let kill = args.get(4).map(|a| a.trim()).unwrap_or("");
            if kill.is_empty() || literal.is_match(kill) {
                let line = code[..m.start()].lines().count();
                bad.push(format!(
                    "{}:{line}: killing blow `{kill}`",
                    file.rel_display()
                ));
            }
        }
    }
    assert!(
        rows >= DAMAGE_CREDITORS.len(),
        "only {rows} damage rows found"
    );
    assert!(
        bad.is_empty(),
        "A damage row's killing blow is not computed from its victim's death:\n{}",
        bad.join("\n")
    );
}
