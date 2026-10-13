# AS-288: the `ally_in_danger` debounce

Tier: **directional**. Success condition: **no regression** (`--expect
nothing`). The point of the change is that the window means what it says, not
win rate.

## The change

`situation::ally_in_danger` is debounced by three `movement.ron` `shared:`
values:

- **`danger_edge_margin: 1.0`** (yards). A threat pressures a teammate
  FIRMLY when it is at least this far inside every boundary of the pressure
  test it passes: `danger_radius`, `threat_intent_radius`, and, for a closing
  threat, its own preferred range (`situation::firmly_pressured`). Otherwise
  it pressures only from the EDGE of the test.
- **`danger_dwell: 0.1`** (seconds), for edge pressure only. A teammate
  pressured firmly opens its window on the first frame it is dying. A teammate
  pressured only from the edge must stay dying and pressured this long first.
- **`danger_release: 0.1`** (seconds). An open window ends only once its
  teammate has been out of danger this long. If the teammate dies, it ends at
  once. Another teammate whose own window could open takes over at once, its
  dwell counted from when its own danger began.

The pressure test itself is unchanged, and so are the per-teammate facts
under it (`allies[].dying`, `allies[].pressured`), which the healers' posture
and dispel rules read. Whether a ranged unit that is still closing should count
as pressure at all is AS-292's question. Here such a unit simply counts as
edge pressure: a caster closing through the 2yd band between its 28yd
preferred range and the 30yd intent radius is never a full margin inside both
boundaries.

**It is not gated.** Nothing outside the `TeamPlan`-gated Peel play reads the
window. `play_for` is the only reader of `ally_in_danger`, and it yields `Peel`
only when `peel_play` is set, which `SituationConfig::under` sets for `TeamPlan`
alone. `danger_attackers` is read only by `class_ai::peel`. A `Legacy` team's
decisions are therefore the same, and its traces now record the debounced
window (§ Legacy byte-identity).

## Measure first: what the short windows were

Seed bank (`tests/ally_in_danger_windows.rs`): the Peel probe's six comps plus
the replay the flicker was found in (Warrior+Druid v Hunter+Shaman), on
PillaredArena and BasicArena, under `Legacy` and `TeamPlan`, seeds 1–8. That is
224 matches. The causes come from a throwaway instrument
(`sweeps/2026-10-12-as288-causes-arm.patch`, applied to main) that logs, at
every window end, why the classifier's test stopped holding. They are reduced
by `sweeps/2026-10-12-as288-causes.py`.

**146 of main's 966 windows (15%) lasted one or two frames; 132 lasted one.**
What ended them:

| cause | windows | detail |
|---|---|---|
| the attacker stepped past `threat_intent_radius` | 72 (49%) | an enemy Priest (54) or Shaman (18) at 30.0–30.2yd |
| the attacker stopped counting as closing | 50 (34%) | a Shaman or Priest stepping out past `danger_radius` to 12.0–12.1yd (40), or arriving at its 28yd preferred range (4); a Hunter arriving at its 25yd (6) |
| the teammate was healed over the threshold | 13 (9%) | to 0.50–0.74 of its health, by a heal landing the frame after it dropped |
| the attacker changed target | 10 (7%) | |
| the teammate died | 1 | |

122 of them opened when an enemy entered the pressure test, and 24 on an HP
drop. Every pressure blip was an enemy at a boundary of the test: within 0.2yd
of a radius or of its own preferred range. The attacker was usually an enemy
healer targeting the dying teammate, passing the edge for one frame. The
immunity and held-attacker filters the card names are not in the classifier:
those live in the Peel step.

They were **isolated blips, not an oscillation**. 96 of the 146 teammates had no
new window for over a second, and 41 never had another. Only 6 re-opened on the
next frame. A release delay alone would only have stretched each blip to its
own length. What removes them is a dwell on pressure gained at a boundary. That
dwell must not reach an enemy well inside the test: a Warrior 1.5yd from a
dying Priest is danger on the first frame. The release keeps a real window in
one piece through a one-frame heal or step.

## Windows before and after

Same bank, `36c4702` → branch. Per-window rows:
`sweeps/2026-10-12-as288-windows-{main,branch}.csv`.

| profile | map | windows per match | 2 frames or fewer | median length |
|---|---|---|---|---|
| Legacy | PillaredArena | 4.88 → 3.00 | 78 (28.6%) → 0 | 26 → 189 frames |
| Legacy | BasicArena | 4.16 → 3.77 | 18 (7.7%) → 0 | 204 → 220 |
| TeamPlan | PillaredArena | 4.32 → 4.20 | 20 (8.3%) → 1 (0.4%) | 205 → 212 |
| TeamPlan | BasicArena | 3.89 → 3.41 | 30 (13.8%) → 0 | 171 → 233 |
| all | | 4.31 → 3.59 | 146 (15.1%) → 1 (0.1%) | |

The one short window left ended because its teammate died the next frame. The probe
asserts the release's promise on every window: none ends inside the release
unless its teammate died or another window took its place.

