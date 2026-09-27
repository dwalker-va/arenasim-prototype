//! Sim RNG audit (AS-154) — visuals never draw from `game_rng`.
//!
//! `GameRng` is the seeded stream that decides every crit, damage roll, proc
//! and dispel pick. A draw spent on a visual shifts every later roll, so what
//! an effect LOOKS LIKE decides how a match PLAYS OUT — and the visual freezes,
//! because retuning it now fails the byte-identity gate. Immolate's apply
//! burst did exactly this from `process_casting` (57-85 draws per landing, in
//! headless too) until it became a deterministic marker plus a graphical-only
//! system with its own RNG.
//!
//! Two checks, covering `process_casting` and `rendering/` only — not every
//! place such a draw could hide (a visual spawned from another sim system is
//! outside this audit):
//!
//! 1. **`process_casting`** — the resolution site every visual landing spawns
//!    from. Every `game_rng` in its body must be handed (`&mut game_rng`) to a
//!    NAMED sim draw, and the multiset of those draws is asserted EQUAL to
//!    [`PROCESS_CASTING_SIM_DRAWS`]. A raw `game_rng.random_*()` fails; so does
//!    a new helper call, which must be added to the list by someone deciding
//!    that it is a sim draw.
//! 2. **`rendering/`** — graphical-only systems. No file there may name
//!    `GameRng` or `game_rng` in code: a graphical system that drew from the
//!    sim stream would desync the client from headless on the same seed.
//!
//! Lexical, like the other audits built on `tests/common/source_audit.rs`:
//! comments and string literals are blanked before scanning.

mod common;

use common::source_audit::{
    blank_comments_and_strings, find_fn_body, load_sources, repo_path, SourceFile,
};
use regex::Regex;
use std::collections::BTreeMap;

const CASTING_REL: &str = "src/states/play_match/combat_core/casting.rs";
const RENDERING_REL: &str = "src/states/play_match/rendering";

/// The sim draws `process_casting` makes, by the helper each `&mut game_rng`
/// is passed to, with their counts. Exact: a new entry is a decision that the
/// new call is SIMULATION, not presentation.
const PROCESS_CASTING_SIM_DRAWS: &[(&str, usize)] = &[
    ("calculate_ability_damage_config", 1),
    ("calculate_ability_healing_config", 1),
    // One for a damage crit, one for a heal crit.
    ("roll_crit", 2),
    ("roll_procs", 1),
];

/// Classify every `game_rng` token in `body`: handed to a named helper as
/// `&mut game_rng` (counted by helper name), or anything else (a raw use,
/// reported with its line within `body`).
fn classify(body: &str) -> (BTreeMap<String, usize>, Vec<String>) {
    let token = Regex::new(r"\bgame_rng\b").unwrap();
    let handed = Regex::new(r"&\s*mut\s*$").unwrap();
    let ident = Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*$").unwrap();

    let mut draws = BTreeMap::new();
    let mut raw = Vec::new();
    for m in token.find_iter(body) {
        let before = &body[..m.start()];
        let line = before.matches('\n').count() + 1;
        let snippet = body[m.start()..].lines().next().unwrap_or("").trim();
        if !handed.is_match(before) {
            raw.push(format!("body line {line}: {snippet}"));
            continue;
        }
        match enclosing_call(before).and_then(|open| ident.captures(&before[..open])) {
            Some(c) => *draws.entry(c[1].to_string()).or_insert(0) += 1,
            None => raw.push(format!("body line {line}: not a call argument: {snippet}")),
        }
    }
    (draws, raw)
}

