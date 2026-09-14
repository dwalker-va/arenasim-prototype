# Relics in the Ranged socket — what closing the gap was worth (AS-86)

Paladin and Shaman could not fill their Ranged socket at all: the slot held only
wands, bows and crossbows, and neither class trains any of them. They wore a
permanently empty socket while the other six classes drew stats from theirs.
AS-86 added relics — Librams and Totems — to fill it.

Both default relics are the same item in two skins: **+5 spell power, +3 mana**
(10.5 of an ilvl-58 accessory's 24.5 budget points).

## Method

A **paired** before/after sweep: one JSONL of 8,650 seeded matches run twice —
once through main's binary reading main's assets, once through this branch's
binary reading this branch's assets. Same seeds, same comps, same maps, so the
relic is the only variable and outcomes can be compared match-by-match.

Two binaries are needed rather than the usual "edit the `.ron` and re-run",
because the relic introduces a `WeaponType::Relic` variant that main's binary
cannot parse. Each binary therefore sits in its own directory beside its own
`assets/` tree.

Because the design is paired, **McNemar's test on the matches whose outcome
flipped** is the instrument, not a comparison of two independent Wilson
intervals — the latter is badly conservative here. Wilson 95% intervals are
still reported for the level.

Both sides of the arena got relics, so a cell whose OPPONENT also fields a
Paladin or a Shaman measures a symmetric buff and is expected to wash. The
CLEAN slice — enemy has no relic class — is the honest "what is a relic worth"
number.

## Results

`2026-09-13-as86_relics_{before,after}.csv`. Winrate is team1's.

### 2v2, team1 = Paladin + partner (n=3,500; 175 cells x 20 seeds)

| slice | before | after | delta | flips | McNemar |
|---|---|---|---|---|---|
| all opponents | 48.4% [46.7-50.1] | 50.8% [49.1-52.4] | **+2.4pt** | 285 (+184/-101) | z=4.86 **sig** |
| CLEAN (enemy has no relic class) | 55.7% [53.6-57.8] | 58.4% [56.3-60.5] | **+2.7pt** | 131 (+94/-37) | z=4.89 **sig** |
| MIRRORED (enemy has one too) | 37.4% [34.9-40.0] | 39.3% [36.8-41.9] | +1.9pt | 154 (+90/-64) | z=2.01 sig |

### 2v2, team1 = Shaman + partner (n=3,500)

| slice | before | after | delta | flips | McNemar |
|---|---|---|---|---|---|
| all opponents | 52.3% [50.7-54.0] | 52.7% [51.1-54.4] | +0.4pt | 208 (+111/-97) | z=0.90 ns |
| CLEAN | 58.0% [55.9-60.1] | 59.0% [56.8-61.0] | +0.9pt | 101 (+60/-41) | z=1.79 ns |
| MIRRORED | 43.8% [41.2-46.4] | 43.4% [40.9-46.0] | -0.4pt | 107 (+51/-56) | z=0.39 ns |

### 3v3, team1 = Paladin+Shaman+Warrior (n=1,650; 55 cells x 30 seeds)

| slice | before | after | delta | flips | McNemar |
|---|---|---|---|---|---|
| all opponents | 51.9% [49.5-54.3] | 52.4% [50.0-54.8] | +0.4pt | 257 (+132/-125) | z=0.37 ns |
| CLEAN | 57.2% [53.2-61.1] | 58.8% [54.9-62.7] | +1.7pt | 82 (+46/-36) | z=0.99 ns |
| MIRRORED | 49.0% [45.9-52.0] | 48.7% [45.7-51.7] | -0.3pt | 175 (+86/-89) | z=0.15 ns |

## Read

**The stats did not wash out for Paladin.** The same item is worth +2.7pt to a
Paladin comp against a relic-less enemy (significant at z=4.89) and at most
about +1pt to a Shaman comp (z=1.79, not resolvable at this n). The asymmetry
is a measurement, not a theory — this sweep does not establish its cause, and
the obvious candidate (Paladin carries the lowest total spell power of the three
healers, 103 vs Shaman's 119, so +5 is a larger relative step) is untested.

**It moves Paladin toward parity, not away from it.** Paladin comps sat at 48.4%
across all opponents and land at 50.8%. Shaman was already at 52.3% and stays
there. The socket that could not be filled was costing the weaker of the two
healers more than the stronger one, which is what the card predicted and why
closing it reads as a correction rather than a buff.

**Sample-size note.** n=20/cell (2v2) and n=30/cell (3v3) makes any PER-CELL
number noise; every claim above is an aggregate over thousands of matches, where
the paired design does the work. Machine contention (a shared box) is why this
is n=20 rather than the n=100 this repo usually prefers — the paired flip count,
not the per-arm interval, is what carries the significance, so the aggregate
conclusions stand. Re-running the canonical baselines at full n is still owed.
