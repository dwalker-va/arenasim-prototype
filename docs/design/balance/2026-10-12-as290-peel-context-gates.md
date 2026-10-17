# AS-290: the Peel play's context gates

Tier: **directional**. Success condition, stated before the sweep: with the play
switched off where it measured negative, `--expect improvement`. Giving a team
the gated play raises its win rate against the same team without it.

**Result: the gates hold; the improvement is met before the Burst play and
not shown after it.** Peel is now switched off in nine peeler-for-teammate
contexts (`class_ai::peel::PEEL_GATES`).

- **Before Burst** (§3): on seeds no gate was chosen or confirmed on, the gated
  play is worth **+0.5pt to the side that runs it (z=3.24, 14,400 paired
  matches, floor 0.3pt)**, and no map × assignment slice is negative. The
  ungated play is worth +0.0pt.
- **With Burst in every arm** (AS-275, §3b): on fresh seeds the gated play is
  worth **+0.2pt (z=1.35, floor 0.3pt): not an improvement at this size**. No
  slice is negative.
- **The gates themselves hold on both bases.** Lifting them costs the side that
  peels −0.40pt (z=−3.99) before Burst and −0.36pt (z=−3.41) with it.

Burst took over part of the play's value. The two plays now compete for the
same tools, the Rogue's Kidney Shot first. Whether Peel graduates is the 0.8
milestone sweep's call, by the criterion at the end of this note.

The user's ruling (2026-10-12): if a play measures negative in a context, it is
not used there. Each exception is named, justified by a measurement, and
re-checked at milestone sweeps.

## 1. The ungated play on current main

AS-274's head-to-head, re-run on `e1e97de`, after AS-288's debounce: seeds 1–4,
21,600 matches (`h2h-main.csv.gz`).

| | pairs | peeling side wins: without → with | flips (+/−) | z |
|---|---|---|---|---|
| overall | 14,400 | 47.5% → 47.5% | 322/320 | +0.04 |
| peeling team has Hunter | 3,840 | −0.9pt | 46/81 | −3.02 |
| peeling team has Shaman | 2,400 | −1.0pt | 60/84 | −1.92 |
| peeling team has Warrior | 3,840 | +1.3pt | 146/96 | +3.15 |
| peeling team has Warlock | 3,840 | +0.6pt | 140/116 | +1.44 |

The debounce did not change the picture: the play is neutral overall, and the
per-class sign is the one AS-274 found.

**A "team has class X" row does not measure X's peel.** It mixes in the
partner's peel for X. To separate the two, each class with a peel tool was
gated on its own (`abl-X.csv.gz`): the same PN and NP rows, with only X not
peeling. The difference is X's own peel, given that its partner still peels.

| peeler | pairs | its own peel | flips (+/−) | z |
|---|---|---|---|---|
| Warlock | 3,840 | +1.6pt | 112/51 | +4.70 |
| Paladin | 2,400 | +0.3pt | 7/1 | +1.77 |
| Priest | 2,400 | +0.3pt | 19/13 | +0.88 |
| Rogue | 3,840 | +0.0pt | 20/19 | 0.00 |
| Hunter | 3,840 | −0.1pt | 15/19 | −0.51 |
| Shaman | 2,400 | −0.5pt | 51/62 | −0.94 |
| Druid | 2,400 | −0.7pt | 63/79 | −1.26 |
| Mage | 3,840 | −1.0pt | 41/78 | −3.30 |

The Hunter's own peel is neutral. Its team's −0.9pt is its partners peeling
for it. Split by the teammate peeled FOR, the losses sit with two teammates:

- **For a Hunter:** −0.95pt (31/63, z=−3.20).
- **For a Warlock:** −1.0pt (33/67, z=−3.30).

Both carry their own escape: the Hunter kites, and the Warlock coils its own
melee. A peel spent on them is not spent on the kill. Peeling for a Warrior
(+1.3pt) or a Mage (+0.8pt) is where the play pays.

## 2. Choosing the gates: select, confirm, validate

The grain is **peeler class × class of the teammate in danger**: the finest
grain the data resolves, and one that is defined in 3v3 too. Three disjoint
seed sets keep a gate from being confirmed on the seeds that picked it:

