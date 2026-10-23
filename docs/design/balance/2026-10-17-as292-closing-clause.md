# AS-292: an enemy healer in its closing band is not pressure

Tier: **directional**. Success condition: **no regression** for the side that
changed (`TeamPlan`), in the cells the change can reach. Measured on `main` at
`ca41a9f1`, which carries AS-362's healer crowd-control avoidance.

## The rule

`situation::pressure_threats` counts an enemy targeting a teammate as pressure
from `danger_radius` (12yd), or from `threat_intent_radius` (30yd) when it is a
melee class, a pet, or a ranged enemy still CLOSING (beyond its own preferred
range). The closing clause applies only to a ranged enemy that pursues its
target (`situation::closes`): a ranged DPS (the Warlock at 28yd, the Hunter at
25yd; the Mage's 38yd is past the intent radius). An enemy HEALER (Priest,
Shaman, Druid; the Paladin is melee) moves by its posture machine, so its
distance to the unit it targets says nothing about where it is heading. It sits
in the 28-30yd band as often as it crosses it. It pressures from `danger_radius`
only.

Gated under `TeamPlan` (`SituationConfig::healers_close`, `true` only under
`Legacy`). `Legacy` is byte-identical: the 1,800 `LL` rows of the head-to-head
below match in winner, end reason and duration (1,780 kills, 1,213 distinct
durations), and `camp_sweep`'s `Legacy` column is unchanged.

## Measure first: how often is pressure only a healer in its band?

Bank: `tests/as292_bank.rs` in `sweeps/2026-10-17-as292-instrument.patch`, 9
2v2 comps x 2 maps x 2 profiles x seeds 1-12 = 432 matches, on `0a8f8a36`
(before AS-362). Reduced by `sweeps/2026-10-17-as292-reduce.py`. "Only" means
nothing else holds the test that frame: nothing inside 12yd, no melee or pet
inside 30yd, no closing DPS.

| profile | map | healer PRESSURED entries held only by a healer in its band | healer frames in PRESSURED held only by one | `ally_in_danger` onsets held only by one |
|---|---|---|---|---|
| Legacy | BasicArena | 0 / 570 | 380 / 95,723 (0.4%) | 2 / 399 (0.5%) |
| Legacy | PillaredArena | 0 / 445 | 648 / 144,574 (0.4%) | 13 / 350 (3.7%) |
| TeamPlan | BasicArena | 733 / 1,184 (61.9%) | 42,673 / 132,028 (32.3%) | 3 / 404 (0.7%) |
| TeamPlan | PillaredArena | 585 / 858 (68.2%) | 293,705 / 463,055 (63.4%) | 50 / 506 (9.9%) |

