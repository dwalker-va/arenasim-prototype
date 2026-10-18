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
| Re-issue | A healer standing blind on a blind point is re-sent however short the move (`formation::sight_shift`). Both the healer and its current point must be blind, so the walk is re-sent once per shadow sweep, not every frame. | Without it a repaired point under `formation_shift_threshold` (3yd) from the parked one never reaches the executor. |
| Profile | `TeamPlan` only. | The AI-profile rule. `Legacy` is unchanged. |

Code: `src/states/play_match/class_ai/formation.rs`, applied at the end of both
`compute_formation_point`s (the Priest's, and the caster-healer machine the
Shaman and Druid share). The Paladin has no formation point. AS-328's caster
standoff is a FREE goal too and belongs behind the same constraint.

## The probe

`tests/formation_sight.rs`, `a_free_druid_regains_sight_of_its_teammate`
(default run, staged): AS-205's pillar edge with the Mage healthy (90%,
Hammer-of-Justice'd against the north face of the (-40, -20) pillar) and the
Druid out of mana so no cast holds it still. Seeds 1-3:

| | Druid regains sight of the Mage | Blind in the 3.5s window |
|---|---|---|
| `TeamPlan` (this change) | by 0.25s, every seed | none after it |
| `Legacy` (the pair: unconstrained point) | never | the whole window |

Mutation-checked: disabling the repair fails it (`regained at None`); applying
the constraint under `Legacy` fails its pair (`blind only 0.18s`); an
`||` in `sight_shift` fails the unit tests.

The seed-bank probe (`#[ignore]`d, 288 matches per profile: Priest, Druid,
Shaman with Mage, Warrior, Hunter, against four pairs, seeds 1-8, Nagrand)
finds 17 FREE blind spells under `TeamPlan`, the longest 1.73s, every one
regained — and 18 on `main`, the longest the same. **Seed-reached, the defect
is rare**: the AS-206 repro seed no longer reaches it (the match now ends at
77s), and no bank match had a FREE healer blind for more than 1.73s before or
after. The staged probe is the guard; the bank bounds every spell at 3s.

## Balance

**Directional paired sweep**, 2v2 PillaredArena, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches:

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  43.7% -> 43.7%  +0.0pt  flips 93 (+42/-40) z=0.11 ns
CLEAN           n=2250  46.7% -> 47.6%  +0.9pt  flips 31 (+23/-3)  z=3.73 SIG
AGAINST         n=2250  44.6% -> 43.8%  -0.8pt  flips 27 (+5/-22)  z=3.08 SIG
MIRRORED        n=2250  39.7% -> 39.6%  -0.0pt  flips 35 (+14/-15) z=0.00 ns
NON-VACUITY: 5969/6830 ended by elimination; 570 moved in winner or duration
```

The side holding a formation healer gains about a point, and loses it again
when the other side holds one too: small, one-directional, consistent across
the two one-sided slices.

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

`Legacy` on Nagrand: 1,366 matches (`--affects` cells, seeds 500-501), CSV
identical (1,344 decisive, 22 draws); and the 288 `Legacy` bank matches'
decision traces identical. BasicArena under `TeamPlan`: 1,366 matches, CSV
identical (1,360 decisive). The repair returns its input when the map has no
obstacles and the re-issue rule is false there, so this is the expected
result, not only an observed one.