/// Offset of the unmatched `(` that `text` ends inside, if any.
fn enclosing_call(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in text.bytes().enumerate().rev() {
        match b {
            b')' => depth += 1,
            b'(' if depth == 0 => return Some(i),
            b'(' => depth -= 1,
            // A statement boundary: the token is not inside a call.
            b';' | b'{' | b'}' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

#[test]
fn process_casting_draws_game_rng_only_through_named_sim_helpers() {
    let file = SourceFile::read(&repo_path(CASTING_REL)).expect("read casting.rs");
    let code = blank_comments_and_strings(&file.raw);
    let body = find_fn_body(&code, "process_casting").expect("process_casting body");

    let (draws, raw) = classify(&body);
    assert!(
        raw.is_empty(),
        "`process_casting` uses `game_rng` other than by handing it to a named sim \
         draw. If this is a visual, spawn a deterministic marker here and draw its \
         variation in a graphical-only system with a visual RNG (see \
         `ImmolateApplyBurst` / `rendering/effects/flame.rs`). Offending uses:\n  {}",
        raw.join("\n  ")
    );

    let expected: BTreeMap<String, usize> = PROCESS_CASTING_SIM_DRAWS
        .iter()
        .map(|(name, n)| (name.to_string(), *n))
        .collect();
    assert_eq!(
        draws, expected,
        "the sim draws in `process_casting` changed. A draw for a VISUAL does not \
         belong here at all (see the first assertion's advice); a new SIM draw is \
         added to PROCESS_CASTING_SIM_DRAWS."
    );
}

#[test]
fn no_rendering_file_names_the_sim_rng() {
    let files = load_sources(&[RENDERING_REL]).expect("read rendering/");
    // Non-vacuity: the walk found the effects tree, not an empty directory.
    assert!(
        files
            .iter()
            .any(|f| f.rel_display().ends_with("effects/flame.rs")),
        "the rendering/ walk did not reach effects/flame.rs"
    );

    let token = Regex::new(r"\b(GameRng|game_rng)\b").unwrap();
    let mut violations = Vec::new();
    for f in &files {
        let code = blank_comments_and_strings(&f.raw);
        for m in token.find_iter(&code) {
            let line = code[..m.start()].matches('\n').count() + 1;
            violations.push(format!("{}:{line}", f.rel_display()));
        }
    }
    assert!(
        violations.is_empty(),
        "a graphical-only file names the sim RNG. Visual variation comes from a \
         visual-only hash or PRNG, never `GameRng` — a graphical system drawing \
         from it would desync the client from headless on the same seed:\n  {}",
        violations.join("\n  ")
    );
}

// Not `#[cfg(test)]`-gated: this file IS the test crate.
mod tests {
    use super::classify;

    /// The shape AS-154 removed, verbatim from the pre-fix `process_casting`:
    /// raw draws for a particle burst. The scan must reject every one.
    #[test]
    fn classify_rejects_the_retired_immolate_burst() {
        let body = r#"
            let particle_count = 8 + (game_rng.random_f32() * 5.0) as i32;
            for _ in 0..particle_count {
                let offset = Vec3::new(
                    (game_rng.random_f32() - 0.5) * 1.0,
                    game_rng.random_f32() * 0.5,
                    (game_rng.random_f32() - 0.5) * 1.0,
                );
                let lifetime = 0.6 + game_rng.random_f32() * 0.4;
            }
        "#;
        let (draws, raw) = classify(body);
        assert!(draws.is_empty());
        assert_eq!(raw.len(), 5, "{raw:#?}");
    }

    /// Helper calls are counted by name, across line-wrapped argument lists,
    /// method receivers and module paths.
    #[test]
    fn classify_counts_helper_draws_by_name() {
        let body = r#"
            let d = caster.calculate_ability_damage_config(def, &mut game_rng, a, b);
            let c = roll_crit(caster.crit_chance + bonus, &mut game_rng);
            let g = super::super::proc_trinkets::roll_procs(
                &mut caster.proc_trinkets,
                fired,
                &mut game_rng,
            );
            let e = roll_crit(x, &mut game_rng);
        "#;
        let (draws, raw) = classify(body);
        assert!(raw.is_empty(), "{raw:#?}");
        let got: Vec<_> = draws.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(
            got,
            [
                ("calculate_ability_damage_config", 1),
                ("roll_crit", 2),
                ("roll_procs", 1)
            ]
        );
    }

    /// A `&mut game_rng` bound to a local rather than passed to a call is not
    /// a named draw, and must not be counted as one.
    #[test]
    fn classify_rejects_a_bare_reborrow() {
        let (draws, raw) = classify("let r = &mut game_rng;\n r.random_f32();");
        assert!(draws.is_empty());
        assert_eq!(raw.len(), 1, "{raw:#?}");
    }
}
