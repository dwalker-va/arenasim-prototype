# AS-281: Hunter snares skip immune units, and trap throws need sight

Tier: **directional**. Success condition: **nothing moves on BasicArena except
through the immunity filter**, and a measurement of what trap sight costs on
the pillar maps (`--expect nothing`, read per map).

## The change

- **Immunity.** Concussive Shot and the Frost Trap peel pass over an immune unit
  (Divine Shield, Cyclone; `CombatContext::entity_is_immune`, the one immunity
  predicate). Each picker takes the nearest melee threat that is NOT immune:
  `concussive_target`, and the Frost Trap anchor via
  `dps_postures::nearest_melee_threat_where`. Concussive Shot's cast gate sets
  `check_target_immune`. `try_place_trap_at` also refuses a trap aimed at an
  immune victim, traced `TargetImmune`. The Peel step already filtered immune
  attackers and still does.
- **Sight.** `try_place_trap_at` holds any throw whose landing the Hunter cannot
  see, traced `LosBlocked`. The check runs after range, and the landing is never
  moved. Every placement path ends there: the dip, the Frost Trap at the feet,
  the Frost Trap peel, the off-target, fallback and pressure Freezing Traps, and
  the Peel step. The Peel step's own sight check is gone, because the placement
  now makes it.

## Arms

- **before**: `36c4702` (main).
- **after**: the fix on `36c4702`. The PR is rebased onto `7917458`, whose only
  change is the UI-script driver.

Both binaries carry an instrument that writes to stderr and nothing else
(`sweeps/2026-10-12-as281-instrument-{before,after}.patch`). Rebuilt from those
patches, each arm reproduces its committed CSV byte for byte on all four maps.
The after patch also holds the experiment switches used below. With
`AS281_LANE=0` and no `AS281_EXEMPT`, it is the shipped behaviour.

Batch: four `gen_sweep.py --full 2 --exclude-double-healer --affects Hunter
--control-cells 8 --n 10` runs, 4,240 matches each. The first three are
`--extra '{"map":"<map>"}'` for BasicArena, TwinPillars and PillaredArena, all
`Legacy`. The fourth is PillaredArena under `TeamPlan`, because the Peel play
only runs there. The inputs are `sweeps/2026-10-12-as281-<map>.jsonl.gz` and
the CSVs are `sweeps/2026-10-12-as281-{before,after}-<map>.csv.gz`.

## Result

`paired_sweep.py --affects Hunter --tier directional --expect nothing`, run once
per map. Every control is 80/80 identical. The Hunter's side is scored with
`sweeps/2026-10-12-as281-hunter-side.py`, over every match with a Hunter on
exactly one side (n=3,520 per map):

| map | Hunter side, before -> after | flips (+/-) | z |
|---|---|---|---|
| BasicArena | 44.1% -> 44.1% (+0.0pt) | +2/-1 | +0.6 |
| TwinPillars | 43.1% -> 38.6% (**-4.5pt**) | +150/-307 | **-7.3** |
| PillaredArena | 40.5% -> 40.5% (+0.0pt) | +8/-8 | 0.0 |
| PillaredArena, TeamPlan | 40.1% -> 39.9% (-0.2pt) | +21/-28 | -1.0 |

- **BasicArena and Nagrand: no measurable change.** BasicArena moved 43 matches
  and flipped 3. PillaredArena moved 84 and flipped 16, eight each way.
- **TwinPillars: the Hunter loses about 4.5pt.** The cause is sight, not
  immunity. This is a direction, not a standing.

## Where the TwinPillars cost comes from

The after binary was run on TwinPillars with sight switched back off per
placement path (`AS281_EXEMPT`). Each CSV is
`sweeps/2026-10-12-as281-variant-<name>-TwinPillars.csv.gz`.

| variant | Hunter side delta | flips |
|---|---|---|
| immunity only (sight off everywhere) | +0.0pt | +1/-0 |
| sight on, except the fallback lane Freezing Trap | -1.1pt | +26/-66 |
| sight on, except the Frost Traps | -4.5pt | +140/-300 |
| sight on, lane pulled back into sight (below) | -4.6pt | +135/-296 |

