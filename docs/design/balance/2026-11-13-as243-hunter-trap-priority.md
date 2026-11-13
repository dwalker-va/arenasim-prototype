# AS-243: the Frost Trap peel waits for a healer freeze in reach

Tier: **directional**. Success condition: **more healer freezes, no lost
wins** (AS-196's blanket hold raised freezes and lost wins). Read from the
Hunter's side, paired, over three seed blocks on three maps, under both AI
profiles.

## The change

The Hunter's traps share one 15s cooldown, so a Frost Trap peel locks the
Freezing Trap out. The peel (the closing band's, and the dead zone's at the
Hunter's feet) is now held only while the pressure trap would freeze the
enemy healer within `HEALER_FREEZE_HOLD_SECS` (3s):
`hunter::frost_trap_held_for_healer`, traced as `PreconditionUnmet` "frost
trap held: a Freezing Trap on the enemy healer is in reach".

Reach is the pressure trap's own predicates, asked forward in time
(`hunter::healer_freeze_in_reach`): a melee in the closing band, Freezing Trap
castable, a `pressure_trap_healer`, its `pressure_trap_landing` (in range,
sprung cleanly by the healer) and the placement's refusals (`trap_landing`,
now shared with `try_place_trap_at`: already held, immune, range, sight). Only
the Hunter's position relative to the healer is projected, in 0.25s steps:
each moves along this frame's heading at the speed its slows leave it, and not
at all while rooted or in hard crowd control (`hunter::forecast_velocity`). The
Peel play's Frost Trap (a teammate in danger) is not held.

Scenes: `tests/hunter_trap_priority_scenes.rs`, ten pairs, one per condition.

## What the peel meets

A diagnostic arm on main, logging every Frost Trap peel the Hunter could
afford (Hunter + Priest/Warrior/Mage/Paladin against Warrior/Rogue + healer,
three maps, 192 matches), found the freeze almost never in reach when the peel
goes out. Of 336 decisions to peel: the healer was out of throw range (117,
mostly 30-45yd and moving apart from the Hunter), a teammate's kill target
(94), not trappable (53), in range but not cleanly sprung (45), not in view
(21), or otherwise refused (6). Asked with a 15s horizon, only 15 of the 336
found a freeze in reach. So the hold fires rarely by
construction; the low freeze rate is mostly not the peel taking the cooldown.

## Sweep

`scripts/gen_sweep.py --full 2 --exclude-double-healer --affects Hunter
--control-cells 8 --n 4`, seed bases 0, 1000 and 2000, on BasicArena,
PillaredArena and TwinPillars: 15,264 matches per arm, run once `Legacy` and
once with `"ai_profile": "TeamPlan"` on every line. Base is main at
`557d6706`; both arms run `tests/milestone_mechanisms.rs` (the observed
runner, read-only). Metrics: `sweeps/2026-11-13-as243-trap-priority.py`; wins
also through `scripts/paired_sweep.py --affects Hunter`.

Per Hunter side (17,280 of them), paired:

| profile | healer freezes / side | sides with a freeze | Frost Traps / side | Hunter wins | flips (+/-) | z |
|---|---|---|---|---|---|---|
| Legacy | 0.0569 → 0.0639 | 5.13% → 5.75% | 1.228 → 1.210 | 43.58% → 43.85% | 71 / 24 | +4.8 |
| TeamPlan | 0.0740 → 0.0773 | 6.92% → 7.22% | 1.099 → 1.088 | 47.36% → 47.46% | 55 / 38 | +1.8 |

By seed block, Hunter wins, Legacy: +0.26, +0.24, +0.31pt (z +2.8, +2.3,
+3.4). TeamPlan: +0.21, −0.10, +0.19pt (z +2.3, −1.0, +2.1). Healer freezes
rise in every block under both profiles. By map the gain is PillaredArena's
(Legacy +0.8pt, z +5.2; TeamPlan +0.4pt, z +2.5); BasicArena and TwinPillars
move within ±0.1pt.

`paired_sweep.py`: the 288 control matches are identical under both profiles.
Legacy, Hunter on team 1 only +0.4pt (z 3.5), on team 2 only +0.3pt for the
Hunter (z 2.9). TeamPlan, team 1 only +0.0pt (ns), team 2 only +0.3pt for the
Hunter (z 2.5). Matches with a Hunter on both sides do not flip.

Directional: it says which way, and it may not be cited as the Hunter's
standing. The sweep ran before the forecast took slows and crowd control into
account. That change moves 30 of 5,088 Hunter matches (both profiles, three
maps). Every one diverges first on a Hunter decision whose hold differs between
the two forecasts, so it was not re-swept.
