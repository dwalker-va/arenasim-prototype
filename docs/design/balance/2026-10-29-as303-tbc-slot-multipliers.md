# E0: TBC slot budget multipliers — the 0.9 baseline

**Card:** AS-303. **Tier: AUTHORITY** (`sweep-tiers.md`): this re-baselines every
class, and the rest of 0.9 (E1, AS-399's target-focus tuning) measures against it.
**Measured commits:** the authority sweep (§3, §4): before `a8a30775` (main as
0.9 began); after `9544e24e` (that commit plus E0's multipliers and a kit re-spent
in each class's own stat mix). Main did not move during it: none of the in-flight
0.9 cards (Life Tap, Holy Light, Rogue poisons, dodge/parry, Shadow Word: Pain)
had merged when it finished. The Hunter's attack-power re-spend (user ruling,
2026-10-30) came after, and is measured on its own (§3.2, DIRECTIONAL): before
`269e4f11` (`9544e24e` merged with main at `e824cb88`, Holy Light), after that
commit with the re-spend.
**Harness:** `arenasim --batch`, 300s cap, default loadouts. Maps: BasicArena
(Basic), PillaredArena (Nagrand), TwinPillars (Twin).

**Seeds and replication.** The 0.8 milestone sweep's seeds and blocks
(`2026-10-25-0.8-milestone-sweep.md`), so every row has a twin in the 0.8 files
at the same cell and seed: 2v2 801-804 / 811-814 / 821-824, 3v3 801 / 811 / 821.
An effect is called an effect only when all three blocks agree in sign and the
pooled McNemar |z| is at least 1.96 (**R** in the scorer output; `*` in the
tables below).

## Verdict

1. **TBC's slot modifiers are confirmed from the TBC client** (§1): the groups
   are 1.0 / 0.75 / 0.5625, legs are a full slot, the waist goes with
   shoulders, hands and feet, and the wrists with neck, back and ring. The
   proposal's "about 0.77" is 0.75. Trinket 0.70 and ranged 0.5625 are rulings
   the client cannot settle.
2. **Every combatant's kit rises by 18.5-19.5 budget points**, in its own stat
   mix, the Hunter's in attack power (§2). Not byte-identical: 72% of the paired matches change winner, end
   reason or duration.
3. **E0 barely moves the standings.** Overall competitive win rate is unchanged
   by construction (2v2 +0.0, 3v3 +0.1). One class moves more than a point:
   **the Hunter loses −1.6pt in 2v2 and −2.3pt in 3v3** (z=−10.6 / −5.1, every
   block, every 2v2 profile and map but Legacy Twin) with its kit re-spent in its
   own stat mix. Re-spent into attack power instead (the shipped kit, §3.2), it
   wins back **+0.5pt** of the 2v2 loss (z=3.7, every block). The other replicated
   2v2 moves are within ±0.6pt: Warrior +0.6, Paladin +0.6, Mage +0.5,
   Shaman +0.5, Rogue +0.4, Druid −0.5. In 3v3 the Shaman gains +1.3.
4. **TeamPlan still beats Legacy head-to-head by the 0.8 margins** (§4): 2v2
   competitive **+5.4pt** (0.8: +5.5), 3v3 **+5.4pt** (0.8: +4.8); Basic /
   Nagrand / Twin +1.5 / +4.5 / +10.3 in 2v2. Paired against main, E0 moves
   the head-to-head effect by −0.2pt (2v2 competitive, ns).
5. **The Paladin is still the strongest class** (2v2 competitive 59.7-67.5%
   on every map and profile) and the Hunter and Warlock the weakest under
   Legacy. E0 widens the Hunter's gap (by about 1.1pt in 2v2 once its kit
   goes to attack power) and leaves the Paladin on top (+0.6).

## 1. TBC's slot modifiers, from the TBC client

The proposal's figures (§6.1 of `docs/design/proposals/2026-10-11-talents-and-gauntlet.md`)
were from memory. They are checked here against the TBC Classic client's own tables
(build 2.5.6.69795 on wago.tools), by
`sweeps/2026-10-29-as303-tbc-slot-modifiers.py`.

**The values.** `RandPropPoints` holds, for every item level and quality, the stat
budget of five slot groups (`Epic_0..4`, `Superior_0..4`, `Good_0..4`). Each column
over column 0, across every TBC item level (80-164):

