# AS-206: the FREE formation point keeps sight of the teammates

**Tier:** DIRECTIONAL (paired sweep) plus a head-to-head per
`docs/design/team-level-positioning-ai.md` *How to measure a step*.
**Success condition, stated up front:** the defect goes (a FREE healer no
longer idles blind behind a pillar from a healthy teammate), the repaired goal
jumps about no more than `main`'s, and there is no balance movement worth the
name. Measured 2026-10-18, `main` at `261eedf2` against this branch.

## The shape

| Question | Answer | Why |
|---|---|---|
| Sight to whom? | Every living non-pet teammate when a nearby spot has it; otherwise the most injured (the anchor). | A FREE healer has no one dying, so every teammate is a heal target it should keep. In a 2v2 the two rules are the same rule. An exit that sees the anchor always beats one that does not; each teammate seen beyond the first counts `TEAMMATE_YARDS` (3yd) nearer. |
| Constraint or scored term? | A constraint with a nearest-feasible repair. | The formation point is one goal, not a choice among scored steps — there is nothing to add a term to. A point in no shadow comes back exactly as the unconstrained machine returns it — every point on an obstacle-free map. |
| Where does the repair look? | An obstacle's shadow from a teammate is the wedge between the tangent rays past its footprint disc, widened by `SIGHT_MARGIN` (2.5yd). A point in it has one exit per side: the point moved out until it stands the margin beyond that side's ray. Distances are from the raw point, not its standable projection. | Geometry-anchored, so the exits slide with the fight. 2.5yd exceeds the 1.5yd formation dead zone, so wherever the walk stops is in sight. The projection jumps between a pillar's faces; the raw point does not. |
| How far? | An exit up to `REPAIR_FULL` (10yd) from the raw point is taken in full, one up to `REPAIR_LIMIT` (16yd) part of the way, none farther. Only shadows of an obstacle near the point count: no farther from it than the teammate is, or within `REPAIR_FULL` of its footprint. | The defect is a point a few yards into a nearby pillar's shadow (exits 7-9yd away). A teammate close behind a distant pillar casts a shadow whose exits are tens of yards off and whose edges sweep across the point many times faster than the teammate moves (1.5yd a frame, measured); following them sent the point ~70yd and back. |
| Which exit, evaluation to evaluation? | The exit nearest the one last chosen (within `KEEP_RADIUS`, 3yd; remembered on `HealerPosture::formation_repair`, cleared on leaving FREE) counts `KEEP_MARGIN` (6yd) nearer. | Near a shadow's centre line its two exits are about equally near, and the exits of two teammates' shadows behind one pillar lie apart the same way; without the band a tenth of a yard's drift flips the point across the gap. With it the point changes exit once, about half the band past the line. |
| Where can the goal step? | Each exit is continuous in the raw point across every edge of its region: the side edges by construction, the shadow's near end and the near-obstacle limit by fading the move out over `EDGE_FADE` (3yd). So the repair starts and lets go where the point stands. A **change of exit** — the kept one lost the anchor, or another became better by the band — still moves the point the whole gap between the two exits at once. | The fades remove the steps a hard edge made (a Priest beside a pillar dropped from its exit to its projection, 9-10yd). The exit changes are what is left; AS-370 smooths them. |
| Profile | `TeamPlan` only. | The AI-profile rule. `Legacy` is unchanged. |

The goal reaches the healer through the free tick's existing issue rules: a
move past `formation_shift_threshold` (3yd) re-commits at once, and a smaller one
is picked up when the standing walk's directive runs out (within `directive_ttl`,
1s).

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

| | Healer regains sight of the Mage | After that, in the 3.5s window |
|---|---|---|
| `TeamPlan` (this change), Druid and Priest | within 1s, every seed | never blind, never sent to a blind point |
| `Legacy` (the pair: unconstrained point) | never | blind throughout |

The AS-206 repro seed itself no longer reaches the situation (the match ends at
77s); a 288-match FREE-healer bank on `main` found no blind spell longer than
1.73s, so the staged scene is the guard.

**No flip** — `the_repaired_point_never_flips_across_a_shadow` (default run,
seed bank): every formation healer in Warrior+Priest v Mage+Rogue,
Warrior+Mage+Priest v Rogue+Warlock+Druid and Warlock+Priest v Rogue+Druid on
Nagrand under `TeamPlan`, seeds 0-9, both team orders, the first 30s. On every
frame the trace shows the healer FREE on a formation decision, its point goal
never jumps over 25yd frame to frame and never jumps over 10yd and comes back
within a second.