1. **Select** (seeds 1–4): the per-class ablations, split by teammate.
2. **Confirm** (seeds 5–16, `conf-*.csv.gz`, 23,040 matches per arm): every
   peeler for a Hunter and for a Warlock, plus Druid for a Warrior, gated
   together. Every team holds exactly one gated direction, except
   Warlock+Hunter, so each team's slice is one context.
3. **Validate** (seeds 17–20): the shipped binary, below.

**The bar.** Gate a context when its peel costs the peeling side win rate
significantly over seeds 1–16 (pooled z ≤ −1.96), with the same sign on the
selection seeds and on the confirmation seeds (`pool.py`).

**The bar pools the selection seeds.** It requires significance only on the
pooled 1–16, so the seeds that picked a gate also count toward its z. The
confirmation set alone carries the sign, not significance. Four gates do not
reach z ≤ −1.96 on one of the two disjoint sets on its own:

- **Shaman@Warlock:** significant on seeds 1–4, not on seeds 5–16 (z=−1.56).
- **Druid@Hunter:** significant on seeds 5–16, not on seeds 1–4 (z=−0.17).
- **Druid@Warrior:** significant on neither (z=−1.56 and −1.80).
- **Rogue@Hunter:** significant on neither (z=−1.50 and −1.51).

The validation in §3 is the one test on seeds no gate saw. These four are the
first to re-check.


| context | seeds 1–4 | seeds 5–16 | pooled (1,920 pairs) | |
|---|---|---|---|---|
| Mage@Hunter | −4.2pt (0/20) | −3.1pt (2/46) | **−3.3pt (2/66), z=−7.64** | gated |
| Mage@Warlock | −2.7pt (3/16) | −2.7pt (19/58) | **−2.7pt (22/74), z=−5.21** | gated |
| Shaman@Warlock | −3.3pt (10/26) | −1.0pt (33/48) | **−1.6pt (43/74), z=−2.77** | gated |
| Druid@Warlock | −1.7pt (8/16) | −1.2pt (16/33) | **−1.3pt (24/49), z=−2.81** | gated |
| Druid@Hunter | −0.4pt (16/18) | −2.0pt (39/68) | **−1.6pt (55/86), z=−2.53** | gated |
| Druid@Warrior | −2.5pt (19/31) | −1.5pt (57/79) | **−1.8pt (76/110), z=−2.42** | gated |
| Rogue@Hunter | −0.8pt (0/4) | −0.3pt (1/6) | **−0.5pt (1/10), z=−2.41** | gated |
| Shaman@Hunter | −1.3pt (7/13) | −0.8pt (24/36) | −0.9pt (31/49), z=−1.90 | not gated (watch) |
| Priest@Hunter | −0.4pt | −0.1pt | −0.2pt, z=−0.44 | not gated |
| Priest@Warlock | +0.6pt | +1.1pt | +1.0pt, z=+2.23 | kept |
| Paladin@Hunter | +0.4pt | +0.3pt | +0.3pt, z=+2.04 | kept |
| Paladin@Warlock | 0.0pt | +0.3pt | +0.2pt, z=+1.22 | kept |
| Hunter@Warlock + Warlock@Hunter | 0.0pt | +0.9pt | +0.7pt, z=+1.59 | kept |

Peeling for a Hunter or a Warlock is not bad in itself: the Paladin and the
Priest are worth keeping there. What is bad is spending a Mage's, Druid's or
Shaman's control on them.

**By tool.** Can a gated context keep one tool? Each two-tool context was run on
seeds 5–16 with one tool gated at a time (`tool-*.csv.gz`), giving each tool's
marginal worth:

| context | first tool | second tool |
|---|---|---|
| Mage@Hunter | Frost Nova −1.1pt (2/18) | Polymorph −2.2pt (2/33) |
| Mage@Warlock | Frost Nova −1.5pt (7/29) | Polymorph −1.8pt (11/37) |
| Druid@Hunter | Cyclone −2.8pt (16/56) | Roots −1.2pt (19/36) |
| Druid@Warlock | Cyclone −0.1pt (8/10) | Roots 0.0pt (14/14) |
| Druid@Warrior | Cyclone +0.8pt (64/53, z=+0.92) | Roots −0.5pt (12/19) |

No tool is significantly positive in any gated context, so every gate takes
the whole context. The Druid@Warlock pair substitutes for each other: either
tool alone is neutral, and the two together cost 1.3pt.

