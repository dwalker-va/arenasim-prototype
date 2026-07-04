# Canonical Balance Baselines

**Generated 2026-07-04** — regenerated after the **Warrior Berserker Rage**
change (fear break + 10s fear immunity, horror bypasses). Supersedes the
2026-07-02 Warlock-Death-Coil baselines.

**Headline: the Warrior buff landed exactly on target — +2.5pt 2v2 competitive
(36.6 → 39.1), paid for almost entirely by the two fear classes (Priest −2.1,
Warlock −1.6), with non-fear matchups untouched.** Warrior is *still* the 2v2
competitive floor, but the gap to 7th (Rogue 42.3) narrowed from 6.1pt to
3.2pt, and Warrior triples entered the 3v3 top-8 for the first time
(Mage+Paladin+Warrior 70.8, Mage+Shaman+Warrior 70.5). The predicted
Priest+Warlock over-counter materialized in moderation: the comp dropped into
the 2v2 bottom tier (35.4) but is nowhere near floor-level. Both standing
canaries still fire: **Paladin+Shaman 2v2 unchanged at 51.4/56.3**, and
**Mage+Rogue+Warlock 3v3 cooled slightly but stays above water (52.6 vs
competitive)**.

Authoritative current-state references. Use as the "before" when assessing a
balance change — **compare batch-vs-batch only**, and **full-canonical vs
full-canonical**.

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 8×8 | 100 | 6,400 | 11.1% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (784) | 100 | 78,400 | 2.6% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (3,136) | 50 | 156,800 | 1.1% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` (and `--t1 '{p}' --t2-size 1`
for 1v1) + `arenasim --batch`, then analyze with **`scripts/comp_tiers.py
<csv> --size {2,3}`** (all-comps + competitive tiers, comp lists, canaries).
See the `balance-sweep` skill.

---

## Two views: all-comps vs competitive

Class scores are reported two ways, per methodology (2026-07-02):

- **All-comps** — every comp, for completeness. Polluted by structurally
  non-competitive comps a class happens to appear in.
- **Competitive** — both sides restricted to competitive shapes. This is the
  meta read. Definitions (healers = Priest/Paladin/Shaman):
  - **2v2 competitive:** at most 1 healer (double-DPS is playable;
    double-healer is not).
  - **3v3 competitive:** 1 **or 2** healers — double-healer 3v3 is a
    legitimate meta shape; only triple-DPS and triple-healer are out.

## Class standings (winrate of comps containing the class)

Sorted by 2v2 competitive. Δ = change vs the 2026-07-02 baseline.

| Class | 1v1 | 2v2 all (Δ) | **2v2 comp (Δ)** | 3v3 all (Δ) | **3v3 comp (Δ)** |
|---|---|---|---|---|---|
| Mage | 62.6 | 65.6 (+0.5) | **66.0** (+0.5) | 62.0 (+0.1) | **65.0** (−0.3) |
| Shaman | 37.6 | 55.8 (+0.4) | **60.1** (+0.7) | 54.6 (−0.1) | **52.3** (+0.0) |
| Paladin | 60.7 | 52.7 (+0.2) | **58.2** (+0.2) | 53.0 (+0.3) | **49.5** (+0.4) |
| Priest | 8.6 | 41.5 (−1.3) | **45.9** (−2.1) | 49.0 (−0.9) | **46.2** (−0.6) |
| Hunter | 44.9 | 44.4 (−0.2) | **44.9** (+0.0) | 40.7 (+0.2) | **38.0** (+0.0) |
| Warlock | 37.4 | 43.8 (−1.4) | **43.3** (−1.6) | 46.2 (−0.6) | **46.2** (−0.6) |
| Rogue | 79.6 | 44.7 (−0.4) | **42.3** (−0.4) | 46.1 (−0.3) | **51.2** (−0.4) |
| **Warrior** | **24.2** | **41.2 (+2.2)** | **39.1 (+2.5)** | **43.9 (+1.3)** | **46.2** (+1.4) |

Reading the deltas: **the entire cost of the Warrior buff was paid by the fear
classes** — Priest (Psychic Scream now broken/immuned) and Warlock (Fear
chains answered; Death Coil unaffected). Every other class moved ≤0.7. In 3v3
competitive the Warrior (46.2) now ties Priest and Warlock in the mid-pack;
in 2v2 it remains last but within striking distance of Rogue.

## 2v2 comp tier list (784 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **80.9%** | **Mage+Shaman** | 1h |
| 77.0% | Mage+Paladin | 1h |
| 74.0% | Mage+Priest | 1h |
| 66.3% | Mage+Warlock (−1.5) | 0h |
| 66.0% | Rogue+Shaman | 1h |
| 63.4% | Paladin+Warrior (+1.1) | 1h |
| 59.8% | Paladin+Warlock | 1h |
| 58.9% | Shaman+Warlock | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 37.8% | Hunter+Warlock (new to list) |
| 36.1% | Priest+Shaman |
| **35.4%** | **Priest+Warlock (new to list — Berserker Rage counters the double-fear plan)** |
| 34.2% | Hunter+Warrior (+2.8) |
| 25.6% | Rogue+Warrior |
| 24.2% | Rogue+Warlock |
| 24.1% | Warlock+Warrior (+1.2) |
| **11.0%** | **Paladin+Priest** |

Mage+Shaman holds #1 (80.9, unmoved). The top-8 is stable; Paladin+Warrior
ticked up with the buff. The notable arrival in the bottom tier is
**Priest+Warlock (35.4)** — the double-fear comp whose game plan Berserker
Rage answers directly. Watch it next cycle: it fell from mid-table, not to
the floor, which is the intended amount of counterplay.

## 3v3 comp tier list (3,136 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **86.1%** | **Mage+Shaman+Warlock** | 1h |
| 80.6% | Mage+Paladin+Warlock (−0.6) | 1h |
| 76.4% | Mage+Priest+Shaman | 2h |
| 74.2% | Mage+Paladin+Shaman | 2h |
| 72.3% | Mage+Priest+Warlock (−1.9) | 1h |
| **70.8%** | **Mage+Paladin+Warrior (new)** | 1h |
| **70.5%** | **Mage+Shaman+Warrior (new)** | 1h |
| 70.3% | Mage+Paladin+Rogue | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 32.4% | Paladin+Priest+Warlock |
| 31.8% | Hunter+Priest+Warrior |
| 28.0% | Hunter+Paladin+Priest |
| 20.3% | Hunter+Rogue+Warlock |
| 19.5% | Paladin+Priest+Shaman |
| 17.2% | Rogue+Warlock+Warrior |
| 16.7% | Hunter+Warlock+Warrior |
| **13.5%** | **Hunter+Rogue+Warrior** |

Mage+Shaman+Warlock stays the hottest comp in the game (86.1, unmoved by the
Warrior change). **Two Warrior triples entered the top-8** — Mage + healer +
Warrior now outrates Mage+Paladin+Rogue. The nerf-watch pattern is unchanged:
"Mage + (Warlock or Shaman or both)" concentrates every point of power added
anywhere in the game.

## Canaries

Two standing structural checks, run every regeneration
(`scripts/comp_tiers.py`).

### Anomaly check — non-competitive comps performing competitively

| Bracket | Comp | Full-field | vs competitive | Verdict |
|---|---|---|---|---|
| 2v2 | **Paladin+Shaman** (2h) | **51.4%** | **56.3%** | **ANOMALY — unchanged** (identical to 2026-07-02) |
| 3v3 | **Mage+Rogue+Warlock** (0h) | **52.8%** | **52.6%** | **ANOMALY — cooled slightly** (was 54.1/54.2) |
| 3v3 | Hunter+Mage+Warlock (0h) | 50.8% | 42.7% | farm-the-trash profile, not beating real comps — watch |

- **Paladin+Shaman**: byte-for-byte the same numbers as last cycle — the
  Warrior change doesn't touch this shape. The standing read holds: the lever
  is the Shaman's offensive output in a two-healer frame (Flametongue
  magnitude / Purge cadence), not its healing. This is now the oldest open
  anomaly; it should headline the next tuning cycle.
- **Mage+Rogue+Warlock**: eased 1.6pt (the Warrior — a triple-DPS prey class —
  got harder to chain-fear) but remains above water vs the competitive field.
  Same trigger as before: if a future change pushes it back up,
  burst-vs-healing needs a structural look.

### Dominant-shape watch (3v3)

**Current: 2/10 of the top-10 3v3 comps are double-healer** (Mage+Priest+Shaman
76.4, Mage+Paladin+Shaman 74.2) — present, not dominant. No action.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1** (2v2 comp 66.0, 3v3 comp 65.0), anchors every
  top comp in both brackets, and its grip did not loosen this cycle;
  LoS/pillar play remains the deferred structural answer.
- **The Warrior buff worked as designed**: +2.5pt 2v2 comp, +1.4pt 3v3 comp,
  zero movement in non-fear matchups (verified in the pre-ship targeted
  sweep: 33.6 → 33.5 vs no-fear teams). It remains the 2v2 competitive floor
  (39.1) — the next Warrior lever, if wanted, should target its non-fear
  problem (kiting; Intercept is the queued candidate) rather than more of the
  same. In 3v3 it is now mid-pack and appears in two top-8 comps.
- **Priest took the largest hit (−2.1 2v2 comp)** — Psychic Scream was doing
  more anti-Warrior work than its druthers suggested. Priest is not in
  trouble (45.9, above Hunter/Warlock/Rogue/Warrior), but a further
  fear-economy nerf would start to bite.
- **Hunter is still the competitive 3v3 floor (38.0, unmoved)** — two cycles
  running. Its diagnosed levers (gear mana +9pt, trap-window burst
  conversion) are implemented-nowhere and remain the highest-value queued
  buff work.
- **Shaman holds #2 and the Paladin+Shaman anomaly is now two cycles old
  unchanged** — the offensive-kit-in-two-healer-frame nerf is overdue.
- **Double/triple-healer and no-sustain melee piles remain the structural
  floors** (Paladin+Priest 11.0 in 2v2; Hunter+Rogue+Warrior 13.5 in 3v3),
  with the standing Shaman and Mage+Warlock-package exceptions (see
  Canaries).

## Changes this cycle (vs the 2026-07-02 baseline)

- **Warrior Berserker Rage shipped (branch feat/warrior-berserker-rage)**:
  instant self-buff, 30s CD, usable while feared — breaks the active Fear and
  grants 10s of Fear immunity. TBC-faithful scope: **Death Coil's horror
  bypasses both** (own DR bucket discriminates it), and the TBC Incapacitate
  immunity was deliberately dropped to avoid collaterally nerfing Freezing
  Trap while Hunter is the 3v3 floor. AI presses it reactively while feared
  (Divine-Shield-style while-CC arm). Side fix: same-frame CC snapshot
  reflection now respects charge/disengage CC immunity (a scream mid-Charge
  used to bait a wasted zerk on a phantom fear).
- **Effect:** Warrior 2v2 comp 36.6 → 39.1 (+2.5), 3v3 comp 44.8 → 46.2
  (+1.4), 1v1 21.7 → 24.2; paid by Priest (−2.1 2v2 comp) and Warlock (−1.6).
  The Warrior-vs-Warlock 1v1 cell recovered 13 → ~32 from the Warrior side.
  Priest+Warlock (double-fear) dropped into the 2v2 bottom tier (35.4).
- **Canaries:** Paladin+Shaman unchanged (51.4/56.3, two cycles old);
  Mage+Rogue+Warlock cooled to 52.8/52.6 but still fires;
  Hunter+Mage+Warlock crossed 50 full-field but loses to real comps (42.7).
- Draw rates stable (2v2 2.6 → 2.6%, 3v3 1.1 → 1.1%) — the fear-break did not
  create stall pathology.
- Measurement details: seeded before/after targeted sweeps in the session
  notes; the headline slices were +7.8 vs Warlock-teams, +9.4 vs
  Priest-teams, 0.0 vs no-fear teams.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (poisons, openers, pets,
  totems, curses…) are a separate axis — see the `balance-sweep` skill.
- **Class-tier metric:** winrate over all matches where the class appears on a
  side (draws count as losses); computed by `scripts/comp_tiers.py`.