**Leaving FREE** — `kept_exit_reset::leaving_free_forgets_the_kept_exit`: a
Priest and a Druid pressed out of FREE drop the kept exit; the control, left
FREE, keeps it.

**In-module** (`formation.rs`):
- walking a point up a shadow to its near end, and out to the near-obstacle
  limit, no exit steps and the last exits before each edge are the point itself;
- a point drifting from open ground to a shadow's centre line and back moves
  the goal under 0.2yd a step;
- drifting over the centre line keeps its side (under 0.5yd a step, against
  over 8yd settled with nothing kept);
- carried well past the line it changes exit once, about `KEEP_MARGIN / 2`
  past it;
- with a second teammate behind the pillar it keeps its exit;
- the band holds an exit 2yd off the one last chosen and not one 4yd off;
- a point back in sight forgets its exit;
- an exit between `REPAIR_FULL` and `REPAIR_LIMIT` is taken part of the way and
  one beyond the limit not at all;
- a shadow far from its obstacle does not count.

**Mutation-checked** — each mutant fails the named test(s):

| Mutation | Fails |
|---|---|
| The Priest's `compute_formation_point` skips the repair | staged probe (`Priest seed 1`) |
| The caster-healer one skips it | staged probe (`Druid seed 1`) |
| No hysteresis (band never applied) | six in-module tests |
| The band given to every candidate | six in-module tests |
| `KEEP_RADIUS` unbounded / `KEEP_RADIUS` 0.5 | `the_band_is_for_the_exit_last_chosen` |
| The kept exit not cleared on a point in no shadow | `a_point_in_sight_forgets_its_kept_exit` |
| Posture transitions keep the kept exit | `leaving_free_forgets_the_kept_exit` |
| No `REPAIR_LIMIT` | `an_exit_beyond_the_limit_is_not_taken`, `a_far_exit_is_taken_part_of_the_way` |
| No fade by distance | `a_far_exit_is_taken_part_of_the_way` |
| Shadows of distant obstacles count | `a_shadow_counts_only_near_its_obstacle`, `the_exits_fade_out_at_the_near_obstacle_limit` |
| No fade at the shadow's near end | `the_exits_fade_out_at_the_shadows_near_end` |
| No fade at the near-obstacle limit | `the_exits_fade_out_at_the_near_obstacle_limit` |

## Formation-goal jumps against `main`

`measure_formation_goal_jumps` (`#[ignore]`d, `FORMATION_JUMPS_SEEDS=50`), seeds
0-49, both team orders, 120s, every formation healer's point goal on FREE
formation frames, main → branch:

| Comp | > 5yd | > 10yd | > 25yd | returns (> 5yd, back within 1s) | 0.5s spans over 10yd |
|---|---|---|---|---|---|
| Warrior+Priest v Mage+Rogue | 100 → 1 | 0 → 0 | 0 → 0 | 0 → 0 | 100 → 8 |
| Warrior+Mage+Priest v Rogue+Warlock+Druid | 103 → 106 | 68 → 71 | 4 → 4 | 13 → 13 | 99 → 104 |
| Mage+Druid v Hunter+Paladin | 42 → 42 | 18 → 18 | 0 → 0 | 6 → 6 | 30 → 30 |
| Warlock+Priest v Rogue+Druid | 35 → 31 | 12 → 11 | 0 → 0 | 1 → 0 | 46 → 39 |
| Hunter+Shaman v Warrior+Paladin | 13 → 13 | 11 → 11 | 3 → 3 | 0 → 0 | 12 → 11 |

**The one cell worse than `main`** is Warrior+Mage+Priest v Rogue+Warlock+Druid:
three more jumps over 10yd per 100 matches (68 → 71), and three more over 5yd
(103 → 106). The branch makes four changes of exit there that step the goal
over 5yd, all the Priest's, each a change of exit made in one evaluation (traced
with a per-call log of the repair): three over 10yd, and one just under.

| Seed, order | Jump | Exit, before → after |
|---|---|---|
| 20, Rogue+Warlock+Druid first | 15.1yd at 53.88s | (48.78, -16.11) → (29.67, -21.64); the kept exit 12.7yd from the raw point, in the part-way zone |
| 36, Rogue+Warlock+Druid first | 11.6yd at 49.80s | (36.98, -29.03) → (47.17, -25.39) |
| 49, Warrior+Mage+Priest first | 16.2yd at 57.63s | (43.17, -28.52) → (35.22, -11.81) |
| 27, Warrior+Mage+Priest first | 9.8yd at 45.52s | (33.64, -31.84) → (32.18, -7.04); the raw point moved 0.05yd |

