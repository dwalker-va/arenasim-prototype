# AS-292: an enemy healer in its closing band is not pressure

Tier: **directional**. Success condition: **no regression** for the side that
changed (`TeamPlan`). Shipped under the user's ruling of 2026-10-18: the rule
lands now, and the three `TeamPlan` probes it moves are rebaselined by
threshold to the new healer behaviour (§ What it moves). AS-362 owns the
healer positioning that should restore them.

## The rule

`situation::pressure_threats` counts an enemy targeting a teammate as pressure
from `danger_radius` (12yd), or from `threat_intent_radius` (30yd) when it is a
melee class, a pet, or a ranged enemy still CLOSING (beyond its own preferred
range). The closing clause now applies only to a ranged enemy that pursues its
target (`situation::closes`): a ranged DPS (the Warlock at 28yd, the Hunter at
25yd; the Mage's 38yd is past the intent radius). An enemy HEALER (Priest,
Shaman, Druid; the Paladin is melee) moves by its posture machine, so its
distance to the unit it targets says nothing about where it is heading. It sits
in the 28-30yd band as often as it crosses it. It pressures from `danger_radius`
only.

Gated under `TeamPlan` (`SituationConfig::healers_close`, `true` only under
`Legacy`). `Legacy` is byte-identical: 216 bank matches identical in result and
whole trace, and 2,450 `LL` head-to-head rows identical (2,440 kills, 1,449
distinct durations).

## Measure first: how often is pressure only a healer in its band?

Bank: `tests/as292_bank.rs` in `sweeps/2026-10-17-as292-instrument.patch`, 9
2v2 comps x 2 maps x 2 profiles x seeds 1-12 = 432 matches, on `0a8f8a36`.
Reduced by `sweeps/2026-10-17-as292-reduce.py`. "Only" means nothing else holds
the test that frame: nothing inside 12yd, no melee or pet inside 30yd, no
closing DPS.

| profile | map | healer PRESSURED entries held only by a healer in its band | healer frames in PRESSURED held only by one | `ally_in_danger` onsets held only by one |
|---|---|---|---|---|
| Legacy | BasicArena | 0 / 570 | 380 / 95,723 (0.4%) | 2 / 399 (0.5%) |
| Legacy | PillaredArena | 0 / 445 | 648 / 144,574 (0.4%) | 13 / 350 (3.7%) |
| TeamPlan | BasicArena | 733 / 1,184 (61.9%) | 42,673 / 132,028 (32.3%) | 3 / 404 (0.7%) |
| TeamPlan | PillaredArena | 585 / 858 (68.2%) | 293,705 / 463,055 (63.4%) | 50 / 506 (9.9%) |

