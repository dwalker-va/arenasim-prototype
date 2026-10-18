# AS-360: pursuit walks for sight

**Tier: DIRECTIONAL.** Which way, and roughly how much. Not a class's standing.

## The change

Normal pursuit (`combat_core::movement::move_to_target`) stopped at a unit's
preferred range whether or not it could see its target. On PillaredArena a
Warlock 23yd from a lone, out-of-mana Shaman with a pillar between them stood
still, and every cast it chose was refused `LosBlocked`. The Shaman had nothing
to cast and no reason to move, and the match drew at the 300s cap.

Now a non-healer in range and out of sight keeps walking. It takes the same
tangent-steered pursuit step it already took, and it stops on the frame sight
returns (`pursuit_closes`). Healers are exempt: their posture machine decides
where they stand. The rule applies under both AI profiles, and it is a no-op on
an obstacle-free map.

## Measurement

Inputs, CSVs and scripts are listed in `sweeps/README.md` (`2026-10-17-as360-*`).
Arms are `main` @ `0a8f8a36` and the branch, run with `--jobs 4-5 --trace-mode off`.

**BasicArena, Legacy:** 1,800 2v2 matches (`--full 2 --exclude-double-healer
--n 2`). The two CSVs are byte-identical, across 1,780 decisive matches and
20 draws. This run is the control. The PillaredArena sweeps have no
unreachable cell, because the change reaches every non-healer, and every 2v2
team without one has two healers.

**PillaredArena, 2v2, 900 cells x 10 seeds = 9,000 matches per profile**
(`--full 2 --exclude-double-healer --n 10`). Draw counts below are matches
drawn, before -> after. "Side" is the side that fields the Warlock, scored by
`sweeps/2026-10-17-as360-warlock-side.py`.

| profile | matches moved (winner / duration only) | draws, all cells | draws, Warlock cells (4,160) | Warlock side, winners gained / lost (one-sided cells, 3,520) |
|---|---|---|---|---|
| Legacy | 187 (5 / 182) | 107 -> 104 | 57 -> 54 (1.37% -> 1.30%) | +4 / -0 |
| TeamPlan | 2,328 (474 / 1,854) | 977 -> 866 | 135 -> 24 (3.25% -> 0.58%) | +245 / -176 (McNemar z = 3.4) |

Under TeamPlan the Warlock's side wins about two points more often in both
slots: 41.7% -> 43.9% as team 1 and 42.3% -> 44.0% as team 2. The cells with no
Warlock keep their draws (842 -> 842). `paired_sweep.py` puts the whole run in
its one MIRRORED slice: +0.0pt under Legacy, with 5 flips, and +0.7pt under
TeamPlan (z = 3.05). The tool reads a significant mirrored slice as noise
unless a mechanism corroborates it. Here the draw counts and the attribution
below are that mechanism.

Legacy parks the Warlock in range and out of sight far less often than
TeamPlan, so Legacy barely moves.

## Attribution

`sweeps/2026-10-17-as360-attribute.py` replays every moved match on its own,
under `main` and under the branch with `sweeps/2026-10-17-as360-fire-probe.patch`
applied. The patch adds one stderr line on each frame where the new clause
decides. For each match the script checks two things. First, the clause fired
at or before the first log line on which the two runs differ. Second, the
probe's run reproduces the batch's after row.

- **Legacy:** all 187 moved matches are attributed. The clause fired for the
  Warlock in 182 of them and only for the Mage in the other 5.
- **TeamPlan:** 2,327 of the 2,328 moved matches are attributed. The last one
  first differs on a log line stamped in the frame where the clause first
  fired. The clause fired for the Warlock in 1,979 matches. It fired only for
  the Mage in 338, and only for the Hunter in 10, for one to a few dozen
  frames in each. Those are frames where the ENGAGE machine has not yet
  answered an occlusion, and the pursuit walks the unit then.
- The clause never fired for a Warrior, a Rogue or a pet in a moved match.
