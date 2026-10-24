# Team sight cohesion and the team pillar stance (AS-385): design

Status: **PROPOSED, awaiting the user's sign-off (§9).** Nothing here changes
behaviour. The measurements of §2 are on `main` @ `43a8ee54` (AS-373 and
AS-383 merged). This card replaces AS-382 (the DPS's sight duty) and AS-384
(the purpose of the Priest's scream dip).

## 1. The model

The user's model (2026-10-23), which this design takes as its brief:

> it's misleading to measure some of these changes in isolation. For example,
> with the warrior losing more when his priest is pillaring, we havent
> addressed the dps responsibility of not lining the healer yet which may help.
> In general there is a heuristic here that is team wide and not individual
> about movement - you stay in line of sight of one another - the person who
> has to compromise to maintain that is a team decision, but usually it's up to
> the dps to lead. Pillaring should be a strategy to avoid hard CC, but any
> time your are lining the teammate you are supposed to be healing/helping or
> enemy you are damaging, you are CCing yourself! For classes that benefit from
> downtime (mana/hp over time effects), pillaring can also be about resource
> efficiency - stalling the game, but it is a team wide strategy to do that,
> not one combatant in isolation

Four rules follow from it:

1. **Teammates keep sight of one another.** Who gives ground to keep it is a
   team decision. By default the DPS leads and the healer follows. The DPS
   must not walk its healer out of sight ("line" it).
2. **Breaking sight of a unit you owe something is crowd-controlling
   yourself.** That covers the teammate you should heal or help and the enemy
   you should damage.
3. **Pillaring is a team stance, never one unit alone.** It has two purposes:
   avoiding hard CC, and stalling when the comp gains from downtime.
4. **Judge the package, not the pieces.** Win rate is judged on the combined
   team package. Each piece reports its own mechanism metric.

## 2. Measurements on current main

**The bank** is AS-373's (`2026-10-19-as373-gen.py <profile> 2 3`). It covers
PillaredArena (Nagrand) and TwinPillars. Each map has every 2v2 team with a
healer against every 2v2 team (no double healers) on seeds 1-2, plus six
one-healer 3v3 teams against each other in both orders on seeds 1-3. Both
teams run the same profile. That is 2,580 matches per profile. A same-binary
rerun of the whole bank reproduced every row.

**The harness** is new: `tests/team_sight_cohesion.rs` (ignored, a
measurement). It reads every frame through the read-only observer, and reads
the healers' posture and traced pillar mode from the decision trace. Where
AS-373's `healer_pillar_modes` measures the healer, this one measures every
unit and the team.

- **Rows:** `balance/sweeps/2026-10-24-as385-rows-{teamplan,legacy,camp}.jsonl.gz`
- **Reducer:** `balance/sweeps/2026-10-24-as385-reduce.py`

```bash
docs/design/balance/sweeps/2026-10-19-as373-gen.py TeamPlan 2 3 > bank.jsonl
TSC_IN=bank.jsonl TSC_OUT=rows.jsonl TSC_TMP=/tmp/tsc TSC_JOBS=8 \
  cargo test --release --test team_sight_cohesion -- --ignored --nocapture
docs/design/balance/sweeps/2026-10-24-as385-reduce.py rows*.jsonl.gz
```

**Definitions.** All are measured after contact, the first health lost in the
match.

- **Blind:** a pillar blocks the eye-height line between two units.
- **Hidden:** no living enemy ranged unit sees the unit. Pets count; the
  Warrior, Rogue and Paladin do not.
- **Dying:** below 50% health.
- **In danger:** dying, and an enemy is targeting the unit.
- **Lining:** blind, while not hard-CC'd, to the unit you owe something.
  - For a DPS, its target, alive and within reach (10yd melee, 36yd ranged).
  - For a healer, its anchor (the most injured teammate) while the anchor is
    below 80%.
- **Who broke sight:** AS-373's attribution, comparing positions 0.5s apart.
  It is whose step broke sight: the DPS's, the other unit's, both, or neither
  alone (joint).

### 2.1 Mutual sight: the DPS and its healer

The share of a DPS's time blind to its own healer:

| profile | map | fmt | blind | while dying | while in danger | episodes ≥1s per DPS-match | broke: DPS / healer / both / joint |
|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | 1.6% | 1.2% | 1.0% | 0.12 | 45 / 23 / 0 / 31 |
| Legacy | Nagrand | 3v3 | 1.3% | 2.5% | 1.1% | 0.07 | 41 / 26 / 0 / 33 |
| Legacy | TwinPillars | 2v2 | 8.9% | 7.5% | 7.7% | 0.77 | 42 / 29 / 3 / 26 |
| Legacy | TwinPillars | 3v3 | 16.3% | 10.3% | 9.9% | 1.14 | 27 / 33 / 4 / 36 |
| TeamPlan | Nagrand | 2v2 | **11.8%** | 6.4% | 3.9% | 0.92 | **79** / 13 / 1 / 7 |
| TeamPlan | Nagrand | 3v3 | 6.1% | 6.4% | 5.0% | 0.35 | 60 / 19 / 0 / 21 |
| TeamPlan | TwinPillars | 2v2 | 11.7% | 9.8% | 8.7% | 1.21 | 54 / 29 / 3 / 14 |
| TeamPlan | TwinPillars | 3v3 | 19.5% | 15.1% | 13.9% | 1.24 | 36 / 35 / 4 / 25 |

By DPS class under `TeamPlan` on Nagrand, blind to the healer: Warrior 13.9%,
Rogue 13.2%, Warlock 11.3%, Hunter 9.7%, Mage 9.1%. The DPS's own step broke
sight in 54-91% of each class's episodes: Warlock 91%, Hunter 85%, Mage 83%,
Warrior 78%, Rogue 54%.

**What the "in danger" gate would reach.** Of a DPS's blind time, only a small
part falls while it is dying, and less while it is in danger:

| map | role | dying | in danger |
|---|---|---|---|
| Nagrand | melee | 12% | 6% |
| Nagrand | ranged | 7% | 4% |
| TwinPillars | melee | 17% | 13% |
| TwinPillars | ranged | 11% | 7% |

A rule that holds sight only while the DPS is in danger (AS-373 §6 as
written) reaches about a twentieth of the blindness on Nagrand. Most of the
blind time is a healthy DPS walking out of its healer's sight. That blindness
becomes the dying kind once the DPS is hit.

Episodes by the healer's posture and traced mode as sight was lost (`TeamPlan`,
Nagrand). Most blind episodes start with the DPS at the pillar's skin:

| posture | mode | episodes | median | broke: DPS / healer / both / joint | DPS within 1yd of the skin |
|---|---|---|---|---|---|
| free | open | 820 | 3.6s | 86 / 9 / 0 / 5 | 77% |
| free | hug | 51 | 2.6s | 69 / 2 / 0 / 29 | 84% |
| pressured | open | 976 | 3.5s | 75 / 16 / 2 / 7 | 64% |
| pressured | hug | 35 | 3.0s | 80 / 9 / 3 / 9 | 49% |
| escape | open | 51 | 4.4s | 10 / 51 / 0 / 39 | 41% |

The camp comp (Warrior+Priest against Warlock+Priest, Nagrand, seeds 1-12,
`TeamPlan`, 10 of 12 won):

- The Warrior is blind to its Priest 18.4% of the time. Its own step broke 79%
  of those episodes. `Legacy`: 0%.
- The team stood with the Priest hidden and the Warrior exposed for 7-50% of
  each match's team time.
- Both losses are Warrior deaths behind the pillar: 6.5s and 2.4s blind, the
  final sight broken by the Priest's step.

### 2.2 Self-CC from lining, against the enemy's hard CC

Share of alive time after contact:

| profile | map | class | hard-CC'd | lining | lining share of duty time | DPS's own step broke the line |
|---|---|---|---|---|---|---|
| Legacy | Nagrand | Mage | 7.8% | 0.4% | 0.5% | 17% |
| Legacy | Nagrand | Warlock | 7.2% | 1.4% | 1.6% | 5% |
| Legacy | Nagrand | Hunter | 5.2% | 1.1% | 1.4% | 37% |
| Legacy | Nagrand | Warrior | 10.0% | 0.1% | 0.1% | — |
| TeamPlan | Nagrand | Mage | 6.6% | **26.6%** | 30.8% | 6% |
| TeamPlan | Nagrand | Warlock | 7.3% | **25.3%** | 26.7% | 4% |
| TeamPlan | Nagrand | Hunter | 5.2% | **19.3%** | 23.6% | 11% |
| TeamPlan | Nagrand | Warrior | 12.5% | 0.9% | 1.2% | 7% |
| TeamPlan | Nagrand | Rogue | 9.0% | 0.5% | 0.7% | 18% |
| TeamPlan | Nagrand | Priest | 4.1% | 1.9% | 6.5% (of anchor-hurt time) | — |
| TeamPlan | Nagrand | Druid | 3.3% | 1.7% | 5.8% | — |
| TeamPlan | Nagrand | Shaman | 3.7% | 1.6% | 6.4% | — |
| TeamPlan | Nagrand | Paladin | 4.2% | 1.9% | 8.0% | — |

TwinPillars reads the same way: `TeamPlan` Mage 24.6%, Warlock 19.5% and
Hunter 19.3% lining, against 4.6-7.7% hard-CC'd.

**Under `TeamPlan` a ranged DPS loses three to four times as much of its time
to being blind to its target as it does to the enemy's crowd control.** The
cause is the enemy, not its own step. Ranged lines on a target within reach
(episodes of 0.25s or more):

| profile | map | target | episodes | seconds | broke: DPS / target / both / joint | another enemy in reach and sight (share of the seconds) |
|---|---|---|---|---|---|---|
| Legacy | Nagrand | healer | 118 | 465 | 6 / 82 / 8 / 3 | 17.8% |
| TeamPlan | Nagrand | healer | 6,077 | 14,838 | 0 / 95 / 4 / 0 | **7.3%** |
| TeamPlan | Nagrand | dps | 48 | 145 | 12 / 56 / 31 / 0 | 25.4% |
| TeamPlan | TwinPillars | healer | 6,960 | 13,182 | 1 / 91 / 7 / 1 | 19.4% |

This is the other face of the `TeamPlan` healer's cover. The enemy healer
hides (44% of its time against `Legacy`'s 21%), and the ranged DPS on it is
blind. Ninety-three percent of the time there is no other enemy it could
switch to. The healer solve already counts this as a win (AS-362, AS-381).
For this design it means two things:

- A team that pillars denies the enemy's ranged DPS for real.
- The ranged DPS's answer to a hidden target belongs to the offence against
  a pillar (seek-chase, target choice), not to cohesion. That is a follow-up,
  not this card.

Healers line rarely: 1.6-1.9% of their time, 6-8% of the time a heal is owed.
That is below their hard-CC time. The healer's sight-first rules (AS-206,
AS-373, AS-381) are working. **What lines the team is the DPS's movement
relative to its healer (§2.1), and the enemy healer's cover relative to the
enemy DPS.**

### 2.3 The pillar stance today

Team frame shares after contact, teams with a healer and two or more alive:

| profile | map | fmt | all hidden | all hidden and mutually sighted | healer hidden, a DPS exposed | a DPS hidden, the healer exposed | mutually sighted | all at a pillar (within 6yd of a skin) | healer at a pillar, a DPS not |
|---|---|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | 19.4% | 19.2% | 0.8% | 0.4% | 98.4% | 0.9% | 5.3% |
| Legacy | Nagrand | 3v3 | 11.5% | 10.6% | 0.5% | 0.4% | 97.7% | 0.7% | 3.8% |
| TeamPlan | Nagrand | 2v2 | 33.8% | 27.9% | **10.9%** | 1.9% | 88.2% | 26.9% | 11.1% |
| TeamPlan | Nagrand | 3v3 | 14.4% | 12.3% | 6.4% | 1.3% | 88.4% | 6.0% | 16.0% |
| TeamPlan | TwinPillars | 2v2 | 29.2% | 26.1% | **22.6%** | 3.1% | 88.3% | 42.0% | 18.3% |
| TeamPlan | TwinPillars | 3v3 | 13.6% | 10.0% | **20.7%** | 10.1% | 59.3% | 28.8% | 36.2% |

While a `TeamPlan` healer hugs (its traced mode), every DPS is hidden too for
only 45.5% of the hug time on Nagrand 2v2, 28.5% on Nagrand 3v3 and 9.6% on
TwinPillars 3v3.

**The pillar is played by one unit today.** `TeamPlan` takes cover for the
healer alone, and leaves a DPS exposed a tenth to a fifth of the time. That is
the user's "one combatant in isolation". The melee DPS is hidden 20% of its
time under either profile, against the healer's 44-48%.

### 2.4 Downtime: what a stall is worth, per comp

**Lulls.** A lull is a span of 2s or more after contact in which no non-pet
unit on either side lost health. Lulls already fill 40.5% of post-contact time
under `TeamPlan` on Nagrand (27.9% under `Legacy`), at 4.8 per match.

**Mana does not regenerate in any useful way.** Healers regenerate 1 mana/s
from gear (base regen is 0, and there is no five-second rule), against pools
of 260-290. A 10s stall is worth about 3.5% of a pool, a third of a heal. By
60s after the gates every healer stands at 10-20% mana in both profiles, and
spends 19-38% of its alive time below 10%. So "stall for mana" has nothing to
buy beyond Innervate and totems. What a stall buys is health over time, from
heals over time and from casts the lull frees.

Net team health gained per lull second against the enemy's, in early lulls
(before 75s) with the team below 95%, `TeamPlan` 2v2. This is an upper estimate:
it selects lulls in which the comp had health to regain.

| comp | net per lull second | own | in lulls the team spent hidden |
|---|---|---|---|
| Warrior+Druid | **+0.98%** | 1.52% | +0.83% |
| Warlock+Druid | +0.75% | 1.07% | +0.80% |
| Mage+Druid | +0.58% | 1.13% | +0.53% |
| Warlock+Priest | +0.54% | 1.26% | +0.24% |
| Rogue+Druid | +0.47% | 1.16% | +0.38% |
| Warlock+Paladin, Hunter+Druid, Hunter+Paladin | +0.39 to +0.46% | | |
| Rogue+Paladin, Warrior+Paladin, Mage+Paladin, Mage+Priest | +0.29 to +0.36% | | |
| Rogue+Shaman, Warrior+Priest, Rogue+Priest, Mage+Shaman, Warlock+Shaman | +0.15 to +0.22% | | |
| Hunter+Shaman, Hunter+Priest | +0.03 to +0.04% | | |
| Warrior+Shaman | **-0.21%** | 0.56% | -0.36% |

By healer, the net in early lulls is: Druid +0.64%/s, Paladin +0.34%/s,
Priest +0.25%/s, Shaman +0.05%/s. After the dampening onset each falls to
+0.03-0.21%/s. `Legacy` agrees on the order and is harsher on the Shaman:
every Shaman comp is at or below zero there (Hunter+Shaman -0.67%/s), and
the Druid and Paladin comps are on top.

Read as a stall: a 10s team stall is worth up to about +10% of team health to
Warrior+Druid, about +3% to a Paladin comp, and nothing to a Shaman comp. It
is worth nothing to anyone once dampening bites.

### 2.5 Deaths at a pillar, by cause

A **pillar death** is a DPS death, with its healer alive, after the DPS had
been hidden from that healer for the last 2s or more. Unlike AS-373 §2.6, this
count does not require heal range. Cause:

- the healer was hard-CC'd for most of the last 5s; otherwise,
- whose step broke the final sight.

| profile | map | fmt | deaths | pillar deaths | healer CC'd | healer's step | DPS's step | both | joint |
|---|---|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | 886 | 3 (0.3%) | 0 | 1 | 1 | 0 | 1 |
| Legacy | TwinPillars | 2v2 | 869 | 50 (5.8%) | 16 | 11 | 10 | 1 | 12 |
| TeamPlan | Nagrand | 2v2 | 859 | **35 (4.1%)** | 4 | 5 | 10 | 1 | 15 |
| TeamPlan | Nagrand | 3v3 | 173 | 4 (2.3%) | 1 | 1 | 1 | 0 | 1 |
| TeamPlan | TwinPillars | 2v2 | 902 | 55 (6.1%) | 17 | 17 | 10 | 1 | 10 |
| TeamPlan | TwinPillars | 3v3 | 167 | 21 (12.6%) | 2 | 7 | 8 | 1 | 3 |

By class (`TeamPlan`): Warrior 5.9% of its deaths on Nagrand and 14.5% on
TwinPillars, Rogue 8.4% and 11.8%, Mage 6.5% and 5.2%. Hunter (0.8% on
Nagrand) and Warlock (1.1%) are low.

**Every cause is present.** On Nagrand the DPS's step and the joint walk
dominate. On TwinPillars, CC and the healer's step each match the DPS's. No
single-unit rule removes pillar deaths. Each cause is answered by a different
piece of §3:

| cause | answered by |
|---|---|
| DPS's step | the yield rule (§3.1) |
| joint walk | the team stance's shared side (§3.3) |
| healer's step | the existing healer sight rules |
| CC | the stance's CC avoidance |

## 3. The proposal

### 3.0 Where it lives

| piece | home | why |
|---|---|---|
| who yields sight (cohesion) | a team situation, `sight_yield`, classified in `situation.rs` | A fact more than one unit's movement reads, from the shared snapshot, debounced and traced like `ally_in_danger`. "Read a situation, never re-derive one." |
| the team pillar stance | a team situation, `pillar_stance`, plus a movement play, `Pillar`, in `plays::PLAY_ORDER` (after TakeCover, before Chain) | The play machinery gives the `TeamPlan` gate, per-team withholding (`team{1,2}_withheld_plays: ["Pillar"]`, the head-to-head control), a named exceptions table and the trace, all for free. TakeCover is the precedent for a movement play. |
| what each role does | the existing executors: the healer's FREE and PRESSURED machines with `pillar_mode`'s ring, the kiter scorer, melee pursuit's `steer_toward_goal` | No new solver. Each consumer gains one constraint of a shape it already has. |

`team_plan::Stance` (`Hold` / `Press` / `Withdraw`) keeps the opening camp.
The stance is the in-fight counterpart the team-level design called
`Withdraw` with `StackAnchor`, re-cut along the user's two reasons, with a
sharper finding behind the second (§2.4: a stall is worth health over time,
not mana).

### 3.1 Team sight cohesion: who gives ground

**The duty graph.** Each living non-pet unit owes sight:

- a healer to each teammate, the anchor first;
- a DPS to its healer, to be healed;
- a DPS to its target.

A DPS and its healer form a **sight pair**. For each pair, the team names the
**yielder**: the unit whose movement must not break that pair's sight. The
other unit leads. The healer follows a leading DPS with today's machinery
(open heading, hug ring, solve `C_SIGHT`), so "the DPS leads, the healer
follows" is the default and costs nothing new.

**The yield rule.** Classified per pair; the first rule that holds decides.

1. **A unit that cannot move leads.** Hard CC, a root, or a cast in progress:
   it cannot follow, so the other must not leave it.
2. **A healer under pressure leads.** It is pressured (`AllyStanding`), or it
   is the subject of `ally_in_danger`. Its DPS yields, and walking round a
   pillar away from a trained healer is the opposite of a peel.
3. **The pillar stance leads (§3.3).** While the team holds a stance, every
   DPS yields to the stance's side.
4. **A hurt DPS yields.** It is below `SIGHT_YIELD_HP` (proposed 80%, held
   until 90%), or it is the subject of `ally_in_danger`. This is the user's
   "the DPS must not line its healer", widened from AS-373 §6's "in danger"
   because §2.1 measured that gate reaching about a twentieth of the
   blindness. A healthy DPS that walks out of sight is about to become a hurt
   one behind a pillar (the camp losses).
5. **Otherwise the DPS leads** and the healer follows.

The rule never asks a healer to yield more than it already does. The healer is
always sight-first, and the yield rule only adds duties to the DPS.

**Anti-strobe.**

- Every continuous key is a Schmitt pair: health 80% to yield, 90% to release.
- A yield, once set, holds `SIGHT_YIELD_DWELL` (2s, as `MODE_DWELL`). Only
  rule 1 can cut that short.
- Pressure and danger inherit `ally_in_danger`'s own dwell and release.
- Each change is traced as a `situation_change`, `name: "sight_yield"`, with
  the pair and the rule. A strobe shows as a count.

**What a yielding DPS does.** This is item 4 of the brief, §3.4.

### 3.2 Pricing self-CC

**One currency: action-seconds.** A second blind to the unit you owe is one
second of that duty lost. A second of hard CC is one second of every duty
lost, movement included. §2.2's table is that currency, and the build's
mechanism metric (§7) reports both sides of it per class.

**The ranking that follows, the same for every role:**

1. **An owed duty's sight comes before hiding from crowd control.** A healer
   owes a heal (the anchor is hurt). A yielding DPS owes its healer sight. A
   DPS owes its target unless the stance suspends that (§3.3).
   - This is the `TeamPlan` solve's order today: dying reach ≫ sight > CC
     reach > cover (AS-381's user ruling: sight outranks cover and CC
     together, at any caster count). It is extended unchanged to the DPS.
   - The grounds are the measurement:
     - Lining is certain, and the CC it dodges is not. A ready Fear in reach
       may go elsewhere, or be dispelled or trinketed.
     - Lining is already the larger loss. A `TeamPlan` ranged DPS loses 20-27%
       of its time to it, against 5-8% to CC (§2.2).
2. **Hiding from CC is bought only as a team.** The team stance moves the team
   to where the owed sightlines and the cover hold together. A unit alone
   never buys cover with blindness. That is the user's "pillaring should be a
   strategy to avoid hard CC, but ... lining ... you are CCing yourself"
   resolved: the stance is what makes the two compatible.
3. **Where no spot satisfies both, sight wins.** That is the existing
   fallback, with the existing ladders.

**No new weights.** The healer side is already priced this way: `W_SIGHT` with
the AS-381 surcharge, `C_DYING`, and the hug's defended arc. The DPS side is
a mask (§3.4), which is a lexicographic preference, so it needs no tuning
against `flee`.

### 3.3 The team pillar stance

**The situation.** `pillar_stance: Option<StanceWindow>`, per team, where
`StanceWindow = { pillar, side, reason: AvoidCc | Stall, since }`.
`TeamPlan` only, and classified only on a map with obstacles.

**Entering for `AvoidCc`.** All of the following hold:

- **The team is threatened by CC a pillar defends against.** The union over
  the living teammates of `pillar_mode::pillar_threats`: a ready (5s), sight-
  needing crowd control from beyond 8yd, by a non-pet, that the unit is not
  immune to. This is AS-362's `CcField`, read for the team instead of the
  healer.
  - In practice, the enemy fields a Mage, Warlock or Druid with its CC ready.
  - A Rogue's stun or a Scream never qualifies (AS-373 §2.2: a pillar buys
    nothing against them).
