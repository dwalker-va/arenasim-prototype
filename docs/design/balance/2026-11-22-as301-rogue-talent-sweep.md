# AS-301: Rogue reference builds, node arms and Shadow Dance

Tier: **directional**. Success condition (design §4, "Sweeps measure nodes and
reference builds"): **no node dominates**. No node may move the Rogue by more
than the run's resolution floor beyond a stat rank of the same price.

**Verdict: the condition fails on three nodes. All three replicate, and all
three clear the Bonferroni threshold on their pooled margin.**

- **Cheat Death** beats a stat rank of its price by +6.5pt pooled over two
  seed blocks (margin z=23.3, floor 0.6).
- **Hemorrhage** beats a stat rank of its price by +1.6pt (margin z=6.4,
  floor 0.6). Its margin clears the threshold in both blocks separately.
- **Swift Blades** beats a stat rank of its price by +1.7pt (margin z=4.1,
  floor 0.9).
  - It is over the floor in both blocks, but clears the threshold only in
    block 1 (z=3.6; block 2 alone z=2.2).
  - Its verdict does not rest on the price model: its raw +1.0pt already beats
    a full Vitality, at 1.85 ranks, by more than the floor.

**Shadow Dance does not dominate.** It sits 0.9pt *below* a stat rank of its
price (margin z=−3.4, replicated).

- A Dance applies 9.1 stun-seconds.
- The Dance raises the chance its first victim is dead by the end of its
  window from 70% (the same matches with Vitality in its place) to 76%.
  Nearly all of that comes from Dances on the enemy healer.
- In the same twin an enemy is dead by then 81% of the time, and the matches
  with a Dance are won about as often without it.

Rupture, Expose Armor and Shadow Dance are significantly **below** a stat
rank of their price. Lightning Reflexes and Deflection are the same node in
effect: their arms played byte-identical matches.

Nothing here is retuned. Tuning is AS-441's batch, and the Dance's counterplay
is the user's call (see *What it says*).

Measured at `main` 27d76940 plus this card's three reference builds. They are
data only, and leave untalented play byte-identical (see *Byte-identity*).
`main` f2b4dc25, merged since, reproduces the committed CSVs row for row (the
`sub` and `sub_vit` arms of block 1, re-run).

## Method

**The reference builds** (`assets/config/talents.ron`, `reference_builds`):
each costs 99 points of item budget.

| name | shape | ranks |
|---|---|---|
| Assassination | 6/2/0 | Improved Poisons 2, Vile Poisons 1, Master Poisoner 1, Rupture 1, **Blood Spatter** 1; Dual Wield Specialization 2 |
| Combat | 2/6/0 | Improved Poisons 2; Lightning Reflexes 2, Deflection 2, Swift Blades 1, **Expose Armor** 1 |
| Subtlety | 0/2/6 | Lightning Reflexes 2; Master of Subtlety 2, Initiative 2, Hemorrhage 1, **Shadow Dance** 1 |

- **Assassination** gives the poisons: more applications, stronger effects and
  a second poison. Its off-tree points make the off-hand swings that carry that
  second poison hit harder. Its arms wear **Deadly Poison** on the off hand,
  with Crippling on the main hand (the default).
- **Combat** is the anti-melee duellist the tree is written as.
- **Subtlety** is the opener and the Dance. Initiative's discount feeds the
  chain's energy.

Each reference's last point is its capstone. Its **stem** is the reference
without the capstone (7 points).

**The comparison is Vitality**, +5% maximum health. It is the Rogue's one
`StatPercent` node with a computed price: 20.4 at the reference Rogue, or 1.85
ranks. Dual Wield Specialization is also `StatPercent`, but its price of 11 is
declared rather than computed: off-hand weapon damage is a free stat on the
item budget. Malice is computed but costs 3 a rank.

Vitality needs two Combat tier-1 points, so every reference holds two and none
holds Vitality. That lets Vitality go on any stem. Every node arm is the node
against **Vitality in its place**:

- **A node the stem lacks:** the stem plus the node, against the stem plus
  Vitality.
- **A node the stem holds:** the stem itself, against the stem with that
  node's point moved to Vitality. This covers Master Poisoner, Rupture, Swift
  Blades, Hemorrhage and Initiative's second rank.
- **A capstone:** the reference, against the stem plus Vitality.

Vitality costs 1.85 ranks, so the allowance scales its measured worth on the
same stem linearly in price. The allowance is `v × (p / 20.4 − 1)`, where `v`
is "stem + Vitality" against "stem" and `p` is the node's price:

- **A one-rank node (11)** is allowed `−0.46 v`. It dominates if it beats
  Vitality by more than `floor − 0.46 v`.
- **A capstone (22)** is allowed `+0.08 v`.

The verdict compares the margin (delta minus allowance) with the pair's floor:

- **dominates** when the margin is above the floor;
- **below** when it is under minus the floor;
- **within** otherwise.

**The margin z** tests the success condition's own null: that the node is
worth a stat rank of its price. It is computed per paired match. For each
one-sided match `i`, with 1 for a Rogue-side win and 0 otherwise:

- `x_i = 100·[(A_i − B_i) − k·(V+_i − V0_i)]`, where:
  - `A` is the node's arm and `B` its comparison;
  - `V+` is the stem plus Vitality and `V0` the stem;
  - `k = p/20.4 − 1`;
- the margin is `mean(x)` and its standard error is `sd(x)/√n`.

The four arms play the same matches, and a node row shares an arm or a stem
with its Vitality row, so the two deltas are correlated. The per-match form
carries that covariance; adding the two rows' errors as if independent does
not.

The raw delta's z, also printed, tests only "equal to Vitality". Significance
below always means the margin z.

| node | arm | comparison |
|---|---|---|
| Lightning Reflexes, Deflection, Master of Subtlety (rank 1 each) | Assassination stem + node | Assassination stem + Vitality |
| Master Poisoner, Rupture | Assassination stem | the stem with the node's point on Vitality |
| Blood Spatter (capstone; a `Patch`, shown for scale) | Assassination | Assassination stem + Vitality |
| Swift Blades | Combat stem | the stem with its point on Vitality |
| Expose Armor (capstone) | Combat | Combat stem + Vitality |
| Initiative (rank 2), Hemorrhage | Subtlety stem | the stem with that point on Vitality |
| Cheat Death | Subtlety stem + Cheat Death | Subtlety stem + Vitality |
| Shadow Dance (capstone) | Subtlety | Subtlety stem + Vitality |

**The sweep.** Every arm is one directional 2v2 sweep, under **`TeamPlan`** on
both sides (what players get):

- **Cells:** every cell a Rogue can reach, plus 8 control cells, 424 cells in
  all. They come from `gen_sweep.py --full 2 --exclude-double-healer --affects
  Rogue --control-cells 8`.
- **Maps:** BasicArena, PillaredArena and TwinPillars, 4 seeds a cell a map.
  That is 5,088 matches an arm.
- **Builds:** every Rogue, on either side, plays the arm's build. Arms
  therefore differ only in the build and pair at identical (label, seed).
- **Scoring:** for the side fielding the Rogue, over the 4,224 matches where
  exactly one side does (`--headline affected`). The 768 Rogue mirrors are
  excluded from the score.
- **Format:** 2v2 only. The directional tier does not call for 3v3.

**Block 1** (seeds 0-3) ran 19 arms: 0 points, the three references, their
three stems, their three stem + Vitality comparisons and nine node arms.
**Block 2** (seeds 4-7) re-ran the 12 arms behind every headline and every
allowance it reads. That is 157,728 matches in all.

**Multiple comparisons.** The threshold is Bonferroni over the table's 12
rows: |margin z| ≥ 2.87 (α=0.05, two-sided). Those rows are:

- the 10 nodes under test;
- Blood Spatter, shown for scale;
- Deflection, which plays the same matches as Lightning Reflexes.

That makes 2.87 conservative. Over the 10 distinct nodes alone the threshold
would be 2.81, which changes no row below.

## Results

In every table, "before" is the comparison arm and "after" is the arm under
test, both as the Rogue side's win rate.

- **Flips** are matches whose winner moved: + toward the Rogue side, − away
  from it.
- **The floor** is the smallest delta the pair's own discordance could
  resolve.
- **BONF** marks a margin z past the threshold.

### Each node against Vitality in its place (block 1)

| node | before | after | delta (pt) | flips | z (delta) | floor | allowance | margin | margin z | verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| **Cheat Death** | 58.5% | 64.9% | **+6.4** | 343 (+306/−36) | 14.55 | 0.9 | −0.6 | +7.0 | +17.12 BONF | **dominates** |
| **Swift Blades** | 52.7% | 54.0% | **+1.3** | 680 (+368/−311) | 2.15 | 1.2 | −0.8 | +2.2 | +3.63 BONF | **dominates** |
| **Hemorrhage** | 56.2% | 57.3% | **+1.0** | 261 (+152/−108) | 2.67 | 0.8 | −0.6 | +1.6 | +4.86 BONF | **dominates** |
| Master Poisoner (Deadly off hand) | 53.4% | 52.2% | −1.2 | 372 (+161/−211) | 2.54 | 0.9 | −1.0 | −0.2 | −0.49 | within |
| Initiative (rank 2) | 58.4% | 57.3% | −1.1 | 133 (+43/−90) | 3.99 | 0.6 | −0.6 | −0.5 | **−3.38 BONF** | within |
| Blood Spatter (capstone, for scale) | 54.3% | 54.0% | −0.4 | 327 (+156/−171) | 0.77 | 0.9 | +0.2 | −0.5 | −1.19 | within |
| Master of Subtlety (rank 1) | 54.3% | 52.6% | −1.8 | 216 (+71/−145) | 4.97 | 0.7 | −1.0 | −0.8 | −2.78 | below |
| Lightning Reflexes (rank 1) | 54.3% | 52.2% | −2.1 | 377 (+144/−232) | 4.49 | 0.9 | −1.0 | −1.1 | −2.56 | below |
| Deflection (rank 1) | 54.3% | 52.2% | −2.1 | 377 (+144/−232) | 4.49 | 0.9 | −1.0 | −1.1 | −2.56 | below |
| Shadow Dance (capstone) | 58.5% | 57.7% | −0.8 | 220 (+93/−127) | 2.22 | 0.7 | +0.1 | −0.9 | −2.47 | below |
| Expose Armor (capstone) | 55.8% | 54.7% | −1.1 | 396 (+175/−221) | 2.26 | 0.9 | +0.1 | −1.2 | −2.55 | below |
| Rupture | 54.9% | 52.2% | −2.7 | 699 (+293/−405) | 4.20 | 1.2 | −1.0 | −1.7 | −2.75 | below |

The Lightning Reflexes and Deflection arms are the same CSV, byte for byte.
In this sim, dodge and parry are one avoidance roll with no facing and no
parry haste (`combat_core/avoidance.rs`). So +4% dodge and +4% parry avoid
exactly the same attacks.

**Verdicts that change at allowance 0**, with the node against a full
Vitality:

- **Block 1:** Master Poisoner and Initiative's second rank go from within to
  **below**.
- **Block 2 alone:** Swift Blades goes from dominates to within.
- **Pooled:** no replicated row changes.

### The replication (seeds 4-7), and pooled

Block 2 re-ran the three nodes over the line, Shadow Dance, and the two
significant "below" rows it reaches. Each row reads its allowance from the
Vitality rank on the same stem in the same block (or pooled).

| comparison | block | before | after | delta (pt) | flips | z (delta) | floor | margin | margin z | verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| Cheat Death | 1 | 58.5% | 64.9% | +6.4 | 343 (+306/−36) | 14.55 | 0.9 | +7.0 | +17.12 BONF | dominates |
| | 2 | 59.1% | 64.5% | +5.5 | 308 (+269/−38) | 13.13 | 0.8 | +6.0 | +15.74 BONF | dominates |
| | **pooled** | 58.8% | 64.7% | **+5.9** | 651 (+575/−74) | 19.63 | 0.6 | **+6.5** | **+23.25 BONF** | **dominates** |
| Hemorrhage | 1 | 56.2% | 57.3% | +1.0 | 261 (+152/−108) | 2.67 | 0.8 | +1.6 | +4.86 BONF | dominates |
| | 2 | 56.9% | 57.9% | +1.0 | 311 (+176/−134) | 2.33 | 0.8 | +1.5 | +4.18 BONF | dominates |
| | **pooled** | 56.6% | 57.6% | **+1.0** | 572 (+328/−242) | 3.56 | 0.6 | **+1.6** | **+6.36 BONF** | **dominates** |
| Swift Blades | 1 | 52.7% | 54.0% | +1.3 | 680 (+368/−311) | 2.15 | 1.2 | +2.2 | +3.63 BONF | dominates |
| | 2 | 54.2% | 54.8% | +0.6 | 692 (+359/−333) | 0.95 | 1.2 | +1.3 | +2.17 | dominates |
| | **pooled** | 53.4% | 54.4% | **+1.0** | 1372 (+727/−644) | 2.21 | 0.9 | **+1.7** | **+4.10 BONF** | **dominates** |
| Shadow Dance | 1 | 58.5% | 57.7% | −0.8 | 220 (+93/−127) | 2.22 | 0.7 | −0.9 | −2.47 | below |
| | 2 | 59.1% | 58.3% | −0.7 | 219 (+93/−124) | 2.04 | 0.7 | −0.8 | −2.28 | below |
| | **pooled** | 58.8% | 58.0% | **−0.8** | 439 (+186/−251) | 3.06 | 0.5 | **−0.9** | **−3.36 BONF** | below |
| Expose Armor | 1 | 55.8% | 54.7% | −1.1 | 396 (+175/−221) | 2.26 | 0.9 | −1.2 | −2.55 | below |
| | 2 | 56.3% | 55.6% | −0.7 | 377 (+174/−203) | 1.44 | 0.9 | −0.8 | −1.71 | within |
| | **pooled** | 56.0% | 55.2% | **−0.9** | 773 (+349/−424) | 2.66 | 0.7 | **−1.0** | **−3.02 BONF** | below |
| Rupture | 1 | 54.9% | 52.2% | −2.7 | 699 (+293/−405) | 4.20 | 1.2 | −1.7 | −2.75 | below |
| | 2 | 56.0% | 52.2% | −3.8 | 676 (+258/−418) | 6.12 | 1.2 | −3.1 | −5.21 BONF | below |
| | **pooled** | 55.4% | 52.2% | **−3.2** | 1375 (+551/−823) | 7.31 | 0.9 | **−2.4** | **−5.60 BONF** | below |

**What clears the threshold:**

- **Pooled:** all six replicated rows. Cheat Death, Hemorrhage and Swift
  Blades dominate; Shadow Dance, Expose Armor and Rupture are below.
- **In both blocks separately:** Cheat Death and Hemorrhage. Swift Blades
  clears in block 1 only, Rupture in block 2 only.
- **Block 1 only, cleared:** Initiative's second rank (margin z −3.38). It is
  significantly below a stat rank of its price, but by less than the floor
  (−0.5pt against 0.6pt), so its verdict stays within.
- **Block 1 only, not cleared:** Lightning Reflexes, Deflection and Master of
  Subtlety (margin z −2.6 to −2.8) are "below" on the floor rule only.

**Swift Blades** dominates by the floor rule in both blocks. Its block 2 margin
(+1.3) is just over that block's floor (1.24), and the margin clears the
threshold pooled and in block 1, not in block 2 alone. Its pooled raw +1.0pt is
already over the 0.9pt floor against a full Vitality. Only an allowance above
zero, an 11-point rank worth more than a 20.4-point one, would bring it back
inside. Block 2 alone is within at allowance 0 (+0.6pt against its 1.2pt
floor).

**Expose Armor** is below pooled (margin z −3.0), but within in block 2 alone.

### What Vitality buys, and each reference against 0 points

| comparison | block | before | after | delta (pt) | flips | z | floor |
|---|---|---|---|---|---|---|---|
| Vitality on the Assassination stem | pooled | 52.2% | 54.1% | +1.8 | 272 (+214/−58) | 9.40 | 0.4 |
| Vitality on the Combat stem | pooled | 54.4% | 56.0% | +1.6 | 284 (+210/−73) | 8.08 | 0.4 |
| Vitality on the Subtlety stem | pooled | 57.6% | 58.8% | +1.2 | 258 (+180/−77) | 6.36 | 0.4 |
| **Assassination 6/2/0** vs 0 points | 1 | 48.9% | 54.0% | **+5.1** | 746 (+481/−265) | 7.87 | 1.3 |
| **Combat 2/6/0** vs 0 points | 1 | 48.9% | 54.7% | **+5.8** | 773 (+509/−262) | 8.86 | 1.3 |
| **Subtlety 0/2/6** vs 0 points | 1 | 48.9% | 57.7% | **+8.9** | 690 (+532/−158) | 14.20 | 1.2 |

The three builds all cost 99, and are worth +5.1 to +8.9pt.

- **The capstones add little.** Each reference against its own stem:
  - Blood Spatter: +1.8pt (z=4.6).
  - Expose Armor: +0.7pt (z=1.5).
  - Shadow Dance: +0.4pt (z=1.6).
- **Subtlety's lead is its stem's.** The stem alone is worth +8.4pt over 0
  points.
- **Cheat Death is the eighth point Subtlety does not take.** On the stem it
  is worth +7.6pt, nearly as much as the other seven points together.

The script's `table` output holds each arm against its own reference.

### Non-vacuity

- **Control:** every arm's 96 control matches are identical in winner and
  duration to the 0-point arm's. Those matches have no Rogue on either side,
  and the control fields all 8 other classes on both sides.
- **Decisive:** 5,066-5,081 of 5,088 matches end by elimination in every arm.

**The nodes fire.** The batch CSV carries outcomes only, so a fixed slice of
Rogue matches is replayed with full logs (`sample`): 60 per arm, and 600 for
the arms below. That slice has 696 Rogues, counting both sides.

| arm (600 matches) | node events |
|---|---|
| Assassination | Deadly Poison procs 956 (off hand), Crippling applied 1,176, Rupture cast 838 |
| Assassination stem, Rupture's point on Vitality | Deadly 940, Rupture 0 |
| Assassination stem + Lightning Reflexes | 155 dodges against 4,399 melee hits taken |
| Combat | Expose Armor cast 429; the Rogue dodges 403 times and parries 345 |
| Subtlety | Shadow Dance cast 226; Hemorrhage cast 3,495; Initiative procs 6,912, of which 5,869 are spent; Master of Subtlety procs 696 (one a Rogue: its stealth break) |
| 0 points | 4,771 melee hits taken |

| arm (60 matches, 69 Rogues) | node events |
|---|---|
| Assassination stem + Lightning Reflexes | 16 dodges (its comparison: 0) |
| Assassination stem + Deflection | 16 parries, on the same attacks |
| Assassination stem + Master of Subtlety | 69 procs |
| Assassination stem, Master Poisoner's point on Vitality | Deadly Poison procs 0 (Master Poisoner's arm: 101) |
| Subtlety stem + Cheat Death | **34 saves**: half the Rogues are saved once a match |
| every Subtlety arm | about 10 Initiative procs a Rogue a match |

