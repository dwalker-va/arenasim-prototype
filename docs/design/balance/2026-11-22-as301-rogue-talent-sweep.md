# AS-301: Rogue reference builds, node arms and Shadow Dance

Tier: **directional**. Success condition (design §4, "Sweeps measure nodes and
reference builds"): **no node dominates**. No node may move the Rogue by more
than the run's resolution floor beyond a stat rank of the same price.

**Verdict: the condition fails clearly on Cheat Death, modestly on
Hemorrhage, and only reaches the line on Swift Blades.**

- **Cheat Death** beats Vitality by +5.9pt, pooled over two seed blocks
  (z=19.6). It is priced at half of Vitality, so its margin is +6.5pt against a
  0.6pt floor.
- **Hemorrhage** beats Vitality by +1.0pt in each block. Pooled, that is z=3.6
  and a margin of +1.6pt against a 0.6pt floor.
- **Swift Blades** is over the line pooled (+1.7pt against a 0.9pt floor).
  However, z is 2.2, which is not significant after correcting for the 12 node
  comparisons, and its second block alone (+0.6pt) is inside the floor.

**Shadow Dance does not dominate.** It is 0.9pt *below* a stat rank of its
price in both blocks. A Dance lands 9.1 stun-seconds. Its chain's victim dies
in 75% of Dances, against 51% after a plain Kidney Shot. The Dance is spent
almost only when a kill is already coming, though, so the matches it is cast
in are won about as often with Vitality in its place.

Lightning Reflexes and Deflection are the same node in effect: their arms
played byte-identical matches.

Nothing here is retuned. Tuning is AS-441's batch, and the Dance's counterplay
is the user's call (see *What it says*).

Measured at `main` 27d76940 plus this card's three reference builds. They are
data only, and leave untalented play byte-identical (see *Byte-identity*).

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

**Multiple comparisons:** 12 node comparisons. A Bonferroni threshold at
α=0.05 two-sided is z ≥ 2.87.

## Results

In every table, "before" is the comparison arm and "after" is the arm under
test, both as the Rogue side's win rate.

- **Flips** are matches whose winner moved: + toward the Rogue side, − away
  from it.
- **The floor** is the smallest delta the pair's own discordance could
  resolve.

### Each node against Vitality in its place (block 1)

| node | before | after | delta (pt) | flips | z | floor | allowance | margin | verdict |
|---|---|---|---|---|---|---|---|---|---|
| **Cheat Death** | 58.5% | 64.9% | **+6.4** | 343 (+306/−36) | 14.55 | 0.9 | −0.6 | +7.0 | **dominates** |
| **Swift Blades** | 52.7% | 54.0% | **+1.3** | 680 (+368/−311) | 2.15 | 1.2 | −0.8 | +2.2 | **dominates** |
| **Hemorrhage** | 56.2% | 57.3% | **+1.0** | 261 (+152/−108) | 2.67 | 0.8 | −0.6 | +1.6 | **dominates** |
| Master Poisoner (Deadly off hand) | 53.4% | 52.2% | −1.2 | 372 (+161/−211) | 2.54 | 0.9 | −1.0 | −0.2 | within |
| Initiative (rank 2) | 58.4% | 57.3% | −1.1 | 133 (+43/−90) | 3.99 | 0.6 | −0.6 | −0.5 | within |
| Blood Spatter (capstone, for scale) | 54.3% | 54.0% | −0.4 | 327 (+156/−171) | 0.77 | 0.9 | +0.2 | −0.5 | within |
| Master of Subtlety (rank 1) | 54.3% | 52.6% | −1.8 | 216 (+71/−145) | 4.97 | 0.7 | −1.0 | −0.8 | below |
| Shadow Dance (capstone) | 58.5% | 57.7% | −0.8 | 220 (+93/−127) | 2.22 | 0.7 | +0.1 | −0.9 | below |
| Lightning Reflexes (rank 1) | 54.3% | 52.2% | −2.1 | 377 (+144/−232) | 4.49 | 0.9 | −1.0 | −1.1 | below |
| Deflection (rank 1) | 54.3% | 52.2% | −2.1 | 377 (+144/−232) | 4.49 | 0.9 | −1.0 | −1.1 | below |
| Expose Armor (capstone) | 55.8% | 54.7% | −1.1 | 396 (+175/−221) | 2.26 | 0.9 | +0.1 | −1.2 | below |
| Rupture | 54.9% | 52.2% | −2.7 | 699 (+293/−405) | 4.20 | 1.2 | −1.0 | −1.7 | below |

The Lightning Reflexes and Deflection arms are the same CSV, byte for byte.
In this sim, dodge and parry are one avoidance roll with no facing and no
parry haste (`combat_core/avoidance.rs`). So +4% dodge and +4% parry avoid
exactly the same attacks.

### The replication (seeds 4-7), and pooled

Block 2 re-ran the three nodes over the line, Shadow Dance, and the two
significant "below" rows it reaches. Each pooled verdict reads its allowance
from the pooled Vitality rank on the same stem.

| comparison | block | before | after | delta (pt) | flips | z | floor | margin | verdict |
|---|---|---|---|---|---|---|---|---|---|
| Cheat Death | 1 | 58.5% | 64.9% | +6.4 | 343 (+306/−36) | 14.55 | 0.9 | | |
| | 2 | 59.1% | 64.5% | +5.5 | 308 (+269/−38) | 13.13 | 0.8 | | |
| | **pooled** | 58.8% | 64.7% | **+5.9** | 651 (+575/−74) | 19.63 | 0.6 | +6.5 | **dominates** |
| Hemorrhage | 1 | 56.2% | 57.3% | +1.0 | 261 (+152/−108) | 2.67 | 0.8 | | |
| | 2 | 56.9% | 57.9% | +1.0 | 311 (+176/−134) | 2.33 | 0.8 | | |
| | **pooled** | 56.6% | 57.6% | **+1.0** | 572 (+328/−242) | 3.56 | 0.6 | +1.6 | **dominates** |
| Swift Blades | 1 | 52.7% | 54.0% | +1.3 | 680 (+368/−311) | 2.15 | 1.2 | | |
| | 2 | 54.2% | 54.8% | +0.6 | 692 (+359/−333) | 0.95 | 1.2 | | |
| | **pooled** | 53.4% | 54.4% | **+1.0** | 1372 (+727/−644) | 2.21 | 0.9 | +1.7 | **at the line** |
| Shadow Dance | 1 | 58.5% | 57.7% | −0.8 | 220 (+93/−127) | 2.22 | 0.7 | | |
| | 2 | 59.1% | 58.3% | −0.7 | 219 (+93/−124) | 2.04 | 0.7 | | |
| | **pooled** | 58.8% | 58.0% | **−0.8** | 439 (+186/−251) | 3.06 | 0.5 | −0.9 | below |
| Expose Armor | 1 | 55.8% | 54.7% | −1.1 | 396 (+175/−221) | 2.26 | 0.9 | | |
| | 2 | 56.3% | 55.6% | −0.7 | 377 (+174/−203) | 1.44 | 0.9 | | |
| | **pooled** | 56.0% | 55.2% | **−0.9** | 773 (+349/−424) | 2.66 | 0.7 | −1.0 | below |
| Rupture | 1 | 54.9% | 52.2% | −2.7 | 699 (+293/−405) | 4.20 | 1.2 | | |
| | 2 | 56.0% | 52.2% | −3.8 | 676 (+258/−418) | 6.12 | 1.2 | | |
| | **pooled** | 55.4% | 52.2% | **−3.2** | 1375 (+551/−823) | 7.31 | 0.9 | −2.4 | below |

Of the rows with a verdict other than "within", these stand at the
Bonferroni threshold:

- Cheat Death, Hemorrhage, Shadow Dance and Rupture;
- in block 1 alone, Lightning Reflexes, Deflection and Master of Subtlety.

Swift Blades and Expose Armor do not. Swift Blades is "at the line" for two
reasons:

- **Its margin rests on the allowance.** It beats a 1.85-rank Vitality by
  +1.0pt with one rank, which the linear-price allowance turns into +1.7pt
  over a stat rank of its price.
- **It does not replicate.** It is not significant after correction, or in
  its second block alone.

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
| Combat | Expose Armor cast 429; the Rogue dodges 403 times and parries 345 |
| Subtlety | Shadow Dance cast 226; Hemorrhage cast 3,495; Initiative procs 6,912, of which 5,869 are spent; Master of Subtlety procs 696 (one a Rogue: its stealth break) |

| arm (60 matches, 69 Rogues) | node events |
|---|---|
| Assassination stem + Lightning Reflexes | 16 dodges (its comparison: 0) |
| Assassination stem + Deflection | 16 parries, on the same attacks |
| Assassination stem + Master of Subtlety | 69 procs |
| Assassination stem, Master Poisoner's point on Vitality | Deadly Poison procs 0 (Master Poisoner's arm: 101) |
| Subtlety stem + Cheat Death | **34 saves**: half the Rogues are saved once a match |
| every Subtlety arm | about 10 Initiative procs a Rogue a match |

