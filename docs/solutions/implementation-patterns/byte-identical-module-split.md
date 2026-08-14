---
title: "Splitting a Large Module Byte-Identically (and the Two Traps the Plan Misses)"
date: 2026-08-13
category: implementation-patterns
module: states/play_match/rendering
problem_type: design_pattern
severity: medium
applies_when:
  - "Splitting a large single-file module into per-concern submodules with zero intended behavior change"
  - "Any pure code-move refactor where a reviewer or downstream work needs proof the move changed nothing"
tags:
  - refactoring
  - module-split
  - rust
  - byte-identity
  - verification
  - visual-effects
---

# Splitting a Large Module Byte-Identically (and the Two Traps the Plan Misses)

## Context

`src/states/play_match/rendering/effects.rs` had grown to 4,624 lines holding ~25
unrelated visual-effect families and was the standing prerequisite before the next
signature animation (Fear) could land in it (flagged on PR #103; see
[signature-ability-animation-procedure.md](signature-ability-animation-procedure.md)).
It was split into `rendering/effects/` with one file per effect family (25 content
files + a wiring-only `mod.rs`), as a pure behavior-preserving move.

The move itself is mechanical. What is *not* obvious up front — and what a
carefully-written plan still got wrong in three places — is (a) how to *prove* the
move changed nothing, and (b) two false assumptions that plans about this shape of
work reliably make. Capturing them so the next split (and the split that precedes
every future large-file signature animation) inherits the solved shape.

## Guidance

### 1. The byte-identity multiset diff is the load-bearing verification — do it *during* the move, not after

Extract each submodule's body from a **pristine copy** of the original via `sed -n
'A,Bp'` (byte-exact copy, never hand-retyped), then prove the move introduced no
drift with an import-agnostic multiset diff:

```bash
# expected = original body (the lines below the shared import preamble)
sed -n '19,4624p' orig_effects.rs | sort > expected.txt

# actual = every new submodule with its LEADING `use`/blank lines stripped,
# so trimmed imports and added `use super::...;` lines don't pollute the compare
for f in effects/*.rs; do
  [ "$(basename "$f")" = mod.rs ] && continue
  awk 'started{print;next} /^use /||/^$/{next} {started=1;print}' "$f"
done | sort > actual.txt

diff expected.txt actual.txt   # empty ⇒ bodies are byte-identical
```

A clean diff proves no line was lost, added, or altered. A *small* diff is even more
valuable than an empty one: it surfaces **exactly** the intended edits and nothing
else. In this split the diff was exactly the 5 boundary-crossing edits (below) — that
is the whole verification, far stronger than a "spot-check a few functions" eyeball.
`registration_audit` + an unedited registration file separately prove no system
dropped out of the graphical schedule, so a live GUI eyeball is optional, not the
gate.

### 2. Trap A — "no private helper is shared across group boundaries" is usually false

Inside one module, every `fn` (even private) is mutually visible, so cross-helper
calls are invisible until you draw module lines through them. A planning-time grep
that checks only a *sample* of helper names will miss the rest. Here four private
helpers were called across what became boundaries:

- `spawn_transform_puff` (transform_puffs) — called by polymorph
- `dispel_burst_colors` (dispel_burst) — called by dispel_ribbon
- `trap_type_rgb`, `trap_type_emissive` (traps) — called by ice_block

The minimal, non-body fix: bump each shared helper `fn` → `pub(crate) fn` and add a
targeted `use super::<defining_mod>::<fn>;` to each caller (both are import/visibility
changes, not body changes — they show up in the multiset diff as exactly the intended
lines). **Grep for every private helper's call sites, not a sample**, before drawing
the boundaries — or just let the first compile enumerate them (the compiler names
each `cannot find function ... in this scope`, which is a reliable oracle for this
class).

### 3. Trap B — inline `#[cfg(test)]` blocks sit next to their *neighbors*, not the code they test

Assigning test blocks to submodules by physical line position mis-assigned **two of
three** here:

- `dispel_ribbon_mesh_tests` sat at the tail of the *windfury* range but tests
  `build_dispel_ribbon_mesh` → belongs in `dispel_ribbon.rs`.
- the `bubble_visible` tests sat at the tail of *casting_orbs* but test speech-bubble
  logic → belongs in `speech_bubbles.rs`.

A test block travels with **what its `use super::*;` needs to resolve**, which is the
code it asserts on — read each block's actual calls, never trust its line neighbors.
Both `windfury` and `casting_orbs` ended up with no inline test. A test-only compile
(`cargo test --lib`) is what surfaces this: the block fails to resolve its target
function from the wrong module.

### 4. `super::` in an item body is a hidden boundary edit

Top-of-file imports being absolute does **not** mean the whole file is. One function
took `Res<super::emoji::EmojiIcons>`; `super` meant `rendering` inside `effects.rs`
but `effects` inside `effects/speech_bubbles.rs`, so the path silently broke on the
move. Grep the body for `super::` (excluding `use super::*;` in test mods) — an
in-body relative path is a required rewrite to an absolute path, and it is the kind of
edit a "stop on any compile error, it's a boundary error" rule will misclassify unless
you pre-authorize it.

## Why This Matters

All three traps fail the same way: the plan looks airtight, the move looks mechanical,
and the breakage is a compile error the plan told the implementer to treat as a "did I
mis-cut a boundary?" investigation rather than a known, pre-authorized edit. Naming
them up front converts three investigations into three one-line fixes. The multiset
diff is what lets you *ship* a 4,600-line move with confidence instead of hoping a
spot-check caught everything — it is cheap, exhaustive, and turns "looks right" into
"provably only the intended edits changed."

## When to Apply

- Any pure module-split or large code-move refactor (the multiset diff + all three
  traps apply verbatim).
- Before the next large-file signature animation: the same `effects/` split shape
  recurs whenever a rendering file crosses ~a few thousand lines.
- The traps generalize beyond Rust: shared-visibility helpers, test-adjacency, and
  relative-path breakage exist in any language with module-private scope and
  relative imports.

## Examples

The split itself: `effects.rs` → `src/states/play_match/rendering/effects/`
(25 submodules + `mod.rs`), extracted via a `sed`-from-pristine script, verified with
the multiset diff above, `cargo fix --lib` to trim the per-file import preambles, and
the full suite green (520 lib tests incl. the three relocated inline blocks,
`registration_audit`, `polymorph_visual_probes`). The 5 boundary edits — one
`super::emoji` path rewrite + four `fn`→`pub(crate) fn` visibility bumps — were the
*entire* content of the multiset diff.

## Related

- [signature-ability-animation-procedure.md](signature-ability-animation-procedure.md) — flagged this split as the prerequisite before the next signature (Fear); this doc is the "how the prerequisite went" companion.
- [graphical-mode-missing-system-registration.md](graphical-mode-missing-system-registration.md) — why `registration_audit` is a sufficient backstop that no system dropped from the graphical schedule during the move.
- [adding-visual-effect-bevy.md](adding-visual-effect-bevy.md) — the per-effect three-system lifecycle that makes each effect family a clean extraction unit.