| column | epic median | range | superior | good |
|---|---|---|---|---|
| 0 | 1.000 | — | 1.000 | 1.000 |
| 1 | **0.750** | 0.729-0.763 | 0.753 | 0.750 |
| 2 | **0.561** | 0.551-0.576 | 0.561 | 0.561 |
| 3 | **0.424** | 0.408-0.439 | 0.425 | 0.420 |
| 4 | **0.317** | 0.305-0.330 | 0.317 | 0.316 |

So the groups are 1.0 / 0.75 / 0.5625 / 0.42 / 0.32. The proposal's "about 0.77" for
the shoulder group is 0.75 in the client; its 0.56 and 0.32 hold.

**Which slot is in which group.** The client chooses a slot's column in code, not
data, so it is read off the items: every TBC item (required level 61-70) with no
spell and no socket, whose stats are all primary attributes or ratings, valued as TBC
prices an item, (Σ stat^1.5)^(1/1.5), over column 0 at its item level and quality,
normalised to the head/chest/legs median:

| slot | n | median | q1-q3 | nearest column |
|---|---|---|---|---|
| Two-hand | 28 | 1.040 | 0.975-1.046 | 0 |
| Legs | 47 | 1.037 | 0.900-1.101 | 0 |
| Head | 11 | 0.970 | 0.843-1.043 | 0 |
| Chest | 22 | 0.929 | 0.603-1.080 | 0 |
| **Waist** | 33 | 0.813 | 0.744-0.852 | **1** |
| Feet | 42 | 0.800 | 0.710-0.857 | 1 |
| Shoulder | 9 | 0.760 | 0.722-0.847 | 1 |
| Hands | 23 | 0.721 | 0.639-0.794 | 1 |
| Shield | 11 | 0.613 | 0.579-0.630 | 2 |
| Neck | 35 | 0.577 | 0.367-0.597 | 2 |
| **Wrist** | 21 | 0.554 | 0.358-0.641 | **2** |
| Finger | 54 | 0.553 | 0.451-0.633 | 2 |
| Back | 32 | 0.496 | 0.388-0.573 | 2 |
| One-hand | 30 | 0.415 | 0.378-0.445 | 3 |
| Ranged / thrown / wand | 19 | 0.31-0.35 | — | 4 |
| Trinket | 0 | — | — | — |

Every slot lands in the group the proposal put it in. **Legs** are a full slot,
the **waist** goes with shoulders, hands and feet, and the **wrists** with neck,
back and ring. The item reading is noisy (armour and quest rewards are priced
loosely, hence the wide quartiles), but no median is nearer another group's column.

**Two numbers are rulings, not readings.**

- **Trinket 0.70.** No TBC trinket can be read: every one carries a spell, so none is
  valued by its stats alone, and the client has no trinket column of its own. 0.70 is
  the user's ruling (2026-10-12), above the small accessories because a trinket's
  effect is its value.
- **Ranged 0.5625**, not the client's 0.32: moving it would cut every bow's budget and
  re-baseline the Hunter for no gain (§6.1).

## 2. What changed

| slot | before | after |
|---|---|---|
| Head, chest | 1.0 | 1.0 |
| **Legs** | 0.875 | **1.0** |
| Shoulders, hands, feet | 0.75 | 0.75 |
| **Waist** | 0.625 | **0.75** |
| **Wrists** | 0.5 | **0.5625** |
| Neck, back, ring | 0.5625 | 0.5625 |
| **Trinket** | 0.5625 | **0.70** |
| Ranged | 0.5625 | 0.5625 |
| Hands held (two-hand / one-hand / off-hand) | 1.0 / 0.42 / 0.5625 | unchanged |

**The starting kit re-spent.** Every starting-kit leg, waist, wrist and trinket
spends its slot's budget RISE (ilvl × 0.75 × the multiplier's change). Every
class but the Hunter spends it across the item's existing budgeted stats, close to
their existing proportions: whole points (mana regen in 0.5 steps) chosen to land
nearest the rise, so a small stat may not move (the Magister's Bracers keep 3
health) and crit is left as it was (one 0.01 step is 3 points). The Hunter's legs,
belt and bracers spend all of theirs on attack power (user ruling, 2026-10-30;
§3.2), keeping their pre-E0 health and mana. Its trinket, Mark of the Champion,
is shared with the Warrior and the Rogue and keeps the common re-spend (+2 attack
power, +2 spell power). Every combatant gains about the same:

