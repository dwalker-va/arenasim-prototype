# Canonical Balance Baselines

**Generated 2026-10-10 at `ea63f4f`** (the 0.7 milestone sweep, card AS-164):
nine classes, the Druid's first canonical run. AS-240 (TBC dispel classes)
landed after this commit and is not in these numbers. Full findings:
`2026-10-10-as164-milestone-sweep.md`.

Authoritative current-state references. Use as the "before" when assessing a
balance change — **compare batch-vs-batch only, full-canonical vs
full-canonical**, and for exact attribution run a paired A/B at a fixed commit
rather than diffing against these files (cells drift between cycles; see the
`balance-methodology` memory).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv.gz` | full 9×9 | 100 | 8,100 | 8.95% |
| 2v2 | `canonical_2v2_full_n100_300s.csv.gz` | every distinct-class pair × pair (1,296) | 100 | 129,600 | 2.55% |
| 3v3 | `canonical_3v3_full_n50_300s.csv.gz` | every distinct-class triple × triple (7,056) | 50 | 352,800 | 0.79% |

Distinct-class comps, both orderings, 300s cap, BasicArena, `Legacy` AI,
default loadouts. Committed gzipped (`gzip -9 -n`). Regenerate with
`scripts/gen_sweep.py --full {1,2,3}` and `arenasim --batch`, compress, then
`scripts/comp_tiers.py <csv.gz> --size {2,3}` (it reads `.gz` directly).

## Two views: all-comps and competitive

Healers are Priest, Paladin, Shaman and Druid.
- **2v2 competitive:** at most one healer on each side.
- **3v3 competitive:** one or two healers on each side.

## Class standings

Win rate of every match the class's comp plays; draws count as non-wins.

| Class | 1v1 | 2v2 all | **2v2 competitive** | 3v3 all | **3v3 competitive** |
|---|---|---|---|---|---|
| Paladin | 52.6 | 49.5 | **63.1** | 51.0 | **51.5** |
| Mage | 70.9 | 61.1 | **58.1** | 57.0 | **56.4** |
| Priest | 38.4 | 44.7 | **52.3** | 49.4 | **50.1** |
| Shaman | 30.6 | 41.1 | **51.4** | 46.3 | **47.0** |
| Rogue | 75.9 | 55.6 | **49.5** | 52.7 | **54.7** |
| Warrior | 38.5 | 52.3 | **46.3** | 52.8 | **54.0** |
| Hunter | 61.8 | 50.7 | **45.2** | 45.1 | **43.2** |
| Druid | 8.2 | 37.0 | **45.1** | 45.3 | **46.9** |
| Warlock | 32.8 | 46.6 | **40.0** | 46.8 | **44.7** |

**The read.** The Paladin is the strongest healer and the strongest 2v2 class
(63.1), the healer four of five DPS do best with. The Mage leads 3v3 (56.4) and
every 3v3 top-5 comp contains one. The Druid is the weakest 2v2 healer (45.1,
six points under the Shaman) and sits level with the Shaman in 3v3; its
mechanism metrics point at healing throughput, not mana, as the constraint. The
Warlock is the 2v2 floor (40.0) and the Hunter the 3v3 floor (43.2). 1v1 is
diagnostic only: the Druid's 8.2 there is a healer with almost no damage (it
draws 100% of its mirror and of Druid v Shaman at the cap).

## 2v2 comp tier list

| Top | | Bottom | |
|---|---|---|---|
| Warrior+Paladin | 77.5% | Druid+Shaman | 4.3% |
| Mage+Shaman | 72.7% | Druid+Paladin | 9.8% |
| Rogue+Paladin | 70.5% | Druid+Priest | 16.8% |
| Mage+Priest | 70.5% | Paladin+Priest | 19.3% |
| Mage+Paladin | 69.1% | Priest+Shaman | 25.4% |
| Hunter+Paladin | 65.8% | Paladin+Shaman | 25.5% |
| Priest+Rogue | 64.2% | Rogue+Warlock | 36.9% |
| Druid+Rogue | 63.9% | Warlock+Warrior | 38.3% |

## 3v3 comp tier list

| Top | | Bottom | |
|---|---|---|---|
| Mage+Paladin+Warlock | 76.6% | Druid+Priest+Shaman | 4.6% |
| Mage+Shaman+Warlock | 74.7% | Druid+Paladin+Priest | 4.9% |
| Mage+Priest+Warlock | 73.7% | Druid+Paladin+Shaman | 5.8% |
| Mage+Paladin+Warrior | 72.9% | Paladin+Priest+Shaman | 9.2% |
| Mage+Paladin+Rogue | 71.9% | Hunter+Rogue+Warrior | 20.9% |
| Druid+Paladin+Warrior | 71.9% | Hunter+Rogue+Warlock | 25.0% |
| Paladin+Priest+Warrior | 70.6% | Hunter+Warlock+Warrior | 26.3% |
| Hunter+Priest+Rogue | 70.0% | Druid+Priest+Warlock | 28.0% |

## Canaries

- **Anomaly (non-competitive comp at or above 50% against competitive comps):**
  2v2 none (best: Paladin+Shaman 30.3%). **3v3: Mage+Warlock+Warrior fires**,
  58.7% full-field and 59.0% against competitive comps — a no-healer comp
  out-trading healed ones. Watch.
- **Dominant shape (3v3):** 3 of the top-10 comps are double-healer. Present,
  not dominant.

## Caveats

- **Draws are mostly the cap, and mostly double-healer comps.** 2,940 of the
  2v2's 3,302 draws hit 300s. In competitive comps the 2v2 draw rate is 0.83%
  with no Druid, 0.80% with one side's and 4.12% with both — under the Shaman
  mirror's 6.36%. Druid mirrors run long (median 95s) but resolve; only in 1v1,
  where it has no damage-dealer beside it, does a Druid fail to kill (Druid v
  Druid and Druid v Shaman draw 100% at the cap).
- **Spawn side.** Both orderings run, so tier lists average it out; AS-104
  measured no team-1 advantage beyond ±1.3pt.
- **Default loadouts and strategy.** Strategy-variable sweeps (pets, openers,
  curses, auras) are a separate axis — see the `balance-sweep` skill.
- **Dampening horizon.** Any change aimed at the late game interacts with the
  75–195s dampening ramp; sweep healer pairs and Druid comps explicitly.
