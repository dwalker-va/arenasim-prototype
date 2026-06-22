# Canonical Balance Baselines

**Generated 2026-06-22** on the Hunter mana-economy + trap/Concussive-AI meta —
i.e. the 2026-06-15 Psychic Scream state *plus* the Hunter rebalance (PR #74): a
gear-mana itemization fix (the Hunter got zero mana from gear), Freezing Trap
cost cut, and smarter Concussive/trap targeting. Supersedes the 2026-06-15
Psychic Scream baselines.

**The dominant change this cycle is the Hunter rebalance.** Its binding
constraint was a hidden itemization gap — every Hunter (mail) item had zero
`max_mana`, giving it the *smallest* effective mana pool of any mana class
despite the largest base. +60 gear mana (→ pool 240→300) plus a Freezing Trap
cost cut and smarter Concussive/trap AI lift the Hunter from the C-tier floor to
mid-B in 2v2 and toward the middle in 3v3, **without overshooting 50%.** The
change is Hunter-isolated; non-Hunter-vs-non-Hunter cells are byte-identical to
the 2026-06-15 binary (verified) and carry that cycle's reading (Priest Psychic
Scream, the Mage kiting pilot, etc. — see "prior cycle" notes below).

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 5.1% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 0.8% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.4% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` + `arenasim --batch` (see the
`balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 69.2, Mage 65.6 | **Mage 64.6** | **Mage 61.7** |
| **A — strong** | Hunter 58.1, Rogue 56.6 | Paladin 56.9 | Paladin 56.2 |
| **B — playable** | — | **Hunter 50.9**, Priest 47.8, Warlock 45.0, Warrior 42.7 | Priest 52.6, **Hunter 46.5**, Warrior 46.2, Warlock 45.0 |
| **C — weak** | Warlock 38.9, Warrior 32.6 | Rogue 39.2 | Rogue 40.5 |
| **D — bottom** | Priest 11.3 | — | — |

