# Canonical Balance Baselines

**Generated 2026-06-22** on the Rogue Kidney Shot chain meta — i.e. the Hunter
mana-economy + trap/Concussive state (PR #74) *plus* the Rogue rework: Kidney
Shot moved onto its own diminishing-returns category, a Cheap Shot → Kidney Shot
opener that chains a ~10s undiminished stun on the kill target, and a
Kick→hold→Kidney denial chain against caster targets. Supersedes the earlier
2026-06-22 Hunter-rebalance baselines.

**The dominant change this cycle is the Rogue Kidney Shot chain.** It is a large
**1v1** control buff and a heavy **team-format redistribution that nets small.**
Sticking a stun chain wins 1v1s (Rogue **56.6 → 64.9**, now the #2 1v1 class),
but in teams the Rogue's binding constraint is damage/sustain against kiting and
peels, which better *control* does not fix: 2v2 **39.2 → 40.3 (+1.1)**, 3v3
**40.5 → 41.5 (+1.0)** — the Rogue stays the **bottom class in both team
formats.** The redistribution underneath is enormous (37% of 2v2 and 52% of 3v3
matches flip outcome) but bidirectional, so the aggregate barely moves. The
change is Rogue-isolated code + Rogue-only behavior: each other class's aggregate
shifts ≤1.5pt, and only through its own matchup column vs the Rogue (e.g. 1v1
Mage 65.6 → 57.4 is the Mage-vs-Rogue cell; 3v3 Mage 61.7 → 63.2 is the *ally*
Mage+Rogue comps improving). Non-Rogue-vs-non-Rogue cells are unchanged.

Authoritative current-state references. Use as the "before" when assessing any
balance change — **compare batch-vs-batch only** (these differ a few points from
the older multithreaded `--matrix` numbers), and **full-canonical vs
full-canonical** (a focused `--t1` slice compared against a label subset of the
canonical is biased — it cost this cycle a wrong first read).

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 | 5.1% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (441) | 100 | 44,100 | 0.9% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (1225) | 50 | 61,250 | 0.4% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy
(Rogue opener defaults to Cheap Shot). Regenerate via `scripts/gen_sweep.py
--full {2,3}` (and `--t1 '{p}' --t2-size 1` for 1v1) + `arenasim --batch` (see
the `balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | Paladin 68.6, **Rogue 64.9** | **Mage 63.2** | **Mage 63.2** |
| **A — strong** | Hunter 60.4, Mage 57.4 | Paladin 57.6 | Paladin 55.9 |
| **B — playable** | — | Hunter 51.6, Priest 46.5, Warrior 44.0, Warlock 43.8 | Priest 51.9, Hunter 46.9, Warrior 45.0, Warlock 44.2 |
| **C — weak** | Warlock 38.9, Warrior 32.3 | **Rogue 40.3** | **Rogue 41.5** |
| **D — bottom** | Priest 9.9 | — | — |

**The team-format meta is still Mage + Paladin.** The Rogue change is
Rogue-isolated, so the carries are unchanged in ordering. **The action is the
Rogue, and it is mostly a 1v1 story:** it leaps to the #2 1v1 class (64.9, behind
only Paladin) because a Cheap Shot → Kidney Shot lockdown plus a Kick/Kidney
denial chain lets it stick and kill before a lone target escapes. In teams the
same control redistributes matchups massively but nets only +1.1 (2v2) / +1.0
(3v3): the Rogue is still the worst class in both, because control quality
doesn't solve its team problem (it gets peeled and kited off, and lacks a burst
finisher). Other classes' aggregates move only via their Rogue matchup column
(≤1.5pt); their non-Rogue-vs-non-Rogue cells are unchanged.

## 2v2 comp tier list (441 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **76.4%** | Mage+Paladin |
| 74.9% | Mage+Priest |
| 70.6% | Paladin+Warrior |
| 66.3% | Mage+Warlock |
| 64.0% | Paladin+Warlock |
| 63.4% | Hunter+Paladin |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 40.8% | Hunter+Warrior |
| 38.7% | Priest+Rogue |
| 32.7% | Rogue+Warrior |
| 20.6% | Rogue+Warlock |
| 19.4% | Warlock+Warrior |
| **13.0%** | **Paladin+Priest** |

Rogue's worst pairings ticked up but stayed in the cellar (Priest+Rogue
36.4 → 38.7, Rogue+Warrior 29.1 → 32.7, Rogue+Warlock 19.1 → 20.6). **No Rogue
comp reaches the top tier in 2v2** — its best partners are still the carries
(Rogue+Mage, Rogue+Paladin) and even those sit mid-pack. The bottom of the format
is still double-melee-no-sustain and double-healer (Paladin+Priest 13.0%).

## 3v3 comp tier list (1225 comps)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **83.7%** | Mage+Paladin+Warlock |
| 80.3% | Mage+Priest+Warlock |
| 79.4% | Mage+Paladin+Warrior |
| **79.1%** | **Hunter+Mage+Paladin** |
| **75.1%** | **Mage+Paladin+Rogue** |
| 74.9% | Hunter+Mage+Priest |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 31.6% | Priest+Warlock+Warrior |
| 28.3% | Paladin+Priest+Warlock |
| 22.8% | Hunter+Rogue+Warlock |
| 22.7% | Hunter+Warlock+Warrior |
| 15.9% | Hunter+Rogue+Warrior |
| **8.5%** | **Rogue+Warlock+Warrior** |

The headline for the Rogue in 3v3: **Rogue-with-two-carries now reaches the top**
(Mage+Paladin+Rogue 75.1%, Mage+Priest+Rogue 72.4%) — the chain's lockdown is
genuinely strong when a carry converts the window. But no-carry Rogue piles are
still the floor — Rogue+Warlock+Warrior 8.5% remains the single worst comp in the
game — so the Rogue's aggregate stays last. Those are comp-composition problems,
not a Rogue stat surplus.

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1 in teams** (63.2 / 63.2) and anchors the top comps;
  nothing counters its kiting structurally. **LoS/pillar play remains the
  deferred answer.**
- **Paladin holds #2**; Mage+Paladin is the top 2v2 (76.4%).
- **The Rogue is now an S-tier 1v1 bully** (64.9, the Kidney Shot chain) but
  **still the bottom team-format class** (40.3 / 41.5). The chain change is a
  control-quality buff, not a power buff: it does not address why the Rogue
  craters in teams (peeled/kited off the kill target, no burst finisher). Its
  best team comps (Rogue + two carries, e.g. Mage+Paladin+Rogue 75.1% in 3v3) are
  now strong, but its floor comps remain the worst in the game.
- **Double-healer is still a trap** (Paladin+Priest 13.0% — worst 2v2).
- **No-sustain melee piles are the floor** (Rogue+Warlock+Warrior 8.5% in 3v3).

## Changes this cycle (vs the prior 2026-06-22 Hunter-rebalance baseline)

- **Rogue Kidney Shot chain (this PR).** Kidney Shot moved to its own
  `KidneyShotStun` DR category (a Cheap Shot opener now chains into an
  undiminished Kidney Shot — a ~10s lockdown on the kill target); default opener
  changed Ambush → Cheap Shot; in-combat Kidney Shot AI rewritten as a planner:
  opener-extend on the kill target, a Kick→hold→Kidney denial chain vs casters
  (no double-spend; extend an active lockout; stun an un-locked second-school
  cast immediately), aggressive opportunistic stuns vs Warrior/Rogue/Hunter, and
  proactive stuns when no lockout is active (an early "wait indefinitely for
  Kick" reservation wasted Kidney vs kiting healers and was removed).
- **Effect: Rogue 1v1 56.6 → 64.9 (+8.3), 2v2 39.2 → 40.3 (+1.1), 3v3 40.5 →
  41.5 (+1.0).** The team-format aggregate barely moves despite 37% (2v2) / 52%
  (3v3) of matches flipping outcome — a large, bidirectional matchup
  redistribution. The Rogue remains last in both team formats.
- **Knock-on, not nerfs.** Each non-Rogue class's aggregate shifts ≤1.5pt and
  only through its Rogue matchup column: 1v1 Mage 65.6 → 57.4 and Hunter
  58.1 → 60.4 are the enemy-Rogue cells; 3v3 Mage 61.7 → 63.2 is the *ally*
  Mage+Rogue comps improving. Non-Rogue-vs-non-Rogue cells are unchanged.
- **Draw rates ~flat** (1v1 5.1%, 2v2 0.8 → 0.9%, 3v3 0.4%): no stall pathology;
  the longer Rogue stun-lockdowns resolve within the cap.

### Methodology note (this cycle's correction)

An initial read used a focused `--t1 'Rogue+{p}'` sweep compared against the
*subset* of canonical labels it happened to overlap (80 of 441), which reported a
spurious +5–8pt 2v2 lift. The true effect (+1.1) only appeared on a
**full-canonical vs full-canonical** comparison. Always regenerate the full
matrix for an aggregate verdict; reserve `--t1` slices for per-matchup diagnosis.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out, but don't read a single ordered cell as
  definitive. The Rogue's now-deterministic Cheap Shot opener *amplifies* this in
  Rogue-vs-Rogue mirrors (a mirror can swing to one side at n=100). Mechanism in
  `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`; fix deferred.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (pets, openers, curses…)
  are a separate axis — see the `balance-sweep` skill. Note the Rogue opener
  default is now Cheap Shot; Ambush remains selectable and is the stronger opener
  in sustain matchups (it keeps the opening burst).