- **The fight is at a pillar.** The team's centroid, or its anchor, is within
  `HUG_ENTER` (12yd) of a pillar's skin, using AS-373's Schmitt pair (12 in,
  20 out) and sticky pillar.
- **A team arc exists.** Ring ground on the pillar's far side from the
  threatening casters, from which the healer sees every living teammate
  within heal range: `defended_arc`, intersected over the teammates. It must
  hold the margin (`holds_margin`).
  - This is AS-373's feasibility key with "the anchor" generalised to "every
    teammate". It is what makes the stance a team position and not a healer's
    hide.
- **The team is not pressing for a kill.** No `kill_window` is open: Burst
  wins. The team is not `Pressing` with the enemy healer locked.

**Entering for `Stall`.** All of the following hold:

- **The comp gains from downtime.** In v1 a named table, the play's exceptions
  table read the other way round: comps whose measured stall value is
  positive (§2.4). Today that means the Druid comps, then Paladin; no Shaman
  comp. Each entry cites its measurement, as `PEEL_GATES` does.
  - A live predicate, our heals over time active, is the v2 refinement.
- **The team trails or is even, and is hurt.** Tempo `Trailing` or `Even`,
  and team health below 70%.
- **Dampening has not bitten.** Reduction below 25%; the value falls about
  fivefold after onset (§2.4).
