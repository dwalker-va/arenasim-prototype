---
title: "Detecting a development checkout: neither CARGO_MANIFEST_DIR nor a manifest walk is sufficient alone"
category: implementation-patterns
tags:
  - packaging
  - distribution
  - cargo
  - balance-tooling
  - paths
module: src/paths.rs
symptom: "Balance scripts stop finding their output under match_logs/, or cargo run panics at startup resolving assets under an external target directory"
root_cause: "CARGO_MANIFEST_DIR misses binaries invoked directly rather than through cargo; the manifest walk misses checkouts whose CARGO_TARGET_DIR points outside the repo"
severity: high
applies_when:
  - "Touching src/paths.rs or adding any new 'am I a shipped build?' check"
  - "Diagnosing why a balance script's output landed somewhere unexpected"
  - "Setting CARGO_TARGET_DIR or build.target-dir outside the repo"
date: 2026-08-08
---

# Development-vs-installed detection needs two signals

A shipped build must write to a per-user directory; a checkout must keep writing
into the checkout, because every balance script and probe in this repo assumes
it. Getting that classification wrong breaks one or the other, and both failures
are quiet.

## The signal that looks right and is not

`CARGO_MANIFEST_DIR` is the obvious discriminator. Bevy's own asset resolution
keys on it, and cargo sets it for every process it launches.

It is wrong **on its own** here, because the balance tooling does not go through
cargo:

```bash
# scripts/hunter_2v2_matrix.sh:90 (and mage/shaman/run_combat_tests)
BINARY_PATH="target/release/arenasim"
```

Running the binary directly leaves the variable unset. Keying on it alone
classifies exactly the workflows that must stay checkout-relative as *installed*,
relocating their output out of the repo.

## The signal that also looks right and is also not

Walking up from the executable looking for a `Cargo.toml` fixes that: it catches
`cargo run` and a direct `target/release/arenasim` invocation alike, and an
installed `.app` or unpacked zip has no manifest above it.

It is wrong **on its own** too. A checkout that sets `CARGO_TARGET_DIR` (or
`build.target-dir` in `~/.cargo/config.toml`) outside the repo — a common
shared-build-directory setup — puts the binary somewhere with no manifest above
it at all. There, `cargo run` and `cargo test` classify as installed, assets
resolve to `<target-dir>/debug/assets`, and startup panics.

Reproduce:

```bash
CARGO_TARGET_DIR=/tmp/tgt cargo test --lib paths::
```

## The fix: two independent signals, OR'd

```rust
fn detect_development(manifest_dir_set: bool, exe: Option<&Path>) -> bool {
    manifest_dir_set || exe.map(is_development_build).unwrap_or(true)
}
```

Each covers the other's blind spot. The asymmetry that makes this safe:
`CARGO_MANIFEST_DIR` is **never set for an end user's build**, so that arm can
only ever *widen* the development classification. Its worst case is a developer's
odd shell exporting it, where the failure mode is "writes next to the app" — the
old behaviour, not a crash. The manifest walk is the arm that can wrongly say
"installed", and it is the one being backstopped.

An unresolvable executable path degrades to *development* for the same reason:
guessing "installed" would relocate a developer's files, which is the more
destructive error.

## Prevention

- Keep the pure predicate testable without touching the real filesystem layout —
  `detect_development` takes both signals as arguments so each can be pinned
  independently, including the external-target-dir case.
- When adding a new "am I shipped?" check anywhere, do not invent a second
  discriminator. Ask `crate::paths`, or the two answers will eventually disagree.
- Regression-guard the checkout side by asserting the **glob the scripts actually
  use** (`tests/write_paths.rs` pins `match_logs/match_*.txt` for
  `scripts/behaviour_baseline.sh:69`), not just that some path was returned.

Related: [[packaged-build-asset-path-resolution]] — the read side of the same seam.
