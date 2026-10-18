# AS-206: the FREE formation point keeps sight of the teammates

**Tier:** DIRECTIONAL (paired sweep) plus a head-to-head per
`docs/design/team-level-positioning-ai.md` *How to measure a step*.
**Success condition, stated up front:** the defect goes (a FREE healer no
longer idles blind behind a pillar from a healthy teammate), with no balance
movement worth the name. Measured 2026-10-18, `main` at `15309d38` against this
branch.

## The shape

| Question | Answer | Why |
|---|---|---|
| Sight to whom? | Every living non-pet teammate when a nearby spot has it; otherwise the most injured (the anchor). | A FREE healer has no one dying, so every teammate is a heal target it should keep. In a 2v2 the two rules are the same rule. When no spot sees all (two pillars, two shadows), the anchor is never given up for the others. |
| Constraint or scored term? | A constraint with a nearest-feasible repair. | The formation point is one goal, not a choice among scored steps — there is nothing to add a term to. The repair has `nearest_standable`'s shape: a point whose standable projection already sees comes back exactly as the unconstrained machine returns it, so every point in sight (all of them on an obstacle-free map) is untouched. |
| Where does the repair look? | The two tangent rays bounding the teammate's shadow past the obstacle's footprint disc; the raw point projected onto each, pushed `SIGHT_MARGIN` (2.5yd) clear. | Geometry-anchored candidates slide continuously with the fight. A lattice round the point moves with it and hops cells (`team_solve::candidates_for` documents the pathology). 2.5yd exceeds the 1.5yd formation dead zone, so wherever the walk stops is in sight. |
| Which side of the shadow? | The side the point was last repaired to, kept until the other is nearer by `SIDE_MARGIN` (6yd) or it loses sight; remembered on `HealerPosture::formation_side`. Distances are taken from the raw point, not its standable projection. | Near a shadow's centre line the two exits are about equally near, and a tenth of a yard's drift would otherwise flip the point ~20yd across the shadow. A raw point inside a pillar moves continuously, but its standable projection jumps between faces; measuring from the projection carried that jump into the side choice at Nagrand's camp release. |
| Profile | `TeamPlan` only. | The AI-profile rule. `Legacy` is unchanged. |

The repaired point reaches the healer through the free tick's existing issue
rules: a move past `formation_shift_threshold` re-commits at once, and a smaller
one is picked up when the standing walk's directive runs out (within
`directive_ttl`, 1s).

Code: `src/states/play_match/class_ai/formation.rs`, applied at the end of both
`compute_formation_point`s (the Priest's, and the caster-healer machine the
Shaman and Druid share). The Paladin has no formation point. AS-328's caster
standoff is a FREE goal too and belongs behind the same constraint.

## The probes

**Sight regained** — `tests/formation_sight.rs`,
`a_free_healer_regains_sight_of_its_teammate` (default run, staged): AS-205's
pillar edge with the Mage healthy (90%, Hammer-of-Justice'd against the north
face of the (-40, -20) pillar), played once with a Druid and once with a Priest
as the healer, each out of mana so no cast holds it still. Seeds 1-3 each:

| | Healer regains sight of the Mage | Blind after that, in the 3.5s window |
|---|---|---|
| `TeamPlan` (this change), Druid and Priest | within 1s, every seed | never |
| `Legacy` (the pair: unconstrained point) | never | the whole window |

**No flip across a shadow** —
`the_priests_repaired_point_never_flips_across_a_shadow` (default run, seed
bank): Warrior+Priest v Mage+Rogue on Nagrand under `TeamPlan`, seeds 0-9 in
both team orders, the first 25s (the walk out of the room and the camp release
at ~16.7s). It reads the Priest's point goal on every frame the trace shows it
FREE on a formation decision and finds no frame-to-frame jump over 10yd, across
3,340 frame pairs. In-module, `the_point_keeps_its_side_across_the_centre_line`
drifts a point over a shadow's centre line in 0.05yd steps (largest repair step
under 2yd; with no side remembered, the same drift jumps over 8yd), and
`the_point_changes_side_once_well_past_the_centre_line` shows the band lets go
once, about `SIDE_MARGIN / 2` past the line.