## 3. Validation: the shipped binary on untouched seeds

The gated branch, rebased on `380af5f3`, on seeds 17–20. That base adds
AS-279 (Hammer of Justice asks for sight) and AS-281 (the Hunter's peel
pickers skip immune attackers, and a trap needs sight of where it lands).
Selection and confirmation ran on `e1e97de`, before either. Neither change
touches a gated context's peeler, and this run carries the gates onto that
base.

**Control: 7,200/7,200 NN rows identical** to main's `TeamPlan` in winner, end
reason and duration (6,848 kills, 3,174 distinct durations). Legacy: AS-274's
legacy batch (`gen.py legacy 2`, 3,600 matches) is identical to main row for
row (3,580 kills, 2,014 distinct durations).

**The gated play against no play** (`final.csv.gz`):

| slice | pairs | peeling side wins: without → with | flips (+/−) | z | floor |
|---|---|---|---|---|---|
| BasicArena, team 1 peels | 3,600 | 49.9% → 50.1% | 68/62 | +0.44 | 0.6pt |
| BasicArena, team 2 peels | 3,600 | 49.2% → 49.6% | 85/70 | +1.12 | 0.7pt |
| PillaredArena, team 1 peels | 3,600 | 45.0% → 45.8% | 74/45 | +2.57 | 0.6pt |
| PillaredArena, team 2 peels | 3,600 | 45.3% → 46.0% | 71/46 | +2.22 | 0.6pt |
| **overall** | **14,400** | **47.4% → 47.9%** | **298/223** | **+3.24** | **0.3pt** |

**The gates re-checked** (`lifted.csv.gz`): the same PN and NP rows with the
peeling side's gates lifted (`team{1,2}_lifted_gates`). Lifting them costs
**−0.40pt (70/127, z=−3.99)**. Only the gated teams move; every other team's
rows are identical. Per gate, the effect of peeling in the gated context:

| context | seeds 17–20 |
|---|---|
| Mage@Hunter | −3.3pt (0/16) |
| Druid@Warlock | −3.5pt (4/21) |
| Mage@Warlock | −1.7pt (10/18) |
| Druid@Hunter | −1.9pt (15/24) |
| Rogue@Hunter | −1.3pt (0/6) |
| Shaman@Warlock | −0.6pt (14/17) |
| Druid@Warrior | +0.4pt (27/25) |

Six of the seven hold their sign on a third seed set. Druid@Warrior does not:
it is the weakest gate, and the first one to re-check.

**Watch, not gated.** These are the validation's per-team slices of the
gated play. Gating on them would fit the validation seeds:

- **Warlock+Shaman:** −2.5pt (5/17, z=−2.35). Shaman@Warlock is gated, so this
  is the Warlock peeling for the Shaman.
- **Mage+Hunter:** −1.3pt (0/6, z=−2.04). The Hunter peeling for the Mage.
- **Rogue+Paladin:** −0.8pt (0/4).
- **Shaman@Hunter:** −0.9pt pooled, z=−1.90, just short of the bar.
- **Warlock@Paladin:** −1.46pt on seeds 1–4 (2/9, z=−1.81). It fell short of
  the selection, so it was never carried into the confirmation run, and it is
  unmeasured beyond those seeds.
- **Teams with a Shaman** in the gated play: −1.1pt (41/68, z=−2.49), most of
  it the Warlock+Shaman slice above.
- **Priest+Hunter** on the Burst base, seeds 21–24: −2.7pt (0/13, z=−3.33).
  This is one team on one seed set (§3b).

## 3b. On the Burst base

AS-275 (the Burst play, `983fa3de`) and AS-293 (interrupt sight, `51b5c046`)
then landed. Burst runs in every `TeamPlan` arm, the NN control included, so
the head-to-head now measures Peel on top of Burst.

**The seeds 17–20 batch again, on `51b5c046`** (`final-burst.csv.gz`,
`lifted-burst.csv.gz`). Same seven gates.

- The gated play is +0.3pt (275/235, z=1.73). It was +0.5pt (z=3.24) before
  Burst.
