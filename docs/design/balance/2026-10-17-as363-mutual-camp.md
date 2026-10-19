# AS-363: two campers never meet

**Tier: DIRECTIONAL.** Which way, and roughly how much. Not a class's standing.

## The defect

Under `TeamPlan`, a team with a melee (the Paladin counts) and a healer takes
the pillar nearest its gate at the opening and holds it until the teams come
within 15yd of each other (`team_plan::teams_in_contact`). That works against
a team that walks over. When both teams camp, neither walks over: each holds
its pillar, contact never comes, and the match draws at the 300s cap with no
damage dealt by anyone. In a traced Warrior+Shaman vs Rogue+Priest match
(seed 0), every unit ended at its camp spot with 0 damage dealt and 0 taken.

None of the candidate causes on the card applied. Nobody out-heals anything,
because no damage happens. The melee never sets out to re-acquire, and
dampening has nothing to dampen. AS-206 (#348) and AS-209 (#352) do not touch
the camp. AS-206's own after-sweep still has 600 of 600 melee+healer mirrors
drawn.

On current `main`, PillaredArena 2v2, 9,000 matches: 812 of the 866 draws
are in the 121 cells where both teams camp (1,210 matches). Those cells are
the 640 melee+healer mirrors, plus every cell where Mage+Paladin,
Warlock+Paladin or Hunter+Paladin meets a camping comp (11 camping comps, so
121 cells). The other 54 draws are spread thinly
over the remaining cells, and the change does not move them.

## The change

A camp holds only when the enemy would not camp too
(`team_plan::camp_holds`). When both teams would camp, both press, which is
how they played before the camp existed. Whether the enemy would camp is
asked of its own profile, so an enemy on `Legacy` never camps, and the
`TeamPlan` side of a head-to-head keeps its pillar.

## Measurement

Inputs, CSVs and the scoring script are listed in `sweeps/README.md`
(`2026-10-17-as363-*`).

| run | matches | draws before -> after | moved |
|---|---|---|---|
| `TeamPlan`, PillaredArena | 9,000 | 866 -> 73 | 1,199, all in mutual-camp cells |
| ...without a Warlock (the card's slice) | 4,840 | 842 -> 50 (17.4% -> 1.0%) | |
| ...mutual-camp cells only | 1,210 | 812 -> 19 | 1,199 |
| `Legacy`, PillaredArena | 9,000 | byte-identical | 0 |
| `TeamPlan`, BasicArena | 900 | byte-identical | 0 |
| `TeamPlan` vs `Legacy`, both assignments, mutual-camp cells | 2,420 | byte-identical | 0 |

The non-Warlock draw rate under `TeamPlan` is now `Legacy`'s: 50 of 4,840 in
both. In the mutual-camp cells the sides split evenly after the change, 595
team-1 wins and 596 team-2 wins.

The head-to-head does not move, as predicted: a `Legacy` team never camps, so
it was never part of a standoff. `TeamPlan` wins 1,201 of those 2,420 matches
(49.6%) in both arms. So this change does not alter any head-to-head
`TeamPlan` result. It removes draws that only a uniform-`TeamPlan` world
produces, and the 0.8 milestone sweep runs one of those.