**Melee hits taken.** The dodge and parry nodes are weightless, so these
avoidance counts are their footprint.

- A Rogue takes about 6.5 melee attacks a match: 4,399 hits plus 155 dodges
  over 696 Rogues.
- Those are its melee hits landed on it: auto-attacks (a pet's included),
  Mortal Strike, Sinister Strike, Hemorrhage, Ambush, Crusader Strike and
  Heroic Strike. Strikes that deal no damage (Cheap Shot, Kidney Shot, Kick,
  Pummel) are not counted.
- One 4% rank avoids 0.22 of those attacks a match.

Avoidance counts only against melee, while health counts against everything.

Damage-broken crowd control is unchanged by Rupture: 225 breaks in 600 matches
with it, 226 without.

## Shadow Dance

The user ruled (2026-10-14) that the Dance is Subtlety's threat, and that its
diminishing-returns-capped chain is intended:

- Cheap Shot 4s, then Cheap Shot 2s, then Kidney Shot 6s;
- about 12s of stun in all.

**How it is counted** (`dance`): every Rogue match of the Subtlety reference,
replayed with full logs. That is 4,992 matches and 5,760 Rogues a block.

- **Its chain** is every Cheap Shot or Kidney Shot its Rogue begins inside
  the Dance's 8s. It also includes a Kidney Shot begun up to 1s after the
  Dance ends: the planner's hand-off as the last Cheap Shot runs out.
- **Stun-seconds applied** are the durations the log reports, after
  diminishing returns.
- **Held seconds** cut each stun at its victim's death.
- **The window** runs from the Dance's cast to 2s past the *later* of the
  Dance's own end and its chain's last stun's end. The 2s is one global
  cooldown and a half of follow-through. The window averages 12.1s.
- **Converts:** a Dance converts when an enemy team member (never a pet) dies
  inside the window.
- **The counterfactual** is the **Vitality twin**: the same match (label and
  seed) with Vitality in the Dance's place. It asks whether the Dance's first
  victim is dead by the Dance's window end, and whether any enemy team member
  is.
  - The twin diverges from the reference from the gates on, since its builds
    differ, so this is a same-time comparison, not a frame-matched one.
  - The Dance's own side is read the same way: the first victim dead by the
    window's end.

| | block 1 | block 2 |
|---|---|---|
| Dances | 1,809 (0.31 a Rogue a match) | 1,739 (0.30) |
| stuns a Dance | 2.35 | 2.35 |
| **stun-seconds applied a Dance** | **9.08** | **9.06** |
| held (cut at the victim's death) | 7.05 | 6.95 |
| DR-immune stuns a Dance | 0.00 | 0.00 |
| full chain CS4 CS2 KS6 (12s) | 946 (52%) | 899 (52%) |
| CS4 alone | 430 (24%) | 431 (25%) |
| CS4 CS2 | 227 (13%) | 219 (13%) |
| **converts: an enemy dies in the window** | **75.7%** | **76.7%** |
| first victim a healer | 58.2% | 58.0% |
| Dances that spent Initiative's discount on the free Dance | 893 (49%) | 883 (51%) |

**What lets a Dance go.** `try_shadow_dance` spends it only when the chain can
run:

- the kill target is in melee and held by nothing;
- both stun diminishing-returns buckets are clear;
- Kidney Shot will be ready in time;
- `enemy_healer_answers` returns false.

That last function admits the Dance when:

- the target is the team's kill window; or
- no living enemy healer, other than the target itself, stands outside the one
  the team holds locked.

So it admits a Dance:

- on **any enemy healer** as the target;
- in **any comp with no enemy healer standing** (none fielded, or dead);
- and, with a healer standing, only when that healer is locked or the target
  is the kill window.

Only that last branch implies a kill is already coming. Each Dance, classified
by the branch that admitted it, with its first victim read against the
Vitality twin at the Dance's window end:

| admission | block | Dances | share | victim dead, Dance | victim dead, twin | any enemy dead, twin |
|---|---|---|---|---|---|---|
| the target is the enemy healer | 1 | 1,052 | 58.8% | 74.5% | 64.4% | 80.0% |
| | 2 | 1,008 | 58.3% | 75.0% | 63.3% | 79.9% |
| no healer in the enemy team | 1 | 590 | 33.0% | 73.4% | 76.1% | 80.0% |
| | 2 | 578 | 33.4% | 74.7% | 75.8% | 79.8% |
| the enemy healer is dead | 1 | 37 | 2.1% | 97.3% | 70.3% | 100.0% |
| | 2 | 47 | 2.7% | 97.9% | 80.9% | 100.0% |
| a healer stands: its lock or the kill window | 1 | 110 | 6.1% | 98.2% | 91.8% | 92.7% |
| | 2 | 97 | 5.6% | 93.8% | 88.7% | 88.7% |
| **all** | 1 | 1,789 | | **76.1%** | **70.0%** | 81.2% |
| | 2 | 1,730 | | **76.6%** | **69.4%** | 80.9% |

Dances that landed no stun (20 and 9) have no victim to follow and are left
out.

**What the twin says.**

- **The Dance moves its victim's fate by about 6-7pt overall:** dead by the
  window's end 76% of the time, against 70% without it.
- **Nearly all of that is on an enemy healer.** There the Dance kills
  10-12pt more often (74.5% against 64.4%, 75.0% against 63.3%). The small
  branches gain too (a dead or a locked healer, 8% of Dances).
- **Against a team with no healer it adds nothing:** 73-75% against 76%.
- **Most of the kill comes anyway.** In the twin some enemy is dead by the
  same moment 81% of the time.

The matches a Dance is cast in are also won about as often without it. This
pairs the Subtlety reference against the stem plus Vitality, over one-sided
matches where the reference cast a Dance:

| block | matches | Vitality | reference | delta | flips | z | floor |
|---|---|---|---|---|---|---|---|
| 1 | 1,561 | 68.4% | 68.8% | +0.4 | 138 (+72/−66) | 0.43 | 1.5 |
| 2 | 1,492 | 69.1% | 70.0% | +0.9 | 133 (+72/−59) | 1.05 | 1.6 |

That slice is selected on the reference arm, which favours the Dance.

**Why the Dance did not go.** Over 39,834 traced Rogue decisions of the
Subtlety reference (`gates`, the 60 sample matches), the Dance candidate was:

| outcome | share |
|---|---|
| refused for the kill target out of melee | 71.3% |
| refused for the target already held | 15.4% |
| on cooldown | 10.5% |
| refused for diminishing returns not clear | 1.9% |
| refused for Kidney Shot not ready | 0.7% |
| refused for the enemy healer free to answer | 0.1% |
| chosen | 0.1% |

The gates are checked in that order, and a decision reports only the first
that refused it. So each share counts the decisions that reached that gate and
stopped there, not how often its condition held. The Rogue never pooled
energy for the Dance in that slice.

**Answer to the ruling's question: the chain does not dominate.**

- It lands its designed 12s in half of Dances, and 9.1 stun-seconds on
  average.
- It raises the chance its victim dies in the window by 6-7pt, nearly all of
  it on an enemy healer.
- The kill mostly comes anyway, and win rate does not register it: −0.9pt
  past the allowance against a stat rank of its price, at margin z −3.4,
  replicated.
- No defending-side counterplay is called for by these numbers.

If the Dance is to be Subtlety's threat in the results as well as in the log,
two levers matter more than its magnitude, and both are design questions for
the user:

- **where the gate sends it:** a third of Dances go into comps with no healer,
  where they add nothing;
- **when the AI spends it.**

## What it says (findings for the user, not changes)

1. **Cheat Death dominates** (+6.5pt past its allowance, margin z=23.3,
   replicated).
   - It is deterministic: the match's first killing blow leaves the Rogue at
     10%, with 90% less damage for 3s.
   - It saves half of all Rogues once a match, and on the Subtlety stem it is
     worth +7.6pt alone.
   - Direction: TBC's chance (31230 rolled 33% a rank), a smaller cut or
     window, or a higher price.
2. **Hemorrhage dominates modestly** (+1.6pt past its allowance, margin
   z=6.4, past the threshold in both blocks separately).
   - It is a 30-energy Sinister Strike that leaves +2% Physical damage taken a
     stack, up to 5, which nothing removes.
   - Direction: a smaller stack, or fewer stacks.
3. **Swift Blades dominates modestly** (+1.7pt past its allowance, margin
   z=4.1 pooled).
   - It is over the floor in both blocks, and past the threshold pooled and in
     block 1 (block 2 alone: z=2.2).
   - Its raw +1.0pt beats a full Vitality by more than the floor, so the
     verdict does not depend on the price model.
   - Direction: +10% melee haste for one rank is worth more than 1.85 ranks of
     health. That points to a smaller haste, or a higher price.
4. **Shadow Dance and Expose Armor, both capstones, sit below a stat rank of
   their price**, both past the threshold pooled.
   - **Shadow Dance** (margin z −3.4): see above.
   - **Expose Armor** (margin z −3.0 pooled, within in block 2 alone) is cast
     0.6 times a Rogue a match, and the Combat reference is only +0.7pt over
     its stem.
5. **Rupture is below** (−2.4pt past its allowance, margin z −5.6,
   replicated).
   - It is 60 damage over 12s for 25 energy, on a 20s cooldown, cast 1.2
     times a Rogue a match.
   - No AI cause was found. It does not break more crowd control (225 breaks
     against 226), and its cast count is what the cooldown allows.
   - Direction: its magnitude, before Blood Spatter's +50% on it.
6. **Lightning Reflexes and Deflection are one node in two places.**
   - A rank of either is worth nothing on the Assassination stem (+0.0pt).
   - It avoids 0.22 of the 6.5 melee attacks a Rogue takes a match.
   - Their "below" (margin z −2.6) does not clear the threshold.
   - Directions: give parry its own effect (TBC's parry haste, or
     front-only); and re-read their declared 11 a rank against how few melee
     hits a Rogue takes.
7. **Initiative's discount is spent on Shadow Dance itself** in half of all
   Dances (893 of 1,809).
   - The Dance costs nothing, so that discount is wasted.
   - The modifier's `applies_to: Any` is consumed by the first ability the
     Rogue uses, free or not.
   - This is a mechanic defect, not a tuning one: a follow-up card.
8. **Master of Subtlety's first rank is below on the floor rule** (−0.8pt past
   its allowance, margin z −2.8, block 1 only). It fires exactly once a match,
   on the opener.
9. **Master Poisoner and Initiative's second rank are within only by the price
   model.** Against a full Vitality, at allowance 0, both are below (−1.2 and
   −1.1pt, against floors of 0.9 and 0.6). Initiative's second rank is also
   significantly below a stat rank of its price (margin z −3.38, block 1),
   but by less than its floor, so it stays within.

Each item is a direction for AS-441, not a value. A directional run may not be
cited as the Rogue's standing.

## Byte-identity

The card adds data only: three reference builds. No Rust source changes, so
the binary is `main`'s.

**The method.** One release binary of this branch was copied twice, each
beside an `assets/` directory: one with `main` 27d76940's assets, one with
this branch's. Assets resolve beside the executable first. Each copy was
confirmed to read its own: `main`'s refuses `"team1_builds": ["Subtlety"]`.

**The results.**

- **Batches.** 1,272 seeded matches gave identical CSVs from both. 1,270 of
  them were decisive, with 288, 312 and 395 distinct durations in the three
  batches:
  - 424 Rogue-reachable 2v2 cells, `Legacy`, BasicArena;
  - 424 cells, `TeamPlan`, PillaredArena;
  - 424 Warlock cells, `TeamPlan`, TwinPillars, with the Warlock's
    Demonology reference.
- **Logs.** Four seeded `--headless` logs are identical: 2v2 and 3v3, both
  profiles, three maps.

## Reproducing it

Every arm is a config on this branch's binary, so no patch is needed. The
committed artefacts are the arms' JSONLs and CSVs.

```bash
cargo build --release
S=docs/design/balance/sweeps
# the inputs as run (or regenerate: $S/2026-11-22-as301-arms.py gen <dir>, and
# `gen <dir2> 4 <the REPLICATED_ARMS>` for block 2 -- both regenerate byte for byte)
mkdir -p /tmp/as301 && tar -xzf $S/2026-11-22-as301-arms.jsonl.tar.gz -C /tmp/as301
for f in /tmp/as301/block1/*.jsonl /tmp/as301/block2/*.jsonl; do
  target/release/arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 4 --trace-mode off
done
# or take the CSVs as run:
tar -xzf $S/2026-11-22-as301-arms.csv.tar.gz -C /tmp/as301
$S/2026-11-22-as301-arms.py table /tmp/as301/block1
$S/2026-11-22-as301-arms.py replicate /tmp/as301/block1 /tmp/as301/block2
$S/2026-11-22-as301-arms.py sample /tmp/as301/block1 target/release/arenasim
$S/2026-11-22-as301-arms.py sample /tmp/as301/block1 target/release/arenasim 600 \
  assn_stem assn_norup_vit assn assn_vit assn_lr sub sub_vit combat combat_vit zero
# the Dance figures, the admission split and the Vitality twin:
$S/2026-11-22-as301-arms.py dance /tmp/as301/block1 target/release/arenasim
$S/2026-11-22-as301-arms.py dance /tmp/as301/block2 target/release/arenasim
$S/2026-11-22-as301-arms.py gates /tmp/as301/block1 target/release/arenasim
# any one pair, with its control and slices:
scripts/paired_sweep.py /tmp/as301/block1/sub_vit.csv /tmp/as301/block1/sub_cd.csv \
  --affects Rogue --tier directional --expect nothing --headline affected
```

Three sets of figures replay matches through the binary (minutes): the
non-vacuity counts, the Shadow Dance tables, and the gate histogram. Every
other figure recomputes from the committed CSVs. The `dance` replays the
Vitality twin of every match with a Dance from that block's committed
`sub_vit` JSONL. The script's printed claims (margin z, admission, the
window, the twin) are pinned by `scripts/tests/test_as301_arms.py`.
