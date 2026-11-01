# The extra swing's price (AS-427)

**Tier: DIRECTIONAL.** Success condition: an extra swing priced at
`WEIGHT_EXTRA_SWING` is worth the attack power the same budget buys
(`--expect nothing` on the item pair).

**Result:** the extra swing is worth **1.5x haste's weight** per unit of
chance, so `WEIGHT_EXTRA_SWING = 1.5 * WEIGHT_ATTACK_SPEED = 321.4`. Hand of
Justice's 5% swing costs 16.07 points, and the item is +21 attack power with
it (47.57 of a Champion trinket's 48.3).

## What was measured

An extra swing has no stat weight, so its price is DECLARED (talents proposal
§6.5, item 4). The unit is haste's: an extra swing on a fraction `p` of landed
hits is about `p` more main-hand swings in the same time. The size is what a
sweep can check: trade the swing for the attack power its price buys, and a
fair price moves nothing.

Six arms, each the same release binary with its own `items.ron` beside it,
differing only in Hand of Justice (`sweeps/2026-10-31-as427-arms.py`):

| arm | Hand of Justice |
|---|---|
| ship-swing | +21 AP, 5% extra swing (as shipped) |
| ship-power | no proc, +31.71 AP (the swing paid in AP at the declared 1.5x) |
| dose-swing | +21 AP, 50% extra swing |
| dose-power-1x / 1.5x / 2x | no proc, the 50% swing paid in AP at 1x / 1.5x / 2x haste's weight (+92.42 / +128.13 / +163.85 AP) |

The 5% item is too small for any sweep to see — its whole swing is about 10
attack power — so the RATE is measured at a 10x dose. Both sides of the trade
are linear in chance (each landed hit rolls once; attack power adds per point),
so the rate the dose finds is the rate the item is charged.

Cells: every 2v2 a melee swinger can reach, plus 8 control cells, 10 seeds each
(`scripts/gen_sweep.py --full 2 --exclude-double-healer --affects
Warrior,Rogue,Paladin,Shaman,Druid --control-cells 8 --n 10 --seed-base 42700`,
8,720 matches). `sweeps/2026-10-31-as427-gen.py` puts Hand of Justice in
Trinket2 on every Warrior, Rogue, Paladin, Shaman and Druid; wand users and the
Hunter keep their kit. Every arm plays the same JSONL (sha1 `ae88988c`), so
each match has twins differing only in Hand of Justice. The arm CSVs are
`sweeps/2026-10-31-as427-arms.csv.tar.gz`.

## Results

`scripts/paired_sweep.py <power arm> <swing arm> --affects
Warrior,Rogue,Paladin,Shaman,Druid --tier directional --expect nothing`, so a
positive CLEAN delta means the swing beat the attack power when only team 1
wore it. The control was identical (80/80) in every pair. Non-vacuity: 8,629+
of 8,720 matches per pair ended in a kill, and 6,346+ moved in winner or
duration.

| swing vs | CLEAN (team 1 wears) | AGAINST (team 2 wears) | verdict |
|---|---|---|---|
| AP at 1x haste (dose) | **+2.2pt** z=2.82 | **-2.2pt** z=2.85 | the swing is worth more |
| AP at 1.5x haste (dose) | +0.1pt z=0.00 | -0.4pt z=0.46 | even |
| AP at 2x haste (dose) | **-1.7pt** z=2.18 | **+1.6pt** z=1.97 | the attack power is worth more |
| AP at 1.5x (shipped 5%) | +0.7pt z=0.71 | +1.3pt z=1.41 | no movement; resolves >=1.8pt |

CLEAN and AGAINST agree in sign at every dose (a gain on one side is a loss on
the other), and the bracket crosses zero at 1.5x. An earlier pass on the same
cells, with the item at +25 AP, found the same three dose signs (+2.4 / +0.2 /
-1.4pt CLEAN); the 1.5x price is the one that replicated.

Haste's own weight is TBC's rating exchange rate, not a measured value here,
so the ratio says what an extra swing is worth in the attack-power currency the
item budget is denominated in, not that haste is mispriced. A whole weapon hit
on the main hand carries crit and a full weapon's damage, and the sim's
auto-attacks scale with weapon damage alone, so a swing outweighs what a
rating exchange rate predicts.

## What this does not say

- **Directional.** It may not be cited as any class's standing, and the
  per-class split was not read: the bracket is the claim.
- **2v2 only, BasicArena.** Map and 3v3 effects on the swing's worth are not
  measured.
- **The shipped item's null is uninformative on its own** (resolution 1.8pt
  against an effect expected near 0.2pt); the rate rests on the dose bracket.