- **About three quarters of the cost is the fallback lane Freezing Trap.** That
  trap is thrown at the midpoint toward the enemy healer. Before the change,
  1,865 of those throws on TwinPillars landed where the Hunter could not see:
  behind the map's two central pillars, where the enemy team walks.
- **The rest is the pressure trap and the Frost Trap peel.**
- **Moving the lane back into sight does not recover it.** One alternative was
  tried: when cover hides the lane, land the trap at the farthest point the
  Hunter can see along the same line, one yard short of the pillar. It measured
  the same as holding the throw (-4.6pt). The value was in landing behind the
  pillar, where the enemy walks, not in throwing. So the shipped rule holds a
  hidden throw everywhere and never moves it, as it already does for range.

The orchestrator ruled that a ground-targeted placement needs sight of its
point (WoW's rule, and the Trap Launcher model,
`hunter-traps-trap-launcher-model`). This is what that ruling costs the Hunter
on TwinPillars. Exempting the lane throw is the one lever that recovers most of
it, and doing so would be a product call.

## What changed in the decisions

Counts come from `sweeps/2026-10-12-as281-event-counts.py`, and its output is
`sweeps/2026-10-12-as281-event-counts.csv`. Decisions are counted once per
cast or throw. Held throws are counted once per tick (60 per second), not once
per throw.

| per arm, 4,240 matches per map | before | after |
|---|---:|---:|
| **Concussive Shots fired at an immune unit** | 26 / 0 / 38 / 63 | 0 / 0 / 0 / 0 |
| **traps thrown at an immune victim** | 24 / 18 / 28 / 19 | 0 / 0 / 0 / 0 |
| **traps thrown where the Hunter could not see the landing** | 0 / 2,559 / 29 / 323 | 0 / 0 / 0 / 0 |
| Concussive Shots redirected to a melee that is not immune | — | 26 / 0 / 5 / 35 |
| ticks a trap was held: immune victim | — | 611 / 455 / 503 / 321 |
| ticks a trap was held: landing out of sight | — | 0 / 375,872 / 1,051 / 42,365 |

The four figures in each cell are for BasicArena, TwinPillars, PillaredArena and
PillaredArena under TeamPlan, in that order.

- **The wasted casts were real.** The Hunter fired Concussive Shot at an immune
  unit 127 times. Every BasicArena match where it did fields a Druid beside the
  Hunter, so the likely source is that Druid's Cyclone on a melee. The fallback
  Freezing Trap was thrown at an immune unit it predicted would spring it 52
  times, and the Frost Trap peel at an immune anchor 37 times.
- **Unseen throws** were almost all TwinPillars: 1,865 fallback lane Freezing
  Traps, 144 pressure traps and 550 Frost Trap peels.
- **The TeamPlan Peel step** held its Frost Trap behind cover for 17,137 ticks
  after the change. It held there before too, through its own sight check, which
  the instrument did not count.

## BasicArena attribution

`sweeps/2026-10-12-as281-attribute.py` replays each of the 43 moved BasicArena
matches on both instrumented binaries with the match log on. For each match it
finds the first log line where the two arms part, and the first decision either
arm made that the immunity filter changes. The table is
`sweeps/2026-10-12-as281-basicarena-attribution.csv`.

**All 43 are attributed to an immune target.** In every match, the first
divergent log line falls in the same frame as the first such decision. The two
clocks differ by the 10s countdown less one frame, 9.98 to 9.99s in all 43.

The first decision broke down as follows:

| first decision | matches |
|---|---:|
| Concussive Shot passed over an immune melee for another | 22 |
| a fallback Freezing Trap held because its predicted victim was immune | 12 |
| a trap thrown at an immune victim (before arm) | 5 |
| Concussive Shot fired at an immune unit (before arm) | 4 |

The sight gate cannot act on an obstacle-free map, and no BasicArena match
moved without an immune target behind it.
