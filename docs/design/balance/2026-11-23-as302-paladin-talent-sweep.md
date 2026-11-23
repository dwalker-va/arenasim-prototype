# AS-302: Paladin reference builds and node arms

Tier: **directional**. Success condition (design §4, "Sweeps measure nodes and
reference builds"): **no node dominates.** No node may move the Paladin by more
than the run's resolution floor beyond a stat rank of the same price.

**Verdict: the condition fails on five nodes.** Sacred Shield and The Art of
War dominate pooled and in each seed block. Repentance, Divine Favor and Aura
Mastery dominate pooled but not in each block: marginally. Aura Mastery
clears the corrected bar by 0.07 (margin z 2.94 against 2.87), the thinnest
verdict here. Infusion of Light is past its floor but does not survive the
correction.

| Node | Delta | Allowance | Margin | Margin z | Floor | Verdict | Seeds 0-3 / 4-7 (margin, z) |
|---|---|---|---|---|---|---|---|
| **Sacred Shield** (capstone) | +5.7pt | −0.2 | **+5.8pt** | 8.51 | 1.2 | **dominates** | +5.8 (5.98) / +5.9 (6.05) |
| **The Art of War** | +3.6pt | −0.0 | **+3.6pt** | 7.78 | 0.9 | **dominates** | +3.7 (5.60) / +3.6 (5.40) |
| **Repentance** | +2.1pt | −0.0 | **+2.2pt** | 4.08 | 1.1 | **dominates, marginally** | +2.9 (3.97) / +1.4 (1.87) |
| **Divine Favor** | +1.3pt | +0.1 | **+1.2pt** | 3.58 | 0.7 | **dominates, marginally** | +0.7 (1.58) / +1.7 (3.43) |
| **Aura Mastery** (capstone) | +1.1pt | −0.4 | **+1.4pt** | 2.94 | 0.7 | **dominates, marginally** | +0.7 (1.06) / +2.2 (3.06) |
| Infusion of Light (rank 1) | +0.9pt | +0.1 | +0.8pt | 2.57 | 0.6 | past the floor, not after correction | +0.7 (1.61) / +0.9 (2.02) |

- The rule: a node dominates when its pooled margin over the allowance is past
  the pair's floor AND its margin z survives Bonferroni over the 12 node
  comparisons (z ≥ 2.87). Each block's own verdict is reported beside it; a
  node that dominates pooled but not in each block dominates marginally.
- **The caveat beside the verdict: the stat rank bought nothing here.** A
  Sacred Duty rank is worth −0.6 to +0.1pt on these stems, where the other
  classes' stat ranks bought +0.5 to +2.0pt (AS-299, AS-300, AS-301). Divine
  Favor and Aura Mastery dominate by 1.2 and 1.4pt over a yardstick of about
  zero; against another class's stat rank they would likely sit within. Sacred
  Shield and The Art of War would not.
- **Retribution Aura, chosen, is 3.4pt below a stat rank** (margin z −5.89),
  below in its first block and below its floor without the correction in its
  second. All of it is the choice of aura: the same build keeping Devotion Aura
  plays byte for byte as its stem.

Nothing here is retuned: tuning is AS-441's batch.

Measured at `main` fe739fb6 plus this card's three reference builds. They are
data only, and leave untalented play byte-identical (see *Byte-identity*).
`main` has since gained AS-357 (Vampiric Embrace and Silence visuals) and
AS-367 (Hemorrhage visuals). Neither writes combat state or draws a roll, so
the measurement stands on the merged head.

## Method

### The reference builds

These are in `assets/config/talents.ron`, under `reference_builds`:

| name | shape | ranks | price |
|---|---|---|---|
| Holy | 6/2/0 | Improved Holy Shock 2, Infusion of Light 1, Divine Favor 1, Light's Grace 1, **Aura Mastery** 1; Improved Hammer of Justice 2 | 99 |
| Protection | 2/6/0 | Improved Holy Shock 2; Improved Hammer of Justice 2, Divine Protection 1, Toughness 1, Blessing of Sacrifice 1, **Sacred Shield** 1 | 99 |
| Retribution | 0/2/6 | Improved Hammer of Justice 2; Conviction 2, Vengeance 2, The Art of War 1, **Crusader Strike** 1 | 83 |

- **Holy** is design §5.3's Holy Shock build: Holy Shock on a shorter cooldown
  priming a crit Flash of Light and a faster Holy Light, with Divine Favor and
  Aura Mastery.
- **Protection** cuts, moves and absorbs damage. It takes Toughness rather
  than Retribution Aura, which is a per-match aura choice and is measured as
  its own arm.
- **Retribution** is the melee build: Conviction's crit feeding Vengeance, and
  The Art of War's instant Flash of Light off its swings. It skips
  Two-Handed Weapon Specialization because the Paladin's loadout holds a
  one-hander and a shield, so that node does nothing. Conviction prices at 3 a
  rank (its crit at the item weight), so this reference costs 83, not 99.

Each reference's last point is its capstone. Its **stem** is the reference
without it (7 points). **None takes Sacred Duty**, so the stat rank fits on
every stem.

### The stat rank

**Sacred Duty**, +3% maximum health a rank. It is the Paladin's one computed
`StatPercent` node on a stat the item budget weighs: 448 → 461 health at the
reference Paladin, which prices at **13.44 a rank** (`talent_price_report`).
Toughness is also a `StatPercent` node, but armor is free on the item budget,
so its 11 is declared. Conviction is computed, at 3 a rank.

Sacred Duty's price is not 11, so the allowance is linear in price, as in
AS-301. With `p` the node's price and `k = p / 13.44 − 1` (−0.18 for a
one-rank node, +0.64 for a capstone):

