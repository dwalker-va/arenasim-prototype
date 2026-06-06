# Canonical Balance Baselines

**Generated 2026-06-06** on the current shipped meta — i.e. *after* this cycle's
changes: Frostbolt 0.8→0.6, Hunter Auto Shot 1.5×, Divine Shield made
data-driven, and the Hunter kiting AI fix (arc around the kill-target).

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 2.9% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 0.5% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.2% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` + `arenasim --batch` (see the
`balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 78.9, Rogue 73.0, Mage 68.9 | **Mage 64.8**, **Paladin 59.7** | **Mage 62.9**, **Paladin 59.4** |
| **A — strong** | — | Warrior 54.0 | Warrior 53.1 |
| **B — playable** | Warrior 40.0, Warlock 38.1 | Rogue 44.8, Priest 44.1 | Priest 49.4 |
| **C — weak** | — | Hunter 40.5, Warlock 39.9 | Warlock 43.6, Rogue 43.3 |
| **D — bottom** | Priest 22.9, Hunter 21.1 | — | **Hunter 36.9** |

**The team-format meta is defined by Paladin + Mage.** In both 2v2 and 3v3 they
are the only classes clearly above the field; everything from Warrior down is
within ~10 points. (1v1 is a different game — Rogue/Paladin dominate via
stealth-burst and bulk; treat it as diagnostic, not a balance target.)

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **86.2%** | Warrior+Paladin |
| 79.6% | Mage+Paladin |
| 77.1% | Rogue+Paladin |
| 74.8% | Mage+Priest |
| 66.8% | Mage+Warlock |
| 63.9% | Warrior+Mage |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 34.0% | Warrior+Warlock |
| 33.0% | Warrior+Rogue |
| 32.8% | Priest+Paladin |
| 29.7% | Priest+Warlock |
| 26.9% | Rogue+Warlock |
| **22.8%** | **Priest+Hunter** |

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **89.0%** | Warrior+Mage+Paladin |
| 83.7% | Warrior+Mage+Priest |
| 81.9% | Warrior+Priest+Paladin |
| 80.9% | Mage+Warlock+Paladin |
| 74.7% | Mage+Rogue+Paladin |
| 73.4% | Mage+Priest+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 21.1% | Warlock+Paladin+Hunter |
| 18.3% | Rogue+Priest+Warlock |
| 16.2% | Priest+Warlock+Hunter |
| 15.8% | Warrior+Rogue+Hunter |
| 12.7% | Priest+Paladin+Hunter |
| **11.9%** | **Warrior+Rogue+Warlock** |

## What's meta-defining vs unplayable — the read

- **Paladin is the single most meta-defining class.** It anchors the #1 comp in
  *both* formats (Warrior+Paladin 86%; Warrior+Mage+Paladin 89%), appears in 3 of
  the top 3 in 2v2 and **5 of the top 6 in 3v3**. The winning recipe everywhere is
  **Paladin + a carry (Warrior or Mage)**. It inherited the throne when the
  Frostbolt nerf knocked Mage back, and is now the prime nerf candidate (its edge
  is bulk + heal throughput; Divine Shield contributes only ~3 pts — see the
  Hunter/Mage findings).
- **Mage is the top *carry*** (64.8/62.9) but no longer runaway-dominant post-nerf.
- **Unplayable comps share one trait: no carry *or* no healer with a passenger.**
  In 3v3 the literal worst is **Warrior+Rogue+Warlock (11.9%)** — three bodies, no
  healer, no Paladin/Mage engine. The Hunter pit (Priest+Paladin+Hunter 12.7%,
  Priest+Hunter 22.8% in 2v2) persists: a healer babysitting a low-pressure
  Hunter has no win condition.
- **Hunter is no longer the floor in 2v2** (40.5%, above Warlock 39.9%) after the
  kiting fix — but remains last in 3v3 (36.9%) and still drags the bottom comps.

## Changes this cycle (vs the pre-cycle baseline)

- **Mage** knocked off the throne: 74→65 (2v2), 69→63 (3v3). Still #1 carry.
- **Hunter** lifted off the floor: 31.7→40.5 (2v2), 33.6→36.9 (3v3) — mostly the
  kiting AI fix (it now stays in shot range of its target in team fights).
- **Paladin** rose into the clear top (inherited from Mage).
- **Rogue** trimmed by the universal kiting improvement (ranged now counters its
  stick-to-target game): 51→45 (2v2), 47→43 (3v3).

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out, but don't read a single ordered cell as
  definitive. Not yet root-caused.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (pets, openers, curses…)
  are a separate axis — see the `balance-sweep` skill.