- **The enemy is not ticking on us.** No damage over time from the enemy on
  any teammate: a Warlock's DoTs keep working through a stall.
- **A team arc exists**, as for `AvoidCc`. The stall's side is the far side
  from the nearest enemy.

**Leaving.** Each stance holds at least `MODE_DWELL` (2s). It leaves when:

- its reason has been false for `HUG_RELEASE` (3s);
- a kill window opens (Burst outranks);
- a teammate is dying with no spot on the arc that reaches it in sight (sight
  first, §3.2);
- the team arc stops holding the margin, which ends it at once because it is
  feasibility;
- for a stall, its reason ends: the comp is healed above 90%, the enemy
  commits round the pillar (an enemy unit within 8yd of a teammate: the stall
  has drawn them in, so the team fights), or dampening crosses the threshold.

Every change is traced as a `situation_change` named `pillar_stance`, plus
AS-373's `PillarModeEnter` / `PillarModeLeave` on the healer.

**What each role does in it:**

| role | in the stance |
|---|---|
| healer | AS-373's hug: the ring point in FREE, the ring terms in the PRESSURED solve. The anchor's arc is replaced by the team arc. The mode's preference keys (`at_pillar`, `cc_ready`) are replaced by the stance, as AS-373 §11.1 designed the seam. Its feasibility key, dwell, trace and ring goal stay. |
| melee DPS | Yields (§3.1 rule 3). It fights whoever comes round the pillar, on the stance's side. Pursuit takes the tangent side that keeps its healer's sight (§3.4). A target that stays on the far side is not chased past the shadow line: a stall's purpose is not to fight. Under `AvoidCc` the melee keeps chasing on the sighted side. |
| ranged DPS | Yields. The kiter scorer gains the healer-sight mask (§3.4) and a pull to the stance's side of the pillar (`formation_pull`'s shape, toward the ring's side point). `flee`, `range_band`, hysteresis and the wand fallback are untouched. Under `Stall` its damage duty is suspended: `los_seek` and the seek-chase arm are off while the stance holds, so it does not walk out to find its target. Under `AvoidCc` they stay on, kept inside the mask. |
| everyone | Movement-locking casts follow the existing walk-deferral rule while a unit walks to the side. Instants still fire. |