**The team-format meta is still Mage + Paladin** at the top — the Hunter change
is Hunter-isolated, so the carries are unchanged in ordering (Mage dips ~2pt in
2v2 because its Hunter matchups got harder, but stays clear #1). **The action is
the Hunter:** in 2v2 it jumps from the C-tier floor to mid-B (**42.8 → 50.9**),
now ahead of Priest/Warlock/Warrior/Rogue and trailing only Mage/Paladin; in 3v3
it climbs out of the bottom into B (**43.0 → 46.5**). It is no longer a
team-format floor. (1v1 dips slightly, 59.4 → 58.1, holding A-tier — the mana
buff de-skews an over-tuned Hunter-v-Warlock matchup; see below.) Other classes'
aggregates move only via their matchup column vs the now-stronger Hunter (a
≤1-3pt shift in that one column); their non-Hunter-vs-non-Hunter cells are
byte-identical to 2026-06-15.

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **75.7%** | Mage+Priest |
| 75.3% | Mage+Paladin |
| 71.7% | Mage+Warlock |
| 69.3% | Warrior+Paladin |
| **63.4%** | **Hunter+Paladin** |
| 63.3% | Warlock+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 36.4% | Priest+Rogue |
| 29.1% | Rogue+Warrior |
| 19.1% | Rogue+Warlock |
| 18.1% | Warlock+Warrior |
| **14.1%** | **Paladin+Priest** |

Hunter's best 2v2 partners are now the carries: **Hunter+Paladin 63.4%
(top-5 overall), Hunter+Mage 61.5%** — and even its weak pairings climbed out of
the cellar (Hunter+Warlock 28.7 → 40.9, Hunter+Warrior 33.0 → 41.0, Hunter+Rogue
33.9 → 42.5). **No Hunter comp is in the bottom-5 anymore** (its floor,
Hunter+Warlock 40.9%, is now mid-pack). Hunter+Priest 47.7 → 55.9.

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **84.1%** | Warrior+Mage+Paladin |
| 84.0% | Mage+Warlock+Paladin |
| 83.2% | Mage+Priest+Warlock |
| **78.2%** | **Hunter+Mage+Paladin** |
| 75.7% | Warrior+Mage+Priest |
| **75.0%** | **Hunter+Mage+Priest** |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 29.4% | Mage+Rogue+Warlock |
| 26.0% | Paladin+Priest+Warlock |
| 22.0% | Rogue+Warlock+Hunter |
| 21.6% | Warrior+Warlock+Hunter |
| **6.0%** | **Warrior+Rogue+Warlock** |

Hunter+carry+carry now reaches the top third (**Hunter+Mage+Paladin 78.2%,
Hunter+Mage+Priest 75.0%** — both up ~+4 to +11), and Hunter+Priest+Warlock
52.3 → 60.7. No-healer melee piles remain the floor — Warrior+Rogue+Warlock 6.0%
is still the worst comp in the game — and Hunter still appears in the bottom when
paired with two other low-sustain classes (Hunter+Rogue+Warrior 15.9%): those
are comp-composition problems, not a Hunter stat deficit.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1 in teams** (64.6 / 61.7) and anchors the top 3v3
  comps. Its kiting (on the shared ENGAGE/KITE machine, PR #69) is still the only
  movement intelligence among DPS and nothing counters it structurally —
  **LoS/pillar play remains the deferred answer.** It dips ~2pt in 2v2 this cycle
  only because Hunter comps now contest it better.
- **Paladin holds #2**; Warrior+Paladin is still a top-4 2v2 (69.3%).
- **The Hunter is no longer a team-format floor** (mana economy + trap/Concussive
  AI, PR #74). It sits mid-B in 2v2 (50.9, above four classes) and climbs into B
  in 3v3 (46.5). Root cause was the gear-mana gap; the fix is Hunter-isolated and
  doesn't overshoot. Hunter+Paladin (63.4) is now a top-5 2v2 comp. Remaining
  holes are matchup-structural, not stat deficits: it still can't kill through
  Paladin/Mage sustain+control 1v1 (Hunter-v-Mage / -Paladin 0%), and Hunter+Mage
  control comps lag.
- **The Priest is mid-B** (Psychic Scream, PR #73 — prior cycle). Mage+Priest is
  still the top 2v2 carry pair (75.7%).
- **Double-healer is still a trap.** Paladin+Priest is still the worst 2v2 comp
  (14.1%) — two healers with no kill pressure lose the attrition war at the cap.
- **No-healer melee piles are the floor** (Warrior+Rogue+Warlock 6.0% in 3v3) —
  no sustain, no carry engine.

## Changes this cycle (vs the 2026-06-15 baseline)

- **Hunter is the headline: mana economy + trap/Concussive AI (PR #74).**
  Root cause: the Hunter got **zero mana from gear** (mail itemization),
  effective pool 240 (smallest of any mana class) vs casters 255–316 — the
  May-2026 mana fix set base=240 but compared base pools and missed the gear gap.
  Fix: **+60 max_mana** across the 9 Hunter-only items (effective pool → 300),
  **Freezing Trap 43→26**, and smarter AI (Concussive peels the nearest melee and
  skips stationary/casting targets; Freezing Trap aims at the off-target, leads
  movers, and dips to set it; burst-during-CC prefers Aimed Shot while the enemy
  healer is CC'd).
- **Effect: Hunter 2v2 42.8 → 50.9 (+8.1), 3v3 43.0 → 46.5 (+3.5)**, broad across
  comps (every Hunter 2v2 comp +4 to +12, biggest gains on the old floor:
  Hunter+Warlock +12.2, Hunter+Rogue +8.6, Hunter+Warrior +8.0). 1v1 ~flat
  (59.4 → 58.1): the buff de-skews the over-tuned Hunter-v-Warlock matchup
  (96 → 81), which the Concussive heuristic partly recovers; Hunter stays A-tier.
- **Knock-on, not nerfs.** Each non-Hunter class's aggregate shifts ≤1-3pt only
  through its single matchup column vs the now-stronger Hunter (Mage 2v2 67.0 →
  64.6 is Mage-vs-Hunter shifting). Non-Hunter-vs-non-Hunter cells are
  byte-identical to 2026-06-15 (the change is Hunter-only code + Hunter-only gear).
- **Draw rates ~flat** (1v1 5.0 → 5.1%, 2v2 1.0 → 0.8%, 3v3 0.3 → 0.4%): no stall
  pathology.

### Prior cycle (2026-06-15, carried forward on non-Hunter cells)

- **Priest** Psychic Scream (PR #73) lifted it off the team-format floor into
  mid-B (+3.7pt 2v2 / +1.5pt 3v3; defensive AoE-fear peel is the driver, the
  offensive dip respects the kill target). **Hunter** pet-damage fix (2026-06-13)
  and **Mage** kiting pilot (PR #69) remain in the non-Hunter reading.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out, but don't read a single ordered cell as
  definitive. Mechanism in `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`;
  fix deferred.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (pets, openers, curses…)
  are a separate axis — see the `balance-sweep` skill.
