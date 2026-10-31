# AS-399: the team target focus's balance shift — diagnosis and tuning options

**Card:** AS-399. **Status: DESIGN — awaiting the user's ruling (§6).** Nothing
is built; the options in §5 are measurement arms, committed as a patch.
**Measured commit:** `51b8101d` (main after E0, AS-303; with Holy Light,
Shadow Word: Pain, Life Tap, the Rogue poisons, dodge/parry). Binaries:
main `6fe2c208` (shasum of `target/release/arenasim`), option arms `f56734a5`
(main plus `sweeps/2026-10-31-as399-options.patch`; with `AS399_OPT` empty it
plays 600 matches of the option bank byte-identical to main's CSV).
**Tier.** §1 is the 0.8 sweep's §4.1 design re-run on the post-E0 main, at
the E0 seeds (2v2 12 seeds, 3v3 3 seeds), so it is cited the way E0's tables
are. §5 is **DIRECTIONAL** (`sweep-tiers.md`): one seed per block on the
30 × 30 2v2 cells. `TeamPlan` only; `Legacy` is untouched by every arm.
**Replication.** Every effect is quoted with its three seed blocks (2v2
801-804 / 811-814 / 821-824, options 801 / 811 / 821; 3v3 801 / 811 / 821)
and is called an effect only when all three agree in sign and the pooled
McNemar |z| ≥ 1.96 (`*`).

## Verdict

1. **The shift is still there, slightly smaller than at 0.8** (§1). 2v2
   competitive on the pillar maps: Shaman −5.8, Rogue −5.3, Druid −3.4,
   Priest −3.1; Hunter +5.9, Warlock +4.4, Mage +4.1; no-healer teams +6.8.
   3v3: Rogue −9.4, Shaman −4.7, Druid −3.7, Paladin −2.3, Hunter +10.0.
   Head-to-head the focus is worth **+5.1** (AS-389 measured +4.4 at merge,
   on its own bank).
2. **Mechanism: the focus takes away a pillar-hiding healer's free defence**
   (§2). Without the focus, a ranged DPS whose target (usually the enemy
   healer) went behind a pillar kept that target and did nothing; the
   healer's pillar protected its partner too. With it, the DPS shoot the
   partner the healer left in the open. 72% of focus moves are off a hidden
   healer, and the new target is its melee partner in 49% of the time held.
   The loser is the **healer + melee team**: its paired win rate falls by
   −10.6 (−13.6 against a healer + ranged team, −15.1 against melee + ranged,
   −21.6 against two ranged). Its partner dies first 30% of the time instead
   of 20%. The Rogue loses because Rogue + healer is the strongest
   healer + melee team. The double-ranged and melee + ranged teams gain
   because a healer team that hides now loses its exposed DPS.
3. **Two of the card's three suspects are not the cause** (§2.3). The focus
   rarely pulls DPS onto healers: healers are the focus's subject in only 15% of
   moves. Removing the convergence changes nothing (`solo`, §5). The Rogue is not
   being dropped while out of sight; it is the target the focus moves onto.
4. **The shift is a correction. It brings the standings together** (§3).
   Before the focus, healer + melee teams won 63.5% of their pillar-map
   matches and double-ranged teams 42.9%; with it, 52.8% and 52.7%. The mean distance of the
   class standings from 50% falls from 3.5 to 2.8pt (2v2, Paladin aside) and
   from 5.2 to 4.4 (3v3). The healers go from above 50% to below it. The
   double-ranged gain is from the weakest classes (Hunter 41.4%, Warlock 43.3%).
5. **No focus tuning gives the healers back without taking it from the
   Hunter and the Warlock** (§5). Measured directionally:
   - **`return`** costs 1.8pt of head-to-head value. It recovers about a third
     of the healers' loss, and halves the Mage + Warlock gain.
   - **A no-healer exception** fixes the Shaman and the canaries. It halves the
     head-to-head value and leaves the standings more spread than the shipped
     focus does.
   - **The two together** are close to switching the focus off.
   - **A healer-side answer**, anchoring the healer on the teammate the
     enemy shoots, is inert as a one-line change.
6. **Recommendation:** keep the focus as shipped. Answer the Druid, the
   Shaman, and Mage + Warlock with class tuning in 0.9. Open a design card for the
   healer that leaves its partner behind a pillar (§6).

## 1. The shift on current main

Uniform, `TeamPlan`: every side with the focus on for both teams (`uni-TT`)
against its twin with it off for both (`focus-off`), 2v2 the full 36 × 36 on
Nagrand and TwinPillars at 12 seeds, 3v3 the `sample3` cells at 3 seeds
(69,408 matches); head-to-head, the 30 × 30 cells, one team running it, both
seats, 6 seeds (21,600 matches). Competitive sides, pillar maps.

| class | 2v2 | per block | 0.8 | 3v3 | per block | 0.8 |
|---|---|---|---|---|---|---|
| Shaman | **−5.8*** | −6.3 / −6.5 / −4.7 | −7.2 | **−4.7*** | −6.0 / −3.5 / −4.5 | −5.0 |
| Rogue | **−5.3*** | −5.3 / −5.5 / −5.2 | −6.6 | **−9.4*** | −8.9 / −8.9 / −10.2 | −9.4 |
| Druid | **−3.4*** | −2.8 / −4.5 / −3.0 | −3.5 | **−3.7*** | −5.0 / −2.9 / −3.2 | −3.3 |
| Priest | **−3.1*** | −3.1 / −3.0 / −3.3 | −4.6 | −0.3 | | −0.4 |
| Warrior | **−0.8*** | −1.3 / −1.0 / −0.2 | −0.8 | −0.6 | | −1.5 |
| Paladin | −0.7 | | +0.4 | **−2.3*** | −1.5 / −3.6 / −1.9 | −2.6 |
| Mage | **+4.1*** | +3.9 / +4.7 / +3.6 | +5.1 | **+3.7*** | +2.8 / +4.7 / +3.6 | +5.2 |
| Warlock | **+4.4*** | +5.3 / +4.3 / +3.7 | +4.4 | **+4.2*** | +4.8 / +2.2 / +5.7 | +4.6 |
| Hunter | **+5.9*** | +5.3 / +6.8 / +5.7 | +7.1 | **+10.0*** | +10.0 / +10.3 / +9.6 | +9.6 |

2v2 n = 11,520 sides per DPS class, 7,200 per healer; floors 0.7-1.0pt.
3v3 n = 2,082-2,652; floors 1.8-2.1pt.

| no-healer 2v2 team (every opponent) | off | on | delta | per block |
|---|---|---|---|---|
| all no-healer teams | 44.4 | 51.1 | **+6.8*** | +6.5 / +7.5 / +6.2 |
| Mage + Hunter | 42.7 | 53.0 | **+10.3*** | +6.9 / +13.5 / +10.4 |
| Warlock + Hunter | 36.1 | 48.7 | **+12.6*** | +13.4 / +15.5 / +9.0 |
| Mage + Warlock | 60.7 | **66.6** | **+5.9*** | +8.2 / +5.9 / +3.6 |

**Head-to-head** (the side running it against the same side without it):
**+5.1*** (+5.2 / +5.1 / +4.9, z=24.0); Nagrand +2.6, TwinPillars +7.5;
Hunter +9.5, Warlock +7.8, Priest +7.0, Mage +6.0, Warrior +4.2, Paladin +3.2,
Shaman +3.2, Rogue +1.4, Druid +1.1. Every class gains by running it.

Against 0.8 the Shaman's, the Rogue's, the Priest's and the Hunter's 2v2
shifts are 1.2-1.5pt smaller, and the rest within a point. The two runs are on
different code and are not paired.
BasicArena is inert (the focus needs obstacles) and is not re-run.
Non-vacuity: 4,999 of the 31,104 2v2 matches change winner between the arms;
draws are 7.7% off and 7.3% on, mostly the double-healer cells, as at E0.

## 2. Mechanism

### 2.1 What the focus does inside a match

`2026-10-31-as399-mechanism.py` plays the 30 × 30 cells on both pillar maps at
seed 801, focus on and off, and reduces each match's log and trace (1,800
paired matches; descriptive, one seed).