**AS-373 and AS-362 as inputs.**

- **AS-373's hug becomes the healer's half of the stance.** Its per-healer
  preference keys go. Hug is entered only under the stance (decision D3), so
  no healer pillars alone.
- **AS-362's `CcField` is the stance's `AvoidCc` trigger** (its sight-needing
  subset, `pillar_threats`). Its `C_CC_REACH` and the FREE reach repair keep
  ranking positions, as today.
- **The open heading and AS-206's repair stay** for the open mode, which is
  every frame without a stance.

**The camp.** The opening camp (`Stance::Hold`) is a pre-contact pillar stance
by another name. It stays as it is, and the in-fight stance takes over at
contact. AS-383 measured that the camp's value after contact came from the
`PRESSURED` solve running (the dip was only its trigger). The stance gives the
healer the solve's ring terms in FREE as well, which is the intended
replacement for that accident (§3.5).

### 3.4 The DPS half

**While it yields, a DPS's own step does not break its healer's sight.** There
are two executors.

**Kiters (Mage, Hunter; the ENGAGE/KITE scorer).**

- The scorer's existing `AnchorConstraint` is set to the DPS's own healer,
  with sight only. Heal range stays the soft `healer_leash` term, already
  measured +14pt for Hunter+Priest.
- **The mask applies only to a sighted DPS.** A candidate step that would put
  the healer out of sight is masked. A DPS already blind gets the
  `close_when_out` rung instead: the steps that regain sight, else the steps
  that end nearer the healer.