Under `Legacy` the onset never fires on it (AS-249's onset margin leaves a
28yd caster's band empty), so it only extends holds. Under `TeamPlan`, which
enters on the plain test, most of the healer's PRESSURED posture was an enemy
healer standing at 28-30yd. A closing DPS alone accounts for at most 0.1% of
pressured frames in any cell.

## Head-to-head, in the cells it can reach

`sweeps/2026-10-19-as292-gen.py 25 4`: every team is one DPS plus a Priest,
Shaman or Druid; each meets each (15 x 15 cells, mirrors included) on both
maps, in both seat assignments (`TL`, `LT`) at 25 seeds, with 4 `LL` control
seeds per cell. 22,500 scored matches per arm, before = `main`, after = this
change merged onto it. Scored from the `TeamPlan` side by
`sweeps/2026-10-19-as292-h2h.py`; CSVs `sweeps/2026-10-19-as292-h2h-{before,after}.csv.gz`.

| slice | n | before | after | delta | z |
|---|---|---|---|---|---|
| all | 22,500 | 51.0% | 51.4% | +0.40pt | +1.66 |
| BasicArena | 11,250 | 52.7% | 51.3% | **-1.40pt** | **-4.02** |
| PillaredArena | 11,250 | 49.2% | 51.4% | **+2.20pt** | **+6.65** |
| `TeamPlan` Priest | 7,500 | 47.6% | 48.8% | +1.16pt | +2.19 |
| `TeamPlan` Shaman | 7,500 | 51.6% | 51.6% | +0.05pt | +0.16 |
| `TeamPlan` Druid | 7,500 | 53.7% | 53.7% | -0.01pt | -0.04 |
| vs enemy Priest | 7,500 | 50.5% | 52.4% | +1.88pt | +4.60 |
| vs enemy Shaman | 7,500 | 52.5% | 51.9% | -0.67pt | -1.40 |
| vs enemy Druid | 7,500 | 49.8% | 49.8% | -0.01pt | -0.04 |

Both seat assignments agree on each map (BasicArena `LT` -1.44pt, `TL`
-1.37pt; PillaredArena `LT` +2.35pt, `TL` +2.06pt). The cells that move most,
1,250 matches each:

| map | `TeamPlan` healer v enemy healer | delta | z |
|---|---|---|---|
| BasicArena | Priest v Shaman | -7.04pt | -4.44 |
| BasicArena | Priest v Druid | -3.76pt | -3.30 |
| BasicArena | Shaman v Shaman | -3.76pt | -4.27 |
| BasicArena | Shaman v Druid | +2.88pt | +3.98 |
| PillaredArena | Priest v Priest | +10.08pt | +8.00 |
| PillaredArena | Priest v Druid | +4.40pt | +4.23 |
| PillaredArena | Priest v Shaman | +4.24pt | +3.46 |
| PillaredArena | Druid v Druid | -3.52pt | -4.69 |

Before AS-362 the same design read BasicArena -2.04pt (z=-5.10), PillaredArena
+0.28pt, pooled -0.88pt (z=-3.92), vs an enemy Shaman -4.61pt and a `TeamPlan`
Druid -3.41pt. AS-362 recovered the Druid and the Shaman matchups and turned
Nagrand positive; BasicArena is still negative, now concentrated on the
`TeamPlan` Priest.

**Why BasicArena loses.** `sweeps/2026-10-19-as292-logscan.py Shaman BasicArena 8`,
every <DPS>+Priest (`TeamPlan`) v <DPS>+Shaman (`Legacy`) cell, 200 matches
per arm, per match, `main` -> this change:

| on the `TeamPlan` Priest | main | AS-292 |
|---|---|---|
| Frost Shock landed | 0.45 | 1.24 |
| Fear landed | 0.06 | 0.23 |
| Polymorph landed | 0.03 | 0.22 |
| casts interrupted | 1.18 | 1.04 |
| healing done | 367 | 268 |
| team-1 wins | 51% | 42% |

With no false pressure from the enemy Shaman at 28-30yd the Priest no longer
backs off the fight on an open map, and stands inside the Shaman's 20yd Frost
Shock and the enemy DPS's crowd control. Frost Shock is a snare, outside
AS-362's crowd-control reach set.

## What it moves on Nagrand

`camp_sweep` (Warrior+Priest v Warlock+Priest, seeds 1-12), `TeamPlan` column,
`main` -> this change:

| | main | AS-292 |
|---|---|---|
| team-1 wins | 10/12 | 12/12 |
| mean heal to the Warrior | 241 | 307 |
| Warrior died | 2/12 | 0/12 |
| heal line to the Warrior occluded | 23% | 33% |
| Warlock denied the Priest, after contact | 14.3s | 12.3s |
| Priest hard-CC'd | 3.6s | 6.0s |

The heal line is blocked more, but not where it matters. Over seeds 1-24 of
the same camp, blocked frames rise from 20.7% to 29.5%, while the share of
frames with the Warrior below 80% and the Priest free to cast that are
blocked is unchanged (46.3% -> 47.0%; 8,524 -> 8,916 frames). The added blind
frames are with the Warrior healthy or the Priest locked: the Priest now
hides from a Warlock that is fighting its Warrior (blocked frames with the
Warrior on the Warlock 7,661 -> 15,046) where it used to stand off the enemy
Priest.

## The three `TeamPlan` probes

All three keep their thresholds and seeds.

- `teamplan_healer_is_not_a_statue_on_basicarena` (floor 0.75 u/s) passes:
  set minimum 1.16, median 1.72. It **no longer catches the flat-field
  anti-statue mutant** on this build: with `field_is_flat` forced false the
  set runs 1.24-2.30 u/s, where on `main` the same mutant fails at seed 5
  (0.32). Outside the set, seed 14 reads 0.42.
- `juke_bounded_over_the_seed_bank` (every dance a Mage+Priest win) passes.
- `teamplan_healer_keeps_its_heal_line_on_nagrand` now measures the solve's
  own heal line: on frames the Priest is PRESSURED and free to cast, the share
  whose movement goal has no line to the Warrior. Seeds 1-24: `main` 4-30%
  (pooled 13.6%), AS-292 5-42% (pooled 21.5%), `OccupyCover` without
  `C_SIGHT` 52-94% (pooled 78.6%). The ceiling is 50%; the mutant fails it at
  seed 7 (68%). The old measure, the share of all frames with the line
  blocked, cannot separate the two: AS-292 runs 12-49% and the mutant 24-63%,
  44 / 33 / 22% against 46 / 35 / 46% at the pinned seeds.

## The Peel play's evidence

`ally_in_danger` is what the `TeamPlan`-gated Peel play answers, so the
milestone sweep that graduates or deletes Peel measures it on fewer windows:
on `0a8f8a36` the bank's `TeamPlan` windows per match fell 3.74 -> 3.33
(BasicArena) and 4.69 -> 4.06 (PillaredArena). The windows removed are the
ones an enemy healer at 28-30yd opened, where no attacker was there to peel.
