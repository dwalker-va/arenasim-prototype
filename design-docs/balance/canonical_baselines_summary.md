# Canonical Balance Baselines

**Generated 2026-06-22** on the Rogue Crippling Poison meta — i.e. the Rogue
Kidney Shot chain state *plus* Crippling Poison: a weapon poison whose
auto-attacks have a 50% chance to apply/refresh a 70% movement slow (an 8s
*poison* debuff — immune to Dispel Magic, removable only by a cleanse).
Supersedes the earlier 2026-06-22 Kidney-chain baselines.

**The dominant change this cycle is Crippling Poison, and it is the sticking
lever the Rogue actually needed.** The Kidney Shot chain (last cycle) was pure
control and barely moved the Rogue's team numbers (+1.1 / +1.0); the Rogue
stayed dead last in both team formats because its real problem was *uptime* —
getting kited and peeled off the kill target. A maintained 70% slow fixes that
directly: **Rogue 1v1 64.9 → 77.3 (now the #1 1v1 class), 2v2 40.3 → 45.6
(+5.2), 3v3 41.5 → 44.9 (+3.4).** The Rogue is **off the team-format floor in
both formats** (5th of 7, above Warlock and Warrior) — the new floor is the
Warrior. Most visible single cell: 1v1 Rogue-vs-Hunter **0% → 64%**, a kite the
Rogue previously could never close. The change is Rogue-isolated; each other
class's aggregate shifts ≤2.3pt and only through its Rogue matchup column.

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (a few points off the older
multithreaded `--matrix` numbers), and **full-canonical vs full-canonical** (a
focused `--t1` slice vs a label subset of the canonical is biased for the
aggregate).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 5.1% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 0.9% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.4% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy
(Rogue opener = Cheap Shot, Rogue poison = Crippling). Regenerate via
`scripts/gen_sweep.py --full {2,3}` (and `--t1 '{p}' --t2-size 1` for 1v1) +
`arenasim --batch` (see the `balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | **Rogue 77.3**, Paladin 65.4 | **Mage 61.6** | **Mage 62.1** |
| **A — strong** | Mage 57.4 | Paladin 56.6 | Paladin 55.8 |
| **B — playable** | Hunter 51.3 | Hunter 50.6, Priest 46.7, **Rogue 45.6**, Warlock 43.6 | Priest 52.5, Hunter 46.7, **Rogue 44.9**, Warlock 43.9 |
| **C — weak** | Warlock 38.9, Warrior 32.3 | Warrior 42.2 | Warrior 42.8 |
| **D — bottom** | Priest 9.9 | — | — |

**The team-format meta is still Mage + Paladin**, unchanged in ordering (the
Crippling change is Rogue-isolated). **The story is the Rogue:** it is now the
**#1 1v1 class (77.3)** — the Cheap Shot → Kidney lockdown plus a maintained 70%
slow means a single target can neither escape nor out-sustain it — and it has
climbed **off the team-format floor** (2v2 45.6, 3v3 44.9, 5th of 7 in both).
The slow gives the Rogue the damage *uptime* on the kill target that the
control-only chain could not. **The Warrior is now the team-format floor**
(42.2 / 42.8): a no-utility melee with no sustain and no peel, it is the class
most exposed once the Rogue is no longer the obvious bottom. Other classes move
only through their Rogue matchup column (≤2.3pt; Warrior −1.8/−2.3 and Mage
−1.7/−1.0 are the largest, both the Rogue-vs-them cells).

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **74.0%** | Mage+Paladin |
| 73.4% | Mage+Priest |
| 68.3% | Paladin+Warrior |
| 63.5% | Mage+Warlock |
| **63.2%** | **Paladin+Rogue** |
| 61.9% | Hunter+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 40.6% | Hunter+Warrior |
| 38.7% | Priest+Warrior |
| 36.6% | Rogue+Warrior |
| 26.5% | Rogue+Warlock |
| 19.8% | Warlock+Warrior |
| **11.2%** | **Paladin+Priest** |

**Paladin+Rogue (63.2%) is now a top-5 2v2 comp** — the Paladin sustains while a
sticky Rogue grinds the kill target down; it was mid-pack before Crippling. The
Rogue's *own* worst pairings also climbed (Rogue+Warlock 20.6 → 26.5, Rogue+
Warrior 32.7 → 36.6). The floor is now **Warrior-heavy** (Hunter+Warrior,
Priest+Warrior, Rogue+Warrior all near the bottom) plus double-healer
(Paladin+Priest 11.2%, still the worst).

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **82.9%** | Mage+Paladin+Warlock |
| 79.7% | Mage+Priest+Warlock |
| 77.6% | Hunter+Mage+Paladin |
| **77.1%** | **Mage+Paladin+Rogue** |
| 74.7% | Hunter+Mage+Priest |
| **74.3%** | **Mage+Priest+Rogue** |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 28.2% | Priest+Warlock+Warrior |
| 25.7% | Paladin+Priest+Warlock |
| 25.3% | Hunter+Rogue+Warlock |
| 22.1% | Hunter+Warlock+Warrior |
| 17.4% | Hunter+Rogue+Warrior |
| **7.9%** | **Rogue+Warlock+Warrior** |

Rogue-with-two-carries is now firmly top-tier (Mage+Paladin+Rogue 77.1%,
Mage+Priest+Rogue 74.3%). No-sustain melee piles are still the floor —
Rogue+Warlock+Warrior 7.9% remains the single worst comp in the game — which is
a comp-composition problem (no healer, no carry), not a Rogue deficit.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1 in teams** (61.6 / 62.1) and anchors the top comps;
  **LoS/pillar play remains the deferred structural answer to its kiting.**
- **Paladin holds #2**; Mage+Paladin is the top 2v2 (74.0%), and **Paladin+Rogue
  (63.2%) is a new top-5** thanks to Crippling.
- **The Rogue is now an S-tier 1v1 bully (77.3) and a mid-B team class (45.6 /
  44.9), off the floor.** Crippling Poison is the *uptime* lever the chain
  lacked. Its remaining team ceiling is comp-driven (it wants a carry/healer
  partner; no-sustain Rogue piles are still the game's floor), not a stat
  deficit. **Counterplay is real: Paladin Cleanse strips the poison** (it's a
  poison, immune to Dispel Magic) — a healthy Paladin lifts it, an under-
  pressure one prioritizes healing, so the slow has partial uptime rather than
  full negation.
- **The Warrior is the new team-format floor** (42.2 / 42.8) — surfaced, not
  caused, by the Rogue's climb: a no-utility, no-sustain melee. Likely the next
  candidate for attention.
- **Double-healer and no-sustain melee piles remain the structural floors**
  (Paladin+Priest 11.2% 2v2; Rogue+Warlock+Warrior 7.9% 3v3).

## Changes this cycle (vs the prior 2026-06-22 Kidney-chain baseline)

- **Crippling Poison (this PR).** A Rogue weapon poison (the `RoguePoison`
  strategic lever, default Crippling): auto-attacks roll a 50% `application_
  chance` to apply/refresh an 8s, 70% movement slow. The slow is a **poison
  debuff** (new `DispelType::Poison`) — immune to Dispel Magic, removable only
  by a poison cleanse; **Paladin Cleanse** was wired to lift it (rated at the
  maintenance-cleanse tier). Refreshed in place, so it never diminishes. Rogues
  carry a "Crippling Poison" weapon self-buff indicator.
- **Effect: Rogue 1v1 64.9 → 77.3 (+12.4, now #1), 2v2 40.3 → 45.6 (+5.2),
  3v3 41.5 → 44.9 (+3.4).** Unlike the control-only chain (which netted ~+1),
  the slow moves the team aggregate meaningfully and lifts the Rogue off the
  floor in both team formats. Single biggest cell: 1v1 Rogue-vs-Hunter 0% → 64%.
- **Knock-on, not nerfs.** Each non-Rogue class shifts ≤2.3pt and only via its
  Rogue matchup column (Warrior −1.8/−2.3, Mage −1.7/−1.0, Hunter −1.0/−0.2 in
  2v2/3v3). Non-Rogue-vs-non-Rogue cells are unchanged.
- **Draw rates flat** (1v1 5.1%, 2v2 0.9%, 3v3 0.4%): no stall pathology.

### Prior cycle (Kidney Shot chain, carried forward)

The Kidney Shot chain (own `KidneyShotStun` DR category; Cheap Shot → Kidney
~10s opener lockdown; Kick→hold→Kidney denial chain) is the control layer
beneath Crippling. It made the Rogue an A-tier 1v1 class but left it the
team-format floor; Crippling is what lifts the team numbers.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs
  both orderings so tier lists average it out. The Rogue's deterministic opener
  amplifies this in Rogue-vs-Rogue mirrors. Mechanism in
  `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`; fix deferred.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (poisons, openers, pets,
  curses…) are a separate axis — see the `balance-sweep` skill. The Rogue poison
  default is Crippling (the only poison so far; more are planned as a lever like
  the opener).
