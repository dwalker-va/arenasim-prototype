# Canonical Balance Baselines

**Generated 2026-07-04 (second regeneration this date)** — regenerated after
the **arena dampening** cycle: (1) all healing/absorbs/lifesteal ramp linearly
to zero from 75s to 195s after gates, (2) dying-blow semantics (same-frame
mutual lethal is a real DRAW, not an iteration-order win), (3) Healing Stream
Totem 12 → 24 mana. Supersedes the earlier-2026-07-04 Warrior-Berserker-Rage
baselines.

**Headline: dampening did exactly what it was built for — draws collapsed
across every bracket (1v1 11.1% → 2.2%, 2v2 2.6% → 0.7%, 3v3 1.1% → 0.2%)
with class standings essentially untouched (every 2v2-competitive move ≤1.1pt,
ordering unchanged) — and it CLEARED the two-cycle-old Paladin+Shaman anomaly
as a side effect (51.4/56.3 → 43.5/48.5).** The two-healer outlast win
condition no longer exists: attrition games now resolve on damage output, so
double-healer frames without a kill threat (Paladin+Shaman, and especially
Paladin+Priest+Shaman, 19.5 → 16.0) fell, while comps that pair a healer with
real damage — Mage+healer above all — converted their former draws into wins
(Mage+Priest +4.4, Mage+Shaman +2.5). Average match duration *dropped*
(2v2 53s → 50s): dampening shortens matches, it does not drag them out.

Authoritative current-state references. Use as the "before" when assessing a
balance change — **compare batch-vs-batch only**, and **full-canonical vs
full-canonical**.

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 8×8 | 100 | 6,400 | 2.2% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (784) | 100 | 78,400 | 0.7% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (3,136) | 50 | 156,800 | 0.2% |

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

Sorted by 2v2 competitive. Δ = change vs the Berserker Rage baseline
(earlier 2026-07-04).

| Class | 1v1 (Δ) | 2v2 all (Δ) | **2v2 comp (Δ)** | 3v3 all (Δ) | **3v3 comp (Δ)** |
|---|---|---|---|---|---|
| Mage | 62.6 (+0.0) | 67.1 (+1.5) | **66.6** (+0.6) | 63.1 (+1.1) | **66.3** (+1.3) |
| Shaman | 43.1 (+5.5) | 54.8 (−1.0) | **60.1** (+0.0) | 53.8 (−0.8) | **51.5** (−0.8) |
| Paladin | 71.2 (+10.5) | 53.6 (+0.9) | **59.3** (+1.1) | 53.8 (+0.8) | **50.4** (+0.9) |
| Priest | 30.0 (+21.4) | 44.2 (+2.7) | **46.7** (+0.8) | 49.8 (+0.8) | **47.3** (+1.1) |
| Hunter | 44.3 (−0.6) | 45.6 (+1.2) | **45.1** (+0.2) | 41.3 (+0.6) | **38.7** (+0.7) |
| Warlock | 37.2 (−0.2) | 45.4 (+1.6) | **43.7** (+0.4) | 46.8 (+0.6) | **46.8** (+0.6) |
| Rogue | 79.6 (+0.0) | 44.9 (+0.2) | **42.3** (+0.0) | 46.5 (+0.4) | **51.8** (+0.6) |
| **Warrior** | **23.5 (−0.7)** | **41.7 (+0.5)** | **39.7** (+0.6) | 44.3 (+0.4) | **46.7** (+0.5) |