| class | legs | waist | wrists | trinket | rise (budget points) |
|---|---|---|---|---|---|
| Warrior | +5.5 | +5.0 | +2.5 | +6.0 | **19.0** |
| Paladin | +5.5 | +5.5 | +2.5 | +5.5 | **19.0** |
| Hunter | +6.0 (+4 AP) | +4.5 (+3 AP) | +3.0 (+2 AP) | +6.0 | **19.5** |
| Rogue | +5.5 | +5.0 | +2.5 | +6.0 | **19.0** |
| Mage, Priest, Warlock | +5.5 | +5.0 | +2.5 | +5.5 | **18.5** |
| Shaman, Druid | +5.5 | +5.0 | +2.5 | +5.5 | **18.5** |

The budget itself rises 18.8-19.0 points per combatant (the proposal's "about 25"
counted two trinkets; every starting kit wears one). Items outside the starting kit
(the ilvl 69-75 sets and the proc trinkets) were not re-spent: they keep their stats,
with more headroom under the new caps.

## 3. What E0 moves, paired

Every uniform match (both teams Legacy, or both TeamPlan) played by main and by
main with E0 at the same cell and seed; each side scored after against before
(`2026-10-29-as303-score.py e0`). 104,112 paired matches: 2v2 the full 36×36
matrix at 12 seeds per map and profile; 3v3 the 0.8 sweep's 600-cell `sample3`
(teams with one or two healers) at 3 seeds. 75,238 (72.3%) change winner, end
reason or duration.

**Competitive, every profile and map pooled:**

| class | 2v2 before → after | delta | per block | z | 3v3 before → after | delta | per block | z |
|---|---|---|---|---|---|---|---|---|
| **Hunter** | 46.3 → 44.7 | **−1.6** | −1.5 / −1.2 / −2.0 | −10.6 **R** | 43.9 → 41.6 | **−2.3** | −2.6 / −1.9 / −2.4 | −5.1 **R** |
| Warrior | 46.4 → 47.0 | **+0.6** | +0.8 / +0.3 / +0.7 | +4.1 **R** | 55.6 → 55.4 | −0.3 | | ns |
| Paladin | 62.6 → 63.2 | **+0.6** | +0.6 / +0.6 / +0.5 | +2.7 **R** | 53.8 → 54.6 | +0.8 | | ns |
| Mage | 54.4 → 54.9 | **+0.5** | +0.6 / +0.6 / +0.5 | +3.7 **R** | 53.2 → 54.0 | +0.8 | | ns |
| Shaman | 47.3 → 47.8 | **+0.5** | +0.1 / +0.0 / +1.4 | +2.6 **R** | 42.7 → 44.0 | **+1.3** | +1.2 / +1.5 / +1.1 | +3.1 **R** |
| Rogue | 51.3 → 51.6 | **+0.4** | +0.1 / +0.7 / +0.4 | +2.5 **R** | 55.9 → 55.8 | −0.1 | | ns |
| Druid | 48.2 → 47.7 | **−0.5** | −0.4 / −0.9 / −0.1 | −2.1 **R** | 45.6 → 45.6 | +0.1 | | ns |
| Priest | 50.6 → 50.4 | −0.2 | | ns | 50.0 → 49.6 | −0.5 | | ns |
| Warlock | 42.9 → 42.7 | −0.2 | | ns | 47.6 → 48.3 | +0.7 | | ns |

2v2 n = 34,560 sides per DPS, 21,600 per healer; 3v3 6,246-7,956. Floors (95%,
from the discordance): 0.3-0.5pt in 2v2, 0.8-0.9pt in 3v3. Nine class slices per
format, so a lone |z| near 2 is what nine tries produce; the Hunter's is not.

**The Hunter is the one class E0 moves by more than a point**, and it moves on
every 2v2 profile and map but Legacy TwinPillars (§3.1), and in 3v3 under both
profiles. These figures are for the Hunter kit re-spent in its own stat mix,
mostly health and mana (+6 / +4, against +2 attack power); §3.2 measures the
shipped kit, which puts the Hunter's whole rise into attack power.

