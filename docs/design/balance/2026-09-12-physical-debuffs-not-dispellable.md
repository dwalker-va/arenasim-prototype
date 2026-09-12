# Physical debuffs are not dispellable — measurement

**Date:** 2026-09-12
**Card:** AS-45
**Change:** a physical aura is never removable by Dispel Magic / Cleanse /
Devour Magic / Purge, whatever its mechanic. Exactly one aura in the game
changes hands: **Concussive Shot's 50% snare** (4s, 12s cooldown), which was a
dispel candidate because `MovementSpeedSlow` was dispellable at the aura-TYPE
level. Rend was already undispellable through the DoT school rule, so nothing
else moves.
**Baseline:** `63d8cbe` (origin/main).

## Method

Paired sweep: one JSONL of configs replayed through a `main` binary and a branch
binary, same `assets/` for both, so the binary is the only variable. 300s cap,
`Legacy` AI, default map. Wilson 95% intervals, pooled two-proportion z. Raw
per-match rows in `2026-09-12-as45_{before,after}.csv`.

Seeds are paired but that buys no variance reduction: once one dispel decision
differs the RNG stream decorrelates, so individual seeds scatter freely (the
"flips" column) while the RATE carries the information. "bit-identical" is the
byte-identity check, not an effect size.

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

## Caveats

- **AS-44 is in flight and changes the same system from the other side.** It
  raises `Incapacitate` and `Silence` in `dispel_priority` so healers start
  lifting Freezing Trap and the UA silence — a Hunter NERF where this is a
  Hunter buff, and it pushes `MovementSpeedSlow` further down a longer priority
  list. The combined state is not the sum of the two measurements; whichever
  lands second, these numbers describe a world that no longer exists.
- Default map, `Legacy` profile. Pillar maps were not swept. The change is
  map-independent by construction (a classification, not a positioning
  behaviour) — but that is an argument, not a measurement.