- The rest of `score_directions`' ladder is unchanged: the anchor mask goes
  before the obstacle and boundary masks.
- `TeamPlan` only. While no yield is set it is `None`, which is the scorer
  bit for bit.

**Melee pursuit (Warrior, Rogue, the Paladin's FREE; pets excluded).**

- `steer_toward_goal` gains an optional sight-keep point. When the pursuit
  must round an obstacle, each tangent side's waypoint is tested for sight of
  the healer. A side that keeps sight beats one that does not. Between two
  that do, or two that do not, today's self-reinforcing score decides.
- **The chosen side is held.** A side picked for sight is stored per pursuit
  (`PursuitSide { obstacle, side, until }`) for the commit window and renewed
  while the obstacle stays the blocker. A sight preference is not
  self-reinforcing the way the taut-path score is (AS-181), so without a held
  side it would flip.
- **If neither side keeps sight:**
  - a yielding DPS that is not in danger takes the side whose waypoint is
    nearer the healer's line;
  - one in danger (rule 4 with `ally_in_danger`) holds at the pillar's edge on
    its healer's side for at most the commit window, then chases.
  - Holding costs swing time (lining its target, §3.2). It is paid only by a
    unit that is dying.
- **`Legacy` is byte-identical by construction.** `steer_toward_goal`'s first
  line already returns on an obstacle-free map, and the sight-keep point is
  `None` unless the unit's team runs `TeamPlan` and a yield is set.

**Why this does not repeat the reverted DPS solve.** The 2026-08-06 decision
reverted `HoldRange` because constraint satisfaction replaced the kiter's
objective. A ring constraint with a nearest-satisfying tie-break stops fleeing
the instant it is satisfied, and lost about 17 points. This design differs on
each point that failed:

1. **The objective is untouched.** `flee`, `range_band`, `los_seek`, kite
   hysteresis, the wand fallback and the seek-chase bucket all stay. The
   change is one hard mask in `argmax_interest`. That is the shape the same
   decision named as right ("`argmax_interest` is already
   hard-constraints-plus-continuous-objective"), and the one the healer's
   anchor mask has used since the context-steering refactor.
2. **It is temporal, not standing.** It is set only while the pair's yield
   says so, the way `OccupyCover`'s sight requirement is conditioned on
   castability. A healthy, unthreatened kiter with its healer following runs
   today's scorer.
3. **It never removes every escape.** When no step keeps sight, the ladder
   drops the mask. A kiter fleeing a melee round a pillar is never pinned.
4. **Melee is not constrained at all, only steered.** It still chases; the
   change is which way round the pillar, and a hold only while dying.
5. **It has a guard.** The kiter's escape metric (`movement_probes`' KITE
   separation gained) must not regress (§7).

### 3.5 The Priest's offensive scream dip (was AS-384)

**What the data says.**

- Under `TeamPlan` the dip never feared the enemy healer (0 of 560).
- In the camp it never completed (72 entries, 0 completions).
- Its value was the walk itself: it tipped the Priest into PRESSURED, where
  the `OccupyCover` solve runs. AS-383's gate keeps the Priest FREE (31.1s
  against 14.4s from the gates), and the camp paid for it: heal to the Warrior
  307 → 137 with the gate. With AS-373 merged it stands at 157, and 10 of 12
  won.