The dodge and parry nodes are weightless, so these avoidance counts are their
footprint. One rank (4%) avoids 0.23 attacks a Rogue a match: a Rogue takes
few melee hits in the comps a Rogue fields. Avoidance counts only against
melee, while health counts against everything.

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
- **Converts:** a Dance converts when an enemy (never a pet) dies between the
  Dance's cast and 2s after the end of its chain's last stun. The 2s is one
  global cooldown and a half of follow-through. The window averages 12.1s.
- **The baseline** applies the same rule to every Kidney Shot of the Subtlety
  stem, which has no Dance: an enemy dies by the stun's end plus 2s.

| | block 1 | block 2 |
|---|---|---|
| Dances | 1,809 (0.31 a Rogue a match) | 1,739 (0.30) |
| stuns a Dance | 2.35 | 2.35 |
| **stun-seconds applied a Dance** | **9.08** | **9.06** |
| held (cut at the victim's death) | 7.04 | 6.95 |
| DR-immune stuns a Dance | 0.00 | 0.00 |
| full chain CS4 CS2 KS6 (12s) | 946 (52%) | 899 (52%) |
| CS4 alone | 430 (24%) | 431 (25%) |
| CS4 CS2 | 227 (13%) | 219 (13%) |
| **converts: an enemy dies** | **75.7%** | **76.7%** |
| converts: the stunned enemy dies | 75.3% | 76.3% |
| baseline: an enemy dies after a plain Kidney Shot | 51.3% (6,137 Kidney Shots) | 52.6% (6,058) |
| first victim a healer | 58.2% | 58.0% |
| Dances that spent Initiative's discount on the free Dance | 893 (49%) | 883 (51%) |

**Does a Dance change who wins the match it is cast in?** The test pairs the
Subtlety reference against the stem plus Vitality, restricted to one-sided
matches where the reference cast a Dance:

| block | matches | Vitality | reference | delta | flips | z | floor |
|---|---|---|---|---|---|---|---|
| 1 | 1,561 | 68.4% | 68.8% | +0.4 | 138 (+72/−66) | 0.43 | 1.5 |
| 2 | 1,492 | 69.1% | 70.0% | +0.9 | 133 (+72/−59) | 1.05 | 1.6 |

That slice is selected on the reference arm, which favours the Dance. Even so,
the matches with a Dance are won about as often without it.

**Why.** The Dance goes only when its chain can run (`try_shadow_dance`):

- the kill target is in melee and held by nothing;
- both stun diminishing-returns buckets are clear;
- Kidney Shot will be ready in time;
- the enemy healer cannot answer.

That last condition holds when the healer is the target itself, the healer is
locked, or the target is in the team's kill window. So the Dance is spent where
a kill is already coming. The chain's victim then dies 76% of the time, but a
plain Kidney Shot's dies 51% of the time, and the match was mostly won anyway.

Over 39,834 traced Rogue decisions of the Subtlety reference (`gates`, the 60
sample matches), the Dance candidate was:

| outcome | share |
|---|---|
| refused for the kill target out of melee | 71.3% |
| refused for the target already held | 15.4% |
| on cooldown | 10.5% |
| refused for diminishing returns not clear | 1.9% |
| refused for Kidney Shot not ready | 0.7% |
| refused for the enemy healer free to answer | 0.1% |
| chosen | 0.1% |

The Rogue never pooled energy for it in that slice.

**Answer to the ruling's question: the chain does not dominate.**

- It lands its designed 12s in half of Dances, and 9.1 stun-seconds on
  average.
- Win rate does not register it: −0.8pt against a stat rank of its price,
  replicated in both blocks.
- No defending-side counterplay is called for by these numbers.

If the Dance is to be Subtlety's threat in the results as well as in the log,
the lever is when the AI spends it. Spending it to *open* a kill window, rather
than inside one, is a design question for the user. It is not a magnitude
change.

## What it says (findings for the user, not changes)

1. **Cheat Death dominates** (+6.5pt past its allowance, replicated, z=19.6).
   - It is deterministic: the match's first killing blow leaves the Rogue at
     10%, with 90% less damage for 3s.
   - It saves half of all Rogues once a match, and on the Subtlety stem it is
     worth +7.6pt alone.
   - Direction: TBC's chance (31230 rolled 33% a rank), a smaller cut or
     window, or a higher price.
2. **Hemorrhage dominates modestly** (+1.6pt past its allowance, replicated).
   - It is a 30-energy Sinister Strike that leaves +2% Physical damage taken a
     stack, up to 5, which nothing removes.
   - Direction: a smaller stack, or fewer stacks.
3. **Swift Blades is at the line.** It is over it pooled, but not significant
   after correction and not in its second block alone. Watch it in AS-441
   rather than act on it alone.
4. **Shadow Dance and Expose Armor, both capstones, sit below a stat rank of
   their price.**
   - Shadow Dance: see above.
   - Expose Armor is cast 0.6 times a Rogue a match, and the Combat reference
     is only +0.7pt over its stem. Its margin of −1.0pt is not significant
     after correction.
5. **Rupture is below** (−2.4pt past its allowance, replicated, z=7.3).
   - It is 60 damage over 12s for 25 energy, on a 20s cooldown, cast 1.2
     times a Rogue a match.
   - No AI cause was found. It does not break more crowd control (225 breaks
     against 226), and its cast count is what the cooldown allows.
   - Direction: its magnitude, before Blood Spatter's +50% on it.
6. **Lightning Reflexes and Deflection are one node in two places**, and a
   rank of either is worth nothing on the Assassination stem (+0.0pt, 0.23
   avoidances a Rogue a match). Directions:
   - Give parry its own effect (TBC's parry haste, or front-only).
   - Re-read their declared 11 a rank against how few melee hits a Rogue
     takes.
7. **Initiative's discount is spent on Shadow Dance itself** in half of all
   Dances (893 of 1,809).
   - The Dance costs nothing, so that discount is wasted.
   - The modifier's `applies_to: Any` is consumed by the first ability the
     Rogue uses, free or not.
   - This is a mechanic defect, not a tuning one: a follow-up card.
8. **Master of Subtlety's first rank is below** (−0.8pt past its allowance,
   block 1 only). It fires exactly once a match, on the opener.

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
  assn_stem assn_norup_vit assn assn_vit sub sub_vit combat combat_vit
$S/2026-11-22-as301-arms.py dance /tmp/as301/block1 target/release/arenasim
$S/2026-11-22-as301-arms.py dance /tmp/as301/block2 target/release/arenasim
$S/2026-11-22-as301-arms.py gates /tmp/as301/block1 target/release/arenasim
# any one pair, with its control and slices:
scripts/paired_sweep.py /tmp/as301/block1/sub_vit.csv /tmp/as301/block1/sub_cd.csv \
  --affects Rogue --tier directional --expect nothing --headline affected
```

Every figure above recomputes from the committed CSVs except three, which
replay matches through the binary (minutes): the non-vacuity counts, the
Shadow Dance table, and the gate histogram.