### 3.1 The new baseline: class standings after E0

Win rate of every match a class's comp plays, draws counted as non-wins,
competitive sides (2v2: at most one healer each; 3v3: one or two). In brackets,
E0's paired effect on that cell; `*` marks one that replicates (all three blocks
one sign, |z| ≥ 1.96). These are the standings 0.9's later sweeps cite.

#### 2v2 competitive

| class | Legacy Basic | Legacy Nagrand | Legacy Twin | TeamPlan Basic | TeamPlan Nagrand | TeamPlan Twin |
|---|---|---|---|---|---|---|
| Paladin | 62.8 (+1.1*) | 61.4 (+1.1) | 59.7 (+0.1) | 67.0 (+1.0) | 60.6 (+0.2) | 67.5 (+0.1) |
| Mage | 59.1 (+1.5*) | 53.2 (+0.7) | 53.8 (-0.1) | 56.8 (+1.0*) | 51.2 (+0.6) | 55.4 (-0.4) |
| Rogue | 49.7 (+0.4) | 57.6 (-0.2) | 58.4 (+0.2) | 45.1 (-0.2) | 49.8 (+0.7) | 49.3 (+1.4*) |
| Priest | 50.0 (-0.0) | 54.4 (-0.3) | 52.8 (-0.9) | 48.9 (-0.1) | 51.7 (+0.5) | 44.4 (-0.7) |
| Shaman | 49.1 (-0.2) | 42.1 (+0.5) | 56.0 (+1.4*) | 49.9 (-0.2) | 46.0 (+0.5) | 43.6 (+1.0*) |
| Druid | 48.9 (-0.3) | 52.2 (-0.4) | 50.2 (-0.1) | 42.6 (-1.1*) | 48.0 (-1.2*) | 44.2 (+0.4) |
| Warrior | 46.8 (+0.2) | 46.8 (+1.0*) | 41.9 (+0.2) | 50.5 (+0.8*) | 49.4 (+0.8) | 46.6 (+0.5) |
| Hunter | 44.9 (-1.8*) | 42.4 (-1.6*) | 41.4 (-0.5) | 44.9 (-1.9*) | 45.9 (-1.8*) | 48.8 (-1.8*) |
| Warlock | 39.5 (-0.4) | 39.0 (-0.6) | 39.0 (-0.2) | 45.0 (+0.9*) | 46.7 (-0.6) | 47.0 (-0.0) |

The Hunter rows above are for its stat-mix kit. With the shipped attack-power
kit (§3.2) its 2v2 competitive standing is **46.0 / 42.9 / 41.8** (Legacy Basic /
Nagrand / Twin) and **45.6 / 45.8 / 49.2** (TeamPlan), on main with Holy Light
(`e824cb88`), which moves only TeamPlan cells with a Paladin. Its 3v3 rows were
not re-measured.

#### 3v3 competitive

| class | Legacy Basic | Legacy Nagrand | Legacy Twin | TeamPlan Basic | TeamPlan Nagrand | TeamPlan Twin |
|---|---|---|---|---|---|---|
| Rogue | 54.7 (-0.5) | 59.9 (-1.5) | 61.8 (+0.7) | 52.8 (-0.7) | 54.1 (-0.6) | 51.5 (+2.2) |
| Warrior | 53.3 (-1.1) | 53.8 (-0.2) | 50.3 (-1.7) | 59.5 (+0.6) | 58.7 (+1.8) | 56.7 (-1.0) |
| Paladin | 54.5 (-0.2) | 55.0 (+2.0) | 53.5 (+1.7) | 54.9 (+0.5) | 56.8 (+0.0) | 53.1 (+1.0) |
| Mage | 55.6 (+0.7) | 53.6 (+2.1) | 53.4 (+1.5) | 54.7 (-0.5) | 51.7 (+0.2) | 54.9 (+0.9) |
| Priest | 50.7 (-0.8) | 52.1 (-1.1) | 49.6 (-0.1) | 49.5 (+0.0) | 48.2 (-0.1) | 47.2 (-0.7) |
| Warlock | 48.3 (+2.6*) | 45.3 (+0.7) | 45.8 (-0.5) | 48.8 (+1.8) | 50.1 (-0.8) | 51.5 (+0.3) |
| Druid | 46.7 (+0.3) | 46.3 (+0.9) | 48.0 (-0.2) | 46.5 (-0.8) | 43.7 (+0.5) | 42.5 (-0.3) |
| Shaman | 47.9 (+1.6) | 43.8 (-0.3) | 48.0 (+0.9) | 44.4 (+3.1*) | 39.0 (+2.9*) | 40.8 (-0.4) |
| Hunter | 37.5 (-2.7*) | 40.1 (-0.8) | 38.9 (-1.3) | 38.3 (-4.2*) | 44.8 (-2.8*) | 50.2 (-1.9) |

