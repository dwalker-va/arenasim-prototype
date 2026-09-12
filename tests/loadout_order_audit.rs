//! Loadout ordering audit (AS-58)
//!
//! Applying equipment sums float stats across a loadout's entries, and float
//! addition is not associative — so the map's iteration order decides the last
//! ULP of every derived stat. A `HashMap` with the default `RandomState` is
//! seeded PER PROCESS, so that order changes between runs of one unmodified
//! binary. Measured: the same release build, run 40 times, produced Rogue
//! `crit_chance` as three distinct bit patterns (0x3e2e147a / 0x3e2e147b /
//! 0x3e2e147c). Harmless as gameplay, fatal as methodology — this project
//! verifies nearly every sim-adjacent change by headless byte-identity.
//!
//! The fix is the [`Loadout`] alias (`BTreeMap<ItemSlot, ItemId>`), which puts
//! the ordering in the TYPE. `equipment::tests::loadout_is_ordered` guards the
//! alias itself. This audit guards the OTHER direction: new code that declares
//! its own `HashMap` keyed by `ItemSlot` instead of using the alias, which the
//! type assertion could never see.
//!
//! **Why not a repetition test.** Computing the same loadout's stats N times in
//! one process cannot fail — `RandomState` is seeded once per process, so a
//! `HashMap` iterates identically for the whole life of a test binary. The bug
//! only exists across processes, so the guard has to be structural.
//!
//! **This is the secondary net, and it has a mesh size.** The primary guard is
//! the type system: everything flowing into `apply_equipment` / `resolve_loadout`
//! / `from_loadout` is forced to `Loadout`, so a stray `HashMap` mostly cannot
//! reach a summation site in the first place. What this scan adds is catching
//! the declaration early, and it does so textually — it can be defeated by
//! anyone determined to. It is written to catch the shapes that arise by
//! ACCIDENT: the generic `rustfmt` wrapped across lines (see [`Squeezed`]), the
//! renaming import, and the map whose type is never written at all.

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

/// Source roots scanned for `HashMap`s keyed by `ItemSlot`.
///
/// Exhaustive: `build.rs` — a wasm/winresource shim with no game types in it —
/// is the only `.rs` file in the repo outside these two roots.
const SCAN_ROOTS: &[&str] = &["src", "tests"];

/// This file names the forbidden pattern in its own prose, and would otherwise
/// flag itself.
const SELF: &str = "tests/loadout_order_audit.rs";