**Mutation-checked:**

| Mutation | Fails |
|---|---|
| The Priest's `compute_formation_point` skips the repair | the staged probe (`Priest seed 1: regained sight of the Mage at None`) |
| The caster-healer `compute_formation_point` skips the repair | the staged probe (`Druid seed 1`) |
| The repair runs under `Legacy` too | the staged probe's `Legacy` pair |
| No side hysteresis (`SIDE_MARGIN` never applied) | both side tests, and the seed bank (a 12.3yd jump at 16.63s in all 20 matches) |
| Distances from the standable projection, not the raw point | the seed bank (an 18.2yd jump at 16.67s in all 20 matches) |

Reached from seeds, the blind-healer defect itself is rare: the AS-206 repro
seed no longer contains it (the match ends at 77s), and a 288-match FREE-healer
bank on `main` found no blind spell longer than 1.73s. The staged scene is the
guard.

## Balance

**Directional paired sweep**, 2v2 PillaredArena, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches (input
and both CSVs: `sweeps/2026-10-17-as206-teamplan-pillaredarena*`):

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  44.4% -> 44.6%  +0.1pt  flips 100 (+54/-44) z=0.91 ns
CLEAN           n=2250  45.8% -> 46.5%  +0.8pt  flips 34 (+25/-8)   z=2.79 SIG
AGAINST         n=2250  46.0% -> 45.7%  -0.3pt  flips 32 (+12/-19)  z=1.08 ns
MIRRORED        n=2250  41.5% -> 41.5%  +0.0pt  flips 34 (+17/-17)  z=0.00 ns
NON-VACUITY: 6068/6830 ended by elimination; 794 moved in winner or duration
One significant slice out of 3 tested.
```

**One cell is the whole one-sided signal: Warrior+Priest vs Mage+Rogue**, which
goes from 0/10 to 10/10 Warrior+Priest wins in both team orders — +10 of CLEAN's
+25 and -10 of AGAINST's -19, one cell seen from both sides. It is the Priest's
formation point: the two binaries' traces first differ at a team-1 Priest FREE
`FormationShift` at 3.57s (seed 3), before contact, and that match goes from a
team-2 win at 32s to a team-1 win at 43s. Without that cell the one-sided slices
are CLEAN +15/-8 and AGAINST +12/-9: nothing beyond noise, and no other cell
moves more than 3 of its 10 seeds.

**Head-to-head**, n=100 per cell, both assignments (`scripts/headtohead_sweep.py`
on each binary). The change's effect is the `TeamPlan` cells' movement between
the two binaries; the `Legacy`-only cell cannot move:

| Comp (team 1 vs team 2) | `TeamPlan` cells changed, main -> branch | Winner flips |
|---|---|---|
| Mage+Druid vs Hunter+Paladin (the AS-206 repro comp) | 0 / 200 | 0 |
| Mage+Druid vs Warrior+Paladin | 2 / 200 (durations only) | 0 |
| Warrior+Priest vs Warlock+Priest | 4 / 200 (durations only) | 0 |
| Hunter+Shaman vs Rogue+Priest | 0 / 200 | 0 |

**`camp_sweep`** (Warrior+Priest vs Warlock+Priest, seeds 1-12): one `TeamPlan`
seed's duration moves (57.2s to 55.7s, seed 3, same winner); 15.1s / 9.4s
denied pre / post contact and 21% heal line occluded on both binaries.

## Byte-identity

`Legacy` on Nagrand: 1,366 matches (the `--affects` cells, seeds 500-501), CSV
identical (1,345 decisive, 21 draws). BasicArena under `TeamPlan`: 1,366
matches, CSV identical (1,360 decisive). The repair returns the unconstrained
point when the map has no obstacles and is never called under `Legacy`, so this
is the expected result, not only an observed one.
