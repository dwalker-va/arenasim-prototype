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

## The Hunter+Priest regression

`Hunter+Priest` vs `Rogue+Priest` on `PillaredArena`. **The healer solve is worth
about -17pt to this comp**, against +8 to +17 for every other comp measured — the
one place it is clearly harmful, and the reason step 4's healer half is not yet
called done.

These are HEAD-TO-HEAD configs: team 2 runs `Legacy` in every one, so the ONLY
difference between a pair is team 1's Priest. Anything you see team 2 do is a
constant.

| File | Seed | Result | Notes |
|---|---|---|---|
| `solve_hunter_s8_legacy.json` | 8 | Team 1 wins, 84.0s | The baseline. |
| `solve_hunter_s8_teamplan.json` | 8 | **Team 2 wins**, 72.1s | Same seed, solve on team 1's Priest. Shortest of the three flips — start here. |
| `solve_hunter_s11_legacy.json` | 11 | Team 1 wins, 73.5s | |
| `solve_hunter_s11_teamplan.json` | 11 | **Team 2 wins**, 108.7s | A 35-second grind where Legacy won in 73. The long one, if the failure is not obvious in seed 8. |

What to look for, and the hypothesis being tested: `OccupyCover` anchors on the
nearest living ally and demands line of sight to it. That is cheap when the ally
is a melee planted in a scrum, and expensive when it is a KITER moving fast and
erratically — the healer would be chasing a sight-line that moves with the fight,
which is the same shape step 3 measured for the camp. So: does the Priest look
like it is following the Hunter around rather than holding a position? Seeds 3
and 12 flip the other way (TeamPlan wins) if a counter-example is useful.

`camp_*.json` are the older step-3 pillar-camp configs; see
`design-docs/2026-08-01-nagrand-camp-handoff.md`.