**In the team model the dip has no purpose of its own. Its two parts belong to
two plays:**

1. **The walk's accidental value (the solve) belongs to the stance.** The
   stance gives the healer the ring and the solve's terms whatever its
   posture. A walk toward the enemy healer to trigger PRESSURED is then
   neither needed nor wanted.
2. **The offensive Scream belongs to the Peel play.** Psychic Scream is
   already the Priest's peel tool (`peel::peel_tools`): while
   `ally_in_danger` names a teammate, the peel step Screams one of its
   `danger_attackers` already inside the Scream. The Priest does not walk to
   bring one into range. A walk at an attacker beyond the Scream measured at
   or below no walk in every form (§9, D5): a ranged attacker crowd-controls
   the Priest on the way in, and a short walk trades a heal for the Scream.
3. **The standalone dip is retired under `TeamPlan`.** `Legacy` keeps its dip
   byte-identically.

Should the team call the Priest forward to enter PRESSURED? No. PRESSURED is a
threat response, and calling it on purpose is how the camp's value became an
accident. The stance is the call.

### 3.6 Profiles

Everything is **`TeamPlan` only. `Legacy` stays byte-identical**:

- the yield situation, and the stance situation and play;
- the kiter's mask and the pursuit's sight side;
- the dip's retirement.

