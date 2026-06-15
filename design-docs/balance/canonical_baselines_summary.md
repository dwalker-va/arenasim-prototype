# Canonical Balance Baselines

**Generated 2026-06-15** on the `feat/priest-psychic-scream` meta — i.e. the
2026-06-13 Hunter/Mage state *plus* the Priest Psychic Scream feature (PR #73):
an instant self-centered AoE fear with a defensive self-peel and a
kill-target-respecting offensive dip. Supersedes the 2026-06-13 baselines.

**The dominant change this cycle is Psychic Scream.** It lifts the Priest — the
prior universal floor of team formats — out of the bottom: the defensive
panic-button peel is the driver (+~4pt in 2v2), and the offensive dip respects
the kill target (fears the enemy healer only when the team is committing
elsewhere), so it adds value without fighting the team's focus. The change is
Priest-isolated; non-Priest cells are byte-identical to the 2026-06-13 binary
and carry that cycle's reading (the Hunter pet-damage fix, Mage kiting pilot,
etc. — see "prior cycle" notes below).

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 5.0% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 1.0% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.3% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` + `arenasim --batch` (see the
`balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 69.4, Mage 66.4 | **Mage 66.9** | **Mage 56.7** |
| **A — strong** | Hunter 58.9, Rogue 52.1 | Paladin 57.7 | Paladin 54.7 |
| **B — playable** | — | **Priest 49.8**, Warrior 45.2 | **Priest 50.7**, Warlock 47.5, Warrior 47.3, Rogue 47.1 |
| **C — weak** | Warlock 35.9, Warrior 32.7 | Warlock 43.2, Hunter 42.7, Rogue 40.7 | Hunter 42.9 |
| **D — bottom** | Priest 12.1 | — | — |

**The team-format meta is still Mage + Paladin** at the top — Psychic Scream is
Priest-isolated, so the carries are unchanged within noise. **The action is the
Priest:** in 2v2 it climbs to the top of B-tier (45.8 → 49.8), now ahead of
Warrior and the C cluster and trailing only Mage/Paladin; in 3v3 it holds B
(~50.7). It is no longer a team-format floor. (1v1 stays D — a lone healer can't
kill, and the panic button mostly converts losses into draws there: 1v1 draws
rose 2.8% → 5.0%.) Non-Priest classes' aggregates move only via their matchup
vs the now-stronger Priest (a ≤1-2pt dip in that one column); their
non-Priest-vs-non-Priest cells are byte-identical to 2026-06-13.

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **78.3%** | Mage+Priest |
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
| **16.3%** | **Paladin+Priest** |

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
- **The Priest is no longer a team-format floor** (Psychic Scream, PR #73). It
  sits mid-B in both formats (~50%), behind only Mage/Paladin. The defensive
  AoE-fear peel is the value; the offensive dip-to-fear-the-enemy-healer fires
  only when the team kills someone else (respecting the kill target), so it
  never breaks its own team's focus. Mage+Priest remains a top-tier carry pair
  (78.3% in 2v2).
- **Double-healer is still a trap.** Paladin+Priest is still the worst 2v2 comp
  (16.3%) — even with Psychic Scream, two healers with no kill pressure lose the
  attrition war at the 300s cap.
- **No-healer melee piles are the floor** (Warrior+Rogue+Warlock 5.4% in 3v3) —
  no sustain, no carry engine.
- **The Hunter is no longer the universal bottom.** The pet-damage fix lifted it
  to A-tier in 1v1 and out of the sole-floor slot in teams. Its remaining holes
  are matchup-structural, not stat-deficits: it cannot kill through Paladin/Mage
  sustain+control 1v1, and Hunter+Priest still loses the 2v2 grind vs Warrior
  (healer self-peel) and vs Mage/Warlock (control/sustain) — see the Hunter
  follow-ups in `design-docs/roadmap.md`.

## Changes this cycle (vs the 2026-06-13 baseline)

- **Priest is the headline: Psychic Scream (PR #73).** In the isolated
  before/after toggle (same seeds, scream off vs on — the rigorous feature
  measure), the Priest gains **+3.7pt in 2v2 and +1.5pt in 3v3** overall. The
  **defensive self-peel is the driver** (+4pt vs no-healer comps in both
  formats); the **offensive dip respects the kill target** (`team_focus` —
  fears the enemy healer only when the team is committing elsewhere), turning a
  first-cut net drag into a gain. It lifts the Priest off the team-format floor
  into mid-B.
- **Knock-on, not nerfs.** Each non-Priest class's aggregate dips ≤1-2pt only
  through its single matchup column vs the now-stronger Priest (e.g. Rogue 1v1
  57.6 → 52.1 is Rogue-vs-Priest shifting). Non-Priest-vs-non-Priest cells are
  byte-identical to 2026-06-13 (the change is Priest-only code).
- **Draw rates ticked up** (1v1 2.8 → 5.0%, 2v2 0.7 → 1.0%, 3v3 0.2 → 0.3%):
  the panic button lets the Priest survive longer, converting some quick losses
  into timeouts — not a stall pathology (sweep draws stayed near baseline, no
  mirror draw-fest).

### Prior cycle (2026-06-13, carried forward on non-Priest cells)

- **Hunter** pet-damage fix (`1a41deb`) was that cycle's headline (+38.7 in 1v1,
  +4.3 2v2, +3.1 3v3), lifting it off the universal floor. **Mage** edged up with
  the kiting pilot (PR #69). These remain the current non-Priest reading.

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