- **v** is what one Sacred Duty rank buys on the node's stem ("stem + Sacred
  Duty" against "stem");
- **allowance** = `k × v`;
- **margin** = delta − allowance;
- **margin z** comes from the per-match paired margin, over the 6,000
  one-sided matches: `x_i = 100 × [(A_i − B_i) − k × (V+_i − V0_i)]`, where
  each term is 1 when the Paladin side wins match i in the node's arm (A), its
  comparison (B), the stem with a Sacred Duty rank (V+) and the stem (V0). The
  margin is `mean(x)`, its error `sd(x)/√n`. The four arms share every match,
  so their errors are correlated; the per-match form counts that.
- **verdict:** *dominates* when the margin is past the pair's floor and its z
  is at least 2.87; *past the floor, not after correction* when only the first
  holds; *below* and *below the floor, not after correction* likewise under
  minus the floor; *within* otherwise. The floor is the raw delta's, from the
  pair's own discordance.

Every `v` measured here is between −0.6 and +0.1pt, so every allowance is
between −0.4 and +0.2pt. The raw delta against Sacred Duty, with its own
McNemar z, is printed beside every verdict.
`scripts/tests/test_as302_arms.py` pins the per-match form and the thresholds.

### The arms

A node comparison is two builds that differ in one point: on the node, or on
Sacred Duty. `each_paladin_node_arm_is_its_comparison_with_one_point_moved`
in `tests/reference_builds.rs` holds every pair to that, and
`scripts/tests/test_as302_arms.py` holds the script's own arms to it and to
the Rust test's list.

- **A node the stem lacks** is added to the stem: `stem + node` against
  `stem + Sacred Duty`.
- **A node the stem holds** is measured by its removal: `stem + Sacred Duty`
  against the stem with the node's point on a second Sacred Duty rank.
- **A capstone** is priced at two ranks. Its arm is the reference itself,
  against `stem + Sacred Duty`.

| node | its arm | comparison |
|---|---|---|
| Infusion of Light (rank 1), Divine Favor, Light's Grace | Holy stem + Sacred Duty | the same with the node's point on Sacred Duty |
| Aura Mastery (capstone) | Holy | Holy stem + Sacred Duty |
| Divine Protection, Blessing of Sacrifice | Protection stem + Sacred Duty | the same with the node's point on Sacred Duty |
| Retribution Aura (rank 1), the aura chosen | Protection stem + Retribution Aura | Protection stem + Sacred Duty |
| Sacred Shield (capstone) | Protection | Protection stem + Sacred Duty |
| Vengeance (rank 2), The Art of War | Retribution stem + Sacred Duty | the same with the node's point on Sacred Duty |
| Repentance | Retribution stem + Repentance | Retribution stem + Sacred Duty |
| Crusader Strike (capstone) | Retribution | Retribution stem + Sacred Duty |

