# AS-206: the FREE formation point keeps sight of the teammates

**Tier:** DIRECTIONAL (paired sweep) plus a head-to-head per
`docs/design/team-level-positioning-ai.md` *How to measure a step*.
**Success condition, stated up front:** the defect goes (a FREE healer no
longer idles blind behind a pillar from a healthy teammate), the repaired goal
does not jump about, and there is no balance movement worth the name. Measured
2026-10-18, `main` at `261eedf2` against this branch.

## The shape

| Question | Answer | Why |
|---|---|---|
| Sight to whom? | Every living non-pet teammate when a nearby spot has it; otherwise the most injured (the anchor). | A FREE healer has no one dying, so every teammate is a heal target it should keep. In a 2v2 the two rules are the same rule. An exit that sees the anchor always beats one that does not; each teammate seen beyond the first counts `TEAMMATE_YARDS` (3yd) nearer. |
| Constraint or scored term? | A constraint with a nearest-feasible repair. | The formation point is one goal, not a choice among scored steps — there is nothing to add a term to. A point in no shadow comes back exactly as the unconstrained machine returns it, so every such point (all of them on an obstacle-free map) is untouched. |
| Where does the repair look? | An obstacle's shadow from a teammate is the wedge between the tangent rays past its footprint disc, widened by `SIGHT_MARGIN` (2.5yd). A point in it has one exit per side: the point moved straight out until it stands the margin beyond that side's ray. Distances are from the raw point, not its standable projection. | Geometry-anchored, so the exits slide with the fight. A point on the widened edge is its own exit, so the repair starts and lets go without a step. 2.5yd exceeds the 1.5yd formation dead zone, so wherever the walk stops is in sight. The projection jumps between a pillar's faces; the raw point does not. |
| How far? | An exit up to `REPAIR_FULL` (10yd) from the raw point is taken in full, one up to `REPAIR_LIMIT` (16yd) part of the way, none farther. Only shadows of an obstacle near the point count: no farther from it than the teammate is, or within `REPAIR_FULL` of its footprint. | The defect is a point a few yards into a nearby pillar's shadow (exits 7-9yd away). A teammate close behind a distant pillar casts a shadow whose exits are tens of yards off and whose edges sweep across the point many times faster than the teammate moves (1.5yd a frame, measured); following them sent the point ~70yd and back. |
| Which exit, frame to frame? | The exit nearest where the point was last repaired to (within `KEEP_RADIUS`, 3yd), remembered on `HealerPosture::formation_repair`, counts `KEEP_MARGIN` (6yd) nearer. Cleared when the point is in no shadow and when the healer leaves FREE. | Near a shadow's centre line its two exits are about equally near, and the exits of two teammates' shadows behind one pillar lie apart the same way; without the band a tenth of a yard's drift flips the point across the gap. |
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

The bound and the near-obstacle rule leave this scene alone: its exit is well
inside `REPAIR_FULL`. The AS-206 repro seed itself no longer reaches the
situation (the match ends at 77s); a 288-match FREE-healer bank on `main` found
no blind spell longer than 1.73s, so the staged scene is the guard.

**No flip** — `the_repaired_point_never_flips_across_a_shadow` (default run,
seed bank): every formation healer in Warrior+Priest v Mage+Rogue and in
Warrior+Mage+Priest v Rogue+Warlock+Druid on Nagrand under `TeamPlan`, seeds
0-9, both team orders, the first 30s. On every frame the trace shows the healer
FREE on a formation decision, its point goal never jumps over 25yd frame to
frame and never jumps over 10yd and comes back within a second.

**Leaving FREE** — `kept_exit_reset::leaving_free_forgets_the_kept_exit`: a
Priest and a Druid pressed out of FREE drop the kept exit; the control, left
FREE, keeps it.

**In-module** (`formation.rs`): a point drifting from open ground to a shadow's
centre line and back steps under 0.5yd; drifting over the centre line keeps its
side (largest step under 2yd, against over 8yd with nothing kept) and changes
side once, about `KEEP_MARGIN / 2` past it; with a second teammate behind the
pillar it keeps its exit; the band holds only the exit within `KEEP_RADIUS` of
the kept spot; a point back in sight forgets its exit; an exit between
`REPAIR_FULL` and `REPAIR_LIMIT` is taken part of the way and one beyond not at
all; a shadow far from its obstacle does not count.

**Mutation-checked** — each mutant fails the named test(s):