2v2 n = 5,760 sides per DPS and 3,600 per healer per cell; 3v3 1,041-1,326.
Each cell's three blocks agree within 3.8pt (2v2) and 5.5pt (3v3); every
per-block value is in the scorer output.

**Against 0.8.** The before arm is main, not the 0.8 commit, and plays 12 of
the 0.8 sweep's 30 2v2 seeds, so it is not paired with the 0.8 tables. Its
levels sit within a point or two of them (2v2 TeamPlan Basic Paladin 66.0
against 65.8, Legacy Basic Mage 57.6 against 57.7).

**Non-vacuity.** Every bank is decisive in 92-99.7% of its matches. Draws,
before → after: Legacy 2v2 2.28 / 2.49 / 2.58% → 2.13 / 2.48 / 2.62% (Basic /
Nagrand / Twin); TeamPlan 2v2 1.17 / 7.57 / 7.29% → 1.25 / 7.74 / 7.29% (the
TeamPlan pillar-map draws are still the double-healer cells: competitive cells
draw 0.67-0.83%); 3v3 0.28-1.44% → 0.28-1.11%. Median durations move by at most
1.3s. Distinct durations: 5,300-6,000 per 2v2 bank, 1,490-1,550 per 3v3 bank.

**Comps.** TeamPlan's #1 2v2 comp is still Warrior+Paladin on every map
(74.0 / 73.8 / 78.1, Basic / Nagrand / Twin), and Mage+Warlock is still #2 on
Nagrand (69.0, from 72.1). Legacy's tops are unchanged: Mage+Shaman on Basic
(76.0), Rogue+Druid on the pillar maps (75.8 / 74.2).

### 3.2 The Hunter's kit re-spent into attack power

User ruling (2026-10-30): the Hunter's E0 rise goes to attack power. Its legs,
belt and bracers keep their pre-E0 health and mana and spend the whole rise on
attack power: +4, +3 and +2 (legs 4→8, belt 3→6, bracers 2→4), a rise of 13.5
budget points against 13.0 in the stat-mix kit. Its trinket, Mark of the
Champion, is shared with the Warrior and the Rogue and keeps the common
re-spend.

**DIRECTIONAL** (`sweep-tiers.md`), paired: the stat-mix kit (`269e4f11`) against
the attack-power kit, the 2v2 cells of the authority bank that field a Hunter on
either side (512 per map and profile) plus 11 control cells without one, on all
three maps under both profiles, at the same 12 seeds in three blocks: 37,656
matches per arm (`2026-10-30-as303-hunter-bank.py`).

- **Control:** 792 matches without a Hunter, fielding every other class on both
  sides, identical in winner, end reason and duration. 20,185 of the 36,864
  Hunter matches (55%) change.
- **Non-vacuity:** 37,220 of 37,656 decisive.

| slice (competitive) | n | stat mix | attack power | delta | per block | z |
|---|---|---|---|---|---|---|
| **Hunter, all** | 34,560 | 44.7 | 45.2 | **+0.5** | +0.8 / +0.5 / +0.4 | +3.7 **R** |
| Hunter, Legacy | 17,280 | 42.9 | 43.6 | **+0.7** | +1.2 / +0.5 / +0.3 | +3.4 **R** |
| — Basic | 5,760 | 44.9 | 46.0 | **+1.0** | +1.0 / +1.0 / +1.1 | +3.0 **R** |
| — Nagrand | 5,760 | 42.4 | 42.9 | +0.6 | +1.5 / +0.4 / −0.2 | +1.7 |
| — Twin | 5,760 | 41.4 | 41.8 | +0.4 | +1.1 / +0.2 / −0.1 | +1.2 |
| Hunter, TeamPlan | 17,280 | 46.5 | 46.9 | +0.4 | +0.4 / +0.4 / +0.4 | +1.9 |
| — Basic | 5,760 | 45.0 | 45.6 | +0.6 | +0.1 / +0.7 / +0.9 | +1.5 |
| — Nagrand | 5,760 | 45.6 | 45.8 | +0.2 | +0.5 / +0.4 / −0.2 | +0.6 |
| — Twin | 5,760 | 48.8 | 49.2 | +0.5 | +0.6 / +0.2 / +0.6 | +1.2 |

