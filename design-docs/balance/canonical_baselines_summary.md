# Canonical Balance Baselines

**Generated 2026-06-07** on the current shipped meta — i.e. *after* this cycle's
changes: the healer posture-movement AI (Priest/Paladin FREE/PRESSURED/ESCAPE/DIP),
the CombatSnapshot casting-visibility fix (attackers no longer skip decision
ticks against casting targets), the Paladin no-ally retreat gate, and the Rogue
energy-pooling fix (Kidney Shot lands again). Supersedes the 2026-06-06
baselines (Frostbolt 0.6 / Hunter Auto Shot 1.5× / hunter kiting fix — all
still in).

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 7.0% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 1.3% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.4% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` + `arenasim --batch` (see the
`balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 70.1, Rogue 68.7, Mage 62.9 | **Mage 66.0**, **Paladin 56.7** | **Mage 62.5**, **Paladin 57.6** |
| **A — strong** | — | Warrior 48.9 | Priest 51.1 |
| **B — playable** | Warlock 47.4 | Priest 46.3, Warlock 46.0 | Warrior 49.5 |
| **C — weak** | Warrior 34.1 | Rogue 42.4 | Warlock 44.4, Rogue 43.1 |
| **D — bottom** | Priest 21.4, Hunter 20.7 | Hunter 39.2 | **Hunter 40.3** |

**The team-format meta is still Mage + Paladin**, but the field compressed:
Paladin lost ~3 points in both team formats (its face-tank bulk is worth less
now that healers reposition), Warrior lost ~5 (the casting-visibility fix cost
it free pressure windows), and the midfield (Priest, Warlock) climbed. Priest
overtook Warrior in 3v3 — healer movement working as designed.

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **78.0%** | Mage+Paladin |
| 75.4% | Mage+Priest |
| 75.2% | Paladin+Warrior |
| 74.2% | Mage+Warlock |
| 73.6% | Paladin+Rogue |
| 67.5% | Mage+Warrior |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 36.0% | Hunter+Warrior |
| 32.8% | Rogue+Warrior |
| 31.2% | Hunter+Warlock |
| 26.7% | Warlock+Warrior |
| 24.7% | Rogue+Warlock |
| **12.2%** | **Paladin+Priest** |

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **86.6%** | Mage+Paladin+Warrior |
| 85.8% | Mage+Priest+Warrior |
| 85.0% | Mage+Paladin+Warlock |
| 78.9% | Mage+Priest+Warlock |
| 70.3% | Mage+Paladin+Rogue |
| 68.3% | Mage+Priest+Rogue |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 34.0% | Hunter+Priest+Warlock |
| 24.4% | Hunter+Paladin+Priest |
| 19.1% | Hunter+Warlock+Warrior |
| 18.4% | Hunter+Rogue+Warlock |
| 14.3% | Hunter+Rogue+Warrior |
| **10.0%** | **Rogue+Warlock+Warrior** |

## What's meta-defining vs unplayable — the read

- **Mage is back to clear #1 in teams** (66.0 / 62.5) and anchors all six top
  3v3 comps. Its kiting game is still the only movement intelligence among DPS,
  and nothing counters it structurally — **LoS/pillar play remains the deferred
  answer**.
- **Paladin softened but holds #2.** Warrior+Paladin lost the 2v2 throne
  (86.2 → 75.2): the healer-movement slice traded its face-tank tankiness for
  retreat-and-heal positioning, and the casting-visibility fix lets enemies
  act through its long heals.
- **Double-healer is now a trap, not a wall.** Paladin+Priest collapsed from
  32.8% to **12.2% (worst 2v2 comp)** — at the 300s cap, two healers with no
  kill pressure lose the attrition war they used to draw. The shorter-cap
  "100% draw wall" observed during the healer-movement validation resolves
  into losses at the proper cap.
- **No-healer melee piles are the 3v3 floor** (Rogue+Warlock+Warrior 10.0%,
  Hunter+Rogue+Warrior 14.3%) — no sustain, no carry engine.
- **The Hunter pit persists** (last in every format; in 4 of the 6 worst 3v3
  comps) though it edged up again (36.9 → 40.3 in 3v3).

## Changes this cycle (vs the 2026-06-06 baseline)

- **Priest** up in teams (44.1 → 46.3 in 2v2, 49.4 → 51.1 in 3v3 — passing
  Warrior) while unchanged at the 1v1 floor (21.4): the posture AI helps
  healers exactly where the methodology says it matters.
- **Warlock** is the cycle's stealth winner: +6 in 2v2 (39.9 → 46.0), +9 in
  1v1 (38.1 → 47.4) — the casting-visibility fix cuts both ways and the
  perma-casting Warlock gains most from acting every tick.
- **Warrior** down ~5 everywhere (54.0 → 48.9 in 2v2): it was the biggest
  beneficiary of opponents idling mid-cast.
- **Rogue** 2v2 mostly unchanged net (44.8 → 42.4): the energy-pooling fix's
  +13-point enemy-has-Priest slice is offset by healer movement and the
  visibility fix elsewhere. Kidney Shot lands again (see
  `docs/reports/2026-06-rogue-energy-pooling.md`).
- **Draw rates** up modestly and exactly where predicted (1v1 2.9 → 7.0%, all
  healer mirrors; 2v2 0.5 → 1.3%; 3v3 0.2 → 0.4%) — the R13 watch verdict
  stands, and the offensive-punish slice remains the queued answer.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out, but don't read a single ordered cell as
  definitive. Mechanism documented in
  `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`; fix deferred.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (pets, openers, curses…)
  are a separate axis — see the `balance-sweep` skill.