- **Vengeance is measured at its second rank.** The Art of War needs three
  Retribution points below it, so taking both of Vengeance's ranks off the stem
  would leave The Art of War illegal.
- **Retribution Aura is an aura choice.** Rank 1 adds it to the Paladin's aura
  menu, and choosing it means no Devotion Aura that match. Its arm chooses it
  (`team{1,2}_paladin_auras`), so the comparison is the node as a player would
  use it. A twentieth arm, `prot_ra_dev`, holds the same rank and keeps
  Devotion, which splits the arm into the rank and the choice.

### The sweep

Every arm is one directional 2v2 sweep:

- **Cells:** every cell a Paladin can reach, plus 8 control cells:
  `gen_sweep.py --full 2 --exclude-double-healer --affects Paladin
  --control-cells 8`, 283 cells. A Paladin plays as its team's one healer.
- **Maps:** BasicArena, PillaredArena and TwinPillars, 4 seeds a cell a map:
  3,396 matches an arm a block.
- **Profile:** `TeamPlan` on both sides, which is what players get.
- **Builds:** every Paladin, on either side, plays the arm's build. The arms
  differ only in build and aura, so they pair at identical (label, seed).
- **Scoring:** for the side fielding the Paladin (`paired_sweep.py --headline
  affected`), over the 6,000 pooled matches an arm where one side does. The
  600 Paladin mirrors are excluded.
- **Format:** 2v2 only; the directional tier does not call for 3v3.

All 20 arms ran on **two seed blocks**, seeds 0-3 and 4-7: 135,840 matches.
The tables pool the blocks, and every node comparison is also shown per block.

**Multiple comparisons:** 12 node comparisons, so the Bonferroni threshold is
z ≥ 2.87 (two-sided α 0.05/12), applied to the margin z.

## Results

In every table:

- "before" is the comparison arm and "after" the arm under test, both as the
  Paladin side's win rate;
- flips are matches whose winner moved: + toward the Paladin side, − away;
- z beside a delta is that raw delta's McNemar z; a node's verdict reads its
  margin z instead;
- the floor is the smallest delta the pair's own discordance could resolve;
- n is 6,000 scored matches a row unless a block is named (3,000).

### Each node against Sacred Duty in its place

| node | before | after | delta (pt) | flips | z | floor | allowance | margin | margin z | verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| **Sacred Shield** (capstone) | 67.7% | 73.3% | **+5.7** | 1325 (+832/−493) | 9.29 | 1.2 | −0.2 | +5.8 | 8.51 | **dominates** |
| **The Art of War** | 66.8% | 70.4% | **+3.6** | 753 (+485/−268) | 7.87 | 0.9 | −0.0 | +3.6 | 7.78 | **dominates** |
| **Repentance** | 70.4% | 72.6% | **+2.1** | 1084 (+606/−477) | 3.89 | 1.1 | −0.0 | +2.2 | 4.08 | **dominates, marginally** |
| **Divine Favor** | 68.3% | 69.7% | **+1.3** | 408 (+244/−164) | 3.91 | 0.7 | +0.1 | +1.2 | 3.58 | **dominates, marginally** |
| **Aura Mastery** (capstone) | 69.7% | 70.8% | **+1.1** | 438 (+251/−186) | 3.06 | 0.7 | −0.4 | +1.4 | 2.94 | **dominates, marginally** |
| Infusion of Light (rank 1) | 68.8% | 69.7% | +0.9 | 315 (+184/−131) | 2.93 | 0.6 | +0.1 | +0.8 | 2.57 | past the floor, not after correction |
| Blessing of Sacrifice | 66.9% | 67.7% | +0.8 | 714 (+379/−333) | 1.69 | 0.9 | +0.1 | +0.7 | 1.56 | within |
| Crusader Strike (capstone) | 70.4% | 71.2% | +0.7 | 706 (+375/−331) | 1.62 | 0.9 | +0.1 | +0.7 | 1.21 | within |
| Light's Grace | 69.3% | 69.7% | +0.4 | 297 (+160/−137) | 1.28 | 0.6 | +0.1 | +0.3 | 0.94 | within |
| Vengeance (rank 2) | 70.6% | 70.4% | −0.1 | 245 (+118/−126) | 0.45 | 0.5 | −0.0 | −0.1 | −0.44 | within |
| Divine Protection | 68.4% | 67.7% | −0.8 | 377 (+164/−211) | 2.38 | 0.6 | +0.1 | −0.8 | −2.48 | below the floor, not after correction |
| **Retribution Aura** (rank 1, chosen) | 67.7% | 64.3% | **−3.4** | 1270 (+533/−734) | 5.62 | 1.2 | +0.1 | −3.4 | −5.89 | **below** |