Every new branch is reached only through `CombatContext::pillar_modes`, the
play set (`SituationConfig::runs`) or `ai_profile.is_team_plan()`, as AS-373's
are.

Measurement switches for the head-to-heads:

- `team{1,2}_withheld_plays: ["Pillar"]` (the stance);
- a new `team{1,2}_sight_yield: false`, in the `team{1,2}_pillar_modes` style
  (the cohesion rule).

With both switched off, a `TeamPlan` match plays byte-identically to `main`.

## 4. What is not proposed

- **Switching a ranged DPS off a hidden healer.** §2.2 shows the real cost,
  but an alternative stands in sight only 7% of that time on Nagrand. It is
  the offence against a pillar (seek-chase, kill-target choice), a follow-up.
- **"Stall for mana".** It has nothing to buy (§2.4). `Stall` is health over
  time.
- **A per-healer hug outside the stance.** It goes (D3).
- **Changing `Legacy`.** Its pillar deaths are rare on Nagrand (3 in 886). The
  problem is `TeamPlan`'s.

## 5. Not strobing, collected

| source | guard |
|---|---|
| yield flips on health | Schmitt 80/90; `SIGHT_YIELD_DWELL` 2s |
| yield flips on pressure or danger | `ally_in_danger`'s dwell and release |
| stance flips | AS-373's Schmitt pair (12/20yd), `MODE_DWELL` 2s, `HUG_RELEASE` 3s, sticky pillar (`PILLAR_SWITCH`) |
| melee side flips | the held `PursuitSide` |
| kiter mask flicker at a shadow edge | the mask tests a 2yd lookahead; `commitment_bonus` keeps the heading; the ladder drops the mask only when every step violates it |
| stance enter and leave on a dying teammate | "dying with no sighted arc spot" is feasibility (immediate), but re-entry waits the dwell |

All of these are traced, so each one's count is a §7 metric.

## 6. The camp, specifically

Today the camp Warrior is blind to its Priest 18% of the time (79% by its own
step), and both losses are Warrior deaths behind the pillar. Under this
design, after contact:

- the Warlock's Fear makes the stance `AvoidCc`;
- the Priest hugs on the team arc;
- the Warrior yields (rule 3), chasing round the Priest's side;
- the Priest has the ring and the solve's terms without walking into
  PRESSURED.

The figures to beat are AS-383's camp: 10/12, heal to the Warrior 157. The
earlier 307 is the target to recover toward.

## 7. Measurement plan for the build

**Tests, written before tuning**, each a pair with its control (one condition
removed):

- **Scenes:**
  - the yield rule's five cases (a CC'd healer; a pressured healer; the
    stance; a hurt DPS with the 80/90 edge held across a tick; a healthy DPS
    leading);
  - the stance keys (a Warlock with Fear ready at a pillar enters `AvoidCc`;
    the Warlock silenced does not; a Rogue alone does not; an empty team arc
    does not);
  - the `Stall` table and gates (dampening, enemy DoT on us, kill window).
- **Staged matches:**
  - *The yield:* a hurt Warrior chases its target round a Nagrand pillar
    while its Priest stands open. It takes the sighted side and the Priest
    never loses it for more than a GCD. Pair: `sight_yield: false`
    reproduces the blind spell.
  - *The kiter:* a hurt Mage kiting a Rogue past a pillar does not put its
    Priest in shadow, and its separation from the Rogue is within 1yd of the
    pair's. Pair: the mask off.
  - *The stance:* Warrior+Priest against Warlock+Priest, staged at contact by
    the camp pillar. The whole team ends on the far side, mutually sighted,
    within 5s. Pair: `Pillar` withheld.
  - *The dip:* `Legacy` dips at an unheld enemy healer; `TeamPlan` does not.
    The Peel step still Screams an attacker in range, and not one beyond it.
- **Mutation-checked:** no held pursuit side, no yield dwell, no team arc
  (the anchor arc instead), and the kiter mask on a blind DPS. Each must fail
  a named test.

**Mechanism metrics, per piece.** This card's harness and bank, before and
after, `TeamPlan`, per map:

| piece | metric (§2) | today, Nagrand 2v2 / TwinPillars 2v2 | target |
|---|---|---|---|
| yield | DPS blind to its healer | 11.8% / 11.7% | halved |
| yield | the same while dying | 6.4% / 9.8% | at most `Legacy`'s Nagrand (1.2%) on Nagrand |
| yield | episodes broken by the DPS's step | 79% / 54% | below 50% |
| yield | ranged DPS lining (its target) | Mage 26.6% | no rise of more than 1pt |
| kiter mask | KITE separation (`movement_probes`) | pinned | no regression |
| stance | team all hidden *and* mutually sighted | 27.9% / 26.1% | up |
| stance | healer hidden with a DPS exposed | 10.9% / 22.6% | halved |
| stance | hard-CC share per healer × map cell (AS-373 table) | §11.2 there | no cell up |
| stance | pillar deaths, by cause | 35 (4 / 5 / 10 / 1 / 15) / 55 | down overall; DPS-step and joint causes down |
| stance | stance changes per minute, dwell distribution | new | dwell ≥ 2s by construction; report |
| stall | lull net health per comp in the table | §2.4 | in-stance lulls at or above the table's |
| all | `camp_sweep` | 10/12, heal 157 | heal up; occlusion not down |

