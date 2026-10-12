# AS-283: tempo hysteresis — flip count and head-to-head

Tier: **directional**. Success condition: **no regression** (`--expect nothing`).
The point of the change is stability of the `tempo` situation, not win rate.

## The change

`Situations::tempo` latches between two `movement.ron` `shared:` thresholds: a
team starts pressing (trailing) at a lead (deficit) of `press_advantage_margin`
(0.2, unchanged) and keeps that tempo until it falls below
`press_release_margin` (0.1, new). The band applies only to a team under
`AiProfile::TeamPlan` (`SituationConfig::under`); a `Legacy` team releases at the
onset, which is the unlatched test it always ran.

## Arms

Both arms are the PR-head binary (rebased on `b07bcd7`), each beside its own
`assets/`:

- **band** — the shipped `movement.ron` (`press_release_margin: 0.1`).
- **noband** — `sweeps/2026-10-12-as283-noband-arm.patch` applied
  (`press_release_margin: 0.2`, release at the onset: no band).

The noband arm *is* main's `TeamPlan`: on the 80-match flip bank below, its match
logs and decision traces are byte-identical to `b07bcd7`'s binary (160 files, 0
differ).

## Flip count

Seed bank: 8 two-v-two comps (Warrior or a healer on each side) x PillaredArena and
BasicArena x seeds 1-5, uniform `TeamPlan`, 80 matches.
`sweeps/2026-10-12-as283-tempo-flips.py` counts the `situation_change` events
named `tempo`, both teams together.

| arm | PillaredArena | BasicArena | all (mean / median / max) |
|---|---|---|---|
| noband (= main) | 11.75 | 20.55 | 16.15 / 14 / 66 |
| band | 8.35 | 14.35 | 11.35 / 10 / 38 |

Per match, flips fall 30% and the worst case halves. Per-match rows:
`sweeps/2026-10-12-as283-tempo-flips-{noband,band}.csv`.

The flips that remain cross the whole band in one step. A heal, a big hit or a
death moves a 2v2 lead by more than 0.1 at once, so a wider band buys little.
On the pre-rebase base (`569630f`, the same `TeamPlan` behaviour), the same bank
gave 10.20 at a release of 0.05 and 9.48 at 0.0, against 11.35 at 0.1. The
shipped 0.1 keeps "pressing" meaning a real lead.

## Head-to-head

`sweeps/2026-10-12-as283-h2h.jsonl.gz`: `gen_sweep.py --full 2
--exclude-double-healer --affects Warrior,Priest,Paladin,Shaman,Druid
--control-cells 8 --n 2`. That is 864 reachable cells and 8 controls. It is run
on PillaredArena and BasicArena, each in both assignments: team 1 `TeamPlan`
against team 2 `Legacy`, then the reverse. 6,976 matches per arm. Both arms ran
the same JSONL. CSVs: `sweeps/2026-10-12-as283-h2h-{noband,band}.csv.gz`.

```
paired_sweep.py noband.csv band.csv --affects Warrior,Priest,Paladin,Shaman,Druid --tier directional --expect nothing

CONTROL: 64/64 matches with no affected class on either side are IDENTICAL in winner and duration.
  ALL reachable      n=6912  before 49.9%  after 49.9%  delta +0.0pt  flips 4 (+2/-2) z=0.00 ns  resolves >=0.1pt
  CLEAN              n=1152  before 54.6%  after 54.6%  delta +0.0pt  flips 0
  AGAINST            n=1152  before 45.1%  after 45.1%  delta +0.0pt  flips 0
  MIRRORED           n=4608  before 49.9%  after 49.9%  delta +0.0pt  flips 4 (+2/-2)
NON-VACUITY: 6926/6976 decided by elimination (50 draws); 18 moved in winner or duration.
VERDICT: consistent with no movement.
```

**No regression.** The band moved 18 of 6,976 matches at all, and changed the
winner in 4, two each way. The printed floor of 0.1pt is the tool's flip-rate
formula, applied to a change that almost never flips. Read it as: the change has
no outcome effect this run could see, not that it was measured to ±0.1pt.

The effect is that small because only two decisions read tempo. A healer zeroes
`cover_pull` only while it is PRESSURED on an obstacle map, and a Warrior's tempo
reset arms only under movement CC. The latch changes what tempo says far more
often than either reader acts on it. The plays from AS-274 onward read tempo on
every decision, and they are what the band is for.

## Legacy byte-identity

`b07bcd7` against the PR head, both under `Legacy`:

- **Batch.** `gen_sweep.py --full 2 --exclude-double-healer --n 1 --seed-base
  1000`, on PillaredArena and BasicArena, 1,800 matches. The CSVs are
  byte-identical. 1,788 matches ended in a kill and 12 at the cap; 22 were draws;
  there were 1,165 distinct durations.
- **Flip bank under `Legacy`.** All 80 match logs and traces are byte-identical.
  The bank recorded 1,668 tempo transitions, which a leaked band would have cut,
  and 199 deaths.

## What recomputes from the committed data

| claim | from |
|---|---|
| head-to-head verdict, control, flips | the two `h2h` CSVs and `paired_sweep.py` |
| flip-count table | the two `tempo-flips` CSVs |
| noband arm = main `TeamPlan`, Legacy byte-identity | rebuild: `b07bcd7` and the PR head, plus the arm patch |