Floors 0.3pt (all), 0.4pt (per profile), 0.5-0.8pt (per map). The re-spend wins
back about a third of E0's 2v2 Hunter loss (−1.6 → about −1.1 against main);
every slice is positive, and the pooled one and Legacy's replicate. The 3v3 was
not re-measured: the attack-power kit should recover a similar share there, but
that is untested.

## 4. TeamPlan against Legacy, head-to-head, after E0

The 0.8 sweep's §2 design on the E0 build: the TeamPlan side's result against
the same side in the all-Legacy twin (same cell, seed and seat), both seats.
2v2: every cell of the 36×36 matrix on every map, 12 seeds (93,312 sides);
3v3: the 600-cell `sample3`, 3 seeds (10,800 sides).

| slice | n | Legacy | TeamPlan | effect | per block | z | 0.8 |
|---|---|---|---|---|---|---|---|
| **2v2 competitive** | 64,800 | 49.5 | 54.9 | **+5.4** | +5.0 / +5.5 / +5.7 | +26.9 | +5.5 |
| — Basic | 21,600 | 49.6 | 51.1 | +1.5 | +0.9 / +2.4 / +1.1 | +4.7 | +1.2 |
| — Nagrand | 21,600 | 49.4 | 53.8 | +4.5 | +4.3 / +4.1 / +4.9 | +12.7 | +4.8 |
| — Twin | 21,600 | 49.5 | 59.8 | **+10.3** | +9.8 / +10.1 / +11.0 | +27.2 | +10.5 |
| 2v2 every cell | 93,312 | 48.8 | 50.6 | +1.8 | +1.6 / +1.8 / +1.9 | +11.0 | +1.7 |
| 2v2 no-healer side | 25,920 | 46.3 | 53.8 | +7.5 | +7.6 / +7.1 / +7.8 | +31.1 | +7.4 |
| 2v2 double-healer side | 15,552 | 20.2 | 18.5 | −1.7 | −1.3 / −1.8 / −2.1 | −5.4 | −2.2 |
| — on Nagrand | 5,184 | 19.6 | 16.2 | −3.5 | −3.6 / −3.1 / −3.6 | −5.8 | −4.7 |
| **3v3 competitive** | 10,800 | 49.8 | 55.3 | **+5.4** | +5.4 / +4.8 / +6.1 | +9.7 | +4.8 |
| — Basic | 3,600 | 49.9 | 50.2 | +0.3 | −0.3 / −0.3 / +1.6 | ns | −0.2 |
| — Nagrand | 3,600 | 49.9 | 53.4 | +3.5 | +4.5 / +2.6 / +3.4 | +3.5 | +2.9 |
| — Twin | 3,600 | 49.8 | 62.3 | **+12.5** | +12.1 / +12.1 / +13.4 | +12.1 | +11.5 |

**By class, competitive, on the pillar maps** (the TeamPlan side fields the class):

| class | 2v2 | per block | 3v3 | per block | 0.8 (2v2 / 3v3) |
|---|---|---|---|---|---|
| Warlock | **+12.1** | +11.5 / +11.8 / +13.1 | +8.7 | +8.2 / +8.7 / +9.2 | +12.0 / +7.7 |
| Hunter | **+11.5** | +11.2 / +11.0 / +12.4 | **+13.3** | +14.9 / +9.9 / +15.2 | +11.6 / +13.3 |
| Shaman | +9.5 | +7.2 / +11.0 / +10.4 | +7.7 | +7.7 / +7.2 / +8.2 | +11.1 / +6.5 |
| Warrior | +8.3 | +7.5 / +8.6 / +8.8 | **+11.4** | +11.0 / +10.2 / +13.1 | +8.6 / +10.8 |
| Paladin | +7.6 | +7.6 / +7.7 / +7.5 | +8.5 | +8.9 / +8.7 / +7.9 | +7.3 / +9.8 |
| Mage | +7.0 | +6.7 / +7.3 / +7.0 | +6.5 | +8.0 / +6.5 / +5.1 | +7.0 / +5.6 |
| Priest | +3.9 | +3.6 / +3.6 / +4.3 | +7.2 | +7.4 / +6.3 / +7.8 | +3.0 / +5.8 |
| Rogue | +2.4 | +3.7 / +1.0 / +2.6 | +3.7 | +2.3 / +4.6 / +4.0 | +3.3 / +2.6 (ns) |
| Druid | +1.3 (ns) | +1.0 / −0.2 / +3.1 | +5.3 | +6.1 / +4.3 / +5.5 | +2.1 / +3.2 |

