---
title: "Packaged builds panic at startup: Bevy's executable-relative asset resolution does not cover direct std::fs reads"
category: implementation-patterns
tags:
  - bevy
  - packaging
  - assets
  - distribution
  - macos
  - windows
module: src/paths.rs
symptom: "A double-clicked .app or unpacked .zip panics with 'Failed to read assets/config/abilities.ron: No such file or directory', or runs but silently renders placeholder art"
root_cause: "Bevy's AssetServer resolves relative to the executable, but the RON config files are read with std::fs and inherit the process working directory, which is / for a GUI-launched app"
severity: high
applies_when:
  - "Adding any std::fs read of a bundled asset (RON config, icon directory, data file)"
  - "Packaging the game for distribution, or debugging a build that works from the checkout but not when installed"
  - "Auditing whether a path sweep was actually complete"
date: 2026-08-08
---

# Packaged builds and the two asset-path worlds

A packaged build died before opening a window:

```
Failed to load ability definitions:
Failed to read assets/config/abilities.ron: No such file or directory (os error 2)
```

The assets were *in* the bundle, beside the binary, exactly where Bevy wants them.

## Why it happens

Bevy's file asset reader (`bevy_asset-0.16.1/src/io/file/mod.rs:19`) resolves its
root as `BEVY_ASSET_ROOT` -> `CARGO_MANIFEST_DIR` -> **the executable's parent
directory**. That last fallback is what makes `assets/` beside the binary work in
a shipped build, and it is why the bundle layout is correct.

But that logic only applies to things loaded **through `AssetServer`**. This repo
reads its RON configuration with plain `std::fs::read_to_string`:

```rust
let contents = std::fs::read_to_string("assets/config/abilities.ron")?;
```

A relative path there resolves against the **process working directory**, which
Bevy never touches. During development the working directory is the checkout, so
the two worlds agree by coincidence. A GUI-launched macOS app runs with a working
directory of `/`, and they diverge.

This hits the graphical client exactly as hard as headless — the config plugins
run in both modes.

## The fix

Route every direct filesystem read through a seam that mirrors Bevy's own
fallback (`src/paths.rs`):

```rust
pub fn asset_path(relative: &str) -> PathBuf {
    assets_dir().join(relative)   // "assets/..." in a checkout,
}                                 // "<exe dir>/assets/..." when installed
```

Seven load sites were affected: abilities, items, loadouts, movement, maps,
banter, and the emoji directory.

## Two traps worth knowing

**The failure can be silent.** The startup panic was loud, but the emoji loader
was not: its "directory does not exist" branch is a legitimate state (nobody has
added art yet), so it warned, marked itself loaded, and every banter bubble
rendered a placeholder forever. A missing-asset path that has a graceful fallback
will not announce itself.

**Grepping for the string literal is not a sweep.** The original sweep used
`grep -rn '"assets/' src/` and looked complete. It missed
`Path::new("assets").join(EMOJI_DIR)`, which builds the same path a different
way. Sweep by **I/O call site** instead, and check what each one's path argument
is:

```bash
grep -rn "read_to_string\|File::open\|read_dir" src/ --include="*.rs"
```

Every hit should either take a caller-supplied path or go through the seam.

## Prevention

- Any new `std::fs` read of a bundled asset goes through `crate::paths::asset_path`.
  Only `AssetServer` loads get executable-relative resolution for free.
- Verify packaging by **assembling a bundle outside the checkout and running it**,
  not by reading code. Both bugs here were invisible to inspection and obvious
  within seconds of running the artifact.
- A graceful fallback around a missing asset hides this class of bug. When adding
  one, ask whether it could mask a path error rather than a genuinely absent file.

Related: [[development-vs-installed-detection]] — the other half of the same seam.