**Who the focus moves off, and onto.** 868 of 1,800 matches have a focus.

| moved off (the hidden target) | onto | moves | share | time held |
|---|---|---|---|---|
| healer | melee DPS | 495 | 40.7% | 48.6% |
| healer | ranged DPS | 377 | 31.0% | 32.8% |
| ranged DPS | healer | 111 | 9.1% | 4.5% |
| ranged DPS | melee DPS | 101 | 8.3% | 5.6% |
| melee DPS | healer | 75 | 6.2% | 5.5% |
| other | | 57 | 4.7% | 3.0% |

The top class pairs are every healer onto the Rogue (Druid 82, Paladin 78,
Priest 77, Shaman 68), then every healer onto the Warrior.

**The hidden healer is fleeing.** In a spot check, a healer + melee team
against a healer + ranged team, both pillar maps, 96 matches
(`2026-10-31-as399-posture.py`), the enemy focus moved off one of its units 90
times. That unit's last movement decision was PRESSURED in 54 of those moves
and ESCAPE in 18. In 5 it was FREE, and in 13 it had made no posture decision.
The healer was retreating behind cover from the DPS shooting it, and its
partner was left in the open.

**Where the enemy's ranged damage goes.** This is the share of the enemy
ranged DPS's damage that lands on each unit of a side (the Hunter's pet is left out).