Every effect replicates except the 2v2 Druid (z=2.0, block 2 −0.2). The 0.8
column is the 0.8 sweep's (main at `40c75048`, 12 seeds at the same blocks),
unpaired with this one.

**What E0 does to the head-to-head, paired.** The same banks were played on the
before build too, so each TeamPlan side's effect after E0 can be set against the
same side's effect before it (`2026-10-29-as303-score.py h2h-shift`, 104,112
sides): **2v2 competitive −0.2pt** (−0.1 / −0.3 / −0.2, z=−1.2), 3v3 competitive
+0.0 (z=0.1); no map slice moves beyond ±0.4pt. Of 18 class slices two replicate,
the 2v2 Shaman (−1.0, z=−2.9) and the 2v2 Rogue (−0.6, z=−2.1), which is about
what 18 tries produce at |z| ≈ 2; neither is a finding without a mechanism. **E0
leaves the TeamPlan advantage where 0.8 measured it.**

## 5. Concerns

1. **The Hunter falls further behind** (§3, §3.2): with its rise in attack power,
   2v2 competitive 41.8-46.0% under Legacy and 45.6-49.2% under TeamPlan, about
   1.1pt below main; 3v3 (stat-mix kit) 37.5-40.1% under Legacy and 38.3% on
   TeamPlan BasicArena. The attack-power re-spend recovers a third of the loss;
   the rest is a balance question (a bow budget, or the Hunter's kit beyond the
   E0 slots).
2. **The Paladin stays on top** (59.7-67.5% 2v2 competitive, +0.6 from E0), as in 0.8.
3. **TeamPlan double-healer stalls** on the pillar maps are unchanged (23% of
   non-competitive Nagrand cells draw at the cap).

## 6. Recompute

Every figure recomputes from the committed files, from
`docs/design/balance/sweeps/`:

| figures | command |
|---|---|
| §1 | `2026-10-29-as303-tbc-slot-modifiers.py` (fetches the TBC tables into `.db2-cache/`) |
| §3 paired effect | `2026-10-29-as303-score.py e0 2026-10-29-as303-before.csv.gz -- 2026-10-29-as303-after.csv.gz` |
| §3.1 standings, draws, comps | `2026-10-25-0.8-score.py standings` (and `draws`, `comps 2 3`) `2026-10-29-as303-after.csv.gz` |
| §4 head-to-head | `2026-10-29-as303-score.py h2h 2026-10-29-as303-after.csv.gz` |
| §3.2 Hunter re-spend | `2026-10-29-as303-score.py e0 2026-10-30-as303-hunter-base.csv.gz -- 2026-10-30-as303-hunter-ap.csv.gz`; standings: `2026-10-25-0.8-score.py standings 2026-10-30-as303-hunter-ap.csv.gz` |
| §4 E0's shift of it | `2026-10-29-as303-score.py h2h-shift 2026-10-29-as303-before.csv.gz -- 2026-10-29-as303-after.csv.gz` |

The banks are `2026-10-29-as303-gen.py`: `uni <map> <Legacy|TeamPlan> 2
801-804,811-814,821-824`, `uni3 <map> <profile> 801,811,821`, `h2h <map> <1|2> 2
801-804,811-814,821-824` and `h2h <map> <1|2> 3 801,811,821 sample`, on all three
maps; both arms ran the same files (208,224 matches each). The before arm is
`main` at `a8a30775` built in a detached worktree; the after arm is this branch's
`9544e24e`.
