# Canonical Balance Baselines

**Generated 2026-07-02** — regenerated after the **Warlock healer-lockout +
Death Coil** changes (PR #79). Supersedes the 2026-06-28 Shaman-debut baselines.

**Headline: the Warlock is off the floor in both team brackets (2v2 40.9 →
45.2, 3v3 43.3 → 46.8) and the Warrior is the new clear 2v2 floor (39.0 all /
36.6 competitive).** The cost of the Warlock buff lands almost entirely on the
two melee classes its Death Coil peels (Rogue −2.1, Warrior −2.0 in 2v2).
Two canaries fire this cycle: **Paladin+Shaman 2v2 (double-healer) is now 51.4%
— an anomaly that worsened**, and **Mage+Rogue+Warlock (triple-DPS) beats the
competitive 3v3 field at 54.2%** — both flagged below.

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

Sorted by 2v2 competitive. Δ = change vs the 2026-06-28 baseline.

| Class | 1v1 | 2v2 all (Δ) | **2v2 comp (Δ)** | 3v3 all (Δ) | **3v3 comp (Δ)** |
|---|---|---|---|---|---|
| Mage | 62.6 | 65.1 (+0.4) | **65.5** (+0.5) | 61.9 (−0.5) | **65.3** (−0.9) |
| Shaman | 37.6 | 55.4 (−0.2) | **59.4** (−0.3) | 54.7 (+0.4) | **52.3** (+0.7) |
| Paladin | 60.7 | 52.5 (−0.5) | **58.0** (−1.2) | 52.7 (−0.1) | **49.1** (−0.1) |
| Priest | 8.6 | 42.8 (−0.4) | **48.0** (−0.2) | 49.9 (−0.4) | **46.8** (−0.3) |
| Hunter | 44.9 | 44.6 (−0.2) | **44.9** (−0.2) | 40.5 (−0.5) | **38.0** (−0.9) |
| **Warlock** | **39.9** | **45.2 (+4.3)** | **44.9 (+5.3)** | **46.8 (+3.5)** | **46.8 (+3.0)** |
| Rogue | 79.6 | 45.1 (−2.1) | **42.7** (−2.3) | 46.4 (−1.3) | **51.6** (−1.0) |
| Warrior | 21.7 | 39.0 (−2.0) | **36.6** (−2.5) | 42.6 (−1.1) | **44.8** (−0.7) |

Reading the competitive column vs all-comps: **healers rise in 2v2** once the
double-healer floor comps are excluded (Paladin 52.5 → 58.0, Priest 42.8 →
48.0); **Mage rises to 65.3 in competitive 3v3** (its comps are all real
comps, and the triple-DPS punching bags leave the field); **Rogue is a
respectable #3 in competitive 3v3** (51.6) despite its poor all-comps score —
triple-DPS piles were dragging it. **Hunter is the competitive 3v3 floor
(38.0)**; **Warrior is the competitive 2v2 floor (36.6)**.

## 2v2 comp tier list (784 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **80.9%** | **Mage+Shaman** | 1h |
| 77.0% | Mage+Paladin | 1h |
| 73.6% | Mage+Priest | 1h |
| **67.8%** | **Mage+Warlock** (+6.8) | **0h** |
| 66.0% | Rogue+Shaman | 1h |
| 62.3% | Paladin+Warrior | 1h |
| 60.0% | Paladin+Warlock (+4.3) | 1h |
| 59.6% | Shaman+Warlock (+7.4) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 36.4% | Priest+Shaman |
| 31.4% | Hunter+Warrior |
| 26.1% | Rogue+Warlock |
| 25.6% | Rogue+Warrior (−4.8) |
| 22.9% | Warlock+Warrior |
| **10.3%** | **Paladin+Priest** |

Mage+Shaman holds #1 (80.9). **All three Warlock+healer comps jumped into the
top-8** (Paladin+Warlock 60.0, Shaman+Warlock 59.6), and **Mage+Warlock (67.8,
double-DPS) is now the #4 comp** — the Death Coil peel + healer-Fear package
gives the Warlock exactly what its comps lacked. Warrior comps sank across the
board (Paladin+Warrior 66.8 → 62.3, Rogue+Warrior 30.4 → 25.6).

## 3v3 comp tier list (3,136 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **86.1%** | **Mage+Shaman+Warlock** (+7.5) | 1h |
| 81.2% | Mage+Paladin+Warlock (+3.6) | 1h |
| 77.1% | Mage+Priest+Shaman | 2h |
| 74.8% | Mage+Paladin+Shaman | 2h |
| 74.2% | Mage+Priest+Warlock | 1h |
| 70.3% | Mage+Paladin+Rogue (−6.4) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 28.3% | Hunter+Paladin+Priest |
| 20.5% | Paladin+Priest+Shaman |
| 20.4% | Hunter+Rogue+Warlock |
| 17.9% | Rogue+Warlock+Warrior |
| 15.0% | Hunter+Warlock+Warrior |
| **11.0%** | **Hunter+Rogue+Warrior** |

**Mage+Shaman+Warlock at 86.1% (+7.5) is the hottest comp in the game** — the
Warlock buff amplified the already-#1 triple. Nerf-watch: the pattern is
"Mage + (Warlock or Shaman or both)", and every point of Warlock/Shaman power
concentrates there.

## Canaries

Two standing structural checks, run every regeneration
(`scripts/comp_tiers.py`).

### Anomaly check — non-competitive comps performing competitively

A non-competitive comp winning ≥50% (especially vs the competitive field)
points at a fundamental balance issue, not a comp-selection quirk.

| Bracket | Comp | Full-field | vs competitive | Verdict |
|---|---|---|---|---|
| 2v2 | **Paladin+Shaman** (2h) | **51.4%** | **56.3%** | **ANOMALY — worsened** (was 50.0/54.8) |
| 3v3 | **Mage+Rogue+Warlock** (0h) | **54.1%** | **54.2%** | **ANOMALY — new this cycle** |
| 3v3 | Hunter+Mage+Warlock (0h) | 51.3% | 43.2% | farm-the-trash profile, not beating real comps — watch |

- **Paladin+Shaman**: a double-healer 2v2 comp beating real comps 56.3% of the
  time. The offensive Shaman removes the no-kill-pressure failure mode that
  makes double-healer non-competitive. Confirmed issue (user-ack'd 2026-07-02);
  the lever is the Shaman's offensive output in a two-healer frame, not its
  healing.
- **Mage+Rogue+Warlock**: a triple-DPS 3v3 comp now beating the competitive
  field. Driven by the Warlock buff (+7.5 on this comp): healer-Fear +
  Death Coil peel + Mage control apparently substitute for a healer's
  attrition-proofing. If the next Warlock or Mage change pushes this further,
  burst-vs-healing balance needs a look.

### Dominant-shape watch (3v3)

Double-healer 3v3 is a legitimate meta shape; the warning sign is it becoming
the *dominant* shape at the top of the bracket. **Current: 2/10 of the top-10
3v3 comps are double-healer** (Mage+Priest+Shaman 77.1, Mage+Paladin+Shaman
74.8) — present, not dominant. No action.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1** (2v2 comp 65.5, 3v3 comp 65.3) and anchors every
  top comp in both brackets; LoS/pillar play remains the deferred structural
  answer.
- **The Warlock buff worked as designed and landed mid-pack** (2v2 comp 44.9,
  3v3 comp 46.8 — below 50 in both). The class is not overtuned; what IS hot is
  the **Mage+Warlock package** (2v2 #4 at 67.8; 3v3 #1/#2/#5) — Mage burst
  converts the healer-Fear/horror windows the Warlock now creates. Any further
  Warlock buff should be checked against Mage+Warlock cells first.
- **Warrior is the new 2v2 floor** (comp 36.6, 1v1 21.7) and needs the next
  buff cycle. Its losses are concentrated vs Warlock (1v1 33 → 13 before/after
  from the Warlock's side: 87% Warlock+X vs Warrior slices) — Death Coil hard-
  counters a class whose whole plan is "stand on the caster", and the Warrior
  has no dispel and no ranged pressure. A Warrior lever that isn't "more melee
  damage" (berserker rage fear-break, intercept, spell reflect) fits the gap.
- **Hunter is the competitive 3v3 floor** (38.0) — unchanged from last cycle's
  read; its known structural issues ([hunter-2v2-warrior-loss], mana economy)
  stand.
- **Shaman holds #2 and the Paladin+Shaman 2v2 anomaly worsened** — the
  Shaman nerf-watch from last cycle stands, sharpened: the problem shape is
  Shaman-as-second-healer, so the lever is its offensive kit (Flametongue
  magnitude / Purge cadence) in frames that already have a healer, not its
  solo-healer viability.
- **Double/triple-healer and no-sustain melee piles remain the structural
  floors** (Paladin+Priest 10.3 in 2v2; Hunter+Rogue+Warrior 11.0 in 3v3) —
  except where the Shaman or the new Warlock package bends the rule (see
  Canaries).

## Changes this cycle (vs the 2026-06-28 Shaman-debut baseline)

- **Warlock healer-lockout + Death Coil shipped (PR #79)**: (1) AI aims Fear at
  the enemy healer early (own DR limits the chain) and Felhunter Spell Lock
  prefers heal casts; (2) new ability Death Coil — instant 30yd, 30s CD, ~49
  damage + 3s horror that never breaks on damage (own Horror DR) + 100%
  lifesteal; AI uses it as a reactive anti-melee peel (8yd trigger).
- **Effect:** Warlock 2v2 40.9 → 45.2 (+4.3), 3v3 43.3 → 46.8 (+3.5); paid for
  by Rogue (−2.1 2v2) and Warrior (−2.0 2v2, new floor). Healer/caster classes
  ~flat. 1v1: Warlock 35.4 → 39.9, Warrior 28.2 → 21.7 (the Warlock-vs-Warrior
  cell flipped 33 → 87 — no dispel, no healer, Death Coil is a hard counter).
- **New canaries fired:** Mage+Rogue+Warlock triple-DPS above 50 vs the
  competitive 3v3 field (new); Paladin+Shaman 2v2 anomaly worsened to
  51.4/56.3.
- Draw rates stable (2v2 2.4 → 2.6%, 3v3 1.1 → 1.1%) — the added CC did not
  create stall pathology.
- Details: `2026-06-28-warlock-balance-findings.md`,
  `2026-06-28-warlock-death-coil-prototype.md`.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (poisons, openers, pets,
  totems, curses…) are a separate axis — see the `balance-sweep` skill.
- **Class-tier metric:** winrate over all matches where the class appears on a
  side (draws count as losses); computed by `scripts/comp_tiers.py`. Historical
  1v1 tier numbers in the 2026-06-28 doc used a slightly different ad-hoc
  aggregation (≤1pt differences); team-format numbers match to 0.1pt.