| side | healer, off → on | its DPS, off → on | ranged damage taken, off → on |
|---|---|---|---|
| healer + melee | 55 → 43% | 45 → 57% | 252k → 315k (+25%) |
| healer + ranged | 47 → 39% | 53 → 61% | 548k → 575k (+5%) |

The focus turns idle time into damage on the partner the healer left
exposed.

**Who dies first, and how often the side loses** (side vs any opponent):

| side | losses off → on | first death: healer | first death: its DPS |
|---|---|---|---|
| healer + melee | 37.6 → 46.9% | 18.9 → 14.9% | 19.7 → 30.3% |
| healer + ranged | 51.3 → 49.7% | 11.5 → 7.9% | 38.6 → 41.3% |
| melee + ranged | 56.4 → 50.8% | — | 55.9 → 52.9% |
| two ranged | 56.1 → 48.6% | — | 60.6 → 52.5% |

### 2.2 By team shape, paired and replicated

The §1 bank sliced by team shape (H a healer, M a melee DPS, R a ranged DPS;
double healers left out). Each row is the side's win rate, off → on.

| side | vs H+M | vs H+R | vs M+R | vs R+R | vs any |
|---|---|---|---|---|---|
| H+M | 49.1 → 49.1 | 74.4 → 60.8 (**−13.6***) | 61.4 → 46.3 (**−15.1***) | 71.6 → 50.0 (**−21.6***) | 63.5 → 52.8 (**−10.6***) |
| H+R | 25.4 → 38.9 (**+13.5***) | 49.0 → 48.9 | 66.1 → 60.1 (**−6.0***) | 64.7 → 57.2 (**−7.5***) | 48.4 → 50.1 (**+1.6***) |
| M+R | 37.8 → 53.4 (**+15.7***) | 33.9 → 39.9 (**+6.0***) | 49.0 → 49.0 | 31.4 → 26.9 (**−4.5***) | 39.2 → 45.3 (**+6.1***) |
| R+R | 28.4 → 50.0 (**+21.6***) | 35.3 → 42.7 (**+7.4***) | 67.8 → 72.7 (**+4.9***) | 48.6 → 49.5 | 42.9 → 52.7 (**+9.8***) |
| M+M | | | | | 35.8 → 35.3 |

The mirrors and every pairing with no ranged DPS on either side do not move:
the focus needs a ranged DPS to act, and a hidden target to act on.

### 2.3 The card's three suspects

- **"The focus pulling DPS onto healers."** No. A healer is the focus's
  subject in 15% of moves and 10% of its time. The healer is what the focus
  moves *off*.
- **"Double-ranged comps converging fire."** No. `solo` (§5) lets only a DPS
  blind to its own target take the focus, so the second DPS never converges.
  Against the shipped focus it moves every class by under 0.5pt and the
  head-to-head not at all. The
  double-ranged gain is the same mechanism as the rest: both DPS can punish
  an exposed partner.
- **"Rogue targets out of sight being dropped."** No. A melee DPS is the
  hidden target in 7% of moves. The Rogue is the subject of 362 of the 1,216
  moves (30%), the most common subject; the Warrior is next at 241.

## 3. The standings it leaves

Class win rates, 2v2 competitive, pillar maps, focus off → on (the §1 bank):

| | Warrior | Mage | Rogue | Priest | Warlock | Paladin | Hunter | Shaman | Druid | mean \|x − 50\| (no Paladin) |
|---|---|---|---|---|---|---|---|---|---|---|
| off | 47.5 | 49.5 | 54.4 | 53.1 | 43.3 | 64.4 | 41.4 | 51.1 | 49.2 | 3.46 |
| on | 46.6 | 53.6 | 49.0 | 49.9 | 47.7 | 63.7 | 47.3 | 45.3 | 45.8 | **2.75** |

3v3 (`sample3`): mean |x − 50| 5.18 → **4.41** without the Paladin; Rogue
59.7 → 50.3, Hunter 37.5 → 47.5, Shaman 44.6 → 40.0, Druid 46.9 → 43.2.

