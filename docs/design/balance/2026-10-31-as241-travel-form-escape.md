# AS-241: the Travel Form run keeps its room to run

**Tier: DIRECTIONAL.** Paired arms at identical seeds. Nothing here may be cited
as the Druid's standing.

## The change

- The Druid's `corner_penalty` is 8.0, above its `flee` (6.0), the Hunter's
  pair. `MovementConfig::validate()` refuses any weights block whose `flee` is
  on and whose `corner_penalty` does not exceed it.
- The chaser escape (`escape_tick` with `chasers_only`, the shifted Druid's run)
  sets two `ScorerInputs` fields no other scorer sets:
  - `avoid_dead_ends`: `MASK_DEAD_END` masks every heading whose 2yd lookahead
    or 6yd run (`DEAD_END_LOOKAHEAD`, one commit window) ends deeper in a dead
    end (`ArenaBounds::dead_end_depth`; Nagrand's two starting rooms). On the
    fallback ladder the mask is lifted after the anchor's and before the
    obstacle's.
  - `flee_reach` (`SHIFT_FLEE_REACH`, 8yd): `flee` scores a heading by the
    distance from the chaser it gains over the open ground it has up to the
    reach (`open_run`), as a fraction of the reach. In the open this orders
    headings as the plain `flee * dot` does. With a wall or a pillar at the
    Druid's back, a heading along the obstacle beats one into it.
- With neither field set the scorer is unchanged: the 80 control matches of
  every directional block below are identical in winner and duration.

## Nagrand Druid panel (mechanism)

The AS-164 Nagrand Druid panel (3,000 configs, seeds 0-9), played under
`Legacy` and again under `TeamPlan` (`sweeps/2026-10-31-as241-panel.py`)
through `tests/milestone_mechanisms.rs`, which now records each shift's escape
window and the shifted time in a starting room. An escape window is the first
1.5s of a shift with a melee or pet enemy within 12yd. Analysis:
`sweeps/2026-10-31-as241-escape.py`; rows (slimmed to the Druid):
`sweeps/2026-10-31-as241-{before,after}.rows.jsonl.gz`.

| per 3,000 matches | `Legacy` before | after | `TeamPlan` before | after |
|---|---|---|---|---|
| matches with a shifted starting-room entry | 694 (23.1%) | 63 (2.1%) | 649 (21.6%) | 59 (2.0%) |
| shifted seconds in a room, per match | 0.55 | 0.04 | 0.31 | 0.04 |
| Druid deaths inside a room | 91 | 33 | 62 | 32 |
| Druid deaths in Travel Form | 448 of 1,524 (29.4%) | 358 of 1,512 (23.7%) | 394 of 1,532 (25.7%) | 380 of 1,492 (25.5%) |
| escape windows gaining no ground | 110 of 1,828 (6.0%) | 53 of 1,962 (2.7%) | 62 of 2,111 (2.9%) | 90 of 2,118 (4.2%) |
| ...of them within 6yd of a wall / a pillar | 16 / 38 | 3 / 9 | 24 / 5 | 28 / 15 |
| ...with the chaser within 6yd at the shift | 36 of 1,435 (2.5%) | 11 of 1,552 (0.7%) | 20 of 1,751 (1.1%) | 6 of 1,733 (0.3%) |
| mean ground gained per window | 2.35yd | 2.51yd | 2.85yd | 2.76yd |
| Druid side's wins (McNemar z) | 55.2% | 55.2% (0.14) | 52.5% | 53.7% (2.59) |

Read with care:

- **The windows cluster.** Many panel matches replay one deterministic
  situation across all ten seeds, so the no-gain counts are a few situations
  each: the 36 close-chaser misses before are 23 distinct (comp, time)
  situations under `Legacy`, the 11 after are 8. The McNemar z is inflated the
  same way and is not a win-rate claim; the directional sweep below is.
- **The `TeamPlan` rise in no-gain windows is the far ones.** Windows whose
  chaser was more than 6yd off at the shift went from 42 to 84; the chaser is
  still closing there, and the window says little about the run. With the
  chaser within 6yd, misses fall in both profiles.
- **The corner penalty alone does little here.** The same panel with only the
  RON change (`sweeps/2026-10-31-as241-corner-only.rows.jsonl.gz`): room entry
  in 18.6% (`Legacy`) and 20.4% (`TeamPlan`) of matches. The penalty's
  per-heading difference over a 2yd lookahead is a fraction of a point against
  `flee`'s six, which is why the mask and the reach exist.

## Directional sweep (win rate)

`scripts/gen_sweep.py --full 2 --exclude-double-healer --affects Druid
--control-cells 8 --n 10`, BasicArena, three seed blocks (`--seed-base` 0,
1000, 2000; 2,830 matches each), per profile; `scripts/paired_sweep.py
--affects Druid --tier directional --expect nothing`. Control identical in
every block. CLEAN is the Druid on team 1 only; AGAINST on team 2 only (team 1's
win rate, so a positive delta there is the Druid losing).

| | CLEAN by block | pooled flips, z | AGAINST by block | pooled flips, z |
|---|---|---|---|---|
| `Legacy` | -1.0 / -1.6 / -0.2pt | +168/-202, -1.77 | +1.0 / +1.0 / +0.2pt | +201/-172, +1.50 |
| `TeamPlan` | +1.0 / +1.4 / +1.8pt | +168/-114, +3.22 | +0.2 / -0.3 / 0.0pt | +141/-143, -0.12 |

Under `Legacy` the Druid side gives up about 0.8pt on BasicArena (both slices
lean that way, neither alone significant). Under `TeamPlan` it gains about
1.4pt where only its side fields a Druid and nothing measurable against one. The `Legacy` cost is the corner penalty's: it is
also read by the Druid's unshifted postures. The RON change alone, on the same
`Legacy` blocks, moves CLEAN -1.2 / -2.6 / -1.2pt (+72/-135, z=-4.38) and
AGAINST +0.4 / +2.2 / -0.1pt (+121/-90, z=+2.14); the mask and the reach on top
of it move CLEAN back +0.2 / +1.0 / +1.0pt.

**Measured alternative, not shipped:** leaving `corner_penalty` at 6.0 and
lowering `flee` to 5.0 also satisfies the rule and leaves the unshifted
postures alone. On BasicArena it measures CLEAN +1.0pt (`Legacy`, z=+2.22) and
+1.7pt (`TeamPlan`, z=+4.06); on the Nagrand panel room entry falls to 3.7% /
2.1% and close-chaser misses to 0.9% / 0.1%, with less ground gained per
window under `Legacy` (2.42yd). It fails the tightened outrun probe on one
shift (`a_shift_frees_the_druid_and_it_outruns_the_chaser`, BasicArena seed 2:
a Spider's web broken, the run held toward its anchor by the heal-range mask).
Rows: `sweeps/2026-10-31-as241-flee5.rows.jsonl.gz`.

## What is not settled

- A shifted Druid's run stays inside heal range of its anchor (the ESCAPE
  window's anchor mask), which can bend it back toward the chaser — what held
  the run the flee-5 arm lost in the outrun probe. It is not counted across the
  panel, and it sits with AS-242's emergency leave rather than here.
- Whether Travel Form deaths cost games remains a correlation (AS-164 §4).
