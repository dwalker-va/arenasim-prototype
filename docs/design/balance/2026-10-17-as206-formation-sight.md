# AS-206: the FREE formation point keeps sight of the teammates

**Tier:** DIRECTIONAL (paired sweep) plus a head-to-head per
`docs/design/team-level-positioning-ai.md` *How to measure a step*.
**Success condition, stated up front:** the defect goes (a FREE healer no
longer idles blind behind a pillar from a healthy teammate), with no balance
movement worth the name. Measured 2026-10-17, `main` at `0a8f8a36` against this
branch.

## The shape

| Question | Answer | Why |
|---|---|---|
| Sight to whom? | Every living non-pet teammate when a nearby spot has it; otherwise the most injured (the anchor). | A FREE healer has no one dying, so every teammate is a heal target it should keep. In a 2v2 the two rules are the same rule. When no spot sees all (two pillars, two shadows), the anchor is never given up for the others. |
| Constraint or scored term? | A constraint with a nearest-feasible repair. | The formation point is one goal, not a choice among scored steps — there is nothing to add a term to. The repair has `nearest_standable`'s shape: a point that already satisfies it comes back bit-identical, so every point in sight (all of them on an obstacle-free map) is untouched. |
| Where does the repair look? | The two tangent rays bounding the teammate's shadow past the obstacle's footprint disc; the point projected onto each, pushed `SIGHT_MARGIN` (2.5yd) clear. | Geometry-anchored candidates slide continuously with the fight. A lattice round the point moves with it and hops cells (`team_solve::candidates_for` documents the pathology). 2.5yd exceeds the 1.5yd formation dead zone, so wherever the walk stops is in sight. |
| Profile | `TeamPlan` only. | The AI-profile rule. `Legacy` is unchanged. |

The repaired point reaches the healer through the free tick's existing
issue rules: a move past `formation_shift_threshold` re-commits at once, and a
smaller one is picked up when the standing walk's directive runs out (within
`directive_ttl`, 1s).

Code: `src/states/play_match/class_ai/formation.rs`, applied at the end of both
`compute_formation_point`s (the Priest's, and the caster-healer machine the
Shaman and Druid share). The Paladin has no formation point. AS-328's caster
standoff is a FREE goal too and belongs behind the same constraint.

## The probe

`tests/formation_sight.rs`, `a_free_healer_regains_sight_of_its_teammate`
(default run, staged): AS-205's pillar edge with the Mage healthy (90%,
Hammer-of-Justice'd against the north face of the (-40, -20) pillar), played
once with a Druid and once with a Priest as the healer, each out of mana so no
cast holds it still. Seeds 1-3 each:

| | Healer regains sight of the Mage | Blind after that, in the 3.5s window |
|---|---|---|
| `TeamPlan` (this change), Druid and Priest | within 1s, every seed | never |
| `Legacy` (the pair: unconstrained point) | never | the whole window |

Mutation-checked: replacing the repair with the unrepaired point in the
Priest's `compute_formation_point` fails it (`Priest seed 1: regained sight of
the Mage at None`), as the same mutation does in the caster-healer one
(`Druid seed 1`); applying the constraint under `Legacy` fails its pair.

Reached from seeds, the defect is rare: the AS-206 repro seed no longer
contains it (the match now ends at 77s), and a 288-match FREE-healer bank on
`main` found no blind spell longer than 1.73s. The staged scene is the guard.

## Balance

**Directional paired sweep**, 2v2 PillaredArena, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches (input
and both CSVs: `sweeps/2026-10-17-as206-teamplan-pillaredarena*`):

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  43.7% -> 43.7%  +0.0pt  flips 97 (+44/-42) z=0.11 ns
CLEAN           n=2250  46.7% -> 47.7%  +1.0pt  flips 33 (+25/-3)  z=3.97 SIG
AGAINST         n=2250  44.6% -> 43.7%  -0.8pt  flips 29 (+5/-24)  z=3.34 SIG
MIRRORED        n=2250  39.7% -> 39.6%  -0.0pt  flips 35 (+14/-15) z=0.00 ns
NON-VACUITY: 5969/6830 ended by elimination; 554 moved in winner or duration
```

**One cell carries most of the one-sided flips: Warrior+Priest vs Mage+Rogue**,
which goes from 0/10 to 10/10 Warrior+Priest wins in both team orders — so it is
+10 of CLEAN's +25 and -10 of AGAINST's -24, one cell seen from both sides. It
is the Priest's formation point: the two binaries' traces first differ at a
team-1 Priest FREE `FormationShift` at 3.57s (seed 3), before contact, and the
match goes from a team-2 win at 32s to a team-1 win at 40s. Without that
cell the one-sided slices are CLEAN +15/-3 and AGAINST +5/-14 — still the side
holding a formation healer gaining, but each remaining cell is also counted in
both slices (Mage+Rogue vs Rogue+Shaman, 5 flips each way; Rogue+Druid, 4), so
the slices are not two independent readings. Read it as: a small gain for the
formation-healer side, concentrated in a handful of comps, led by one Priest
comp.

**Head-to-head**, n=100 per cell, both assignments (`scripts/headtohead_sweep.py`
on each binary). The change's effect is the `TeamPlan` cell's movement between
the two binaries; `Legacy`-only cells cannot move:

| Comp (team 1 vs team 2) | `TeamPlan` cells changed, main -> branch | Winner flips |
|---|---|---|
| Mage+Druid vs Hunter+Paladin (the AS-206 repro comp) | 0 / 200 | 0 |
| Mage+Druid vs Warrior+Paladin | 3 / 200 (durations only) | 0 |
| Warrior+Priest vs Warlock+Priest | 1 / 200 (duration only) | 0 |
| Hunter+Shaman vs Rogue+Priest | 0 / 200 | 0 |

**`camp_sweep`** (Warrior+Priest vs Warlock+Priest, seeds 1-12): byte-identical
printout on both binaries — 15.1s / 4.1s denied pre / post contact, 14% heal
line occluded.

## Byte-identity

`Legacy` on Nagrand: 1,366 matches (the `--affects` cells, seeds 500-501), CSV
identical (1,344 decisive, 22 draws). BasicArena under `TeamPlan`: 1,366
matches, CSV identical (1,360 decisive). The repair returns its input when the
map has no obstacles and is never called under `Legacy`, so this is the
expected result, not only an observed one.
