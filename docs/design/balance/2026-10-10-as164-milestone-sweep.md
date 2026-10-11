# 0.7 milestone sweep: nine-class baseline and the Druid's standing

**Card:** AS-164. **Tier: AUTHORITY** (`sweep-tiers.md`): these are the canonical
baselines the 0.7 milestone is cited against.
**Measured commit:** `ea63f4f` (main after AS-230 and AS-234). **AS-240** (TBC
dispel classes) merged after it and is **not included**; its own directional run
on `ea63f4f` measured +0.1pt overall (ns), Hunter side −2.1pt (z=−3.4), Shaman
side −0.6pt (ns) — directional only.
**Harness:** `arenasim --batch`, 300s cap, default loadouts, `Legacy` AI,
BasicArena unless stated.

## Verdict

**The Druid is under-tuned in 2v2 and low-but-in-band in 3v3, against the other
three healers.** 2v2 competitive: Druid **45.1%**, against Shaman 51.4, Priest
52.3 and Paladin 63.1 — six points under the next healer, and the 2v2 field's
second-lowest class. 3v3 competitive: Druid **46.9%**, level with the Shaman
(47.0) and under the Priest (50.1) and Paladin (51.5).

It is not weak everywhere. Same-DPS, the Druid beats the Priest (61%) and loses
to the Shaman (42%) and the Paladin (23%); with a Rogue or Mage it is a winning
comp, with a Hunter or Warlock a losing one. Neither scoping risk makes it an
outlier in competitive play. **Uninterruptible heals:** its win rate does not
move with the number of interrupters it faces. **Dampened endgames:** in
competitive comps, Druid mirrors run the longest of any healer's but resolve
(2v2 draws 4.1% against the Shaman mirror's 6.4%); the Druid's late-game gap
shows only where nobody else on its side can deal damage — 1v1, where every
Druid v Druid and Druid v Shaman match hits the 300s cap.

**The other healer is the bigger outlier: the Paladin, at 63.1% 2v2
competitive**, eleven points clear of the next healer and the best class in the
bracket (Warrior+Paladin 77.5% is the #1 comp).

**Cleanest Druid lever (not tuned here): healing throughput per cast, not mana.**
The Druid is the healer with the most mana left through the first two minutes
(§4) and still loses its healer matchups; mana is not what binds it. The
data-only lever is the heal-over-time magnitude (Rejuvenation / Lifebloom tick,
in `abilities.ron`); the AI-side alternative is a less conservative mana
governor.

## What was run

| Run | Matches | Input | Committed result |
|---|---|---|---|
| 1v1 full 9×9, n=100 | 8,100 | `gen_sweep.py --full 1 --n 100` | `canonical_1v1_n100_300s.csv.gz` |
| 2v2 full, n=100 | 129,600 | `gen_sweep.py --full 2 --n 100` | `canonical_2v2_full_n100_300s.csv.gz` |
| 3v3 full, n=50 | 352,800 | `gen_sweep.py --full 3 --n 50` | `canonical_3v3_full_n50_300s.csv.gz` |
| TeamPlan head-to-head, PillaredArena, seeds 1–300 | 2,700 | `sweeps/2026-10-10-as164-h2h.jsonl` | `2026-10-10-as164-teamplan-headtohead-n300.csv` |
| TeamPlan bisect, 6 commits | 4,600 | `sweeps/2026-10-10-as164-bisect.jsonl`, `…-repro0806.jsonl` | `2026-10-10-as164-teamplan-bisect.csv`, `…-repro-1ade748.csv` |
| AS-234 paired (e4c01dc vs ea63f4f) | 2 × 14,150 | `sweeps/2026-10-10-as164-as234-paired.jsonl` | `2026-10-10-as164-as234-{before-e4c01dc,after-ea63f4f}.csv` |
| Mechanism panels (observed runs) | 19,100 | `sweeps/2026-10-10-as164-mech_*.jsonl` | `sweeps/2026-10-10-as164-mech_*.rows.jsonl.gz` |

Every figure in this doc recomputes from the committed files:

| Figures | Command (from `docs/design/balance/`) |
|---|---|
| §1 standings, non-vacuity | `sweeps/2026-10-10-as164-analyze.py standings canonical_{1v1…,2v2…,3v3…}.csv.gz` |
| §1 tier lists, canaries | `../../../scripts/comp_tiers.py canonical_{2v2,3v3}…csv.gz --size {2,3}` |
| §2 healer tables | `analyze.py healer canonical_2v2….csv.gz`, `analyze.py comps … Druid` |
| §3 interrupts | `analyze.py interrupts canonical_2v2….csv.gz`; lockouts: `mechanisms.py druid …mech_basic…` |
| §3 draws, mirrors, duels | `analyze.py draws …`, `analyze.py mirrors …`, `analyze.py duels canonical_1v1….csv.gz`, `mechanisms.py duels …mech_duels…` |
| §4 mana, Cyclone, Travel Form, roots | `mechanisms.py economy …mech_basic…`, `mechanisms.py druid …mech_basic… …mech_nagrand…` |
| §5 Hunter traps | `mechanisms.py hunter …mech_hunter….rows.jsonl.gz` (the Hunter panel alone) |
| §6 head-to-head and attribution | `analyze.py h2h 2026-10-10-as164-teamplan-headtohead-n300.csv 2026-08-06-team-solve-headtohead-n100.csv`, `analyze.py bisect 2026-10-10-as164-teamplan-bisect.csv`, `analyze.py h2h 2026-10-10-as164-teamplan-repro-1ade748.csv`, `mechanisms.py h2h_hunter …mech_h2h_hunter…` |
| §7 AS-234 | `sweeps/2026-10-10-as164-as234-slices.py 2026-10-10-as164-as234-{before…,after…}.csv` |

Rebuilding a binary is needed only to re-run a sweep, never to check a figure.
The bisect binaries are each commit plus the AS-235 headless-executor patch
(single-threaded `Fixed*` schedules), which changes speed and not output: the
`1ade748` rebuild reproduces the 2026-08-06 table's aggregates exactly.

The mechanism rows come from `tests/milestone_mechanisms.rs`, an `#[ignore]`d
observed-run reporter (per-frame mana, auras, deaths; per-match casts, trap
triggers and damage dealt). Its outcomes are the batch runner's: all 15,200 of
its BasicArena rows that share a config with the 2v2 matrix match it in winner
and duration.

**Non-vacuity.** 1v1: 725 draws (8.95%), 1,450 distinct durations. 2v2: 3,302
draws (2.55%), 11,473 distinct durations. 3v3: 2,770 draws (0.79%), 13,809
distinct durations. Per-class Wilson half-widths: 2.3pt (1v1), 0.57pt (2v2),
0.25pt (3v3) — over matches, which are clustered by cell, so read them as a
resolution floor rather than the interval on a class's standing.

## 1. Class standings

Win rate of every match a class's comp plays (draws are non-wins). Competitive:
2v2 at most one healer, 3v3 one or two healers, both sides (healers = Priest,
Paladin, Shaman, Druid).

| Class | 1v1 | 2v2 all | **2v2 competitive** | 3v3 all | **3v3 competitive** |
|---|---|---|---|---|---|
| Paladin | 52.6 | 49.5 | **63.1** | 51.0 | **51.5** |
| Mage | 70.9 | 61.1 | **58.1** | 57.0 | **56.4** |
| Priest | 38.4 | 44.7 | **52.3** | 49.4 | **50.1** |
| Shaman | 30.6 | 41.1 | **51.4** | 46.3 | **47.0** |
| Rogue | 75.9 | 55.6 | **49.5** | 52.7 | **54.7** |
| Warrior | 38.5 | 52.3 | **46.3** | 52.8 | **54.0** |
| Hunter | 61.8 | 50.7 | **45.2** | 45.1 | **43.2** |
| **Druid** | 8.2 | 37.0 | **45.1** | 45.3 | **46.9** |
| Warlock | 32.8 | 46.6 | **40.0** | 46.8 | **44.7** |

1v1 is diagnostic only. The 2026-07-09 eight-class baselines are not a "before"
for these figures: the roster changed and so did three months of mechanics, and
`balance-methodology` records how individual cells drift between cycles.

**Tier lists** (`comp_tiers.py`): 2v2 top — Warrior+Paladin 77.5, Mage+Shaman
72.7, Rogue+Paladin 70.5, Mage+Priest 70.5, Mage+Paladin 69.1. 3v3 top —
Mage+Paladin+Warlock 76.6, Mage+Shaman+Warlock 74.7, Mage+Priest+Warlock 73.7.

**Canaries.** 2v2: no anomaly fires (the best non-competitive comp,
Paladin+Shaman, is 30.3% against the competitive field). **3v3: one fires —
Mage+Warlock+Warrior (no healer) at 58.7% full-field, 59.0% against competitive
comps.** Dominant-shape watch: 3 of the top-10 3v3 comps are double-healer (among
them Druid+Paladin+Warrior 71.9 and Paladin+Priest+Warrior 70.6) — present,
not dominant.

## 2. Healer-framed: the same DPS, a different healer

Each cell is the Druid-healed team's win rate against the same DPS healed by the
column's healer, both team orders pooled (n=200 per cell, Wilson 95%). The
mirror column is the same comp on both sides — the control: team 1's win rate,
and its share of the decisive matches (n=100).

| DPS | vs Priest | vs Paladin | vs Shaman | mirror control |
|---|---|---|---|---|
| Warrior | 4.5% [2.4, 8.3] | 13.0% [9.0, 18.4] | 27.0% [21.3, 33.5] | 46% (dec 56.8%) |
| Mage | 99.0% [96.4, 99.7] | 67.5% [60.7, 73.6] | 51.0% [44.1, 57.8] | 46% (dec 51.1%) |
| Rogue | 71.5% [64.9, 77.3] | 10.0% [6.6, 14.9] | 78.0% [71.8, 83.2] | 50% (dec 51.5%) |
| Warlock | 73.5% [67.0, 79.1] | 23.0% [17.7, 29.3] | 52.0% [45.1, 58.8] | 17% (dec 37.8%) |
| Hunter | 57.5% [50.6, 64.1] | 0.0% [0.0, 1.9] | 0.5% [0.1, 2.8] | 36% (dec 39.1%) |

Four of the five mirror rows are within sampling of 50% decisive; Hunter+Druid's
39.1% of 92 decisive sits just outside (one row in five, and on the low side for
team 1, so not the slot bias AS-104 ruled out). The Warlock+Druid mirror draws
55 of 100.

**The full healer grid**, pooled over the five DPS (row healer's team vs column
healer's team, n=1,000 per off-diagonal cell):

| row \ col | Priest | Paladin | Shaman | Druid |
|---|---|---|---|---|
| Priest | — | 25.9% | 60.1% | 38.0% |
| Paladin | 74.1% | — | 76.6% | 74.7% |
| Shaman | 39.8% | 23.0% | — | 51.7% |
| Druid | 61.2% | 22.7% | 41.7% | — |

**Which healer each DPS prefers** — the DPS+healer comp against the whole 2v2
field (n=7,200 per cell; half-width about 1.1pt):

| DPS | +Priest | +Paladin | +Shaman | +Druid |
|---|---|---|---|---|
| Warrior | 53.6 | **77.5** | 50.8 | 57.8 |
| Mage | 70.5 | 69.1 | **72.7** | 55.4 |
| Rogue | 64.2 | **70.5** | 61.4 | 63.9 |
| Warlock | 49.0 | **58.9** | 42.9 | 41.2 |
| Hunter | 58.9 | **65.8** | 45.7 | 46.5 |

Every DPS but the Mage prefers the Paladin. The Druid is the second choice for
the Warrior and a close third for the Rogue; it is the worst choice for the Mage
(−17pt against the best) and for the Warlock.

## 3. The two scoping risks

### Uninterruptible healing — not an outlier

The Druid is interrupted far less than any other healer: **0.13 lockouts per
minute alive**, against Priest 1.00, Shaman 0.70, Paladin 0.67 (BasicArena
panel; on Nagrand 0.14 against 0.14–0.49). That is the expected consequence of
three instant heals.

It does not convert into an advantage against interrupt-heavy comps. Each
`<DPS>+<healer>` comp against competitive opponents, by how many interrupt
classes (Warrior, Rogue, Warlock's Felhunter, Shaman) the opponent fields:

| Healer | vs 0 | vs 1 | vs 2 | 0 → 2 |
|---|---|---|---|---|
| Priest | 57.9% | 49.0% | 55.3% | −2.6pt |
| Paladin | 63.3% | 62.0% | 65.9% | +2.6pt |
| Shaman | 45.3% | 50.9% | 59.9% | +14.7pt |
| **Druid** | 47.0% | 43.9% | 46.4% | −0.6pt |

(n=7,000 / 17,000 / 6,000 per row; the opponent set is identical for every
healer, so the rows compare.) The Druid's line is flat: facing more interrupters
costs it nothing and gains it nothing relative to the other healers.

### Dampened endgames — long mirrors that resolve; a 1v1 gap

Draw rates by how many sides field a Druid, with the Shaman — the other healer
whose mirrors run long — beside it. The **competitive** slice is the one to
read: the all-comps figures are dominated by double-healer comps (886 of the
989 2v2 "both sides" Druid draws are non-competitive).

| Format | Class | slice | 0 sides | 1 side | 2 sides |
|---|---|---|---|---|---|
| 2v2 | Druid | competitive | 0.83% | 0.80% | **4.12%** (103 / 2,500) |
| 2v2 | Shaman | competitive | 0.79% | 0.68% | **6.36%** (159 / 2,500) |
| 2v2 | Druid | all | 1.19% | 3.09% | 15.45% |
| 3v3 | Druid | competitive | 0.44% | 0.46% | **0.68%** |
| 3v3 | Shaman | competitive | 0.28% | 0.46% | **1.22%** |

In competitive play the Druid draws less than the Shaman. Its mirrors do run
long — the 2v2 Druid mirror's median is 95s against 29–82s for the other three
healers', and 75% of them pass the 75s dampening start — but they end in a kill.

**Where the gap shows is 1v1**, the format where a healer has no damage-dealer
beside it. 1v1 Druid against each healer (team 1 listed first; 100 seeds each):

| | Druid v Druid | Druid v Shaman | Druid v Paladin | Druid v Priest |
|---|---|---|---|---|
| draws (of them at the 300s cap) | 100 (100) | 100 (100) | 83 (78) | 0 |

The mechanism, per Druid per match over the same 100 seeds (`mechanisms.py
duels`; all 700 observed rows match the 1v1 CSV):

| Druid vs | draws | damage dealt | Moonfire | Lifebloom on itself |
|---|---|---|---|---|
| Druid | 100% | 83 | 2.0 | 25.0 |
| Shaman | 100% | 95 | 2.2 | 18.9 |
| Paladin | 83% | 766 | 10.9 | 9.6 |
| Priest | 0% | 65 | 2.1 | 17.1 |

Against a Priest, a Shaman or another Druid it deals under 100 damage in 300 seconds and
spends its mana keeping Lifebloom on itself. It never swings its staff. That is AI positioning, not a code bug: the
healer movement AI never closes to the 2.5yd melee reach, so the staff's
auto-attack never has a target in range.

2,940 of the 2v2's 3,302 draws are the 300s cap rather than a mutual kill.

| Format | Healer mirror | draws | median duration |
|---|---|---|---|
| 1v1 | Priest / Paladin / Shaman / **Druid** | 11% / 12% / 91% / **100%** | 169 / 209 / 300 / **300s** |
| 2v2 | Priest / Paladin / Shaman / **Druid** | 0.9% / 0.3% / 6.4% / **4.1%** | 47 / 82 / 29 / **95s** |
| 3v3 | Priest / Paladin / Shaman / **Druid** | 0.1% / 0.1% / 1.1% / **0.4%** | 37 / 54 / 32 / **54s** |

## 4. Mechanism metrics

Panel: `<DPS>+<healer>` for each of the five DPS and four healers, against every
competitive 2v2 comp, both orders, seeds 0–9 (12,000 matches; each healer class
meets the same opponents). Druid rows on PillaredArena from the Druid quarter of
the same panel (3,000).

### Relative mana longevity (the user's Innervate read, as a number)

Share of the panel-side healer that has been unable to afford its cheapest heal
at least once, among those alive with the match still running; and the median
time of that first out-of-mana moment, **among the healers that reach it** (a
healer that dies or wins first never does):

| Healer | by 30s | by 60s | by 90s | median first OOM, of those that go OOM |
|---|---|---|---|---|
| Priest | 45% | 96% | 100% | 28.9s (1,639 of 3,000) |
| Paladin | 21% | 91% | 98% | 34.4s (2,192 of 3,000) |
| Shaman | 53% | 100% | 100% | 28.7s (1,511 of 3,000) |
| **Druid** | **3%** | **28%** | **47%** | **56.7s** (914 of 3,000) |

Seconds out of mana before dampening reaches 50% (135s), per healer-match:
Druid 5.4 (median 0), Priest 11.2, Shaman 11.9, Paladin 20.5. **The Druid has
the most longevity by a wide margin**: it is the only healer most of whose
matches end before it ever runs dry, and those that do run dry do so at about
twice the time of any other healer. It casts Innervate in 68% of matches (0.68
per match). The AS-210 dying-heal bypass shows no sign of draining the reserve.
Mean mana fraction is survivor-biased past 60s and is in the analysis output,
not cited here.

### Cyclone

**0.10 Cyclones landed per Druid appearance** (514 in 5,000 on BasicArena; 367 in
3,500 on Nagrand), cast in about 10% of matches; 0.08–0.10 per Druid-minute
alive. AS-163's estimate (about 13 in 300) stands. Cyclone contributes little to
the Druid's standing, so its counterplay is near-invisible in matches.

### Travel Form deaths

60% of Druid appearances end in a Druid death; **32.5% of those deaths happen
while shifted** (981 of 3,020; Nagrand 29.1%). In 30.0% of Druid-side losses
the Druid died shifted. This is a correlation, not a measured cost: a Druid
under a melee is both likelier to be shifted and likelier to die.

### Roots never broken because Travel Form was unaffordable

Roots on the Druid: 0.34 per appearance. A shift broke 80% of them (82% on
Nagrand). Of the roots never broken, the Druid could not afford the 25-mana shift
at any frame of the root in **28% on BasicArena (96 of 340) and 52% on Nagrand
(115 of 221)**. AS-221 reported 14 of 20 on its own sample.

### The card's other Druid asks

- **Travel Form deaths — "if this costs games, design the fix".** Not shown to
  cost games: the 32.5% above is correlational. What would settle it is a paired
  arm carrying AS-162's design (an emergency leave gated on no chaser within the
  shift rule's reach, with a heal on the same beat) over the Druid panel. Filed as
  a follow-up, not designed here.
- **Wall and pillar pins.** Not measured by this sweep (pillar AI is milestone
  0.8 by ruling). Settled by AS-162's outrun-window probe run over the Nagrand
  panel's seeds, counting windows that gain no ground within 6yd of an obstacle.
- **Nagrand starting-room alcoves.** Not measured. Settled by AS-200's tester
  metrics (time in a room, stays and exits and deaths in a room) over the same
  panel.
- **`corner_penalty` 6.0 equals `flee` 6.0.** This breaks the rule CLAUDE.md
  states for a fleeing kiter (corner_penalty must exceed flee; the Hunter runs
  8.0 against 6.0), which makes it close to a correctness defect rather than a
  tuning choice. Not measured here; a paired Nagrand Druid-panel run at 8.0
  settles whether it is the alcove cause.
- **"Leaving for work it doesn't do."** Not measured. Settled from decision
  traces: form exits followed by two or more seconds with no chosen ability.

## 5. Hunter

Standing: 2v2 competitive 45.2%, 3v3 competitive 43.2% (the 3v3 floor). Best
partner Paladin (59.0% against the competitive field), worst Druid (36.0) and
Rogue (36.5).

Traps, from the Hunter panel alone (Hunter + each partner against every
one-healer comp, both orders, seeds 0–9; 3,520 Hunter-appearances): **0.09
Freezing Traps on the enemy healer per match; 92% of Hunter matches have none.**
The Hunter throws Frost Trap first in 46% of matches, and 94% of those have no
healer freeze. By enemy healer: Paladin 0.17, Priest 0.10, Shaman 0.08, Druid
0.01 per match. The healer freeze shows up against melee comps (Rogue / Warrior +
healer: 0.02–0.46 per match); against a Mage, Warlock or Hunter partner it is at
most 0.04. This is AS-196 / AS-223 at full scale: the shared trap cooldown goes
to the peel.

## 6. TeamPlan head-to-head (PillaredArena, n=300 per arm)

| Side given TeamPlan | vs | Legacy baseline | with TeamPlan | effect | z |
|---|---|---|---|---|---|
| Warlock+Priest | Warrior+Priest | 17% | 41% | **+25pt** | +6.7 |
| Warrior+Priest | Warlock+Priest | 83% | 86% | +3pt | +0.9 |
| Hunter+Priest | Rogue+Priest | 0% | 2% | +2pt | +2.5 |
| Rogue+Priest | Hunter+Priest | 100% | 55% | **−45pt** | −13.1 |
| Warrior+Mage+Priest | Rogue+Warlock+Paladin | 84% | 75% | −9pt | −2.8 |
| Rogue+Warlock+Paladin | Warrior+Mage+Priest | 16% | 11% | −5pt | −1.7 |

Seeds 1–100 are the 2026-08-06 table's seeds, so those rows pair with
`2026-08-06-team-solve-headtohead-n100.csv`.

**What moved since 2026-08-06, on those seeds.** Rebuilding `1ade748` (that
table's commit) reproduces its four effects exactly (+14 / −6 / +9 / +36). For
Rogue+Priest against Hunter+Priest, the effect went from −6 to −46pt, and it
splits about evenly between the two arms: the **Legacy** Rogue+Priest went from
79 to 100 wins of 100 (+21), and the **TeamPlan** Rogue+Priest from 73 to 54
(−19). Both pairs are team-1/team-2 counts in the two committed CSVs.

**The Legacy half bisects to two dispel merges on 2026-09-12**, each isolated
between its first parent and itself (`analyze.py bisect`, seeds 1–200):

| commit | what it is | Hunter cell: Legacy Rogue+Priest | TeamPlan Rogue+Priest | effect | 3v3 cell: Legacy W+M+P | TeamPlan W+M+P | effect |
|---|---|---|---|---|---|---|---|
| `1ade748` | the 2026-08-06 table's commit | 164/200 | 150/200 | −7pt | 154/200 | 143/200 | −6pt |
| `1323a25` | AS-44's first parent | 164/200 | 150/200 | −7pt | 154/200 | 143/200 | −6pt |
| `b175653` | AS-44 | **200/200** | 154/200 | **−23pt** | 154/200 | 143/200 | −6pt |
| `e6c0e93` | #161's first parent | 200/200 | 154/200 | −23pt | 154/200 | 143/200 | −6pt |
| `147e95b` | #161 | 200/200 | 154/200 | −23pt | **192/200** | 145/200 | **−24pt** |
| `ea63f4f` | measured commit (head-to-head CSV, seeds 1–200) | 200/200 | 111/200 | −44pt | 173/200 | 147/200 | −13pt |

- **`b175653` (AS-44, dispel priority made exhaustive)** moves the Hunter cell's
  Legacy arm: Legacy Rogue+Priest goes from 164 to 200 wins of 200 against Legacy
  Hunter+Priest, and the effect from −7 to −23pt. The 3v3 cell does not move.
- **`147e95b` (#161, a debuff's removal class)** moves the 3v3 cell's Legacy
  arm: Legacy Warrior+Mage+Priest goes from 154 to 192 of 200, and the effect
  from −6 to −24pt. The Hunter cell does not move.

The TeamPlan half has no single step at this resolution: TeamPlan Rogue+Priest
reads 154/200 at `b175653` and `147e95b`, and 111/200 at `ea63f4f`, a decline
spread over the later merges. The 3v3 cell's Legacy arm also came back down
after #161 (192 → 173 of 200), so its effect is −13pt on these 200 seeds and
−9pt on all 300.

The `1ade748` rebuild's aggregates match the committed 2026-08-06 CSV exactly;
483 of its 600 rows are identical in winner and duration, and the 2 rows whose
winner differs (both `pillar_TL`) offset each other
(`2026-10-10-as164-teamplan-repro-1ade748.csv`).

**Mechanism**, from observed runs of the Hunter cell at `ea63f4f`
(`mechanisms.py h2h_hunter`, seeds 1–100):

| arm | Rogue+Priest won | Hunter Freezing Trap /match | Hunter Frost Trap /match | Rogue-side Priest dispels /match |
|---|---|---|---|---|
| Legacy (LL) | 100/100 | 0.00 | 2.00 | 2.57 |
| Rogue+Priest given TeamPlan (LT) | 54/100 | 1.09 | 0.46 | 2.37 |

The Hunter-side Priest casts no Dispel Magic in either arm. All 200 observed rows
match the head-to-head CSV in winner and duration.

Against Legacy Rogue+Priest the Hunter never lands a healer trap window; against
the TeamPlan side it throws Freezing Trap and wins about half. The Priests'
dispel rates do not explain the gap. TeamPlan's positioning gives the Hunter its
trap windows; the Hunter's trap priority (§5) is the other half of the cell.

## 7. AS-234 (Paladin root Cleanse) at authority scale

Paired, `e4c01dc` (parent) against `ea63f4f`, every 2v2 cell a Paladin reaches
without double healers, n=50 (14,150 matches). Control: 400 of 400 Paladin-free
matches identical in winner and duration. Paladin side, against Paladin-free
opponents:

| Enemy fields | n | delta | McNemar z |
|---|---|---|---|
| all | 12,500 | −0.4pt | −2.55 |
| Mage | 3,500 | **−1.9pt** | −4.09 |
| Rogue | 3,500 | −0.5pt | −3.40 |
| Priest | 2,500 | −0.5pt | −1.85 |
| others | — | −0.4 to +0.4pt | ns |

**The cost is confirmed, concentrated on Mage opponents, and smaller than the
directional −2.7pt.** The Rogue slice is where the card's first narrowing (skip a
root under a stun that outlasts it) would act. Nothing in this data shows which
narrowing the Mage slice needs; that is the follow-up's first measurement.
Slices tested: 8 enemy classes plus the total; two enemy slices significant.

## Follow-ups

- **Druid healing throughput** (0.7 or 0.8). Under-tuned in 2v2 (45.1 against
  51–63) with the most unspent mana of any healer. Recommend a directional sweep
  of a Rejuvenation / Lifebloom magnitude raise against a governor relaxation,
  and pick the one that moves the Druid without moving its Priest matchup.
- **Druid solo endgame** (backlog). 1v1 Druid v Druid and v Shaman draw 100/100
  at the cap; the Druid keeps Lifebloom on itself at full health and never closes
  to staff range. Not a competitive-play problem (§3), so no milestone pull.
- **Druid `corner_penalty` below its `flee`** (0.8). A breach of the stated
  movement rule; measure 8.0 on the Nagrand Druid panel alongside the alcove
  metrics.
- **Travel Form emergency leave** (0.8). Paired arm of AS-162's gated design;
  settles whether the 32.5% shifted deaths cost games.
- **Paladin strength** (backlog, watch). 63.1% 2v2 competitive, preferred healer
  for four of five DPS. Not a 0.7 blocker.
- **Hunter trap priority** (0.8). 92% of Hunter matches have no healer freeze.
  AS-196's hold-for-healer variant cost wins; the next attempt should hold the
  peel only when a healer freeze is reachable inside the cooldown.
- **AS-234 narrowings** (0.8). −1.9pt against Mage, −0.5pt against Rogue; run
  the card's two narrowings as separate arms on the committed paired input.
- **Cyclone triggers** (backlog). 0.10 per appearance; the kit's control half
  contributes little.
- **Travel Form mana reserve** (backlog). 52% of unbroken roots on Nagrand were
  unaffordable shifts; a 25-mana reserve in the governor is the candidate.
- **3v3 anomaly: Mage+Warlock+Warrior** (backlog, watch). A no-healer comp at
  59% against competitive comps.