Under `Legacy` the onset never fires on it (AS-249's onset margin leaves a
28yd caster's band empty), so it only extends holds. Under `TeamPlan`, which
enters on the plain test, **most of the healer's PRESSURED posture is an enemy
healer standing at 28-30yd**. A closing DPS alone accounts for at most 0.1% of
pressured frames in any cell.

## Bank, main -> branch

`sweeps/2026-10-17-as292-compare.py`, same 432 matches:

| profile | map | healer PRESSURED entries/match | healer seconds in PRESSURED/match | `ally_in_danger` windows/match |
|---|---|---|---|---|
| Legacy | both | unchanged | unchanged | unchanged |
| TeamPlan | BasicArena | 10.96 -> 5.94 | 32.8 -> 22.8 | 3.74 -> 3.33 |
| TeamPlan | PillaredArena | 7.94 -> 3.97 | 82.7 -> 32.3 | 4.69 -> 4.06 |

## Head-to-head

`sweeps/2026-10-12-as288-gen.py 2` (every 2v2 team against every other, both
maps, `TL` / `LT` / `LL`), 12,250 matches per arm, scored from the `TeamPlan`
side by `sweeps/2026-10-12-as288-oriented.py`:

```
B_LT  TeamPlan wins 1255 -> 1245 (-0.41pt)  discordant 98 (+44 / -54)  p=0.363
B_TL  TeamPlan wins 1255 -> 1242 (-0.53pt)  discordant 94 (+40 / -54)  p=0.180
P_LT  TeamPlan wins 1270 -> 1284 (+0.57pt)  discordant 89 (+50 / -39)  p=0.289
P_TL  TeamPlan wins 1253 -> 1260 (+0.29pt)  discordant 95 (+50 / -45)  p=0.682
ALL   TeamPlan wins 5033 -> 5031 (-0.02pt)  discordant 376 (+184 / -192)  p=0.718
```

No measurable effect on win rate. 1,345 of 9,800 head-to-head matches moved;
the 376 that changed the `TeamPlan` side's result split evenly.

## What it moves

`camp_sweep` (Warrior+Priest v Warlock+Priest, Nagrand, seeds 1-12), `TeamPlan`
column, main -> branch:

| | main | branch |
|---|---|---|
| team-1 wins | 8/12 | 10/12 |
| mean heal to the Warrior | 122 | 320 |
| matches with zero heal to the Warrior | 5 | 1 |
| Warlock denied the Priest, after contact | 4.1s | 14.7s |
| heal line to the Warrior occluded | 14% | 28% |

On main the camp Priest spends most of the match PRESSURED by the enemy Priest
at 29yd, and in five of twelve matches never heals its Warrior. On the branch it
heals 2.6 times as much and buys three times the cover after contact, but sees
its Warrior less.

Three `TeamPlan` probes in `tests/movement_probes.rs` were calibrated against a
`TeamPlan` healer whose posture was mostly this phantom pressure, and each is
rebaselined by threshold, seeds unchanged:

- `teamplan_healer_keeps_its_heal_line_on_nagrand` (ceiling 35%): seed 12 is
  43%. `scan_nagrand_teamplan`, heal line blocked over seeds 1-12:

  ```text
  main    4  4 19  4 30  4 19  6 21 39  9 11   (median 10%)
  branch 30 13 29 27 22 42 20 35  8 23 23 43   (median 25%)
  ```

  Warlock-denied occlusion rises on 10 of the 12 seeds (main 18.9-25.1s,
  branch 22.0-44.8s).
- `teamplan_healer_is_not_a_statue_on_basicarena` (floor 0.75 u/s): seed 3 is
  0.71. `scan_statue_seeds` over 1-20: main has none under the floor (minimum
  0.90); the branch has four (3: 0.71, 10: 0.40, 11: 0.67, 14: 0.22). In seed
  14 the Priest, no longer backed off early by the enemy Priest at 29yd, walks
  into the fight in FREE formation, is then PRESSURED by that Priest from 2yd,
  and the flat-field solve parks it for 12s.
- `juke_bounded_over_the_seed_bank`: the lone `TeamPlan` Shaman now wins 3 of
  the bank's 32 dances (seeds 30, 42, 43), each a 2v1 under about ten seconds
  with a normal fizzle rate.

The new thresholds:

| probe | was | now | measured |
|---|---|---|---|
| heal line blocked, per seed | <= 35% | <= 50% | worst 43%, median 25% (seeds 1-12) |
| statue floor, per seed | >= 0.75 u/s | >= 0.30 u/s | set minimum 0.40, median 1.91 (0.22 at seed 14, outside the set) |
| statue median | >= 1.2 u/s | unchanged | 1.91 |
| juke: Mage+Priest wins | every dance | >= 80% of dances | 29 / 32 (91%) |

What still fails each. The heal-line ceiling catches the top of the old
40-55% self-blocking band. The statue floor catches a healer frozen while
stood on: a Priest made to stand still whenever an enemy is within 12yd reads
0.00 u/s and fails it. It does not catch the solve-level mutants on this
build: with the flat-field anti-statue key disarmed the set runs 0.90-1.93
u/s, and with the `PRESSURED` solve made to hold in place 0.90-1.72. The low
seeds are therefore not the statue that key fixed, and the key is unguarded by
this probe until AS-362 re-derives it. The juke share is backed by the
fizzle-rate and duration bounds, which a chase that stops closing fails first.

## The Peel play's evidence

`ally_in_danger` is what the `TeamPlan`-gated Peel play answers, so the
milestone sweep that graduates or deletes Peel measures it on fewer windows:
the bank's `TeamPlan` windows per match fall 3.74 -> 3.33 (BasicArena) and
4.69 -> 4.06 (PillaredArena). The windows removed are the ones an enemy healer
at 28-30yd opened, where no attacker was there to peel. `peel_play_probe` on
this build: 320 windows with the play, 24 answered within one GCD (7.5%)
against 2 of 311 withheld (0.6%), answered z 4.33, survival z -0.06.
