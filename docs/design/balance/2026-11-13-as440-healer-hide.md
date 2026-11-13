# AS-440: the healer that hides and the partner it exposes (design round 1)

**Card:** AS-440. **Status: PROPOSED, awaiting the user's ruling.** The rules
measured here are a prototype on this branch (`situation::HideRule`, a
per-team measurement switch, `team{1,2}_hide_rule`); nothing is shipped.
`Legacy` and the shipped `TeamPlan` are byte-identical with the prototype in
(1,392 Legacy and 1,392 TeamPlan matches of the diagnosis bank, CSV for CSV;
the harness rows of the 1,392 TeamPlan matches too).
**Tier: DIRECTIONAL** (`sweep-tiers.md`): 2v2, the 30 x 30 play-bank cells,
PillaredArena and TwinPillars, one seed per block (801 / 811 / 821), both
seats. An effect is starred only when all three blocks agree in sign and the
pooled McNemar |z| >= 1.96.

## 1. Diagnosis

`tests/healer_hide_exposure.rs` (ignored, a measurement) plays every cell in
which a healer + DPS team meets a team with a ranged DPS, uniform `TeamPlan`,
trace on: 4,176 matches, 5,040 sides (2,016 healer + melee, 3,024 healer +
ranged). For every enemy `target_focus` move off a side's healer onto its
partner it finds the enemy DPS that was on the healer, walks back to the
frame that DPS last saw it, attributes whose step broke the sight (positions
0.5s either side), and reads what held the healer's legs on that frame off
its posture machine (`ObservedHealerState`, new on the frame observer).

**Denominators.** Sides (one per qualifying team per match) for rates of
events; seconds both alive with an enemy ranged DPS alive for shares of time.

| | healer + melee | healer + ranged |
|---|---|---|
| sides with a focus move off the healer | 65.5% (0.71 moves/side) | 34.7% (0.38/side) |
| healer sees its partner in heal range at the move | 42% | 86% |
| the move's hide began in the opener (never seen, or lost in the first 10s) | 73% | 66% |
| mid-fight moves per side | 0.19 | 0.13 |
| DPS blind to the healer before the move, median | 7.9s | 4.1s |
| whose step broke the sight (moves not hidden from the gates) | healer 86%, both 9%, enemy 5% | healer 80%, both 16%, enemy 4% |
| focus then held on the partner, mean | 18.5s | 15.1s |
| partner health lost while it holds, mean | 61% of max | 61% |

Each block replicates within 1.5pt on every row (blocks 801 / 811 / 821:
65.6 / 64.7 / 66.2% of H+M sides).

**What put the healer out of the DPS's sight**, healer + melee, all 1,434
moves (the frame the DPS lost it):

