# Build times: diagnosis and tuning (AS-23)

Measured 2026-09-06 on the 18-core dev machine (rustc 1.96.0, cargo 1.96.0,
macOS system linker). Sibling agents ran concurrent cargo builds during parts of
the session; every wall-clock number below is annotated with the cargo/rustc
process count at its start (`load N`). Phase *shares* and interleaved A/B deltas
are the load-robust evidence; the headline warm-touch numbers were re-taken at
end of session under low load.

## Where the time went (baseline: `codegen-units = 1`, `lto = "thin"`)

The reported stall — `Building [==>] 489/491: arenasim(bin)` — is real and is
**not the linker**. `-Ztime-passes` on the bin unit splits it (load 5):

| phase of `arenasim(bin)` unit | time |
|---|---|
| cross-crate ThinLTO (`LLVM_thinlto`) | 51.7s |
| link (`run_linker`, macOS ld) | 0.5s |
| everything else (codegen of `main.rs`, temps) | ~1.5s |

The macOS linker is already fast; linker experiments are a dead end here.

The other half of the tail: `[profile.release] codegen-units = 1` serialized
codegen of *every* crate — the workspace lib, the bin, and all 489 dependency
units. A warm touch-one-file release rebuild was `arenasim (lib)` →
`arenasim (bin)` back-to-back with nothing else to run: the build sat at
~200-340% CPU on an 18-core machine.

Debug builds are already healthy (incremental works): warm touch-one-file
`cargo build` is **2.9s** (lib 1.5s + bin 0.6s). The pain was release-only —
and release is the pipeline's verification path
(`cargo build --release` + `cargo test --release`).

No spurious dependency rebuilds: a no-change `cargo build --release -v`
recompiles nothing, and alternating build/test does not thrash the cache
(dev-dep feature-unification variants are cached side by side). The bin/lib
split also already exists (`src/lib.rs` + thin `src/main.rs`); the bin unit is
pure ThinLTO + link, not duplicated compilation.

## Baselines and deltas

Warm touch-one-file release rebuild
(`touch src/states/play_match/rendering/effects/dispel_ribbon.rs`):

| release profile | lib | bin | total warm-touch |
|---|---|---|---|
| `codegen-units=1`, `lto="thin"` (old) | 132s | 88s | **221s** (load 22) |
| same, bin-only tail (`touch src/main.rs`) | — | 105s | 105s (load 5) |
| `codegen-units=16`, `lto="thin"` (**shipped**) | 17s | 42s | **57-60s** (load 5) |
| `codegen-units=16`, `lto=false` (not shipped) | 18s | 1.3s | **19s** (load 9) |

Clean release build: 2m58s old (load 11) → 2m00s shipped (load 14); a fully
scrubbed `cargo clean --release` rebuild at head took 3m38s but at load 20.
The load-robust signal is CPU utilization: 539% (old) vs 860-913% (shipped) —
CGU=16 actually uses the cores. Clean debug: 3m10s (load 9). Warm-touch debug:
2.9s (unchanged; already good).

First-time `cargo test --release --no-run` at head (builds the release
dev-dependency graph — egui_kittest/wgpu — plus ~40 integration-test binaries,
each paying its own ThinLTO+link tail, now in parallel): 5m11s (load 5).
Subsequent full `cargo test --release` with binaries cached: 1m35s.

## Runtime cost checks (interleaved A/B, saved binaries, child user-CPU)

- **CGU 1 → 16 (ThinLTO kept): free.** 49-cell `--matrix 1` runs, 3 rounds
  interleaved at load 3: CGU16/CGU1 = 0.945 (i.e. ≤ baseline; the 5% "gain" is
  within the noise floor). ThinLTO across 16 units still does the cross-crate
  inlining.
- **Dropping cross-crate LTO (`lto=false`): ~3% slower, within noise.** Five
  interleaved matrix rounds across two probe sessions: paired deltas +4.1%,
  +10.0%, −3.2%, +0.4%, +3.4% (mean ≈ +3%, run-to-run noise floor ≈ 5%). A
  single seeded 2v2 match ×12 measured a flat 1.000 ratio.

## Behavior (byte-identity)

Same seeded config (`random_seed: 424242`, Warrior+Priest vs Warlock+Priest,
BasicArena) produces a byte-identical match log (`521554f0298665…`) under all
three flag sets — old flags built from origin/main's source, the shipped flags
at head, and the rejected `lto=false`. Release behavior is unchanged; only
codegen parallelism changed.

## What shipped

`[profile.release] codegen-units = 1` → `16` (the rustc release default),
`lto = "thin"` kept. Warm touch-one-file release loop 221s → ~57s; the
`arenasim(bin)` tail 105s → 42s; clean release ~1.5× faster; test-binary
ThinLTO tails now overlap. Sim throughput unchanged (measured).

## Levers considered and not taken

- **Linker swap** — link is 0.5s of the tail; nothing to win.
- **`lto = false`** — would cut the warm loop to **19s** (bin tail 1.3s) and
  shrink every test binary's link tail, at a measured ~3%-within-noise sim
  cost and an unmeasured effect on graphical-client frame time. Rejected
  unilaterally; it is a one-line change if the 57s→19s delta ever matters more
  than ~3% sweep throughput.
- **`incremental = true` for release** — would mostly help the 17s lib re-opt,
  the smaller half of the remaining 57s loop, at the cost of much larger target
  dirs in every agent worktree; not worth it while the bin ThinLTO tail
  dominates. Revisit if the lib share grows.
- **Bin/lib split** — already in place; nothing to do.
- **`bevy/dynamic_linking`** — dev-only lever (`--features dev` already
  exists); irrelevant to the release verification path, and debug warm-touch is
  already 2.9s.