### Replication

Each node comparison's margin by block (n=3,000 each; margin z in brackets),
and the verdict each block reaches alone:

| comparison | seeds 0-3 | seeds 4-7 | pooled |
|---|---|---|---|
| Sacred Shield | +5.8 (5.98) dominates | +5.9 (6.05) dominates | +5.8 (8.51) dominates |
| The Art of War | +3.7 (5.60) dominates | +3.6 (5.40) dominates | +3.6 (7.78) dominates |
| Repentance | +2.9 (3.97) dominates | +1.4 (1.87) within | +2.2 (4.08) dominates, marginally |
| Divine Favor | +0.7 (1.58) within | +1.7 (3.43) dominates | +1.2 (3.58) dominates, marginally |
| Aura Mastery | +0.7 (1.06) within | +2.2 (3.06) dominates | +1.4 (2.94) dominates, marginally |
| Infusion of Light | +0.7 (1.61) within | +0.9 (2.02) past the floor | +0.8 (2.57) past the floor |
| Retribution Aura, chosen | −4.6 (−5.60) below | −2.2 (−2.71) below the floor | −3.4 (−5.89) below |
| Divine Protection | −1.0 (−2.12) below the floor | −0.7 (−1.39) within | −0.8 (−2.48) below the floor |

The other four rows are within in each block. The script's `replicate` output
carries every row with its per-block allowance.

### What a Sacred Duty rank buys, and each reference against 0 points

| comparison | before | after | delta (pt) | flips | z | floor |
|---|---|---|---|---|---|---|
| Sacred Duty on the Holy stem | 70.2% | 69.7% | −0.6 | 263 (+114/−148) | 2.04 | 0.5 |
| Sacred Duty on the Protection stem | 68.0% | 67.7% | −0.3 | 325 (+153/−171) | 0.94 | 0.6 |
| Sacred Duty on the Retribution stem | 70.3% | 70.4% | +0.1 | 309 (+157/−152) | 0.23 | 0.6 |
| **Holy 6/2/0** vs 0 points | 66.2% | 70.8% | **+4.6** | 757 (+516/−241) | 9.96 | 0.9 |
| **Protection 2/6/0** vs 0 points | 66.2% | 73.3% | **+7.1** | 1327 (+877/−450) | 11.69 | 1.2 |
| **Retribution 0/2/6** vs 0 points | 66.2% | 71.2% | **+5.0** | 757 (+528/−229) | 10.83 | 0.9 |

- **A Sacred Duty rank buys nothing measurable**: −0.6 to +0.1pt, against
  floors of 0.5-0.6pt. The Holy stem's −0.6 is significant uncorrected only
  (z=2.04), and its two blocks read −0.1 and −1.0.
- **So the line every node is judged against sits at about zero.** That does
  not change a verdict under the rule; it is the caveat beside it.
