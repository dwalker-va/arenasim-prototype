# A debuff's removal class — measurement

**Date:** 2026-09-12
**Card:** AS-45

Every debuff carries a REMOVAL CLASS — Magic, Poison, Disease, Curse or
Physical — decided at construction from the applying ability and consulted
before any mechanic-level rule. Two classifications move, and each was measured
against its own baseline:

| part | change | baseline | aura that moves |
|---|---|---|---|
| 1 | **Physical is never dispellable**, derived from `spell_school` | `63d8cbe` (origin/main) | Concussive Shot's snare |
| 2 | **Curse is a class of its own**, declared in the RON | `e3242a9` (part 1's head) | Curse of Agony |

## Method

Paired sweep: one JSONL of configs replayed through two checkouts of the tree,
so the classification is the only variable. 300s cap, `Legacy` AI, default map.
Wilson 95% intervals, pooled two-proportion z. Raw per-match rows in the
`2026-09-12-as45_*.csv` files beside this doc.

Seeds are paired but that buys no variance reduction: once one removal decision
differs the RNG stream decorrelates, so individual seeds scatter freely (the
"flips" / "bit-identical" columns) while the RATE carries the information.
"Bit-identical" is the byte-identity check, not an effect size.

---

# Part 1 — Physical debuffs are not dispellable

A physical aura is never removable by Dispel Magic / Cleanse / Devour Magic /
Purge, whatever its mechanic. Exactly one aura in the game changes hands:
**Concussive Shot's 50% snare** (4s, 12s cooldown), which was a dispel candidate
because `MovementSpeedSlow` was dispellable at the aura-TYPE level. Rend was
already undispellable through the DoT school rule, so nothing else moves.

## Results

| cell | n | T1 before | T1 after | delta | z | bit-identical |
|---|---|---|---|---|---|---|
| **2v2** Hunter+Priest vs Warlock+Priest | 900 | 25% [22.1-27.7] | 29% [25.7-31.6] | **+4pt** | +1.81 | 3/900 |
| **2v2** Hunter+Priest vs Rogue+Priest | 400 | 73% [68.2-76.9] | 70% [64.8-73.8] | -3pt | -1.01 | 212/400 |
| **2v2** Hunter+Priest vs Warrior+Priest | 100 | 95% [88.8-97.8] | 95% [88.8-97.8] | 0 | 0.00 | 96/100 |
| **2v2** Hunter+Priest vs Mage+Priest | 100 | 3% [1.0-8.5] | 3% [1.0-8.5] | 0 | 0.00 | 94/100 |
| **2v2** Hunter+Priest vs Paladin+Warrior | 100 | 1% [0.2-5.4] | 1% [0.2-5.4] | 0 | 0.00 | 45/100 |
| **2v2** Hunter+Priest vs Warlock+Mage | 100 | 1% [0.2-5.4] | 1% [0.2-5.4] | 0 | 0.00 | 18/100 |
| **3v3** Hunter+Priest+Warrior vs Mage+Priest+Rogue | 100 | 7% [3.4-13.7] | 7% [3.4-13.7] | 0 | 0.00 | 100/100 |
| **3v3** Hunter+Priest+Warrior vs Warrior+Paladin+Mage | 100 | 62% [52.2-70.9] | 62% [52.2-70.9] | 0 | 0.00 | 100/100 |
| **control** Mage+Priest vs Warrior+Priest | 25 | 96% | 96% | 0 | 0.00 | 25/25 |
| **control** Warlock+Priest vs Rogue+Priest | 25 | 24% | 24% | 0 | 0.00 | 25/25 |
| **control** Paladin+Warrior vs Mage+Priest | 25 | 84% | 84% | 0 | 0.00 | 25/25 |
| **control** Shaman+Rogue vs Priest+Warlock | 25 | 88% | 88% | 0 | 0.00 | 25/25 |

Hunter+Priest pooled across the six 2v2 cells at n=100 each: **33.7%
[30.0-37.5] -> 32.8% [29.2-36.7]**.

## Findings

1. **No cell clears the 95% bar.** The largest move is +4pt for Hunter+Priest
   vs Warlock+Priest at n=900 (z=1.81) — a small buff, just under the
   conventional threshold after 900 matches per arm. Nothing here is a balance
   event; the change is safe to ship on its merits as a rules correction.

2. **The cell that moves is the cell that should.** Warlock+Priest carries the
   most dispel pressure in the sample — a Priest's Dispel Magic AND the
   Felhunter's Devour Magic — so it is where the snare was stripped most often.
   It is also the cell where Hunter was WORST (25%), so the change nudges an
   outlier toward the middle rather than strengthening an already-strong comp.

3. **The n=100 reading of vs Rogue+Priest was noise, exactly as the sample-size
   warning predicts.** At n=100 it read -10pt (z=-1.52) — the kind of number
   that gets written up as a regression. At n=400 it is -3pt (z=-1.01). Seeds
   were added, not re-rolled.

4. **Most cells do not move at all, because a slow is a low-priority dispel.**
   `dispel_priority(MovementSpeedSlow)` is 20 — "typically not worth
   dispelling" — so a healer lifted the snare only when the ally carried nothing
   better. Both 3v3 cells came back 100/100 BIT-IDENTICAL: with more debuffs in
   play the snare was never the top candidate, so removing it from the pool
   changed nothing. The 3v3 result is a null, not a confirmation — it says the
   change is inert at that team size on those comps.

5. **Every match without a Hunter is byte-identical.** All four control cells
   returned 25/25 identical winner AND duration. That is the whole blast radius:
   Concussive Shot is the only aura in the game that was both physical and of a
   dispellable type.

---

# Part 2 — Curse is a removal class

Curse of Agony, Curse of Weakness and Curse of Tongues declare
`dispel_type: Curse` in the RON. Curses are removed by curse-removal, which no
class in the arena has, so nothing takes them.

**The expectation going in was that this changes nothing** — and it was WRONG.
Curse of Weakness (`DamageReduction`) and Curse of Tongues (`CastTimeIncrease`)
were indeed already removable by nothing. But **Curse of Agony is a Shadow
`DamageOverTime`**, indistinguishable from Corruption to every predicate the
engine had, and it WAS being dispelled. The curse class takes it out of the
dispel pool. That is the one behaviour change of part 2, and the reason the
round is not byte-identical to `e3242a9`.

## Non-vacuity

30 1v1 matches (Warlock vs Priest and Warlock vs Paladin, 15 seeds each),
counting removal events in the match logs:

| aura removed | before (`e3242a9`) | after |
|---|---|---|
| Corruption | 45 | 32 |
| Unstable Affliction | 39 | 36 |
| **Curse of Agony** | **7** | **0** |
| Immolate | 2 | 3 |

Curse of Agony goes to zero while the other Shadow DoTs keep coming off — so
the probe is measuring the curse rule, not an absence of dispels.

## Results

n = 200 per cell per arm (1400 matches per arm across the curse cells, 500 in
the controls). Every control cell has NO Warlock.

| cell | n | T1 before | T1 after | z | bit-identical |
|---|---|---|---|---|---|
| **1v1** Warlock vs Priest | 200 | 99.0% [96.4-99.7] | 98.5% [95.7-99.5] | -0.45 | 189/200 |
| **1v1** Warlock vs Paladin | 200 | 4.5% [2.4-8.3] | 4.5% [2.4-8.3] | 0.00 | 151/200 |
| **2v2** Warlock+Priest vs Hunter+Priest | 200 | 71.5% [64.9-77.3] | 72.0% [65.4-77.8] | +0.11 | 163/200 |
| **2v2** Warlock+Priest vs Warrior+Priest | 200 | 15.5% [11.1-21.2] | 16.5% [12.0-22.3] | +0.27 | 171/200 |
| **2v2** Warlock+Priest vs Rogue+Paladin | 200 | 52.0% [45.1-58.8] | 52.0% [45.1-58.8] | 0.00 | 0/200 |
| **2v2** Warlock+Paladin vs Mage+Priest | 200 | 49.5% [42.6-56.4] | 53.5% [46.6-60.3] | +0.80 | 65/200 |
| **3v3** Warlock+Priest+Warrior vs Mage+Priest+Rogue | 200 | 12.5% [8.6-17.8] | 14.0% [9.9-19.5] | +0.44 | 1/200 |
| **control** Hunter+Priest vs Rogue+Priest | 100 | 74.0% | 74.0% | 0.00 | **100/100** |
| **control** Hunter+Priest vs Warrior+Priest | 100 | 95.0% | 95.0% | 0.00 | **100/100** |
| **control** Mage+Priest vs Warrior+Priest | 100 | 97.0% | 97.0% | 0.00 | **100/100** |
| **control** Shaman+Rogue vs Priest+Paladin | 100 | 92.0% | 92.0% | 0.00 | **100/100** |
| **control** Hunter+Priest+Warrior vs Mage+Priest+Rogue | 100 | 12.0% | 12.0% | 0.00 | **100/100** |

Warlock pooled across the seven curse cells: **43.5% [40.9-46.1] -> 44.4%
[41.8-47.0]**, z=+0.49, n=1400 per arm.

## Findings

1. **No cell moves beyond noise.** The largest single-cell move is +4pt
   (Warlock+Paladin vs Mage+Priest, z=+0.80) and the pooled move is +0.9pt
   (z=+0.49) over 1400 matches per arm. Making Curse of Agony undispellable is a
   Warlock buff on paper and is not measurable in outcomes.

2. **The RNG decorrelates hard, and the rate does not follow it.**
   Warlock+Priest vs Rogue+Paladin is 0/200 bit-identical with 110 winner flips
   — and lands on exactly 52.0% on both arms. That pairing is the cleanest
   statement of what the "flips" column is and is not: evidence the change is
   live, not evidence it matters.

3. **Every match without a Warlock is byte-identical — 500/500**, winner,
   duration and end reason. The curse family is Warlock-only, so that is the
   whole blast radius, and it includes the Hunter cells part 1 moved (they
   reproduce part 1's result exactly).

4. **Why it barely matters despite firing 7 times in 30 matches.** A Warlock
   under dispel pressure re-applies. Curse of Agony is an instant with no
   cooldown at 25 mana; a dispel trades the healer's GCD and 18 mana for the
   Warlock's GCD and 25. The class change removes one of the Priest's three DoT
   targets, and the Priest simply dispels Corruption or Unstable Affliction
   instead — which the non-vacuity table shows it doing.

## Caveats

- **AS-44 is in flight and changes the same system from the other side.** It
  raises `Incapacitate` and `Silence` in `dispel_priority` so healers start
  lifting Freezing Trap and the UA silence — a Hunter NERF where part 1 is a
  Hunter buff, and it pushes `MovementSpeedSlow` further down a longer priority
  list. The combined state is not the sum of the two measurements; whichever
  lands second, part 1's numbers describe a world that no longer exists.
- Both parts: default map, `Legacy` profile. Pillar maps were not swept. The
  change is map-independent by construction (a classification, not a positioning
  behaviour) — but that is an argument, not a measurement.
- Part 2's cells are n=200, which resolves a ~10pt move and not a ~4pt one. The
  pooled n=1400 read is the one that rules out a systematic Warlock shift; the
  per-cell rows are there to show WHERE the change is live.