The other 68 jumps over 10yd occur on `main` too. AS-370 smooths changes of
exit.

The 2v2 camp comp's jumps on `main` are one per match: the Priest's projected
point stepping 8.6yd between two faces of the camp pillar at the camp release
(16.67s). The branch measures from the raw point there and has none.

## Balance

**Directional paired sweep**, 2v2 PillaredArena, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches (input
and both CSVs: `sweeps/2026-10-17-as206-teamplan-pillaredarena*`):

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  50.0% -> 49.8%  -0.2pt  flips 108 (+46/-57) z=0.99 ns
CLEAN           n=2250  47.9% -> 48.3%  +0.4pt  flips 37 (+22/-13)  z=1.35 ns
AGAINST         n=2250  52.5% -> 51.8%  -0.7pt  flips 40 (+12/-27)  z=2.24 SIG
MIRRORED        n=2250  49.6% -> 49.3%  -0.2pt  flips 31 (+12/-17)  z=0.74 ns
NON-VACUITY: 6775/6830 ended by elimination; 992 moved in winner or duration
SLICES TESTED: 3. Significant: AGAINST.
```

**One cell carries the significant slice: Warrior+Priest vs Mage+Rogue**, which
goes from 0/10 to 10/10 Warrior+Priest wins in both team orders — -10 of
AGAINST's -27 and +10 of CLEAN's +22, one cell seen from both sides. It is
the Priest's formation point, the comp whose Priest repairs most: the two
binaries first diverge at the Priest's repair as the camp releases (16.6s),
and there is no change of exit anywhere in the cell, so it is the continuous
repair, not a side switch. AGAINST's 39
winner flips fall in 11 cells; without that one the slice is +12/-17 (z ≈ 0.8),
spread over ten cells of at most five flips each:

| AGAINST cell (team 1 vs team 2) | flips (+/-) |
|---|---|
| Mage+Rogue vs Warrior+Priest | +0 / -10 |
| Mage+Rogue vs Rogue+Shaman | +3 / -2 |
| Warrior+Rogue vs Warrior+Shaman | +2 / -2 |
| Mage+Rogue vs Warrior+Druid | +3 / -1 |
| Mage+Paladin vs Priest+Warlock | +1 / -3 |
| Warrior+Rogue vs Warrior+Druid | +1 / -2 |
| Mage+Rogue vs Rogue+Druid | +1 / -2 |
| Mage+Rogue vs Rogue+Priest | +0 / -2 |
| Paladin+Hunter vs Rogue+Druid | +0 / -2 |
| Mage+Paladin vs Warlock+Shaman | +0 / -1 |
| Mage+Paladin vs Warlock+Druid | +1 / -0 |

By the formation healer: Priest +1/-15 (10 of them the camp cell), Druid +6/-7,
Shaman +5/-5.

**Head-to-head**, n=100 per cell, both assignments (`scripts/headtohead_sweep.py`
on each binary). The change's effect is the `TeamPlan` cells' movement between
the two binaries; the `Legacy`-only cell cannot move:

| Comp (team 1 vs team 2) | rows changed, main -> branch | Winner flips |
|---|---|---|
| Mage+Druid vs Hunter+Paladin (the AS-206 repro comp) | 0 / 300 | 0 |
| Mage+Druid vs Warrior+Paladin | 3 / 300 (durations only) | 0 |
| Warrior+Priest vs Warlock+Priest | 10 / 300 (durations only) | 0 |
| Hunter+Shaman vs Rogue+Priest | 0 / 300 | 0 |

**`camp_sweep`** (Warrior+Priest vs Warlock+Priest, seeds 1-12): three
`TeamPlan` seeds' durations or occlusion move by a little, same winners; 15.1s /
12.4s denied pre / post contact and 26% heal line occluded on both binaries.

## Byte-identity

`Legacy` on Nagrand: 1,366 matches (the `--affects` cells, seeds 500-501), CSV
identical (1,345 decisive, 21 draws). BasicArena under `TeamPlan`: 1,366
matches, CSV identical (1,360 decisive). The repair returns the unconstrained
point when the map has no obstacles and is never called under `Legacy`, so this
is the expected result, not only an observed one.
