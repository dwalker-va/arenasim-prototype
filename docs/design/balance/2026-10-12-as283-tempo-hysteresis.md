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

**Method: oriented to the TeamPlan side, by hand.** The band acts only on the
team running `TeamPlan`, which is team 1 in half the rows (`_TL`) and team 2 in
the other half (`_LT`). A team-1 win rate pooled over both halves nets a
per-team change out — `paired_sweep.py`'s docstring says so, and its class
slices (`--affects`) answer a different question — so it is not the instrument
here. `sweeps/2026-10-12-as283-oriented.py` scores each paired match from the
TeamPlan side (win, draw, loss), counts the discordant pairs, and tests them
with an exact two-sided sign test (McNemar's exact form):

```
2026-10-12-as283-oriented.py h2h-noband.csv.gz h2h-band.csv.gz

CONTROL: 64/64 rows with no affected class identical in winner and duration
paired matches: 6976; moved in winner or duration: 18
TeamPlan-side result, noband -> band:
  B_LT  n= 1744  TeamPlan wins 838 -> 840  discordant 2 (band+ 2 / band- 0)  exact p=0.500
  B_TL  n= 1744  TeamPlan wins 853 -> 855  discordant 2 (band+ 2 / band- 0)  exact p=0.500
  P_LT  n= 1744  TeamPlan wins 874 -> 874  discordant 0
  P_TL  n= 1744  TeamPlan wins 873 -> 873  discordant 0
  ALL   n= 6976  TeamPlan wins 3438 -> 3442  discordant 4 (band+ 4 / band- 0)  exact p=0.125  corrected z=+1.50
```

**No regression.** The band moved 18 of 6,976 matches at all, and changed the
TeamPlan side's result in 4, all four in the banded arm's favour (+4/-0, exact
p=0.125, not significant), all on BasicArena. The control is 64/64 identical.

**Resolution floor.** With 4 discordant pairs, even the most lopsided split
(4-0) gives exact p=0.125 (continuity-corrected McNemar z=1.50), so no imbalance
among them could reach significance. This run shows no outcome effect it could
see; it cannot say whether the band is worth a few wins, in either direction.

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
| head-to-head verdict, control, oriented flips | the two `h2h` CSVs and `sweeps/2026-10-12-as283-oriented.py` |
| flip-count table | the two `tempo-flips` CSVs |
| noband arm = main `TeamPlan`, Legacy byte-identity | rebuild: `b07bcd7` and the PR head, plus the arm patch |