- Lifting the gates costs −0.35pt (69/120, z=−3.64).
- The Rogue's peel turned negative: teams with a Rogue −0.5pt (14/34,
  z=−2.74). Before Burst they were +0.1pt. Kidney Shot is now one of Burst's
  tools, so a peel spends the stun Burst wanted.

**Re-selection.** These seeds flagged the Rogue's contexts and Warlock@Shaman
(−2.7pt, z=−2.91). Each was then run on seeds 5–16 on the new base, peeling
against gated (`reselect-*.csv.gz`, 12,960 matches per arm; the gated arm is
`extra-gates.patch`). This bar is stricter than §2's: significant on the
confirmation seeds alone, with the same sign on seeds 17–20.

| context | seeds 5–16 (1,440 pairs) | seeds 17–20 sign | |
|---|---|---|---|
| Rogue@Warrior | −2.6pt (35/72), z=−3.48 | − | gated |
| Rogue@Priest | −1.0pt (1/15), z=−3.25 | − | gated |
| Rogue@Mage | +0.6pt (9/0), z=+2.67 | − | kept: positive |
| Warlock@Shaman | −0.8pt (21/32), z=−1.37 | − | watch: not significant |
| the other Rogue contexts | between −0.1pt and +0.1pt | | kept |

**Validation on seeds 21–24**, which no gate saw, on `51b5c046` with the nine
gates (`final2.csv.gz`, `lifted2.csv.gz`, `nn2-main.csv.gz`).

Control: 7,200/7,200 NN rows are identical to main's `TeamPlan` (6,847 kills,
3,178 distinct durations). Legacy: AS-274's legacy batch is identical to main
row for row, 3,600/3,600 (3,580 kills, 2,013 distinct durations).

| slice | pairs | peeling side wins: without → with | flips (+/−) | z | floor |
|---|---|---|---|---|---|
| BasicArena, team 1 peels | 3,600 | 49.3% → 49.6% | 63/52 | +0.93 | 0.6pt |
| BasicArena, team 2 peels | 3,600 | 50.1% → 50.1% | 63/65 | −0.09 | 0.6pt |
| PillaredArena, team 1 peels | 3,600 | 45.7% → 46.1% | 56/41 | +1.42 | 0.6pt |
| PillaredArena, team 2 peels | 3,600 | 44.6% → 44.7% | 49/44 | +0.41 | 0.6pt |
| **overall** | **14,400** | **47.4% → 47.6%** | **231/202** | **+1.35** | **0.3pt** |

**Not an improvement at this size.** The run rules out an effect above 0.3pt
either way.

Lifting the gates costs **−0.36pt (86/138, z=−3.41)**:

- Rogue@Warrior: −3.1pt (11/26). It held on its third seed set.
- Mage@Hunter: −2.1pt (0/10).
- No gate's context is significantly positive.

Peeling still pays for teams with a Warlock (+1.0pt, z=+2.66) and teams with a
Warrior (+1.2pt, z=+4.05).

New on these seeds: Priest+Hunter −2.7pt (0/13, z=−3.33). It is one team on
one seed set, so it is on the watch list, not gated.

**The probe, restated for the enabled contexts.** `peel_play_probe` now counts
only the windows the play is enabled for: those where a teammate of the unit
in danger has a peel tool and no gate for that unit's class. Only such a
teammate's cast answers a window. Which windows count depends on the comp,
never on the arm.

On this base, with the nine gates, the probe reads:

| | peel | withheld | z |
|---|---|---|---|
| windows | 340 | 337 | |
| answered within one GCD | 23 (6.8%) | 5 (1.5%) | **3.45** |
| mean survival | 8.11s | 8.15s | −0.15 |

Its bar is answered z ≥ 3 and survival z > −2, so it passes. The old count,
over every window, read 30/756 against 12/745 (z=2.77), below that bar,
because most of the bank's windows now belong to gated contexts or to a
Warrior's.

## 4. The AS-274 improvement ideas

Each was run as an arm against the ungated play on seeds 1–4, PN and NP rows,
and re-measured on fresh seeds where it was kept (`imp-*.csv.gz`, `pet2.csv.gz`).

