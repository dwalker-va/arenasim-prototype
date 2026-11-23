# AS-300: Priest reference builds and node arms

Tier: **directional**. Success condition (design §4, "Sweeps measure nodes and
reference builds"): **no node dominates.** That means no node moves the Priest by
more than the run's resolution floor beyond a stat rank of the same price.

**The rule, as AS-299 applied it:** a node dominates when its pooled delta
beats the yardstick rank (plus a capstone's allowance) by more than the pair's
floor, and its margin z survives the Bonferroni correction for the 16 node
comparisons (z ≥ 2.96). Per-block replication is reported beside each verdict,
not as part of the rule.

**The margin z is per match** (the AS-301 ruling, for every talent sweep). Each
paired match i, scored for the Priest's side, gives

    x_i = 100 · [(A_i − B_i) − k · (V+_i − V0_i)]

where A, B, V+ and V0 are 1 when that side won in the arm's row:

- A is the node's arm and B its yardstick arm;
- V+ and V0 are the stem with and without one yardstick rank;
- k = 1 for a capstone (its two-rank allowance) and 0 for a one-rank node.

The margin is mean(x), and se = sd(x)/√n. The arms share every match and a
stem, so their deltas correlate: a quadrature sum of the pairs' errors would
be wrong. `margin_z` in the arms script is that formula. For a one-rank node
the margin is the plain paired delta, and its z is McNemar's without the
continuity correction (a few hundredths higher).

**Verdict: the condition fails on five nodes.** Four fail clearly. Each of
them clears the correction in each seed block alone, Pain Suppression's first
block by only 0.03. The fifth, Inner Focus, fails marginally:
0.3pt past its floor, just past the correction pooled, and in neither block
alone.

| Node | Its delta | The line | Margin (floor) | Margin z |
|---|---|---|---|---|
| Guardian Spirit | +8.9pt | +0.9pt (allowance) | +8.0pt (1.0) | 12.44 |
| Silence (in Shadowform) | +6.4pt | +1.0pt (allowance) | +5.3pt (0.9) | 9.39 |
| Reflective Shield | +4.4pt | 0 | +4.4pt (1.0) | 8.90 |
| Pain Suppression | +4.0pt | +0.5pt (allowance) | +3.5pt (1.0) | 5.37 |
| Inner Focus (marginal) | +1.0pt | 0 | +1.0pt (0.7) | 3.09 |

- Each is measured against a yardstick rank. For the three capstones, the line is the capstone's allowance: what a second yardstick rank buys on the same stem.
- Inner Focus is not replicated per block: +0.8 (margin z 1.68) and +1.3 (2.66), neither past the correction alone.
- **Mind Flay sits well below a stat rank in both roles:** −4.8pt in Shadowform and −2.8pt healing.
- Four more nodes sit below significantly, but none survives the correction.
- **Guardian Spirit's save fires on 68% of its casts:** 258 saves from 381 casts, in 0.39 of the Priest's matches.
- **Shadowform costs the hybrid build 10.2pt.**

Nothing here is retuned: tuning is AS-441's batch.

Measured at `main` 27d76940 plus this card's three reference builds. Those
leave untalented play byte-identical; see *Byte-identity*. The binary was
re-checked after each later merge of `main`, and each time it reproduced every
sampled row of the committed CSVs exactly (winner, end and duration), so
nothing here was re-measured:

- at f2b4dc25 (AS-357), 1,700 rows of the Shadowform, Vampiric Embrace and
  Guardian Spirit arms;
- at eacf3d43 (AS-301's Rogue trees), 1,700 rows of the 0-point,
  Discipline, Holy and Shadow arms.

## Method

### The reference builds

These are in `assets/config/talents.ron`, under `reference_builds`:

| name | shape | ranks |
|---|---|---|
| Discipline | 6/2/0 | Unbreakable Will 2, Improved Power Word: Shield 1, Inner Focus 1, Reflective Shield 1, **Pain Suppression** 1; Holy Specialization 2 |
| Holy | 2/6/0 | Improved Power Word: Shield 2; Holy Specialization 2, Inspiration 2, Holy Concentration 1, **Guardian Spirit** 1 |
| Shadow | 2/0/6 | Unbreakable Will 2; Improved Mind Blast 2, Shadowform 1, Blackout 1, Mind Flay 1, **Silence** 1 |

**This departs from §5.2's sketch, following §4's "one deep in each tree":
the third reference is a Shadow-deep 2/0/6, not §5.2's Discipline-Shadow
5/0/3.** The hybrid is not a reference build because it takes no capstone:

- Pain Suppression would be its sixth Discipline point.
- Silence needs five Shadow points below it.

The schema holds each reference build to its tree's capstone, and requires one
reference per tree. So the Shadow reference is a real Shadow-deep build. Its
Silence is castable only in Shadowform, so that reference plays the form.

The hybrid is measured as an arm, written out as its ranks:

- Unbreakable Will 2, Inner Focus 1, Power Infusion 1, Reflective Shield 1;
- Improved Mind Blast 2, Shadowform 1.

It plays both healing and in Shadowform.

### The yardstick

The Priest has no `StatPercent` node near a rank's price. Its one, Holy
Concentration, prices at 3.5 (`talent_price_report`).

So a node is measured against one rank of **Spiritual Healing**: +5% healing, a
plain Patch at the declared 11. That is a fair stat rank:

- Eleven item-budget points of spell power is +7.3 at SP's 1.5 weight.
- At Flash Heal's 0.75 coefficient, +7.3 SP adds 5.5 to a 105 heal, which is +5.2%.

In Shadowform no heal lands. There the yardstick is one rank of **Improved Power
Word: Shield** instead: +5% absorb, the one plain rank still at work in the form.

### The arms

A node comparison is two 8-point builds that differ in one point: either on
the node, or on the yardstick.

- **A node the reference's stem lacks** is added to the stem. The stem is the
  reference without its capstone. The comparison is `stem + node` against
  `stem + Y`.
- **A node the stem holds** is measured by its removal: `stem + Y` against
  `stem − node + 2Y`. AS-299 measured Siphon Life the same way. For Unbreakable Will and
  Inspiration this measures the second rank.
- **A capstone is priced at two ranks.** Its arm is the reference itself, and it
  may beat one yardstick rank by what a second rank buys on the same stem
  (`stem + Y` against `stem`).

| node | its arm | comparison |
|---|---|---|
| Pain Suppression (capstone), Power Infusion | Discipline stem + node | Discipline stem + Spiritual Healing |
| Unbreakable Will (rank 2), Inner Focus, Reflective Shield | Discipline stem + Spiritual Healing | the same with the node's point on Spiritual Healing |
| Guardian Spirit (capstone), Blessed Recovery, Holy Nova | Holy stem + node | Holy stem + Spiritual Healing |
| Inspiration (rank 2) | Holy stem + Spiritual Healing | the same with Inspiration's second point on Spiritual Healing |
| Silence (capstone), Vampiric Embrace, in Shadowform | Shadow stem + node | Shadow stem + Improved PW:Shield |
| Blackout (rank 1), Mind Flay, in Shadowform | Shadow stem + Improved PW:Shield | the same with the node's point on Improved PW:Shield |
| Vampiric Embrace; Blackout and Mind Flay, healing | as for Shadowform, with no form chosen | the same, Spiritual Healing for the yardstick |

The Shadow tree's Blackout, Mind Flay and Vampiric Embrace serve a healing
Priest too (design §5.2), so each is measured in both roles.

**Shadowform is an arm of its own.** It is a per-match choice, made with
`team{1,2}_priest_forms`, so this arm is the same build with the form chosen
against the form not chosen.

### The sweep

Every arm is one directional 2v2 sweep:

- **Cells:** every cell a Priest can reach, plus 8 control cells. That is
  `gen_sweep.py --full 2 --exclude-double-healer --affects Priest --control-cells 8`,
  283 cells. Double-healer teams are excluded as `gen_sweep.py` does, so a Priest
  plays as its team's one healer.
  - Against an enemy team with a healer, every scored match is healer against
    healer. These are 60% of the scored matches; the per-node split is below.
- **Maps:** BasicArena, PillaredArena and TwinPillars, 4 seeds a cell a map.
  That is 3,396 matches an arm.
- **Profile:** `TeamPlan` on both sides, which is what players get.
- **Builds:** every Priest, on either side, plays the arm's build. The arms
  differ only in build and form, so they pair at identical (label, seed).
- **Scoring:** for the side fielding the Priest (`paired_sweep.py --headline
  affected`), over the matches where exactly one side does: 3,000 an arm a
  block, 6,000 pooled. The Priest mirrors (300 an arm a block, 600 pooled) are
  excluded.
- **Format:** 2v2 only; the directional tier does not call for 3v3.

There are 29 arms:

- 0 points;
- the three references, their stems, and the stems with one yardstick rank
  (Shadow in both roles);
- the node arms;
- the hybrid, healing and in Shadowform.

All 29 were run on **two seed blocks**, seeds 0-3 and 4-7: 196,968 matches.
The tables pool the blocks. Every node comparison is also shown per block
(*Replication*).

**Multiple comparisons:** 16 node comparisons, so the Bonferroni threshold is
z ≥ 2.96 (two-sided α 0.05/16).

## Results

In every table:

- "before" is the comparison arm and "after" the arm under test, both as the
  Priest side's win rate.
- Flips are matches whose winner moved: + toward the Priest side, − away from it.
- The floor is the smallest delta the pair's own discordance could resolve.
- n is 6,000 scored matches a row.

### Each node against a yardstick rank in its place

"z" is the pair's McNemar z on its delta. The verdict reads the margin z, for
the margin past a capstone's allowance.

| node | before | after | delta (pt) | flips | z | floor (pt) | margin | margin z | verdict |
|---|---|---|---|---|---|---|---|---|---|
| **Guardian Spirit** (capstone, allowance +0.9) | 54.2% | 63.1% | **+8.9** | 818 (+675/−143) | 18.57 | 1.0 | +8.0 | 12.44 | **dominates** |
| **Silence**, Shadowform (capstone, allowance +1.0) | 38.3% | 44.7% | **+6.4** | 806 (+594/−212) | 13.42 | 0.9 | +5.3 | 9.39 | **dominates** |
| **Reflective Shield** | 56.0% | 60.4% | **+4.4** | 899 (+582/−317) | 8.80 | 1.0 | +4.4 | 8.90 | **dominates** |
| **Pain Suppression** (capstone, allowance +0.5) | 60.4% | 64.4% | **+4.0** | 839 (+538/−301) | 8.15 | 1.0 | +3.5 | 5.37 | **dominates** |
| **Inner Focus** | 59.4% | 60.4% | **+1.0** | 391 (+226/−165) | 3.03 | 0.7 | +1.0 | 3.09 | **dominates** (marginal; in neither block alone) |
| Vampiric Embrace, Shadowform | 38.3% | 39.1% | +0.8 | 1108 (+578/−530) | 1.41 | 1.1 | +0.8 | 1.44 | within |
| Unbreakable Will (rank 2) | 60.1% | 60.4% | +0.3 | 560 (+288/−272) | 0.63 | 0.8 | +0.3 | 0.68 | within |
| Blackout (rank 1), healing | 50.6% | 50.6% | −0.0 | 779 (+389/−390) | 0.00 | 0.9 | −0.0 | 0.04 | within |
| Blessed Recovery | 54.2% | 53.8% | −0.4 | 333 (+154/−179) | 1.32 | 0.6 | −0.4 | 1.37 | within |
| Holy Nova | 54.2% | 53.8% | −0.5 | 607 (+290/−317) | 1.06 | 0.8 | −0.5 | 1.10 | within |
| Inspiration (rank 2) | 55.0% | 54.2% | −0.7 | 261 (+109/−152) | 2.60 | 0.5 | −0.7 | 2.66 | below, not after correction |
| Blackout (rank 1), Shadowform | 39.1% | 38.3% | −0.8 | 622 (+286/−336) | 1.96 | 0.8 | −0.8 | 2.01 | below, not after correction |
| Power Infusion | 60.4% | 59.5% | −0.9 | 490 (+217/−273) | 2.48 | 0.7 | −0.9 | 2.53 | below, not after correction |
| Vampiric Embrace, healing | 50.6% | 49.3% | −1.3 | 1051 (+486/−565) | 2.41 | 1.1 | −1.3 | 2.44 | below, not after correction |
| **Mind Flay**, healing | 53.4% | 50.6% | **−2.8** | 871 (+351/−520) | 5.69 | 1.0 | −2.8 | 5.74 | **below** |
| **Mind Flay**, Shadowform | 43.0% | 38.3% | **−4.8** | 1329 (+522/−807) | 7.79 | 1.2 | −4.8 | 7.86 | **below** |

Moving the verdicts onto the margin z changes no verdict and no threshold
crossing. The capstones' z values fall, because their allowance carries noise
of its own: Guardian Spirit 18.57 → 12.44, Silence 13.42 → 9.39, Pain
Suppression 8.15 → 5.37. Every one-rank z rises by a few hundredths. Inner
Focus moves from 3.03 to 3.09, still past 2.96.

**For scale.** Holy Concentration, the one computed `StatPercent` node, prices
at 3.5. Measured against a second Spiritual Healing rank, it comes out +0.6pt
*ahead*: 53.7% → 54.2%, z=1.74, floor 0.6, so not significant. At a third of
the price, it buys at least what an 11-point rank buys. The price model
undervalues mana regeneration on a healer, or the declared 11 overvalues
Spiritual Healing.

**Silence where it cannot be cast.** In the Shadow build played healing,
Silence is locked. That arm's 6,792 matches are identical, row for row, to its
stem's: a locked Silence changes nothing.

### One yardstick rank on each stem (7 → 8 points)

| stem | before | after | delta (pt) | flips | z | floor (pt) |
|---|---|---|---|---|---|---|
| Discipline + Spiritual Healing | 60.0% | 60.4% | +0.5 | 299 (+163/−136) | 1.50 | 0.6 |
| Holy + Spiritual Healing | 53.3% | 54.2% | +0.9 | 281 (+168/−113) | 3.22 | 0.6 |
| Shadow (Shadowform) + Improved PW:Shield | 37.3% | 38.3% | +1.0 | 147 (+104/−43) | 4.95 | 0.4 |
| Shadow (healing) + Spiritual Healing | 49.5% | 50.6% | +1.1 | 298 (+181/−117) | 3.65 | 0.6 |

A stat rank is worth +0.5 to +1.1pt here. The Warlock's Demonic Embrace rank
was worth +1.4 to +2.0pt (AS-299). The yardstick is a small positive, not a
neutral rank, so a node "within" it is worth about that much, not nothing.

### Each comparison arm against its reference

| comparison arm | reference | after | delta (pt) | flips | z | floor (pt) |
|---|---|---|---|---|---|---|
| Discipline stem + Spiritual Healing | 64.4% | 60.4% | −4.0 | 839 (+301/−538) | 8.15 | 1.0 |
| Discipline stem − Unbreakable Will rank + 2 Spiritual Healing | 64.4% | 60.1% | −4.2 | 1027 (+387/−640) | 7.86 | 1.1 |
| Discipline stem − Inner Focus + 2 Spiritual Healing | 64.4% | 59.4% | −5.0 | 904 (+303/−601) | 9.88 | 1.0 |
| Discipline stem − Reflective Shield + 2 Spiritual Healing | 64.4% | 56.0% | −8.4 | 1248 (+373/−875) | 14.18 | 1.2 |
| Holy stem + Spiritual Healing | 63.1% | 54.2% | −8.9 | 818 (+143/−675) | 18.57 | 1.0 |
| Holy stem − Inspiration rank + 2 Spiritual Healing | 63.1% | 55.0% | −8.2 | 847 (+179/−668) | 16.77 | 1.0 |
| Shadow stem + Imp. PW:Shield (Shadowform) | 44.7% | 38.3% | −6.4 | 806 (+212/−594) | 13.42 | 0.9 |
| Shadow stem − Blackout + 2 Imp. PW:Shield (Shadowform) | 44.7% | 39.1% | −5.5 | 1078 (+373/−705) | 10.08 | 1.1 |
| Shadow stem − Mind Flay + 2 Imp. PW:Shield (Shadowform) | 44.7% | 43.0% | −1.6 | 1579 (+741/−838) | 2.42 | 1.3 |
| Shadow stem + Spiritual Healing (healing) | 49.5% | 50.6% | +1.1 | 298 (+181/−117) | 3.65 | 0.6 |
| Shadow stem − Blackout + 2 Spiritual Healing (healing) | 49.5% | 50.6% | +1.1 | 879 (+472/−407) | 2.16 | 1.0 |
| Shadow stem − Mind Flay + 2 Spiritual Healing (healing) | 49.5% | 53.4% | +3.9 | 953 (+593/−360) | 7.52 | 1.0 |

Every comparison arm also gives up the reference's capstone. So these
deltas are mostly the capstone's worth, with the node's worth on top: the
Discipline, Holy and Shadowform arms sit 4 to 9pt under their references
because Pain Suppression, Guardian Spirit and Silence dominate. The Shadow
build played healing has a locked Silence, so there its yardstick arms sit
at or above the reference (Mind Flay's removal +3.9).

### Replication

Each node's margin, with its margin z in brackets, block by block. Each block
is n=3,000, and a capstone's allowance is read in its own block:

| comparison | seeds 0-3 | seeds 4-7 | pooled |
|---|---|---|---|
| Guardian Spirit | +8.2 (8.62) | +7.7 (9.02) | +8.0 (12.44) |
| Silence, Shadowform | +4.9 (6.15) | +5.8 (7.13) | +5.3 (9.39) |
| Reflective Shield | +4.3 (6.15) | +4.5 (6.43) | +4.4 (8.90) |
| Pain Suppression | +2.8 (2.99) | +4.2 (4.66) | +3.5 (5.37) |
| Inner Focus | +0.8 (1.68) | +1.3 (2.66) | +1.0 (3.09) |
| Mind Flay, Shadowform | −4.2 (4.97) | −5.3 (6.14) | −4.8 (7.86) |
| Mind Flay, healing | −3.0 (4.27) | −2.7 (3.85) | −2.8 (5.74) |
| Power Infusion | −1.7 (3.18) | −0.1 (0.27) | −0.9 (2.53) |
| Inspiration (rank 2) | −0.5 (1.14) | −1.0 (2.78) | −0.7 (2.66) |
| Vampiric Embrace, healing | −1.4 (1.77) | −1.3 (1.67) | −1.3 (2.44) |
| Blackout, Shadowform | −1.2 (2.03) | −0.4 (0.76) | −0.8 (2.01) |

- **The four dominating nodes and both Mind Flay rows** replicate, and each
  block alone clears the correction. **Pain Suppression's first block clears
  it by 0.03** (2.99 against 2.96), so its block-by-block replication is
  marginal. Its pooled margin z is 5.37.
- **Inner Focus** dominates pooled, marginally, and is significant in neither
  block at the corrected bar.
- **Power Infusion's** first block is not reproduced by its second.

The script's `replicate` output also carries the other rows and the yardstick
ranks per block.

### Each reference against 0 points, and Shadowform

| comparison | before | after | delta (pt) | flips | z | floor (pt) |
|---|---|---|---|---|---|---|
| **Discipline 6/2/0** vs 0 points | 49.7% | 64.4% | **+14.7** | 1550 (+1215/−335) | 22.33 | 1.3 |
| **Holy 2/6/0** vs 0 points | 49.7% | 63.1% | **+13.4** | 1107 (+956/−151) | 24.16 | 1.1 |
| **Shadow 2/0/6, Shadowform** vs 0 points | 49.7% | 44.7% | **−5.0** | 2568 (+1133/−1435) | 5.94 | 1.7 |
| Shadow 2/0/6, healing, vs 0 points | 49.7% | 49.5% | −0.1 | 1325 (+658/−667) | 0.22 | 1.2 |
| Hybrid 5/0/3, healing, vs 0 points | 49.7% | 58.4% | +8.7 | 1274 (+899/−375) | 14.65 | 1.2 |
| Hybrid 5/0/3, Shadowform, vs 0 points | 49.7% | 48.2% | −1.5 | 2459 (+1185/−1274) | 1.77 | 1.6 |
| **Shadowform**, hybrid: form vs no form | 58.4% | 48.2% | **−10.2** | 2473 (+930/−1543) | 12.31 | 1.6 |
| **Shadowform**, Shadow build: form vs no form | 49.5% | 44.7% | **−4.9** | 2487 (+1097/−1390) | 5.86 | 1.6 |

- **The two healing references are each worth about 14pt.** That is more than the
  best Warlock reference (Demonology, +11.7).
- **Most of each healing reference's value is its capstone and one tier-3 node:**
  - Guardian Spirit's +8.9 over a stat rank is most of Holy's 13.4.
  - Pain Suppression and Reflective Shield are each about 4pt over a stat rank,
    against Discipline's 14.7.
- **The Shadow reference loses 5pt in its own form.** A Shadowform Priest heals
  nothing, so its team has no healer. Silence's +6.4 is measured on a stem that
  stands at 38.3%. Shadowform is a choice and never a baseline, but in this sweep
  choosing it was never right on average.
- The healer split below shows where Silence helps.

### Each node comparison by the enemy team: a healer, or none

Delta in points (z), pooled. The healer split is 3,600 matches; the no-healer
split is 2,400.

| node | vs a healer | vs no healer |
|---|---|---|
| Guardian Spirit | +8.4 (13.41) | +9.6 (12.92) |
| Silence, Shadowform | +6.6 (10.09) | +6.0 (8.95) |
| Reflective Shield | +3.1 (4.39) | **+6.4** (9.31) |
| Pain Suppression | +4.1 (6.04) | +3.7 (5.60) |
| Inner Focus | +1.6 (3.30) | +0.1 (0.11) |
| Mind Flay, Shadowform | **−6.7** (8.80) | −1.8 (1.75) |
| Mind Flay, healing | −2.3 (3.60) | −3.6 (4.53) |
| Vampiric Embrace, healing | +0.0 (0.00) | −3.3 (4.42) |
| Holy Nova | −1.5 (2.53) | +1.1 (1.84) |

These are two slices per node, and they count toward the comparisons. The
other rows are in the script's `table` output.

### Non-vacuity

**Control.** In every arm, the 192 control matches (no Priest on either side)
are identical in winner and duration to the 0-point arm's.

- The control fields all 8 other classes on both sides (`paired_sweep.py`'s
  `CONTROL FIELDS` line).
- 6,764-6,785 of every arm's 6,792 matches have a winner (the 0-point arm
  included, at 6,780). Counted by `end_reason` instead, 6,777-6,791 end by
  elimination (0 points: 6,786); the difference is mutual kills, which are
  draws.

**The nodes fire.** The batch CSV carries outcomes only. So `sample` replays a
fixed slice of 60 Priest matches an arm with full logs (seeds 0-3). The Holy
reference gets 600 matches, for Guardian Spirit's save rate.

| arm | Priests | node events |
|---|---|---|
| Discipline | 60 | 46 crowd-control effects shortened (Unbreakable Will); 47 Inner Focus; 856 shield reflects; 42 Pain Suppression |
| Discipline stem + Power Infusion | 60 | 42 Power Infusion |
| Holy | 654 | **381 Guardian Spirit casts, 258 saves**; 323 Inspiration procs |
| Holy stem + Blessed Recovery | 60 | 49 Blessed Recovery procs |
| Holy stem + Holy Nova | 60 | 34 Holy Nova casts, 28 enemy hits, 51 heals |
| Shadow (Shadowform) | 60 | 38 Silence; 103 Mind Flay channels; 1 Blackout stun; 60 Shadowform |
| Shadow stem + Vampiric Embrace (Shadowform) | 60 | 79 Vampiric Embrace casts, 579 VE heals |
| Shadow stem + Vampiric Embrace (healing) | 60 | 42 Vampiric Embrace casts, 75 VE heals |
| Shadow stem + Improved PW:Shield (Shadowform) | 60 | 2 Blackout stuns; 118 Mind Blasts begun, 77 landed |
| Hybrid, Shadowform | 60 | 60 Shadowform, 0 Flash Heals |

**Guardian Spirit's save rate:**

- 258 saves from 381 casts, so **68% of casts save a life**.
- That is **0.39 saves a Priest a match**: the Holy Priest prevents a killing
  blow in roughly two matches of five. The saved teammate may still die later
  in the match.

## What it says (findings for the user, not changes)

1. **Guardian Spirit dominates** by +8.0pt past its allowance, replicated.
   - Its save fires on two casts in three: the trigger (an ally below 25%, or
     below 40% with two attackers) is late enough that a killing blow usually
     follows.
   - A 30%-health heal on top of the save turns a lost teammate into a won
     match. It is worth more than every other Holy node combined.
   - **Direction:** a smaller save heal, or no healing-received bonus beside the
     save. Its 60s cooldown makes it once a match in practice, so the cooldown
     is not the lever.
2. **Reflective Shield dominates** (+4.4pt), and most against comps with no
   healer (+6.4).
   - 30% of every absorb the Priest's shields take is dealt back. That includes
     shields on the Priest's teammates, and it adds up to 14 reflects a Priest a
     match.
   - **Direction:** a smaller share (TBC's is 10% a rank), or the Priest's own
     shield only.
3. **Pain Suppression dominates** (+3.5pt past its allowance).
   - −40% damage taken for 8s on a 45s cooldown, cast 0.7 times a Priest a match.
   - **Direction:** a longer cooldown (TBC's is 2 minutes), or a smaller cut.
4. **Silence dominates inside a losing build.** It is +5.3pt past its
   allowance, but the Shadowform Priest it serves is −5.0pt against 0 points.
   - Silence is what makes Shadowform playable at all.
   - **Direction:** read it with Shadowform below, not alone. Weakening Silence
     would sink the form further.
5. **Inner Focus dominates, marginally:** +1.0pt pooled, 0.3pt past its floor
   and past the correction by a hair, but in neither seed block alone. It fires
   on the emergency Flash Heal, and its gain is in healer-against-healer
   matches (+1.6). Watch it in AS-441 rather than acting on it alone.
6. **Mind Flay is below a stat rank in both roles**, by 2.8 to 4.8pt. That is an
   AI question before it is a tuning one. The counts below are from
   `mechanism` (120 replayed Priest matches an arm, seeds 0-3):

   | | healing, with Mind Flay | healing, without | Shadowform, with | Shadowform, without |
   |---|---|---|---|---|
   | Mind Flay channels | 118 | 0 | 197 | 0 |
   | Seconds channelling a match | 2.4 | 0 | 3.9 | 0 |
   | Channels that ran their full 3s | 88 | – | 122 | – |
   | Cut by an interrupt / crowd control or silence / a death | 25 / 2 / 3 | – | 22 / 7 / 46 | – |
   | Mind Blast hits | 101 | 142 | 140 | 169 |
   | Mind Blast damage | 8.8k | 12.2k | 12.6k | 15.0k |
   | Mind Flay damage | 2.7k | 0 | 4.5k | 0 |
   | Shadow Word: Pain damage | 2.0k | 2.6k | 6.8k | 7.2k |
   | Healing done | 63.8k | 66.1k | 0 | 0 |
   | Direct damage taken (hits, crits, channelled ticks) | **54.1k** | **48.4k** | 38.5k | 37.7k |

   - **Healing:** Mind Flay sits above Mind Blast in the healing chain. It
     displaces a harder hit and some heals for 2.7k of channel damage and a
     slow.
   - **Observed: the healing Priest takes 12% more direct damage with Mind
     Flay** (54.1k against 48.4k). Nothing here shows why.
   - **Shadowform:** Mind Flay adds 4.5k damage against 2.8k of Mind Blast and
     Shadow Word: Pain it displaces, and the Priest takes about the same
     damage. Yet the arm loses 4.8pt, 6.7pt of it against a healer.
   - **Two candidate causes, neither shown:**
     - **The channel roots its caster.** Each channel is measured to its real
       end: the first of an interrupt, crowd control or a silence cutting it,
       its target's or the Priest's death, the Priest's next cast, or 3s.
       Measured that way, the Priest stands channelling **2.4s a match
       healing and 3.9s in Shadowform**. 88 of 118 and 122 of 197 channels
       ran their full 3s. Of the channels its Priest cast again after, 87 of
       115 (healing) and 113 of 137 (Shadowform) ran full. (The tester's
       independent count was 114 of 137. The one-channel difference is an
       interrupt that lands 2.9s or more into the channel, which this count
       treats as a cut.) In Shadowform, 46 channels end with a death, of the
       target or of the Priest.
     - A healing Priest channels Mind Flay as a peel on a melee or pet
       attacking a ranged teammate, the Priest included. As reasoning, not
       measurement: if the standing still costs anything, it costs most there.
     - **The pursuit stand-off.** A Shadowform Priest holding Mind Flay or
       Silence stops at 18 yards instead of 28 (AS-416). This applies in
       Shadowform only. The Silence arm keeps the 18 yards on both sides of its
       comparison, so Silence's figure is not confounded by it.
   - Mind Flay also spends mana and global cooldowns, and its 50% slow is the
     benefit both roles buy with them.
   - **Direction:** attribute the loss before tuning it: the channel's standing
     still (both roles) or the 18-yard stand-off (Shadowform). Then consider
     Mind Flay below Mind Blast in the healing chain, or a peel that does not
     root the healer.
7. **Blackout has nothing to act on.** It rides on Mind Blast hits:
   - a Shadowform Priest lands 1.3 a match (77 of 118 begun, the rest
     interrupted or crowd-controlled);
   - a healing Priest lands 0.7 a match.
   - At 4% a rank that is 1 to 6 stuns in 60 Priest matches across the arms.
     The node measures within a stat rank when healing, and below one in
     Shadowform.
   - **Direction:** a wider trigger (Classic's: any Shadow damage) before any
     change to its chance.
8. **Shadowform costs 10.2pt** on the hybrid, and the Shadow reference in its
   form is the one reference below 0 points.
   - In 2v2 a Shadowform Priest's team has no healer.
   - It lands 1.4 Mind Blasts a match, with 30% of those begun lost.
   - **Direction:** the form's damage, or Mind Blast's survival against
     interrupts, before Silence.
9. **Holy Nova, Blessed Recovery, Unbreakable Will and Vampiric Embrace
   (Shadowform)** sit within a stat rank. Power Infusion, Inspiration's second
   rank, Vampiric Embrace (healing) and Blackout (Shadowform) sit below one
   without surviving the correction. Power Infusion's first-block −1.7 was not
   reproduced (−0.1).

10. **Holy Concentration's price reads low.** At 3.5 it measures +0.6pt over
    an 11-point Spiritual Healing rank (not significant). It is the one Priest
    node the item budget prices, so this is evidence for the price model, which
    weighs mana regeneration at 5.0 a point, rather than for a value change.

Each is a direction for AS-441, not a value: a directional run may not be cited
as the Priest's standing.

## Byte-identity

The reference builds leave untalented play, and builds written as maps, byte
for byte as on `main`.

2,040 seeded matches give identical CSVs from a `main` 27d76940 binary and from
this branch's:

- 900 2v2 `Legacy` on BasicArena;
- 900 2v2 `TeamPlan` on PillaredArena;
- 240 Priest 2v2 `TeamPlan` on TwinPillars, with the Holy build written as a map.

Of those matches, 2,016 are decisive, with 1,352 distinct durations. Four seeded
`--headless` logs are also identical: 2v2 and 3v3, both profiles, three maps.

## Reproducing it

Every arm is a config on this branch's binary, so no patch is needed. The
committed artefacts are the arms' JSONLs and CSVs.

```bash
cargo build --release
S=docs/design/balance/sweeps
# the inputs as run (or regenerate: $S/2026-11-22-as300-arms.py gen <dir> 0,
# and gen <dir2> 4 for the second block)
mkdir -p /tmp/as300 && tar -xzf $S/2026-11-22-as300-arms.jsonl.tar.gz -C /tmp/as300
for f in /tmp/as300/block1/*.jsonl /tmp/as300/block2/*.jsonl; do
  target/release/arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
done
# or take the CSVs as run:
tar -xzf $S/2026-11-22-as300-arms.csv.tar.gz -C /tmp/as300
$S/2026-11-22-as300-arms.py table /tmp/as300/block1:/tmp/as300/block2   # pooled
$S/2026-11-22-as300-arms.py replicate /tmp/as300/block1 /tmp/as300/block2
$S/2026-11-22-as300-arms.py sample /tmp/as300/block1 target/release/arenasim
$S/2026-11-22-as300-arms.py mechanism /tmp/as300/block1 target/release/arenasim
# any one pair, with its control and slices:
scripts/paired_sweep.py /tmp/as300/block1/holy_y.csv /tmp/as300/block1/holy.csv \
  --affects Priest --tier directional --expect nothing --headline affected
```

Every figure above recomputes from the committed CSVs, except two sets of
counts that replay matches through the binary:

- the non-vacuity counts (`sample`, a few minutes);
- the Mind Flay counts in finding 6 (`mechanism`). It replays 120 Priest
  matches each of `shadow_y_heal`, `shadow_mf_y_heal`, `shadow_y` and
  `shadow_mf_y`, every n-th Priest config of seeds 0-3, and sums the Priest's
  channels, damage by spell, healing and damage taken.
  `scripts/tests/test_as300_arms.py` pins how it reads a log.