#[test]
fn no_hashmap_is_keyed_by_item_slot() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut violations: Vec<String> = Vec::new();
    for root in SCAN_ROOTS {
        let dir = repo_root.join(root);
        let mut files = Vec::new();
        collect_rs_files(&dir, &mut files).expect("failed to walk source tree");
        for path in files {
            let rel = path
                .strip_prefix(&repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel == SELF {
                continue;
            }
            let src = fs::read_to_string(&path).expect("failed to read source file");
            for (line, snippet) in scan(&Squeezed::new(&src)) {
                violations.push(format!("{rel}:{line}: {snippet}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "A HashMap keyed by ItemSlot iterates in per-process-seeded order. Applying \
         equipment sums floats across a loadout, so that order changes derived stats \
         in the last ULP between runs of the same binary — which breaks this project's \
         byte-identity verification protocol (AS-58).\n\n\
         Use the `Loadout` alias (`BTreeMap<ItemSlot, ItemId>`) from \
         `states::play_match::equipment` instead.\n\n\
         Offending lines (snippets are whitespace-stripped — see Squeezed):\n  {}",
        violations.join("\n  ")
    );
}

/// Every way of naming an `ItemSlot`-keyed `HashMap` that this scan knows about,
/// as `(original line, snippet)` pairs.
fn scan(sq: &Squeezed) -> Vec<(usize, String)> {
    // A renaming import defeats a scan that only knows the real name, so pick
    // the alias up and look for it too: `use std::collections::HashMap as Map;`
    // squeezes to `...::HashMapasMap;`. A stray prose match here is harmless —
    // an invented name simply never matches anything below.
    let mut names = vec!["HashMap".to_string()];
    let alias = Regex::new(r"HashMapas([A-Za-z_][A-Za-z0-9_]*)[;,}]").expect("valid regex");
    for caps in alias.captures_iter(&sq.text) {
        names.push(caps[1].to_string());
    }

    let mut hits: Vec<(usize, String)> = Vec::new();
    for name in &names {
        // The type written out: `HashMap<ItemSlot, _>`, and the turbofish.
        let declared = Regex::new(&format!(r"\b{name}(?:::)?<ItemSlot")).expect("valid regex");
        for m in declared.find_iter(&sq.text) {
            hits.push(sq.at(m.start()));
        }

        // The type never written at all: `let mut m = HashMap::new();` followed
        // by `m.insert(ItemSlot::…)`. Inference gives it the same iteration
        // order and the same bug, with nothing for the spelling rule to see.
        let inferred = Regex::new(&format!(
            r"let(?:mut)?([A-Za-z_][A-Za-z0-9_]*)(?::[^=;]*)?=(?:std::collections::)?{name}::(?:new|default|with_capacity)\("
        ))
        .expect("valid regex");
        for caps in inferred.captures_iter(&sq.text) {
            let binding = &caps[1];
            if sq.text.contains(&format!("{binding}.insert(ItemSlot::")) {
                hits.push(sq.at(caps.get(0).expect("whole match").start()));
            }
        }
    }

    hits.sort();
    hits.dedup();
    hits
}

/// A file with every whitespace character removed, plus a map from each byte of
/// the result back to its 1-based line in the original.
///
/// Matching against this rather than line by line is what lets one pattern cover
/// the spellings that differ only in layout: a generic `rustfmt` wrapped across
/// lines because the signature ran long (`parse_equipment_map`'s return type sits
/// near the wrap column already), and `HashMap< ItemSlot` written with a space.
/// The retained line map is what keeps the failure message pointing at a line.
struct Squeezed {
    text: String,
    /// `lines[i]` is the original line of `text`'s byte `i`.
    lines: Vec<usize>,
}

impl Squeezed {
    fn new(src: &str) -> Self {
        let mut text = String::with_capacity(src.len());
        let mut lines = Vec::with_capacity(src.len());
        let mut line = 1usize;
        for ch in src.chars() {
            if ch == '\n' {
                line += 1;
                continue;
            }
            if ch.is_whitespace() {
                continue;
            }
            text.push(ch);
            // `resize` rather than `push`: a multi-byte char occupies several
            // bytes of `text`, and every one of them needs a line.
            lines.resize(text.len(), line);
        }
        Squeezed { text, lines }
    }

    /// The original line, and a readable snippet, for a byte offset into `text`.
    fn at(&self, offset: usize) -> (usize, String) {
        let line = self.lines.get(offset).copied().unwrap_or(0);
        let mut end = (offset + 72).min(self.text.len());
        while end > offset && !self.text.is_char_boundary(end) {
            end -= 1;
        }
        (line, self.text[offset..end].to_string())
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

// Not `#[cfg(test)]`-gated: this file IS the test crate, and gating risks the
// self-tests silently vanishing.
mod tests {
    use super::{scan, Squeezed};

    /// Every shape the line-by-line scan this replaced let through, plus the two
    /// it already caught. Each is the body of a plausible file, not a fragment:
    /// the inference rule needs the construction AND the insert.
    #[test]
    fn scan_catches_every_known_spelling() {
        let cases: &[(&str, &str)] = &[
            (
                "single-line generic",
                "fn f() -> HashMap<ItemSlot, ItemId> { todo!() }",
            ),
            (
                "turbofish",
                "fn f() { let m = HashMap::<ItemSlot, ItemId>::new(); }",
            ),
            (
                "multi-line generic (what rustfmt emits for a long signature)",
                "fn parse_equipment_map(\n    raw: &str,\n) -> Result<\n    HashMap<\n        ItemSlot,\n        ItemId,\n    >,\n    Error,\n> {\n    todo!()\n}",
            ),
            (
                "space after the angle bracket",
                "fn f() -> HashMap< ItemSlot, ItemId > { todo!() }",
            ),
            (
                "renaming import",
                "use std::collections::HashMap as Map;\nfn f() -> Map<ItemSlot, ItemId> { todo!() }",
            ),
            (
                "renaming import in a braced use",
                "use std::collections::{BTreeMap, HashMap as Map};\nfn f() -> Map<ItemSlot, ItemId> { todo!() }",
            ),
            (
                "inference only — the type is never written",
                "fn f() {\n    let mut m = HashMap::new();\n    m.insert(ItemSlot::Head, ItemId::LionheartHelm);\n}",
            ),
            (
                "inference only, via an aliased import",
                "use std::collections::HashMap as Map;\nfn f() {\n    let mut m = Map::default();\n    m.insert(ItemSlot::Head, ItemId::LionheartHelm);\n}",
            ),
        ];

        for (label, src) in cases {
            let hits = scan(&Squeezed::new(src));
            assert!(!hits.is_empty(), "scan missed the {label} shape:\n{src}");
        }
    }

    /// The shapes that must NOT trip it — a `HashMap` keyed by something else in
    /// a file that also builds a `Loadout` is exactly `equipment.rs`.
    #[test]
    fn scan_passes_the_legitimate_shapes() {
        let cases: &[(&str, &str)] = &[
            (
                "BTreeMap keyed by ItemSlot — the fix itself",
                "pub type Loadout = BTreeMap<ItemSlot, ItemId>;\nfn f() {\n    let mut loadout = Loadout::new();\n    loadout.insert(ItemSlot::Head, ItemId::LionheartHelm);\n}",
            ),
            (
                "HashMap keyed by something else, beside a Loadout insert",
                "fn f() {\n    let mut by_class: HashMap<CharacterClass, Loadout> = HashMap::new();\n    let mut loadout = Loadout::new();\n    loadout.insert(ItemSlot::MainHand, ItemId::ArcaniteReaper);\n    by_class.insert(CharacterClass::Warrior, loadout);\n}",
            ),
            (
                "prose mentioning both names",
                "//! Never widen a Loadout back to a HashMap: ItemSlot ordering is the point.",
            ),
        ];

        for (label, src) in cases {
            let hits = scan(&Squeezed::new(src));
            assert!(
                hits.is_empty(),
                "scan false-positived on the {label} shape: {hits:?}\n{src}"
            );
        }
    }

    /// The snippet in a failure message has to name a real line of the original.
    #[test]
    fn reported_line_is_the_original_line() {
        let src = "fn a() {}\nfn b() {}\nfn c() -> HashMap<ItemSlot, ItemId> { todo!() }\n";
        let hits = scan(&Squeezed::new(src));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 3, "wrong line reported: {hits:?}");
    }
}
