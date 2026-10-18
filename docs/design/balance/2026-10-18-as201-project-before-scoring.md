# AS-201: the TeamPlan solve scores candidates where they would be stood on

**Tier: DIRECTIONAL** (five comps, the cells the change reaches, n=300 per
cell). Not a comp's standing. **Verdict: ship.** The change is win-neutral in
every head-to-head cell and both assignments, and it removes a solve that
could win `OccupyCover` with a spot no unit can stand on.

## The change

`team_solve::solve_unit` scored its candidates where they were generated. The
local ring and the steps toward an ally are offsets from the unit that take no
account of the pillar beside it, so near cover some of them lie inside a
footprint. A point inside a pillar sees no one and is seen by no one, so it
satisfies cover against every caster. AS-190 moved the winner out of the
pillar before issuing it, but the choice was still made at the in-pillar
point, and the spot the healer then walked to could be in the open
(`an_in_pillar_step_does_not_win_cover_its_landing_spot_lacks`: on `main` the
healer is sent to its own skin spot, in the caster's sight).

Now every candidate is moved onto standable ground
(`map_geometry::nearest_standable`) before it is scored, and a candidate no
projection frees is dropped. The issued answer is therefore always a scored
point. `Legacy` never runs the solve, and on an obstacle-free map the
projection is the identity.

## Measurement

Arms: `main` @ `6b584a28` and the branch, the same binary for every cell of an
arm, `--jobs 4 --trace-mode off`. Input: `sweeps/2026-10-18-as201-make-sweep.py
300 1`; analysis: `sweeps/2026-10-18-as201-paired.py` over
`sweeps/2026-10-18-as201-{before,after}.csv.gz`. PillaredArena, 300s cap,
seeds 1-300, four cells per comp: LL, TL, LT (head-to-head, both assignments)
and TT (uniform, for the AS-190 comparison).

**Controls.** All 1,500 LL rows are identical across the arms (winner and
duration), 0 draws among them. A further 300 uniform-TeamPlan BasicArena
matches (three comps, seeds 1-100) produce byte-identical CSVs. There were no
cap draws in the head-to-head cells of either arm, so the AS-363 mutual-camp
draws do not enter this table (one TT flagship match drew at the cap before
the change, none after).

**Head-to-head** (the side given TeamPlan, its wins against the all-Legacy
cell; flips are that side's wins changing between the arms, paired by seed):

| side given TeamPlan | vs | LL | TeamPlan, before | TeamPlan, after | flips +/- | McNemar z |
|---|---|---|---|---|---|---|
| Warrior+Priest | Warlock+Priest | 222 | 276 (+18pt) | 273 (+17pt) | +15 / -18 | -0.5 |
| Warlock+Priest | Warrior+Priest | 78 | 92 | 92 | 0 / 0 | 0 |
| Hunter+Priest | Rogue+Priest | 6 | 1 | 1 | 0 / 0 | 0 |
| Rogue+Priest | Hunter+Priest | 294 | 187 (-36pt) | 189 (-35pt) | +38 / -36 | +0.2 |
| Warrior+Mage+Priest | Rogue+Warlock+Paladin | 253 | 224 | 224 | 0 / 0 | 0 |
| Rogue+Warlock+Paladin | Warrior+Mage+Priest | 47 | 41 | 41 | 0 / 0 | 0 |
| Warrior+Paladin | Warlock+Priest | 216 | 296 (+27pt) | 296 (+27pt) | +4 / -4 | 0 |
| Warlock+Priest | Warrior+Paladin | 84 | 12 | 12 | 0 / 0 | 0 |
| Warrior+Druid | Warlock+Priest | 300 | 297 | 300 | +3 / -0 | +1.7 |
| Warlock+Priest | Warrior+Druid | 0 | 2 | 2 | 0 / 0 | 0 |

Pooled over the cells that moved: 60 flips gained, 58 lost. Six of the ten
cells moved no winner, and three of those moved no row at all: the change reaches a TeamPlan unit
only when one of its candidates lands inside a pillar and that candidate would
have won.

**Uniform TeamPlan (TT), team-1 wins:** Warrior+Priest 265 -> 266 (+32/-31),
Hunter+Priest 122 -> 132 (+57/-47, z=+1.0), Warrior+Mage+Priest 97 -> 96,
Warrior+Paladin 300 -> 300, Warrior+Druid 270 -> 279 (+13/-4, z=+2.2 — one of
fifteen tests, not claimed).

**AS-190's result does not reproduce.** Its engineer measured the flagship
comp under uniform TeamPlan at 99 -> 75 of 100 (24 flips, all one way). On
current `main` the same comp moves 265 -> 266 of 300 with 63 flips split
evenly. Which merge since then removed the effect is not established here:
AS-198's dying-teammate reach is the obvious candidate, but no bisect was run.

## Mechanism (camp_sweep, uniform, seeds 1-12)

`cargo test --release --test camp_sweep -- --ignored --nocapture` on each arm,
TeamPlan column:

| | before | after |
|---|---|---|
| team-1 wins | 11/12 | 11/12 |
| Warlock denied Priest, after contact | 9.4s | 12.4s |
| heal line occluded, after contact | 33% | 40% |
| heal to Warrior (positive health deltas) | 75 | 171 |
| Priest hard-CC'd | 7.0s | 6.7s |
| losses with zero heal | 1 | 1 |

The card asked whether the healer now hides where it cannot heal. It does hide
more after contact: cover is up by three seconds a match, and its line to the
Warrior is blocked a larger share of the time. It does not cost healing: heal
delivered rises on these seeds and the zero-heal losses do not grow. That fits
the reach constraint (C_DYING, which ranks reaching a dying teammate above
cover) taking the healer back into sight when the heal matters. These are
twelve seeds, which is enough for the per-frame occlusion figures and not for
the heal total.

## The band-hold (AS-209)

`dying_unreached` asks `violations` at the healer's own position, and the
change does not touch that. What it could have changed is whether the solve's
answer is "reached" by that question once the healer stands on it.
`a_reaching_answer_reaches_from_where_it_is_stood_on` sweeps a healer round a
pillar against four dying-teammate placements and holds that whenever a
standable candidate reaches, the answer does. A point inside a pillar has no
sightline, so it never reaches: scoring in place could only under-rate such a
candidate for this constraint, and the agreement held on `main` too.