| Mutation | Fails |
|---|---|
| The Priest's `compute_formation_point` skips the repair | staged probe (`Priest seed 1`) |
| The caster-healer one skips it | staged probe (`Druid seed 1`) |
| No hysteresis (band never applied) | six in-module tests, including the centre-line and two-teammate ones |
| The band given to every candidate | six in-module tests |
| `KEEP_RADIUS` unbounded | `the_band_is_for_the_exit_the_point_was_at` |
| The kept exit not cleared on a point in no shadow | `a_point_in_sight_forgets_its_kept_exit` |
| Posture transitions keep the kept exit | `leaving_free_forgets_the_kept_exit` |
| No `REPAIR_LIMIT` | `a_far_exit_is_taken_part_of_the_way_or_not_at_all` |
| No fade (full weight up to the limit) | `a_far_exit_is_taken_part_of_the_way_or_not_at_all` |
| Shadows of distant obstacles count | `a_shadow_counts_only_near_its_obstacle` |
| The exit pushed a full margin out from inside the widened edge (a step at onset) | `the_repair_starts_and_ends_where_the_point_stands` |

## Formation-goal jumps against `main`

`measure_formation_goal_jumps` (`#[ignore]`d), seeds 0-9 both orders, 120s, every
formation healer's goal on FREE formation frames:

| Comp | jumps > 5yd, main → branch | > 10yd | > 25yd | returns (> 5yd, back within 1s) |
|---|---|---|---|---|
| Warrior+Priest v Mage+Rogue | 20 → 0 | 0 → 0 | 0 → 0 | 0 → 0 |
| Warrior+Mage+Priest v Rogue+Warlock+Druid | 21 → 21 | 14 → 14 | 1 → 1 | 0 → 0 |
| Mage+Druid v Hunter+Paladin | 10 → 10 | 3 → 3 | 0 → 0 | 1 → 1 |
| Warlock+Priest v Rogue+Druid | 3 → 5 | 2 → 2 | 0 → 0 | 0 → 0 |
| Hunter+Shaman v Warrior+Paladin | 2 → 2 | 2 → 2 | 0 → 0 | 0 → 0 |

The jumps that remain on both binaries are the raw (unconstrained) formation
point's own: the engaged-ally centroid offset away from the nearest enemy, which
flips as enemies cross it. The 2v2 camp comp's 20 on `main` are one per match:
the Priest's projected point stepping 8.6yd between two faces of the camp pillar
at the camp release (16.67s). The branch measures from the raw point there and
has none.

## Balance

**Directional paired sweep**, 2v2 PillaredArena, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches (input
and both CSVs: `sweeps/2026-10-17-as206-teamplan-pillaredarena*`):

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  50.0% -> 50.0%  +0.0pt  flips 115 (+56/-55) z=0.00 ns
CLEAN           n=2250  47.9% -> 48.0%  +0.1pt  flips 33 (+17/-14)  z=0.36 ns
AGAINST         n=2250  52.5% -> 52.4%  -0.1pt  flips 30 (+14/-16)  z=0.18 ns
MIRRORED        n=2250  49.6% -> 49.6%  +0.0pt  flips 52 (+25/-25)  z=0.00 ns
NON-VACUITY: 6774/6830 ended by elimination; 1060 moved in winner or duration
SLICES TESTED: 3. Significant: none.
```

No cell moves more than 5 of its 10 seeds; Warrior+Priest v Mage+Rogue, whose
Priest repairs most, is unchanged (0/10 and 10/10 in its two orders).

**Head-to-head**, n=100 per cell, both assignments (`scripts/headtohead_sweep.py`
on each binary). The change's effect is the `TeamPlan` cells' movement between
the two binaries; the `Legacy`-only cell cannot move:

| Comp (team 1 vs team 2) | rows changed, main -> branch | Winner flips |
|---|---|---|
| Mage+Druid vs Hunter+Paladin (the AS-206 repro comp) | 0 / 300 | 0 |
| Mage+Druid vs Warrior+Paladin | 2 / 300 (durations only) | 0 |
| Warrior+Priest vs Warlock+Priest | 11 / 300 (durations only) | 0 |
| Hunter+Shaman vs Rogue+Priest | 0 / 300 | 0 |

**`camp_sweep`** (Warrior+Priest vs Warlock+Priest, seeds 1-12): two `TeamPlan`
seeds' durations move (57.4s to 55.7s and 97.4s to 98.7s, same winners); 15.1s /
12.4s denied pre / post contact on both binaries, heal line occluded 26%.

## Byte-identity

`Legacy` on Nagrand: 1,366 matches (the `--affects` cells, seeds 500-501), CSV
identical (1,345 decisive, 21 draws). BasicArena under `TeamPlan`: 1,366
matches, CSV identical (1,360 decisive). The repair returns the unconstrained
point when the map has no obstacles and is never called under `Legacy`, so this
is the expected result, not only an observed one.