- **Hunter pet peels: kept.** While its Hunter runs the play, a Spider webs a
  melee or pet attacker, and a Boar charges an attacker. It goes through the
  existing pet dispatch, and is traced as the play's step on the
  `pet_decision`. It adds +0.16pt on the selection seeds (6/0, z=+2.04) and
  +0.08pt on seeds 5–16 (10/1, z=+2.41). The gain is small but consistent: a pet's
  headline ability is usually on cooldown, or already spent on the Hunter's
  target.
- **Cancelling a cast to peel: dropped.** A unit mid a cast-time spell, off the
  global cooldown, with an instant peel tool ready and an attacker in reach,
  stops its cast. Overall it moves −0.09pt (48/61, z=−1.15). For Hunter teams
  it moves −0.65pt (6/31, z=−3.95): cancelling an Aimed Shot costs more than
  the snare buys. No class gained.
- **Keeping the attackers through the release window: dropped.** It moved 1
  match of 14,400. Inside AS-288's 0.1s release, the attackers almost never
  stop pressuring before the window closes, so the lapse the card describes
  does not happen in practice.

## Graduate or delete, restated for the gated play

At the 0.8 milestone sweep, Peel graduates (made unconditional, its gates
kept) only if all of these hold:

1. **The gated play improves the win rate.** Run AS-274's design (PN and NP
   each against NN, scored from the peeling side, both maps) on seeds this
   note did not use. The overall delta is positive and significant (z ≥ 1.96),
   and no map × assignment slice is significantly negative. Before Burst:
   +0.5pt, z=3.24. With Burst: +0.2pt, z=1.35, which is not yet met. No slice
   is negative on either base.
2. **Every gate is re-checked.** On the same seeds, run the PN and NP rows with
   the gates lifted. A gate whose context no longer costs the peeling side
   (its effect not negative at z ≤ −1.96, pooled with this note's seeds) is
   removed. An enabled context that is significantly negative there, and
   negative on a second seed set, is gated, by the bar in §2.
3. **The probe still passes** (`peel_play_probe`). It counts the windows of
   the enabled contexts only (§3b).

Otherwise the play is deleted, gates and all.

## Reproduce

Everything in `sweeps/2026-10-12-as290/`. `gen.py <batch>` writes each
batch. The selection and confirmation arms need `arms.patch`, applied to
`e1e97de`. It adds the environment variables that gate a context, or switch
on an improvement arm, without a rebuild. Validation needs no patch.

```bash
S=docs/design/balance/sweeps/2026-10-12-as290
python3 $S/paired.py $S/h2h-main.csv.gz $S/h2h-main.csv.gz --nn --by class   # §1
python3 $S/paired.py $S/h2h-main.csv.gz $S/abl-Mage.csv.gz --only Mage --by comp
python3 $S/pool.py                                                          # §2
python3 $S/paired.py $S/conf-ungated.csv.gz $S/tool-first.csv.gz --by comp
python3 docs/design/balance/sweeps/2026-10-12-as274-oriented.py $S/final.csv.gz --expect improvement  # §3
python3 $S/control.py $S/final.csv.gz $S/nn-main.csv.gz
python3 $S/paired.py $S/lifted.csv.gz $S/final.csv.gz --by comp
python3 $S/paired.py $S/imp-cancel.csv.gz $S/h2h-main.csv.gz --by class      # §4
python3 $S/paired.py $S/pet2.csv.gz $S/conf-ungated.csv.gz
```

The Burst-base runs (§3b), on `51b5c046`:

```bash
python3 $S/paired.py $S/reselect-shipped.csv.gz $S/reselect-gated.csv.gz --by comp
python3 docs/design/balance/sweeps/2026-10-12-as274-oriented.py $S/final2.csv.gz --expect improvement
python3 $S/control.py $S/final2.csv.gz $S/nn2-main.csv.gz
python3 $S/paired.py $S/lifted2.csv.gz $S/final2.csv.gz --by comp
python3 docs/design/balance/sweeps/2026-10-12-as274-oriented.py $S/final-burst.csv.gz
```

To run the validation arms again:

```bash
cargo build --release
python3 $S/gen.py final2 > /tmp/final.jsonl
python3 $S/gen.py lifted2 > /tmp/lifted.jsonl
target/release/arenasim --batch /tmp/final.jsonl --out /tmp/final.csv --jobs 8 --trace-mode off
target/release/arenasim --batch /tmp/lifted.jsonl --out /tmp/lifted.csv --jobs 8 --trace-mode off
```
