# Replay configs

Watchable with `cargo run --release -- --replay <file>`, and runnable headless
with `--headless <file>`. A seed reproduces byte-identically between the two,
so what you watch is exactly the match a sweep scored.

## Step-4 team-solve comparisons

`Warrior+Priest` vs `Warlock+Priest` on `PillaredArena`. Each TeamPlan config has
a `_legacy` twin on the same seed, so the pair isolates the AI.

| File | Seed | Result | Why it is kept |
|---|---|---|---|
| `solve_s2_win.json` | 2 | TeamPlan wins | Legacy LOSES this one with zero healing. The clearest case for the solve. |
| `solve_s2_legacy.json` | 2 | Legacy loses | Same seed, other profile. |
| `solve_s10_flipped.json` | 10 | TeamPlan wins | Was TeamPlan's WORST case (zero healing, Legacy won with 715). Flipped by conditioning the healer's sightline on whether it can cast — the Priest now ducks during a school lockout instead of holding its line into a Fear. |
| `solve_s10_legacy.json` | 10 | Legacy wins | Same seed, other profile. |

`camp_*.json` are the older step-3 pillar-camp configs; see
`design-docs/2026-08-01-nagrand-camp-handoff.md`.