## The response cost, timed from when the danger began

A `Legacy` team's matches play out frame for frame the same on both arms, and
on main a window opens on the first frame of the danger. So pairing each of
the branch's `Legacy` windows with main's onset for the same teammate measures
the delay directly (`sweeps/2026-10-12-as288-response.py`):

| opened after the danger began | windows |
|---|---|
| 0 frames | 353 (93.1%) |
| 6 frames (0.100s) | 3 (0.8%) |
| 7 frames (0.117s) | 23 (6.1%) |

**93% of windows open on the first frame. The 7% opened on edge pressure alone
wait one dwell, 0.1s.**

In the staged scene `tests/peel_staged.rs` pins (a Warrior 1.5yd from a dying
Priest), the window opens and the Mage casts Frost Nova on the stage frame, as
on main, on both seeds. That test times its one-GCD deadline from the stage.

## The Peel probe

`peel_play_probe`, main → branch. Its assertions are relative (play against
withheld, in units of the bank's own noise) and its floor is a thinness guard,
so nothing in it moved.

| arm | windows | answered within one GCD | answered z | survival z |
|---|---|---|---|---|
| Peel, main | 745 | 76 (10.2%) | 7.22 | 1.13 |
| withheld, main | 775 | 12 (1.5%) | | |
| Peel, branch | 728 | 75 (10.3%) | 7.10 | 0.87 |
| withheld, branch | 749 | 12 (1.6%) | | |

## Head-to-head

`sweeps/2026-10-12-as288-gen.py 2`: every 2v2 team (35, no double healers)
against every other, 1,225 cells, on both maps, in three assignments:

- **TL**: team 1 `TeamPlan`, team 2 `Legacy`;
- **LT**: the reverse;
- **LL**: both `Legacy`, one seed per cell. This is the control.

That makes 12,250 matches per arm. Both arms ran the same JSONL on fresh release
builds of `36c4702` and the branch. CSVs:
`sweeps/2026-10-12-as288-h2h-{main,branch}.csv.gz`.

**Method:** scored from the `TeamPlan` side, as AS-283 did. The debounce acts
through the Peel play, which only the `TeamPlan` team runs: team 1 in `TL`,
team 2 in `LT`. A pooled team-1 rate would net that out.
`sweeps/2026-10-12-as288-oriented.py` scores each paired match from that side
(win > draw > loss) and tests the discordant pairs with an exact sign test:

```
CONTROL (Legacy v Legacy): 2450/2450 rows identical in winner, end reason and duration (2435 ended in a kill, 1451 distinct durations)
head-to-head paired matches: 9800; moved in winner, end reason or duration: 127
TeamPlan-side result, main -> branch:
  B_LT  n= 2450  TeamPlan wins 1211 -> 1212 (+0.04pt)  discordant 9 (+5 / -4)  exact p=1.000
  B_TL  n= 2450  TeamPlan wins 1206 -> 1212 (+0.24pt)  discordant 10 (+8 / -2)  exact p=0.109
  P_LT  n= 2450  TeamPlan wins 1243 -> 1242 (-0.04pt)  discordant 9 (+4 / -5)  exact p=1.000
  P_TL  n= 2450  TeamPlan wins 1241 -> 1239 (-0.08pt)  discordant 10 (+4 / -6)  exact p=0.754
  ALL   n= 9800  TeamPlan wins 4901 -> 4905 (+0.04pt)  discordant 38 (+21 / -17)  exact p=0.627  corrected z=+0.49
```

**No regression.** The debounce moved 127 of 9,800 head-to-head matches. It
changed the `TeamPlan` side's result in 38, split 21 for and 17 against
(p=0.63). **Resolution:** with 38 discordant pairs, a split reaches p<0.05
only at a net of about 13, which is 0.13pt of 9,800. So this run rules out an
effect larger than that in either direction and says nothing below it.

## Legacy byte-identity

Both against a fresh build of `36c4702`:

- **The sweep's control.** All 2,450 `LL` rows are identical in winner, end
  reason and duration, on both maps. 2,435 ended in a kill, and there are 1,451
  distinct durations.
- **The window bank under `Legacy`** (112 matches). All 112 match logs are
  byte-identical, across 301 deaths. The traces differ only in
  `ally_in_danger`: with its `situation_change` events and its
  `situation.ally_in_danger` tags removed, all 112 are identical. Main's
  traces carried 898 `ally_in_danger` events.

## What recomputes from the committed data

| claim | from |
|---|---|
| short-window causes | `causes-main.txt.gz` and `causes.py`; regenerate with `causes-arm.patch` on main |
| window table, response cost | the two `windows-*.csv` with `response.py`; rerun with `ALLY_IN_DANGER_CSV=… cargo test --release --test ally_in_danger_windows -- --ignored` |
| head-to-head, control | the two `h2h-*.csv.gz` and `oriented.py` |
| Peel probe table, staged-scene timing | a probe run on each arm (minutes) |
| Legacy logs and traces | a rebuild of both arms (not committed) |
