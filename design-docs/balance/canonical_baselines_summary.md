# Canonical Balance Baselines

**Generated 2026-06-13** on the current `worktree-ai-tuning` meta — i.e. *after*
the context-steering mask refactor + Mage kiting pilot (PR #69), the Hunter
ENGAGE/KITE movement migration, and the three Hunter/pet fixes: melee-only kite
filter (`03387f4`), melee-pet dead-zone fix (`1a41deb`), and the friendly-CC
auto-attack guard (`c0dc2af`). Supersedes the 2026-06-07 baselines, which
predated all of the above.

**The dominant change is the pet-damage fix.** Every prior Hunter baseline was
computed with a damage-dead pet — the ranged Auto-Shot dead zone silently
cancelled every melee-pet swing. With the pet now contributing, Hunter is no
longer the universal floor (see below).

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 2.8% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 0.7% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.2% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` + `arenasim --batch` (see the
`balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 69.0, Mage 65.6 | **Mage 67.8** | **Mage 62.6** |
| **A — strong** | Hunter 59.4, Rogue 57.6 | Paladin 59.3 | Paladin 57.4 |
| **B — playable** | — | Priest 45.8, Warrior 45.4 | Priest 50.9, Warrior 48.1 |
| **C — weak** | Warlock 37.1, Warrior 31.4 | Hunter 43.5, Rogue 42.5, Warlock 42.5 | Warlock 44.1, Hunter 43.4, Rogue 42.4 |
| **D — bottom** | Priest 13.9 | — | — |

**The team-format meta is still Mage + Paladin** and barely moved — the Hunter
work is class-isolated, so the top of the board is unchanged from 2026-06-07. The
action is at the bottom: **Hunter climbed out of the universal-floor slot.** In
1v1 it leapt from worst (20.7) to A-tier (59.4) — the pet is a large fraction of
a solo Hunter's damage. In teams the pet is a smaller share of total DPS, so the
gain is modest (39.2 → 43.5 in 2v2, 40.3 → 43.4 in 3v3) but enough that Hunter,
Rogue, and Warlock now form a tied low-C cluster instead of Hunter sitting alone
at the bottom.

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **78.0%** | Mage+Priest |
| 76.5% | Mage+Paladin |
| 74.3% | Warrior+Paladin |
| 70.5% | Mage+Warlock |
| 65.5% | Rogue+Paladin |
| 63.1% | Warlock+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 35.4% | Warrior+Hunter |
| 31.3% | Warrior+Rogue |
| 28.8% | Warlock+Hunter |
| 22.2% | Rogue+Warlock |
| 20.4% | Warrior+Warlock |
| **17.3%** | **Paladin+Priest** |

Hunter's best 2v2 partners are now the carries: **Paladin+Hunter 58.8%,
Mage+Hunter 57.5%** — both solidly mid, where the old baseline had every Hunter
comp in the bottom third. Its floor is still the no-sustain pairings
(Warlock+Hunter 28.8%, Warrior+Hunter 35.4%).

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **87.2%** | Warrior+Mage+Paladin |
| 84.2% | Mage+Warlock+Paladin |
| 80.2% | Warrior+Mage+Priest |
| 78.5% | Mage+Priest+Warlock |
| 78.0% | Mage+Rogue+Priest |
| 72.9% | Mage+Rogue+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 30.1% | Rogue+Priest+Warlock |
| 23.3% | Rogue+Warlock+Hunter |
| 21.4% | Warrior+Rogue+Hunter |
| 16.8% | Warrior+Warlock+Hunter |
| **5.4%** | **Warrior+Rogue+Warlock** |

Hunter+healer+carry now reaches the top third (**Mage+Priest+Hunter 69.6%**), but
no-healer melee piles remain the floor — Warrior+Rogue+Warlock 5.4% is the worst
comp in the game (no sustain, no carry engine), and Hunter still appears in 3 of
the 6 worst comps when paired with other low-sustain classes.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1 in teams** (67.8 / 62.6) and anchors all six top 3v3
  comps. Its kiting (now on the shared ENGAGE/KITE machine via PR #69) is still
  the only movement intelligence among DPS, and nothing counters it
  structurally — **LoS/pillar play remains the deferred answer**.
- **Paladin holds #2**; Warrior+Paladin is still a top-3 2v2 (74.3%). Unchanged
  from the prior cycle within noise.
- **Double-healer is still a trap.** Paladin+Priest is the worst 2v2 comp
  (17.3%) — at the 300s cap, two healers with no kill pressure lose the
  attrition war.
- **No-healer melee piles are the floor** (Warrior+Rogue+Warlock 5.4% in 3v3) —
  no sustain, no carry engine.
- **The Hunter is no longer the universal bottom.** The pet-damage fix lifted it
  to A-tier in 1v1 and out of the sole-floor slot in teams. Its remaining holes
  are matchup-structural, not stat-deficits: it cannot kill through Paladin/Mage
  sustain+control 1v1, and Hunter+Priest still loses the 2v2 grind vs Warrior
  (healer self-peel) and vs Mage/Warlock (control/sustain) — see the Hunter
  follow-ups in `design-docs/roadmap.md`.

## Changes this cycle (vs the 2026-06-07 baseline)

- **Hunter** is the headline: **+38.7 in 1v1** (20.7 → 59.4), +4.3 in 2v2
  (39.2 → 43.5), +3.1 in 3v3 (40.3 → 43.4). Almost entirely the pet-damage fix
  (`1a41deb`); the melee-only kite filter and CC guard keep it from fleeing
  casters and breaking its own Freezing Trap / Web.
- **Hunter's 1v1 victims dropped** as it started winning even matchups: Rogue
  68.7 → 57.6 (Hunter now wins it 84%), Warlock 47.4 → 37.1, Priest 21.4 → 13.9.
  These are Hunter-row knock-on effects, not nerfs to those classes.
- **Mage** edged up with the kiting pilot (62.9 → 65.6 in 1v1, 66.0 → 67.8 in
  2v2; 3v3 flat) — class-isolated to Mage cells per the PR #69 matrix check.
- **Everything else is within noise** of 2026-06-07. The Warrior/Rogue/Priest/
  Warlock/Paladin core was verified byte-identical to the prior binary on the
  non-Hunter, non-Mage cells, so those numbers carry the prior cycle's reading.
- **Draw rates fell** (1v1 7.0 → 2.8%, 2v2 1.3 → 0.7%, 3v3 0.4 → 0.2%): the live
  pet converts Hunter timeouts into decisive games (Hunter draws were a big
  chunk of the old healer-mirror-dominated draw wall).

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out, but don't read a single ordered cell as
  definitive. Mechanism documented in
  `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`; fix deferred.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (pets, openers, curses…)
  are a separate axis — see the `balance-sweep` skill. Note the Hunter pet now
  contributes damage, so pet-type strategy sweeps are newly meaningful.