Reading the deltas: **in team brackets the change is surgical** — every
2v2-competitive move is ≤1.1pt and the ordering is identical; the small
across-the-board upticks are converted draws (draws count as losses for both
sides, so resolving them lifts almost everyone slightly). The big movers are
all **1v1 healers**: the old healer-vs-healer cells were 47–99% draws, which
the draws-count-as-losses metric charged to both healers. With those games
actually resolving, Priest 8.6 → 30.0, Paladin 60.7 → 71.2 (now #2),
Shaman 37.6 → 43.1. **Priest is the healer-vs-healer king** (87/13 over
Paladin, 100/0 over Shaman at N=100/side) — its wand out-damages the other
healers' sustain once dampening bites. 1v1 remains a diagnostic bracket only.

## 2v2 comp tier list (784 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **83.4%** | **Mage+Shaman** (+2.5) | 1h |
| 79.8% | Mage+Paladin (+2.8) | 1h |
| 78.4% | Mage+Priest (+4.4) | 1h |
| 66.9% | Mage+Warlock (+0.6) | 0h |
| 64.9% | Rogue+Shaman (−1.1) | 1h |
| 64.3% | Paladin+Warrior (+1.0) | 1h |
| 62.0% | Paladin+Warlock (+2.2) | 1h |
| 59.9% | Shaman+Warlock (+1.0) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 39.6% | Hunter+Rogue (new to list) |
| 38.3% | Hunter+Warlock (+0.5) |
| 34.8% | Priest+Shaman (−1.3) |
| 34.3% | Hunter+Warrior (+0.1) |
| 25.6% | Rogue+Warrior (+0.0) |
| 24.7% | Warlock+Warrior (+0.6) |
| 24.5% | Rogue+Warlock (+0.3) |
| **14.9%** | **Paladin+Priest** (+3.9) |

Mage+Shaman holds #1 and widened its lead (83.4). The whole Mage+healer trio
gained 2.5–4.4pt — dampening rewards pairing a healer with the game's best
damage. **Priest+Warlock exited the bottom tier entirely (35.4 → 41.6, +6.2)**:
the double-fear comp's long attrition games now resolve in its favor often
enough to lift it back to mid-table, partially refunding the Berserker Rage
counter. Paladin+Priest remains the absolute floor even after its +3.9 draw
conversion.

## 3v3 comp tier list (3,136 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **86.7%** | **Mage+Shaman+Warlock** (+0.6) | 1h |
| 81.5% | Mage+Paladin+Warlock (+0.9) | 1h |
| 78.2% | Mage+Priest+Shaman (+1.9) | 2h |
| 76.1% | Mage+Paladin+Shaman (+1.9) | 2h |
| 74.6% | Mage+Priest+Warlock (+2.3) | 1h |
| 71.4% | Mage+Shaman+Warrior (+0.9) | 1h |
| 70.9% | Mage+Paladin+Warrior (+0.1) | 1h |
| 70.7% | Mage+Priest+Rogue (new to top-8) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 34.4% | Hunter+Mage+Rogue (new to list) |
| 33.0% | Hunter+Priest+Warrior (+1.2) |
| 31.1% | Hunter+Paladin+Priest (+3.1) |
| 20.8% | Hunter+Rogue+Warlock (+0.5) |
| 17.9% | Rogue+Warlock+Warrior (+0.7) |
| 17.0% | Hunter+Warlock+Warrior (+0.3) |
| **16.0%** | **Paladin+Priest+Shaman (−3.5)** |
| 13.9% | Hunter+Rogue+Warrior (+0.4) |

Mage+Shaman+Warlock stays the hottest comp in the game (86.7). Every top-8
comp still contains Mage. **Triple-healer collapsed further** — with no
endgame outlast available, Paladin+Priest+Shaman (19.5 → 16.0) is now
second-worst in the bracket, exactly the structural direction dampening was
meant to push. Paladin+Priest+Warlock climbed out of the bottom-8.

## Canaries

Two standing structural checks, run every regeneration
(`scripts/comp_tiers.py`).

### Anomaly check — non-competitive comps performing competitively

| Bracket | Comp | Full-field | vs competitive | Verdict |
|---|---|---|---|---|
| 2v2 | Paladin+Shaman (2h) | 43.5% | 48.5% | **CLEARED** (was 51.4/56.3 for two cycles) |
| 3v3 | **Mage+Rogue+Warlock** (0h) | **53.3%** | **53.2%** | **ANOMALY — still fires** (was 52.8/52.6) |
| 3v3 | Hunter+Mage+Warlock (0h) | 51.2% | 43.2% | farm-the-trash profile, not beating real comps — watch |
| 3v3 | Mage+Warlock+Warrior (0h) | 49.1% | 49.8% | NEW near-threshold — watch next cycle |

- **Paladin+Shaman: the oldest open anomaly is resolved — by dampening, not by
  a targeted nerf.** Its edge was the two-healer outlast; with healing ramping
  to zero, an offensive healer pair can no longer grind out competitive comps
  (48.5% vs competitive, below water). **The queued
  "Shaman-offensive-kit-in-two-healer-frame" nerf should be re-evaluated
  before shipping — its motivating anomaly no longer exists.**
- **Mage+Rogue+Warlock**: unmoved by this cycle (+0.5) and now the only firing
  anomaly. A triple-DPS comp beating the competitive field is a
  burst-vs-healing statement; dampening (a late-game lever) predictably
  didn't touch a comp that wins early. This is now the headline structural
  item for the next tuning cycle.
- **Mage+Warlock+Warrior** appeared at 49.1/49.8 — half a point from
  flagging. Same burst-shape family; watch it.

### Dominant-shape watch (3v3)

**Current: 2/10 of the top-10 3v3 comps are double-healer** (Mage+Priest+Shaman
78.2, Mage+Paladin+Shaman 76.1) — present, not dominant, unchanged count vs
last cycle. Note both 2h comps *gained* ~1.9pt: dampening hurts double-healer
comps without damage, but a double-healer frame wrapped around a Mage still
converts. No action.

## What's meta-defining vs unplayable — the read

- **Mage tightened its grip** (2v2 comp 66.6, 3v3 comp 66.3, both up; #1 comp
  in both brackets is Mage+Shaman-based; all eight 3v3 top comps contain
  Mage). Dampening turns long games into damage races, and Mage is the best
  damage in the game. LoS/pillar play remains the deferred structural answer,
  and it is now *more* overdue, not less.
- **The dampening cycle was surgical where it should be** (≤1.1pt class-tier
  movement in team brackets) **and structural where intended**: no-damage
  multi-healer comps lost their win condition (Paladin+Shaman anomaly
  cleared, triple-healer near-floor), Mage+healer gained, and every bracket's
  draw rate is now under 2.5%.
- **Priest owns healer-vs-healer** (1v1: 87/13 vs Paladin, 100/0 vs Shaman) —
  wand damage is the healer endgame stat. If a future healer feels
  unwinnable in mirrors, check its OOM damage output before its healing. The
  deferred **Priest Mana Burn** would layer a second, skill-expressive win
  condition on top of this.
- **Warrior remains the 2v2 competitive floor (39.7)** but the gap to 7th
  (Rogue 42.3) is only 2.6pt. The queued non-fear lever (Intercept) stands.
- **Hunter is still the competitive 3v3 floor (38.7)** — three cycles
  running. Its comps did tick up broadly (Hunter+Paladin 2v2 51.3 → 55.3, its
  long trap-attrition games now resolve), but the diagnosed levers (gear
  mana, trap-window burst conversion) remain the highest-value queued buff
  work.
- **Structural floors after dampening**: Paladin+Priest 14.9 in 2v2,
  Hunter+Rogue+Warrior 13.9 and Paladin+Priest+Shaman 16.0 in 3v3. Healer
  stacking without damage is now the single worst thing you can do in a comp
  — by design.

## Changes this cycle (vs the Berserker Rage baseline, earlier 2026-07-04)

- **Arena dampening shipped**: ALL healing, absorb shields, and lifesteal ramp
  linearly to zero starting `DAMPENING_START_SECS` (75s) after gates over
  `DAMPENING_RAMP_SECS` (120s) — full dampening at 195s of combat. Ticked by
  `match_flow::update_dampening` into the `ArenaDampening` resource; applied
  at all six heal/absorb sites (cast heals, channel self-heals, HoT/totem
  ticks, Holy Shock, Death Coil lifesteal, absorb-aura application). Any NEW
  healing/absorb mechanic must apply `Res<ArenaDampening>` at its application
  site. Milestone `[EVENT]` lines land in match logs at 25/50/75/100%.
- **Dying-blow draw semantics**: attacks queued by frame-start-alive attackers
  all land even if the attacker died earlier that frame; same-frame mutual
  lethal is a genuine DRAW. Kills the systematic Team-1-wins frame-order bias
  in mirror endgames (Priest mirror was Team 1 3/3 pre-fix; now 43/50/7 over
  N=100/side).
- **Healing Stream Totem 12 → 24 mana**: at 12, its 30s-duration upkeep
  (0.4 mana/s) was fully funded by the 1.0 mana/s trinket trickle — free
  infinite sustain and the core of the healer-vs-healer stalemate.
- **Effect:** draws 1v1 11.1 → 2.2%, 2v2 2.6 → 0.7%, 3v3 1.1 → 0.2%; healer
  1v1 rehabilitated (Priest 8.6 → 30.0, Paladin 60.7 → 71.2, Shaman
  37.6 → 43.1); Paladin+Shaman anomaly cleared (51.4/56.3 → 43.5/48.5);
  triple-healer collapsed (Paladin+Priest+Shaman 19.5 → 16.0); Mage+healer
  gained 2.5–4.4pt; Priest+Warlock recovered to mid-table (35.4 → 41.6).
  Average durations *fell* (2v2 53 → 50s, 3v3 56 → 54s).
- **Harness fix:** `scripts/hunter_2v2_matrix.sh` had a hardcoded
  `max_duration_secs: 120` — every historical 2v2 CSV "draw" at ~127–130s avg
  from that wrapper was a 120s-cap kill, not a real draw. Fixed to 300s;
  the wrapper's historical draw columns are not comparable with new runs.
  (The canonical batch CSVs were always 300s and are unaffected.)
- **Deferred:** Priest **Mana Burn** — a mana-pressure win condition layered
  on the healer duel — is the queued follow-up from this cycle's ideation.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (poisons, openers, pets,
  totems, curses…) are a separate axis — see the `balance-sweep` skill.
- **Class-tier metric:** winrate over all matches where the class appears on a
  side (draws count as losses); computed by `scripts/comp_tiers.py`.
- **Dampening horizon:** any mechanic or AI change aimed at the late game
  (sustain, attrition, mana economy) now interacts with the 75–195s dampening
  ramp — sweep long-game comps (healer pairs, DoT attrition) explicitly when
  touching it.