| cause | share | kind |
|---|---|---|
| hidden from the gates (never seen) | 38.9% | opener |
| pillar-stance hug (FREE; AS-373 under AS-387's `AvoidCc` stance) | 17.2% | CC avoidance, almost all in the first 10s |
| plain FREE formation point | 10.0% | incidental, opener |
| PRESSURED team-solve cover (`C_OCCLUDED`) | 9.4% | cover |
| PRESSURED walking out of CC reach (`C_CC_REACH`) | 6.0% | CC avoidance |
| PRESSURED while silenced or locked out (owes no sight) | 5.8% | lockout |
| FREE reach repair (AS-362) | 5.5% | CC avoidance |
| ESCAPE, CC'd, walks, dip | 7.2% | other |

So the hand-off is mostly an **opener** effect (the team's opening position
and the stance's opening hug), then **CC avoidance**; the PRESSURED solve's
cover term, which AS-399 suspected, is a tenth of it. Its partner is not
lost by the hide itself: when the healer cannot see its melee partner at the
move, the partner's own step broke that sight (25% of moves) or it was
already blind (28%); the healer's step, 3%.

## 2. Options, measured head-to-head

Each option runs on the healer + DPS side only; its twin is the same cell and
seed with both sides shipped. 7,200 sides per option; healer hard CC is
seconds per side carrying a stun, fear, polymorph, incapacitate or cyclone.

| option | all | H+M | H+R | partner first death | healer hard CC |
|---|---|---|---|---|---|
| **C+A: no 2v2 stance, no hand-off cover** | **+1.7*** (+2.0 / +1.8 / +1.4) | **+1.7*** | **+1.7*** | 45.4 -> 43.6% (z -3.9) | 3.14 -> 3.01s |
| C: no 2v2 stance (Pillar withheld) | **+1.3*** (+1.7 / +1.0 / +1.1) | **+2.0*** (Nagrand +3.4*) | +0.8 | 45.4 -> 44.6% (z -2.1) | 3.14 -> 3.06s |
| A: no hand-off cover (`NoCover`) | **+0.7*** (+0.6 / +0.9 / +0.6) | +0.8 | +0.6 | 45.4 -> 44.8% (z -1.9) | 3.14 -> 3.10s |
| B: follow the partner (`Follow`) | -0.0 | -0.3 | +0.2 | 45.4 -> 45.9% (z +1.6) | 3.14 -> 3.14s |

- **A, `NoCover`.** While an enemy ranged DPS sees a teammate of the healer
  within the focus's 35yd reach, the PRESSURED solve gives no credit for
  cover from any enemy caster; crowd-control reach (`C_CC_REACH`) and sight
  of the anchor are untouched. (A narrower form, ignoring only the casters
  that see a teammate, moved 2% of the focus moves and was not swept.)
- **B, `Follow`.** Sight of a melee anchor counts only where the spot also
  sees 5yd ahead of it toward its target, and the Paladin's band-hold does not
  park blind to its anchor. It raised the link at the move from 43% to 50% and
  moved nothing else.
- **C, Pillar withheld.** The gated Pillar play off for the side: no stance,
  so no hug. Inert for the Paladin (it never hugs: 0 flips of 1,800).
- The H+M partner-first rate does not fall under any option (35.6 -> 36.0%
  under C+A); its gain is the healer surviving (healer first 19.4 -> 18.0%).

## 3. Recommendation

**C+A.** In 2v2 a healer + DPS team does not take the pillar stance, and its
healer takes no cover from casters while an enemy ranged DPS could turn on its
partner. CC exposure does not rise: the side's healer takes slightly less hard
CC (3.14 -> 3.01s a side). Round 2 builds C as a 2v2 format exception on the
Pillar play (`PILLAR_EXCEPTIONS` is class-keyed today; Peel's format gates
are the precedent) and A as a team-solve rule with its scene tests.

Alternatives: **C alone** (data only, the larger H+M gain, leaves the PRESSURED
cover term as it is); **A alone** (keeps the stance, the smallest gain).

## Gaps

- Directional: one seed per block, 2v2 only. The 3v3 effect of either piece
  is unmeasured; the stance measured +4.3 in AS-391's 3v3 bank at 0.8, which is
  why C is proposed for 2v2 only.
- The mechanism of C's gain is not shown. With the stance off, the opening
  hide moves from the hug to the FREE reach repair at about the same time
  (the sketch's match), so the focus still moves; what the side gains is its
  healer's survival.
- Double-healer 2v2 teams are not in any bank.

## Recompute

From `docs/design/balance/sweeps/`:

| figures | command |
|---|---|
| §1 | `2026-11-13-as440-reduce.py diag 2026-11-13-as440-diag.rows.jsonl.gz` |
| §2 | `2026-11-13-as440-reduce.py h2h 2026-11-13-as440-h2h-ctl.rows.jsonl.gz 2026-11-13-as440-h2h-<option>.rows.jsonl.gz` |

Banks: `2026-11-13-as440-gen.py diag 801,811,821`, `ctl 801,811,821` and
`h2h <rule> 801,811,821` (`NoCover`, `Follow`, `NoPillar`,
`NoPillar+NoCover`), run through `tests/healer_hide_exposure.rs`
(`HHX_TRACE=0` for the h2h banks).
