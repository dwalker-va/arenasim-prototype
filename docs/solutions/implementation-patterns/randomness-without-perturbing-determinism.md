---
title: "Varied Graphical Randomness Without Perturbing Headless Determinism"
date: 2026-08-08
category: implementation-patterns
module: states/play_match/banter
problem_type: architecture_pattern
severity: high
applies_when:
  - "A graphical-only feature needs per-match variety (flavor text, cosmetic pick, idle animation choice)"
  - "The codebase has a headless byte-identity requirement that a `GameRng` draw would break"
  - "The same input must produce the same output every run, so `rand::random()` is not acceptable either"
tags:
  - determinism
  - byte-identity
  - headless
  - rng
  - banter
  - hashing
---

# Varied Graphical Randomness Without Perturbing Headless Determinism

## Context

Team banter (PR dwalker-va/arenasim-prototype#98) needed to pick one exchange from a pool of 38, differently per match and per team, but reproducibly: the same match replayed must say the same lines.

Both obvious options are wrong here.

- **Draw from `GameRng`.** This is the codebase's deterministic generator, so replay works — but every draw advances shared generator state. A graphical-only feature that consumes draws shifts the sequence every later combat roll sees, so the same seed produces a different match with the client open than without. `scripts/behaviour_baseline.sh` exists precisely to catch that, and it would.
- **Draw from `rand::random()` / OS entropy.** No effect on `GameRng`, but the same seed now says different things every run, and nothing about a match is reproducible from its seed.

The requirement is genuinely "deterministic in the seed, but *read-only* with respect to the generator."

## Guidance

**Read the seed; hash it with your own inputs; never draw.**

`GameRng` stores its seed as a public field beside a private generator (`src/states/play_match/components/resources.rs:17`):

```rust
pub struct GameRng {
    rng: StdRng,
    /// The seed used to initialize this RNG (if deterministic)
    pub seed: Option<u64>,
}
```

That shape is the whole trick. `rng` is private, so a consumer *structurally cannot* advance it; `seed` is public, so a consumer can derive from it. Reading a `u64` field has no side effect on draw order — determinism is preserved by construction, not by discipline.

Then hash the seed together with whatever should vary the result:

```rust
let roll = banter_roll(seed, lineup.team, context, occurrence);
```

(`src/states/play_match/banter/resolver.rs:190` — a splitmix64-style mix over seed, team, context and a per-team occurrence counter, returning `[0, 1)`.)

Each extra input is an axis of variety: `team` keeps the two sides from telling the same joke, `occurrence` keeps a team that gets corrected three times from repeating itself, `context` separates the pools.

Three details worth copying:

1. **Handle a `None` seed explicitly.** Fall back to a constant so the feature still resolves deterministically rather than panicking or going silent.
2. **The hash's initial state must differ from that fallback constant.** The mix starts with an XOR, so folding a value into an identical state zeroes the accumulator and pins every seedless draw to index 0. This is pinned by a test (`banter_roll(None, ..) > 0.0`).
3. **Prove the no-perturbation claim, do not assert it.** The argument above is sound, but "graphical-only" claims are exactly what silently rot. PR #98 verified byte-identity three ways: two fixed-seed match-log diffs, `scripts/behaviour_baseline.sh` (27 matches, empty diff), and a new `tests/determinism_pin.rs` that pins two `MatchResult`s bit-exactly so a future change trips `cargo test` rather than a manual sweep.

## Why This Matters

An RNG draw is the easiest determinism violation to introduce and one of the hardest to attribute. The symptom appears far from the cause — a balance sweep shifts, or graphical and headless disagree on a seed — and the offending line looks entirely innocent, because *using the deterministic RNG* is what you are supposed to do. See the graphical/headless seed divergence in this codebase's history for how long that class of bug can hide.

Making the generator private and the seed public converts the rule from something a reviewer has to remember into something the type system enforces. Any codebase with a determinism gate should expose its seed this way.

## When to Apply

Any graphical-only or diagnostic feature that wants per-match variety: flavor text, cosmetic variant selection, idle-animation choice, camera flourishes, ambient effects. Also useful for *sim* features that want a value derived from the match without consuming sequence position — though there, prefer a real `GameRng` draw unless there is a specific reason not to, since sim randomness legitimately belongs in the sequence.

Do **not** use this to sneak gameplay randomness past the determinism gate. The idiom is for output that no sim system reads back.

## Examples

Fallback constants, deliberately distinct (`src/states/play_match/banter/resolver.rs:40`):

```rust
/// Selection seed used when `GameRng::seed` is `None`.
const BANTER_FALLBACK_SEED: u64 = 0x4B41_4C4C_4341_4C4C;

/// Initial hash state. Deliberately DIFFERENT from `BANTER_FALLBACK_SEED` —
/// `mix` starts with an xor, so folding a value into an identical state would
/// zero the accumulator and collapse the seedless case to a fixed roll of 0.
const BANTER_HASH_INIT: u64 = 0xA076_1D64_78BD_642F;
```

Related: [Cosmetic marker cross-mode spawn parity](cosmetic-marker-cross-mode-spawn-parity.md) covers the other half of the byte-identity discipline — entity-allocation order must match across modes even when the entities are purely cosmetic.