- **The untalented Paladin side already wins 66.2%** of its one-sided matches
  in this cell set (a directional figure, not the Paladin's standing).
- **Price does not predict power across the trees.** Holy and Protection cost
  the same (99): +4.6 against +7.1pt. Retribution costs 83 and is worth +5.0.
- **Sacred Shield is most of Protection's worth**: the reference is 5.3pt over
  its own stem (`prot_stem vs prot`), of Protection's 7.1.
- **The Art of War is most of Retribution's**: the stem with its point on
  Sacred Duty is 4.3pt under the reference (`ret_noaow_y2 vs ret`), which also
  lacks Crusader Strike (+0.7 over a stat rank).

### Retribution Aura, taken apart

| comparison | before | after | delta (pt) | flips | z | floor |
|---|---|---|---|---|---|---|
| Retribution Aura chosen over Devotion, same build (`prot_ra` vs `prot_ra_dev`) | 68.0% | 64.3% | **−3.6** | 1212 (+495/−714) | 6.27 | 1.2 |
| The rank held, Devotion kept, vs Sacred Duty (`prot_ra_dev` vs `prot_y`) | 67.7% | 68.0% | +0.3 | 325 (+171/−153) | 0.94 | 0.6 |

- **Holding the rank without choosing the aura changes nothing.** `prot_ra_dev`
  is byte-identical, every column of every row in both blocks, to `prot_stem`.
- **So the whole −3.4pt is the choice:** Retribution Aura in Devotion Aura's
  place. The aura fires: 2,622 hits on melee attackers in 120 replayed matches
  (`sample`'s "Retribution Aura hits"), about 80 Holy damage a Paladin a match.
- **Which half of the swap costs it is not shown.** Two candidates:
  - Devotion's 10% damage-taken reduction for the whole team, lost
    (`abilities.ron`'s `DevotionAura`, `DamageTakenReduction` 0.10);
  - Retribution Aura's own side effects: each hit it lands on a Warrior gives
    it rage (`combat_core/follow_ups.rs`, 0.15 rage a point of damage).
- **The enemy team splits them, as evidence, not proof** (the same pair by the
  `team1`/`team2` columns; raw delta, z, n):

  | no melee or pet | with a Warrior | with a Rogue | with a Hunter | with a Warlock |
  |---|---|---|---|---|
  | **−8.5** (5.24) n=720 | −4.0 (3.37) n=1680 | **+3.5** (3.24) n=1680 | −3.6 (3.46) n=1680 | −4.0 (3.62) n=1680 |

  Against a team the aura has nothing to hit (no Warrior, Rogue, or melee pet),
  the whole cost shows and Devotion's loss is all that changes. Against a
  Rogue the aura pays its way. A Warrior costs no more than a Hunter or a
  Warlock, whose pets take the hits and gain no rage, so the rage candidate
  finds no support in this slice.

### Each node by the enemy team: a healer, or none

Raw delta in points (raw McNemar z), pooled; 3,600 matches against a team with
a healer and 2,400 against one without. These are where an effect sits, not
further verdicts.

| node | vs a healer | vs no healer |
|---|---|---|
| Sacred Shield | +5.2 (6.34) | +6.3 (7.01) |
| The Art of War | **+4.7** (7.45) | +2.0 (2.98) |
| Repentance | +1.2 (1.41) | **+3.6** (5.56) |
| Divine Favor | +0.8 (1.81) | +2.1 (3.97) |
| Aura Mastery | +0.9 (1.74) | +1.4 (2.91) |
| Infusion of Light | +0.8 (1.96) | +1.0 (2.26) |
| Blessing of Sacrifice | −0.1 (0.05) | +2.0 (2.97) |
| Crusader Strike | −0.1 (0.04) | +1.9 (3.21) |
| Light's Grace | −0.1 (0.22) | +1.1 (2.49) |
| Vengeance (rank 2) | −0.3 (0.73) | +0.1 (0.11) |
| Divine Protection | −1.2 (2.86) | −0.1 (0.16) |
| Retribution Aura, chosen | −2.9 (3.66) | −4.1 (4.38) |

### Non-vacuity

**Control.** In every arm, the 192 control matches (no Paladin on either side)
are identical in winner and duration to the 0-point arm's. The control fields
all 8 other classes on both sides, with no blind spot (`table`'s control
lines). 6,783-6,789 of each arm's 6,792 matches end by a kill; the rest reach
the cap.

**The nodes fire.** The batch CSV carries outcomes only. So `sample` replays a
fixed slice of 120 Paladin matches an arm (seeds 0-3, 132 Paladins) with full
logs:

| arm | node events in 120 matches |
|---|---|
| Holy | Infusion of Light 361 procs, 349 spent; Light's Grace 361 procs, **42 spent**; Holy Light 55 heals; Divine Favor 74 casts, 74 spent; Aura Mastery 11 casts |
| Protection | Divine Protection 60 casts; Blessing of Sacrifice 92 casts, 675 transfers; **Sacred Shield 328 casts, 741 shields** |
| Protection stem + Retribution Aura | 2,622 Retribution Aura hits |
| Retribution | Vengeance 66 procs; The Art of War 159 procs, 148 spent; Crusader Strike 235 |
| Retribution stem + Repentance | Repentance 93 casts, 93 landed, 45 of them on a healer; **25 dispelled, 0 broken by damage** |

The full table, with Holy Shock, heal and melee-swing counts per arm, is the
script's `sample` output. Paladins land 5 to 6.5 melee swings a match in every
arm (641-854 in 120 matches, 132 Paladins), so the melee procs have swings to
ride on.

## What it says (findings for the user, not changes)

1. **Sacred Shield dominates** by +5.8pt, in each block.
   - It is cast 2.5 times a Paladin a match and raises 5.6 shields of 40 a
     Paladin a match (`sample`'s "Sacred Shield casts" and "Sacred Shield
     shields" columns): about 220 health of shields raised a match, against
     Sacred Duty's 13 maximum health. How much of that is absorbed the log
     slice does not count.
   - It gains against both enemy kinds (+5.2 / +6.3).
   - **Direction:** a smaller shield, or a longer interval between shields
     (6s today). It lasts 30s on one ally at a time with no cooldown, so it can
     cover an ally all match.
2. **The Art of War dominates** by +3.6pt, in each block.
   - It procs 1.2 times a Paladin a match and is spent 93% of the time
     (`sample`'s "Art of War procs" and "Art of War spent").
   - Its gain is largest against a team with a healer (+4.7 against +2.0). Two
     candidate causes, neither shown:
     - the instant, free Flash of Light itself, cast while moving or under
       pressure;
     - matches against a healer run longer (`duration_secs` means for
       `ret_y`: 64.5s against a healer, 46.2s without), so a Paladin swings
       more, procs more (the rate is per minute of melee), and feels more mana
       pressure for the free half to relieve.
   - **Direction:** a lower rate (4.8 a minute today), or instant without free.
3. **Repentance dominates, marginally** (+2.2pt, margin z 4.08; its second
   block alone is within).
   - It is cast 0.7 times a Paladin a match; 45 of 93 land on a healer, 25 are
     dispelled and none is broken by damage (`sample`'s "Repentance lands",
     "lands on a healer", "dispelled" and "broken by damage" columns).
   - Its gain sits against teams with no healer (+3.6 against +1.2). Candidate
     causes, none shown:
     - with no healer to take, it incapacitates one of two damage dealers;
     - it is Magic (TBC 20066), so a Priest's Dispel Magic or a Warlock's
       Felhunter takes it off. Sliced by the enemy team (`team1`/`team2`;
       raw delta, z, n):

       | healer, dispeller | healer, no dispeller | no healer, dispeller | no healer, no dispeller |
       |---|---|---|---|
       | +0.5 (0.41) 1,680 | +1.7 (1.48) 1,920 | +1.8 (1.78) 960 | **+4.9** (5.49) 1,440 |

       Within each healer stratum a dispeller halves the gain or more, which
       leans toward the dispel; only the last cell resolves on its own, so
       the slice suggests and does not show it;
     - any damage breaks it. The replayed slice finds no break in 93, so this
       one has no support there.
   - **Direction:** watch it in AS-441; a longer cooldown (30s today) if it
     holds up.
4. **Divine Favor and Aura Mastery dominate, marginally**, by 1.2 and 1.4pt,
   each in its second block only. Aura Mastery's margin z, 2.94, clears the
   2.87 bar by 0.07: a slightly different draw would put it past its floor
   without the correction, as Infusion of Light is. Beside that verdict: the yardstick bought
   nothing (item 7), so against another class's stat rank they would likely
   sit within. **Direction:** watch them in AS-441 rather than act on them
   alone.
5. **Infusion of Light is past its floor** (+0.8pt) but its margin z, 2.57,
   does not survive the correction, and neither block alone does.
6. **Retribution Aura, chosen, costs 3.4pt**, and all of it is the choice of
   aura (the split above). The rank held unchosen is inert. Of the two
   candidates, Devotion's 10% damage reduction lost and the aura's own side
   effects, the enemy-team slice favours the first: −8.5pt where the aura has
   nothing to hit, +3.5pt against a Rogue.
   - **Direction:** a stronger Retribution Aura, or one that does not replace
     Devotion. Raising its damage alone has to buy back 3.6pt.
7. **Sacred Duty, the stat rank, buys nothing measurable** (−0.6 to +0.1pt).
   That is a finding about the yardstick: the line here sits at zero, where
   the other classes' stat ranks bought +0.5 to +2.0pt. A different yardstick
   would move items 3 to 5, not 1 and 2.
8. **Light's Grace is within, and is spent on 12% of its procs** (42 of 361,
   `sample`'s "Light's Grace spent" against "Light's Grace procs"). The cause
   is that Holy Light is rare, not its timing: the Holy Paladin casts 55 Holy
   Lights in 120 matches (`sample`'s "Holy Light heals", 0.42 a Paladin a
   match), and 42 of those, 76%, carry the proc. **Direction:** an AI question
   (when the Paladin casts Holy Light at all) before a value change.
9. **Vengeance (second rank), Crusader Strike and Blessing of Sacrifice** sit
   within a stat rank. **Divine Protection** is below its floor (−0.8pt,
   margin z −2.48) without surviving the correction.

Each is a direction for AS-441, not a value: a directional run may not be cited
as the Paladin's standing.

## Byte-identity

The reference builds are data only. They leave untalented play, and builds
written as maps, byte for byte as on `main`.

A binary built with `main` fe739fb6's `talents.ron` and this branch's binary
give identical CSVs over 4,862 seeded matches:

- 3,396 2v2 `TeamPlan` on all three maps, untalented (the `zero` arm's first
  block);
- 900 2v2 `Legacy` on BasicArena, untalented;
- 566 Paladin 2v2 `TeamPlan` on PillaredArena, with the Holy build written as
  a map.

Of those matches, 4,858 are decisive, with 2,805 distinct durations.

## Reproducing it

Every arm is a config on this branch's binary, so no patch is needed. The
committed artefacts are the arms' JSONLs and CSVs.

```bash
cargo build --release
S=docs/design/balance/sweeps
# the inputs as run (or regenerate: $S/2026-11-23-as302-arms.py gen <dir> 0,
# and gen <dir2> 4 for the second block)
mkdir -p /tmp/as302 && tar -xzf $S/2026-11-23-as302-arms.jsonl.tar.gz -C /tmp/as302
for f in /tmp/as302/block1/*.jsonl /tmp/as302/block2/*.jsonl; do
  target/release/arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
done
# or take the CSVs as run:
tar -xzf $S/2026-11-23-as302-arms.csv.tar.gz -C /tmp/as302
$S/2026-11-23-as302-arms.py table /tmp/as302/block1:/tmp/as302/block2   # pooled
$S/2026-11-23-as302-arms.py replicate /tmp/as302/block1 /tmp/as302/block2
$S/2026-11-23-as302-arms.py sample /tmp/as302/block1 target/release/arenasim 120
# any one pair, with its control and slices:
scripts/paired_sweep.py /tmp/as302/block1/prot_y.csv /tmp/as302/block1/prot.csv \
  --affects Paladin --tier directional --expect nothing --headline affected
```

Every figure above recomputes from the committed CSVs, except the non-vacuity
counts, which replay matches through the binary (a few minutes).