The healers' "drop" is a fall from above 50% (Priest 53.1, Shaman 51.1) to
below it, and the Rogue's from 54.4 to 49.0. What they lose is an advantage
the AI gave them: an enemy that stood still while their healer hid. What
stands out after the focus is the **Druid and the Shaman** (45-46% in 2v2,
40-43% in 3v3) and **Mage + Warlock** (66.6% against every opponent). Both are
class-strength questions the focus exposes rather than creates.

## 4. What a tuning can and cannot do

The focus's shift is zero-sum across a single mechanism: the time a ranged DPS
used to waste on a hidden healer. Whatever a focus tuning gives back to the
healer + melee teams, it takes from the ranged DPS classes that were weakest
before the focus. So a tuning should be judged by three things:

- the head-to-head value it keeps;
- the standings spread it leaves;
- the outliers it fixes.

The healers' shift alone is not enough to judge it by.

## 5. Options, measured (DIRECTIONAL)

The option bank (`2026-10-31-as399-gen.py opt`): the 30 × 30 2v2 cells on both
pillar maps, seeds 801 / 811 / 821, uniform on, uniform off and both
head-to-head seats, 21,600 matches per arm. Main's row is main's own figures
on the same rows. Every option's focus-off rows are identical to main's, except
`trained`, which acts with the focus off too (127 of 5,400 differ).

| arm | Shaman | Druid | Priest | Rogue | Hunter | Warlock | Mage | no-healer | M+W team | head-to-head | spread (no Pal) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| focus off | 51.9 | 47.9 | 52.3 | 54.8 | 41.2 | 42.9 | 50.8 | 40.5 | 55.8 | — | 3.83 |
| **shipped** | −5.6* | −3.5* | −2.5* | −4.9* | +5.6* | +4.3* | +3.5* | +6.3* | +6.7* | **+4.8*** | **2.88** |
| `return` | −4.1* | −1.1 | −3.6* | −3.7* | +4.1* | +2.4* | +2.0* | +4.2* | +0.3 | **+3.0*** | **2.79** |
| `nohealer` | +0.0 | −0.8 | +1.7 | −4.1* | +2.3* | +1.1* | +1.2* | −0.5* | −0.6 | **+2.3*** | 3.49 |
| `nohealer` + `return` | +0.7 | −0.1 | −0.3 | −2.8* | +0.9 | +1.1* | +0.8 | −0.4 | −0.8 | **+1.4*** | 3.45 |
| `solo` | −5.8* | −3.4* | −2.5* | −4.9* | +5.6* | +4.7* | +3.8* | +6.6* | +9.4* | **+4.8*** | 2.93 |
| `trained` | −5.4* | −3.6* | −2.4* | −4.8* | +5.5* | +4.2* | +3.5* | +6.2* | +6.4* | **+4.8*** | 2.90 |

The class columns are each arm's uniform shift (on against off) in 2v2
competitive on the pillar maps. "M+W team" is Mage + Warlock against every
opponent. Head-to-head is the side running the arm's focus against the same
side with it off. Spread is the mean |level − 50| over the classes, Paladin
aside, with the arm's focus on. Floors: 1.4-2.1pt per class, 0.4-0.6pt
head-to-head; the Mage + Warlock column is n=360 (floor ~5pt).

What each option is:

- **`return`.** The focus returns to the target it left as soon as that target
  is back in a ranged DPS's sight and reach, after the 2s dwell. Shipped, it
  returns only once the focus itself has been hidden for 1.5s. A returned
  focus is not returned again. So the DPS shoot the partner while the healer
  hides and go back to the healer when it steps out to heal.
  - Healer + melee loses −6.4 instead of −9.9: −7.7 against healer + ranged,
    −8.3 against two ranged.
  - The head-to-head keeps +3.0 of +4.8, and the standings spread is the
    lowest of any arm.
  - The Druid's shift goes (−1.1, ns). The Shaman and the Priest keep most of
    theirs.
  - Mage + Warlock no longer gains (+0.3).