**The package (win rate).** DIRECTIONAL tier, paired McNemar on flips, both
seat assignments, every 2v2 healer cell plus the six one-healer 3v3 teams, on
Nagrand and TwinPillars, on fresh seeds:

1. **`TeamPlan` with the package against `TeamPlan` without it** (yield off,
   Pillar withheld). The AS-373 figures to beat are its modes-on-against-off
   result: TwinPillars -0.8pt (z -2.37) and Shaman -1.3pt (z -2.00). The
   success condition: no significant loss overall, on either map, or in any
   healer class, and TwinPillars and the Shaman at or above zero.
2. **`TeamPlan` against `Legacy`, before and after.** The baseline the
   milestone review reads.
3. **Per piece, directional only, not a ship criterion:** the yield alone, the
   stance alone. These say which piece moved the package. They do not gate a
   piece on its own, per the user's rule.

## 8. Build sequencing

Three cards, with one ordering constraint:

| card | contents | depends on |
|---|---|---|
| **A. Sight yield** | `sight_yield` situation, kiter mask, melee sight side with held `PursuitSide`, switch `team{1,2}_sight_yield` | none |
| **B. Team pillar stance** | `pillar_stance` situation, `Pillar` play, `AvoidCc` keys, team arc, AS-373's preference keys moved under it, the ranged pull to the side; `Stall` behind its own exceptions table, shipped empty (D4) | A's rule 3 reads the stance; B can be built in parallel and integrated after A |
| **C. Retire the Priest's dip under `TeamPlan`** | the dip's retirement, the camp re-measure | none in code; measured with B, because the camp's value moves to the stance |

- **A and C can be built in parallel. B can start in parallel.**
- **One package measurement covers all three**, run once A, B and C are
  merged onto one integration branch. The per-piece head-to-heads are run on
  that branch with each switch. Per the user's rule, no card's win rate is
  judged alone, so the cards merge together after the package passes.
- **`Stall` is a fourth card (D)** only if D4 is answered "build it". The
  table is ready for it (§2.4).

## 9. Decisions for the user

**D1. The yield rule (§3.1), including widening the DPS's duty from "in
danger" to "hurt".** *Recommended: yes.* The "in danger" gate reaches about a
twentieth of the DPS's blind time on Nagrand (§2.1). The order is: a unit that
cannot move leads; a pressured healer leads; the stance leads; a hurt DPS
(80/90) yields; otherwise the DPS leads. The alternative is the narrow "in
danger" gate, which leaves the camp's losses (both Warrior deaths behind the
pillar, sight broken while it was still healthy enough to chase).

**D2. Pricing (§3.2).** *Recommended: yes.* Sight of an owed duty outranks
hiding from CC for every unit, healers and DPS alike, and CC is avoided by
hiding only through the team stance. This extends AS-381's ruling to the DPS
unchanged. The alternative is to let a single unit trade sight for CC by
expected CC seconds. That is a weighting the measurement argues against:
lining already costs a `TeamPlan` ranged DPS three to four times its hard-CC
time.

**D3. The healer's hug only under the team stance.** *Recommended: yes.*
AS-373's per-healer preference keys are replaced by the stance; its
feasibility key, ring and PRESSURED terms stay. No healer pillars alone. The
alternative is to keep the per-healer hug as a fallback when no team arc
exists. That is the "one combatant in isolation" the user rejected, and §2.3
measures its cost: a DPS exposed during 54% of hug time.

**D4. `Stall` in the first build?** *Recommended: no. Build `AvoidCc` first,
with `Stall` behind an exceptions table shipped empty.* The stall's measured
value is real but narrow: up to about +10% team health per 10s for
Warrior+Druid, +3% for Paladin comps, nothing for Shaman comps, nothing after
dampening. That is too narrow to justify its gates (dampening, DoTs, trailing
tempo) until `AvoidCc` has shown the stance mechanism works. The alternative
is to build both now, with the comp table as the `Stall` key.

**D5. The Priest's dip (§3.5).** *Ruled 2026-10-24: the standalone dip is
retired under `TeamPlan`, with no walk in its place.* The offensive Scream is
the Peel step's, on an attacker already inside it. The camp's PRESSURED-solve
value moves to the stance. The rejected alternatives are a team-called "go
forward" that enters PRESSURED on purpose (a threat response made into a
plan, the accident the camp ran on), and a walk at the dying DPS's attacker.
The walk was measured against no walk, Priest cells against `Legacy`, seeds
501-512: -0.5pt (z -3.6), and -0.15 to -0.2pt for every narrowed form (melee
attackers only, one global cooldown of travel, the target's ranged crowd
control gated); `docs/design/balance/2026-10-24-as388-scream-dip-retired.md`.
The walk's open questions are closed as measured: deferring non-critical heals
during it made no difference, and exempting the caught enemy healer's crowd
control was worse.

**D6. Sequencing (§8).** *Recommended:* three cards (yield, stance, dip
retirement), built in parallel where possible and merged together after one package
measurement. The alternative is serial merges, each judged on its own win
rate. That is what the user's measurement rule declines.

**D7. The ranged DPS's blindness to a hidden enemy healer** (§2.2: 20-27% of
its time under `TeamPlan`). *Recommended: a follow-up card, not this one.* It
is the offence against a pillar, not cohesion. This design keeps the stance
inside the yield mask, so it cannot make it worse.