- **`nohealer`.** A team that fields no healer runs no focus: a
  composition exception in the house style (a named table, like the plays').
  - The no-healer and Mage + Warlock gains go. The Shaman and the Priest come
    back to their focus-off level, and the Druid nearly.
  - The Rogue keeps −4.1, because its loss is mostly against healer + ranged
    teams, which still focus.
  - The head-to-head halves (+2.3), since no-healer sides gain nothing.
  - The Hunter and the Warlock fall back toward 43-44%, so the spread is
    worse than shipped (3.49).
- **`nohealer` + `return`.** Healers fully back, the Rogue −2.8, the
  head-to-head +1.4, and the spread 3.45: most of the way to switching the
  focus off.
- **`solo`.** Only a DPS blind to its own target takes the focus, so there is
  no convergence. It makes no difference, except Mage + Warlock +2.8
  (n=360). This refutes the convergence suspect.
- **`trained`.** A healer's `TeamPlan` team solve treats a teammate that an
  enemy ranged DPS is shooting in sight as an anchor it must reach. This is
  the healer-side answer to §2.
  - It changes 41 of 10,800 uniform matches and moves nothing.
  - The anchor lives in the PRESSURED solve, and the healer it should hold
    has just been released from pressure: the enemy stopped shooting it.
  - A healer-side answer is a positioning design (pillar hug and FREE
    formation), not a one-line anchor. See §6.

The 3v3 shift of the options was not measured, and neither was a longer hidden
delay (`hidden3` is in the patch, unplayed: the AS-389 prototype measured
0.5s and 2.0s within half a point of each other).

## 6. Proposal and the question

**Recommended: A — keep the focus as shipped**, and treat what it exposes as
class balance:

- **The Druid and the Shaman** at 45-46% in 2v2 and 40-43% in 3v3 belong to
  the 0.9 talent and gear cards, as the card's coordination note already says.
- **Mage + Warlock** at 66.6% is a comp outlier. It was 60.7% with the focus
  off, so the focus adds about 6pt. If it needs a lever, `return` removes the
  focus's share of it (+0.3) at a cost of 1.8pt of head-to-head value.
- **A new design card**: a healer that retreats behind a pillar leaves its
  partner exposed. This is the AS-362 / AS-373 territory (CC avoidance and
  pillar modes), and §5's `trained` shows the PRESSURED solve is the wrong
  place for the fix.

**B — ship `return`.** Smaller healer, Rogue and double-ranged shifts; the
lowest spread of any arm; Mage + Warlock no longer gains; head-to-head +4.8 →
+3.0. A directional measurement; it needs the authority re-run and its 3v3
before it graduates.

**C — a no-healer exception (`nohealer`), with or without `return`.** Fixes
the healers by giving back the double-ranged teams' gain. It halves the
head-to-head value or worse and leaves the standings more spread than the
shipped focus does. Not recommended.

**Question for the user:** accept the shift as the correction §3 shows it to be,
and leave the healers to class tuning (A)? Or tune the focus with `return` (B)
or a no-healer exception (C)?

## 7. Recompute

From `docs/design/balance/sweeps/`:

| figures | command |
|---|---|
| §1, §2.2, §3 | `2026-10-31-as399-score.py focus 2026-10-31-as399-main.csv.gz` |
| §2.1 | `2026-10-31-as399-mechanism.py report 2026-10-31-as399-mechanism.rows.jsonl.gz` (gunzip first) |
| §2.1 posture spot check | `2026-10-31-as399-posture.py <main binary> <scratch dir>` (re-plays 96 matches) |
| §5 | `2026-10-31-as399-score.py options main=2026-10-31-as399-main.csv.gz trained=...-opt-trained.csv.gz nohealer=...-opt-nohealer.csv.gz solo=...-opt-solo.csv.gz return=...-opt-return.csv.gz nohealer+return=...-opt-nohealer-return.csv.gz` |

**Banks** (`2026-10-31-as399-gen.py`):

- §1: `uni <map> on|off 801-804,811-814,821-824`, `uni3 <map> on|off 801,811,821`
  and `fh <map> 801-802,811-812,821-822`, on PillaredArena and TwinPillars:
  91,008 matches, `--jobs 3`.
- §2.1: the `opt` bank at seed 801 without its `fh` rows, through
  `2026-10-31-as399-mechanism.py run`.
- §5: `opt <map> 801,811,821`, played by the patched binary with
  `AS399_OPT=<option>` (`nohealer,return` for the pair).

The patch applies to `51b8101d`. It adds a field to `Focus`, so the crate's
own unit tests do not compile with it; build the binary only.

## Gaps

- §5 is one seed per block on 2v2 only. The option table's class figures have
  floors of 1.4-2.1pt, and no option was played in 3v3.
- The §2.1 tables are one seed and descriptive. The posture spot check is 90
  focus moves in one matchup shape.
- `trained` being inert says the PRESSURED solve is the wrong place. It does
  not say a healer-side answer would fail.
