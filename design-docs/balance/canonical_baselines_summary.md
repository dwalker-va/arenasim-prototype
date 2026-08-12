# Canonical Balance Baselines

> ## Regenerated 2026-08-11 against COMMITTED HEAD (20144b9)
>
> The three `canonical_*.csv` files are now a valid reference for the **committed
> codebase**, replacing the 2026-07-09 set. The narrative below is the
> 2026-07-09 write-up and has NOT been rewritten; read this block first.
>
> ### Why the previous set had to be discarded
>
> **The 2026-07-09 baselines did not reproduce against committed HEAD**, and had
> not for some time — roughly a month of commits landed in between (team-solve,
> probe recalibration, PR #95 fixes, kill-call banter). Nothing was wrong with
> how they were generated; they simply aged out, and nothing detected it.
>
> The drift is **symmetric and cancels in aggregate** — mean per-cell delta
> across matches with no Warlock was +0.01pt (2v2) / -0.10pt (3v3) — but
> individual cells swing up to **±50pt in both directions**, and per-class it
> reaches 1.1pt. So a stale-baseline diff mixes real effect with arbitrarily
> large per-cell noise. One cell (`Warrior+Warlock+Shaman vs Mage+Rogue+Hunter`)
> read 84/100 in the old CSV and 3/100 at HEAD, which looked like a catastrophic
> regression and was pure cascade.
>
> **Lesson: a single swung cell is never evidence.** Compare comp aggregates,
> which ARE drift-stable (the HEAD canary reproduces 2026-07-09's 49.6/48.8 to
> within 0.1pt), or run a controlled A/B.
>
> ### Class standings — committed HEAD reference
>
> | Class | 1v1 | 2v2 comp | 3v3 comp |
> |---|---|---|---|
> | Mage | 62.5 | 65.5 | 67.1 |
> | Shaman | 43.1 | 59.5 | 50.0 |
> | Paladin | 70.6 | 59.3 | 49.8 |
> | Priest | 31.2 | 51.6 | 49.9 |
> | Rogue | 78.8 | 41.9 | 52.3 |
> | Hunter | 44.3 | 45.1 | 39.0 |
> | Warlock | 37.1 | 42.6 | 46.4 |
> | Warrior | 23.8 | 39.3 | 45.1 |
>
> 3v3 anomaly canary is **clean** — `Mage+Rogue+Warlock` at 49.7/48.8, below
> threshold, as it has been since the 2026-07-09 shield-scaling fix.
>
> ### Regeneration is now ~40 minutes, not ~22 hours
>
> The headless runner did not put `FixedUpdate` — where every combat system
> actually runs — on the single-threaded executor, so the whole simulation
> dispatched through the shared `ComputeTaskPool`. Committed HEAD runs the
> matrices at **3 matches/s**; with that one line it is **69-78/s**, a **26x**
> speedup (the 2v2 alone took 7.7 hours at HEAD).
>
> These baselines were generated with a HEAD binary carrying ONLY that patch,
> and its result-neutrality was **verified, not assumed**: the 1v1 and 2v2
> outputs are byte-identical to pure-HEAD runs across **84,800 matches** (winner
> AND duration). The fix is independent of any gameplay work and is worth
> landing on its own — it is what made a properly controlled balance measurement
> affordable.
>
> ---
>
> ## Pending (uncommitted): the Warlock DoT/Fear cost cut
>
> Four ability costs, all Warlock: Corruption, Curse of Agony and Immolate
> 25 -> 16 (matching Dispel Magic, so a DoT application costs what the dispel
> answering it costs); Fear 30 -> 22. Unstable Affliction untouched at 30.
> Rationale: `2026-08-09-warlock-dot-vs-dispel-cost.md`.
>
> Results: `2026-08-10-warlock-costcut-{1v1,2v2,3v3}.csv`. **Diff them against
> `canonical_*.csv` directly** — the canonical set IS the matched control. That
> is not an assumption: the binary carrying this uncommitted work was run on the
> HEAD assets over the full matrix and is **byte-identical to HEAD across all
> 241,600 matches** in winner and duration, because `cc_policy` defaults to
> `Identity`. So every delta below is caused by the four mana costs and nothing
> else, and matches with no Warlock are identical by construction (verified:
> 0 of 4,900 / 44,100 / 61,250).
>
> ### Effect, exact
>
> | Class | 1v1 | 2v2 comp | 3v3 comp |
> |---|---|---|---|
> | **Warlock** | **37.1 -> 40.3 (+3.2)** | **42.6 -> 45.6 (+3.0)** | **46.4 -> 51.5 (+5.1)** |
> | Paladin | 70.6 -> 67.6 (-3.0) | 59.3 -> 58.1 (-1.2) | 49.8 -> 49.6 (-0.2) |
> | Hunter | 44.3 (0.0) | 45.1 -> 44.7 (-0.4) | 39.0 -> 37.4 (-1.6) |
> | Mage | 62.5 (0.0) | 65.5 -> 65.1 (-0.4) | 67.1 -> 65.7 (-1.4) |
> | Rogue | 78.8 (0.0) | 41.9 -> 41.2 (-0.7) | 52.3 -> 51.1 (-1.2) |
> | Warrior | 23.8 (0.0) | 39.3 -> 38.7 (-0.6) | 45.1 -> 44.4 (-0.7) |
> | Priest | 31.2 -> 31.1 (-0.1) | 51.6 -> 51.8 (+0.2) | 49.9 -> 49.8 (-0.1) |
> | Shaman | 43.1 -> 43.0 (-0.1) | 59.5 -> 59.1 (-0.4) | 50.0 -> 50.1 (+0.1) |
>
> No over-correction: the Warlock reaches par in 3v3 (51.5) and stays below par
> in 2v2 (45.6), up from second-weakest. The bill falls mostly on the **Paladin
> in 1v1 (-3.0)** — the long mana war is exactly where a cheaper Warlock gains.
>
> ### What shapes it helped
>
> - **Anything that can dispel — a step function, not a gradient.** 2v2 by
>   opposing dispeller count: 0 -> **+0.8 (z=1.00, ns)**, 1 -> **+5.8**,
>   2 -> **+5.8**. 3v3: 0 -> +1.0 (ns), 1 -> +5.3, 2 -> **+7.4**. Against comps
>   with no dispeller it did essentially nothing, which is precisely the
>   mechanism it was designed around.
> - **Long games.** 2v2: 0-30s **+0.9 (ns)** -> 30-45s +4.5 -> 45-60s +7.5 ->
>   60-90s +6.9 -> **90s+ +11.1**. 3v3 is monotone: +0.5 (ns) -> +2.1 -> +6.0 ->
>   +10.4 -> **+13.6**.
> - **Paired with a healer.** With +4.5, without +2.4. By partner: **Priest
>   +7.2**, Hunter +3.8, Paladin +3.3, Shaman +3.1, Mage +2.9, then the burst
>   partners Rogue +1.5 and Warrior +1.3 — matching the length gradient.
>
> Nothing got measurably worse: worst cell -4.5pt (2v2, z=-1.17) and -12.0pt
> (3v3, z=-2.05); one z=-2 cell in ~1,900 is chance. No shape bucket is negative.
>
> ### Action: one real regression
>
> **`Mage+Rogue+Warlock` crosses back into anomaly territory, and this change
> causes it**: HEAD **49.7 / 48.8** (clean) -> **51.3 / 50.9** (flagged). A
> zero-healer triple-DPS comp beating the competitive field is the shape this
> project has twice decided is wrong; the 2026-07-09 cycle cleared it via shield
> scaling. Other zero-healer Warlock comps rose in step (Mage+Warlock+Warrior
> 43.9/43.8 -> 46.6/46.7; Hunter+Mage+Warlock 47.2/38.0 -> 48.8/40.0).

**Generated 2026-07-09** — regenerated after the **Priest sustain cycle**:
(1) Power Word: Shield now scales with spell power (25 + 0.4 × SP ≈ 70 absorb
at the stock 112-SP loadout, was flat 50), (2) new Priest ability **Mana
Burn** (1.5s interruptible Shadow cast, destroys 50 mana on an enemy mana
user, AI holds it for enemy-healer CC windows), (3) Priest FREE-posture
`burn_pull` positioning. Supersedes the 2026-07-04 arena-dampening baselines.

**Headline: the shield fix centered the healer bracket and cleared the last
standing anomaly.** Priest jumped from bracket-floor healer to within reach
of the pack (2v2 comp 46.7 → 51.8, healer spread 13.4 → 7.5pt; 3v3 healers
converged at 50.2/50.2/49.4), paid for mostly by Paladin/Shaman and melee
burst. The spell-power-scaled shield is the anti-burst lever dampening
couldn't be: **Mage+Rogue+Warlock — the anomaly flagged as this cycle's
headline — cleared (53.3/53.2 → 49.6/48.8)**. New watch item: **Mage+Priest
is the new #1 2v2 comp (83.9)**, dethroning Mage+Shaman head-to-head via
layered absorbs (Ice Barrier + 70pt PW:S) on the Mage. Draw rates and average
durations are unchanged (2v2 0.7% / 50s, 3v3 0.2% / 54s).

Authoritative current-state references. Use as the "before" when assessing a
balance change — **compare batch-vs-batch only**, and **full-canonical vs
full-canonical**.

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 8×8 | 100 | 6,400 | 2.2% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (784) | 100 | 78,400 | 0.7% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (3,136) | 50 | 156,800 | 0.2% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {1,2,3}` + `arenasim --batch`,
then analyze with **`scripts/comp_tiers.py <csv> --size {2,3}`** (all-comps +
competitive tiers, comp lists, canaries). See the `balance-sweep` skill.

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

Sorted by 2v2 competitive. Δ = change vs the dampening baseline (2026-07-04).

| Class | 1v1 (Δ) | 2v2 all (Δ) | **2v2 comp (Δ)** | 3v3 all (Δ) | **3v3 comp (Δ)** |
|---|---|---|---|---|---|
| Mage | 62.6 (+0.0) | 66.4 (−0.7) | **66.0** (−0.6) | 62.3 (−0.8) | **66.2** (−0.1) |
| Shaman | 43.1 (+0.0) | 54.7 (−0.1) | **59.3** (−0.8) | 53.1 (−0.7) | **50.2** (−1.3) |
| Paladin | 70.4 (−0.8) | 52.4 (−1.2) | **57.6** (−1.7) | 53.3 (−0.5) | **49.4** (−1.0) |
| **Priest** | **31.3 (+1.3)** | **48.1 (+3.9)** | **51.8 (+5.1)** | **53.1 (+3.3)** | **50.2 (+2.9)** |
| Hunter | 44.3 (+0.0) | 45.6 (+0.0) | **45.3** (+0.2) | 40.9 (−0.4) | **38.3** (−0.4) |
| Warlock | 37.1 (−0.1) | 45.1 (−0.3) | **43.5** (−0.2) | 46.7 (−0.1) | **47.0** (+0.2) |
| Rogue | 78.8 (−0.8) | 43.9 (−1.0) | **41.6** (−0.7) | 46.3 (−0.2) | **52.1** (+0.3) |
| **Warrior** | **23.8 (+0.3)** | **41.0 (−0.7)** | **39.2** (−0.5) | 43.5 (−0.8) | **45.9** (−0.8) |

Reading the deltas: **this cycle is a targeted Priest lift with a broad,
shallow tax.** Priest +5.1 (2v2 comp) / +2.9 (3v3 comp) is the only move
above 1.7pt in either team bracket; everyone else pays a fraction of a point
to ~1.7 (Paladin) — partly head-to-head vs stronger Priests, partly shields
blunting burst (Rogue/Warrior tick down). **The healer brackets converged**:
2v2 healer spread 13.4 → 7.5pt (Shaman 59.3 / Paladin 57.6 / Priest 51.8),
and 3v3 healers are functionally tied (Priest 50.2 / Shaman 50.2 /
Paladin 49.4). **Priest remains the healer-vs-healer 1v1 king** (94.5/5.5
over Paladin, 100/0 over Shaman at N=200 both-orderings) — wand damage plus
Mana Burn is the healer endgame. 1v1 remains a diagnostic bracket only.

## 2v2 comp tier list (784 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **83.9%** | **Mage+Priest** (+5.5) | 1h |
| 80.5% | Mage+Shaman (−2.9) | 1h |
| 76.6% | Mage+Paladin (−3.2) | 1h |
| 66.6% | Mage+Warlock (−0.3) | 0h |
| 64.1% | Rogue+Shaman (−0.8) | 1h |
| 63.5% | Paladin+Warrior (−0.8) | 1h |
| 60.5% | Paladin+Warlock (−1.5) | 1h |
| 59.6% | Shaman+Warlock (−0.3) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 40.0% | Priest+Shaman (+5.2) |
| 38.7% | Hunter+Rogue (−0.9) |
| 36.7% | Hunter+Warlock (−1.6) |
| 33.3% | Hunter+Warrior (−1.0) |
| 23.3% | Rogue+Warrior (−2.3) |
| 23.0% | Warlock+Warrior (−1.7) |
| 22.8% | Rogue+Warlock (−1.7) |
| **14.2%** | **Paladin+Priest** (−0.7) |

**Mage+Priest took #1 (83.9)**, beating Mage+Shaman head-to-head decisively
in both orderings (62→87 / 38→12): layered absorbs on the game's best damage
dealer. The signature cells of the cycle are the Priest-survives-the-train
matchups — Warrior+Priest vs Warrior+Rogue 19→59, vs Warrior+Mage 27→63 —
and the revived squishy-partner comps (Rogue+Priest vs Mage+Shaman 2→43,
Priest+Warlock vs Mage+Paladin 38→82). Paladin+Priest remains the absolute
floor: two healers still can't out-sustain dampening.

## 3v3 comp tier list (3,136 matchups)

**Meta-defining (top):**
| Winrate | Comp | Shape |
|---|---|---|
| **86.2%** | **Mage+Shaman+Warlock** (−0.5) | 1h |
| 79.8% | Mage+Paladin+Warlock (−1.7) | 1h |
| 78.3% | Mage+Priest+Warlock (+3.7) | 1h |
| 76.6% | Mage+Priest+Shaman (−1.6) | 2h |
| 75.1% | Mage+Paladin+Shaman (−1.0) | 2h |
| 73.1% | Mage+Priest+Rogue (+2.4) | 1h |
| 69.6% | Mage+Shaman+Warrior (−1.8) | 1h |
| 69.4% | Mage+Paladin+Rogue (new to top-8) | 1h |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 33.9% | Hunter+Paladin+Priest (+2.8) |
| 32.6% | Hunter+Mage+Rogue (−1.8) |
| 30.0% | Mage+Rogue+Warrior (new to list) |
| 19.6% | Hunter+Rogue+Warlock (−1.2) |
| 17.2% | Paladin+Priest+Shaman (+1.2) |
| 17.1% | Rogue+Warlock+Warrior (−0.8) |
| 15.5% | Hunter+Warlock+Warrior (−1.5) |
| **13.2%** | **Hunter+Rogue+Warrior** (−0.7) |

Mage+Shaman+Warlock holds #1 (86.2). Every top-8 comp still contains Mage.
The Priest-carrying Mage comps gained (Mage+Priest+Warlock +3.7, now #3;
Mage+Priest+Rogue +2.4) while burst-shape comps slid (Mage+Rogue+Warrior
fell into the bottom-8 at 30.0 — shields hurt triple-burst from both sides
of the table).

## Canaries

Two standing structural checks, run every regeneration
(`scripts/comp_tiers.py`).

### Anomaly check — non-competitive comps performing competitively

| Bracket | Comp | Full-field | vs competitive | Verdict |
|---|---|---|---|---|
| 2v2 | Paladin+Shaman (2h) | 42.9% | 47.6% | **stays cleared** (43.5/48.5 last cycle) |
| 3v3 | **Mage+Rogue+Warlock** (0h) | **49.6%** | **48.8%** | **CLEARED** (was 53.3/53.2 — the standing headline anomaly) |
| 3v3 | Hunter+Mage+Warlock (0h) | 49.0% | 40.4% | farm-the-trash profile, not beating real comps — watch |
| 3v3 | Mage+Warlock+Warrior (0h) | 45.9% | 46.1% | receded from near-threshold (49.1/49.8) — no action |

- **Mage+Rogue+Warlock: resolved — by shield scaling, not a targeted nerf.**
  A triple-burst comp beating the competitive field was a burst-vs-healing
  statement; dampening (a late-game lever) couldn't touch a comp that wins
  early, but a spell-power-scaled Power Word: Shield is exactly the
  early-game EHP that blunts an opener. Its biggest losses are against
  Priest+healer frames (vs Warrior+Priest+Shaman 100→68, vs
  Priest+Warlock+Shaman 84→50). **No anomaly currently fires in any
  bracket** — first time since the canary was introduced.
- **Paladin+Shaman** stays below water (47.6 vs competitive) — the dampening
  fix holds under the new shield math.

### Dominant-shape watch (3v3)

**Current: 2/10 of the top-10 3v3 comps are double-healer** (Mage+Priest+Shaman
76.6, Mage+Paladin+Shaman 75.1) — present, not dominant, unchanged count for
three cycles. Both 2h comps *lost* ~1–1.6pt this cycle (their opponents'
Priests got tankier too). No action.

## What's meta-defining vs unplayable — the read

- **Mage's grip is intact but no longer widening** (2v2 comp 66.0, 3v3 comp
  66.2, both flat-to-slightly-down; every 3v3 top-8 comp still contains
  Mage; its best partner changed from Shaman to Priest in 2v2).
  **Mage+Priest at 83.9 is the comp to watch**: if a future cycle pushes it
  past the mid-80s, the queued counterplay lever is Shaman Purge
  prioritizing shield-stripping — it taxes exactly this comp without
  touching the Priest kit. LoS/pillar play remains the deferred structural
  answer to Mage itself.
- **The healer bracket is centered for the first time**: 2v2 comp
  Shaman 59.3 / Paladin 57.6 / Priest 51.8 (was a 13.4pt spread), 3v3 comp
  50.2/50.2/49.4. The Priest fix was sustain (shield scaling), not the new
  ability — Mana Burn measured roughly +0.3pt on its own (see
  `2026-07-05` findings in the priest-mana-burn memory/PR) and is a
  skill-expressive layer, not the balance lever.
- **Priest owns healer-vs-healer harder than ever** (1v1: 94.5/5.5 vs
  Paladin, 100/0 vs Shaman) — wand damage decides healer duels once
  dampening bites, and Mana Burn accelerates the endgame. All healer-mirror
  1v1s resolve; no draw regression.
- **Warrior remains the 2v2 competitive floor (39.2)**; shields blunting
  burst nicked all melee slightly (Rogue 41.6). The queued non-fear lever
  (Intercept) stands, and any melee buff should be re-sliced against
  shielded targets specifically.
- **Hunter is still the competitive 3v3 floor (38.3)** — four cycles
  running. Unmoved by this cycle (its damage is sustained physical, neither
  the shield tax nor the shield benefit lands on it disproportionately).
  The diagnosed levers (gear mana, trap-window burst conversion) remain the
  highest-value queued buff work.
- **Structural floors**: Paladin+Priest 14.2 in 2v2, Hunter+Rogue+Warrior
  13.2 and Paladin+Priest+Shaman 17.2 in 3v3. Healer stacking without
  damage is still the single worst thing you can do in a comp — by design.

## Changes this cycle (vs the dampening baseline, 2026-07-04)

- **Power Word: Shield scales with spell power**: absorb = 25 + 0.4 × caster
  effective SP (base + gear + aura bonuses) ≈ 70 at the stock 112-SP Priest
  loadout, was flat 50 — the one Priest number that never grew with the kit
  (Flash Heal lands ~100 at the same SP). Implemented as a general
  `applies_aura.magnitude_coefficient` config field applied via
  `AuraPending::from_ability_scaled`; `ability_config::validate()` panics on
  a non-zero coefficient for any ability whose apply site isn't wired, so a
  RON edit can't silently no-op. Only PW:Shield opts in this cycle.
- **New Priest ability: Mana Burn** — 1.5s interruptible Shadow cast, 40yd,
  25 mana, no cooldown, destroys 50 mana on an enemy mana user (healers only
  by AI policy; `ResourceType::Mana` guard in the effect — Warrior rage and
  Rogue energy reuse `current_mana` and are never burnable). Deals no damage,
  so it can never break CC. NOT scaled by ArenaDampening (it is pressure
  toward resolution). The AI holds it for enemy-healer CC windows (two-tier
  health floor above Flash Heal) with standstill safety gates
  (escape/pressured/focus/stealth) that apply only while `cast_time > 0`.
  Measured alone it is a small positive (~+0.3pt 2v2, 73%-favorable winner
  flips); the instant-cast variant measured +3.6pt but was rejected as too
  divergent from Classic.
- **Priest `burn_pull` positioning** (movement.ron): while the burn window
  is open, the FREE formation point clamps toward Mana Burn range of the
  enemy healer, never beyond heal range of the ally centroid.
- **Effect:** Priest 2v2 comp 46.7 → 51.8, 3v3 47.3 → 50.2; healer tiers
  converged; Mage+Rogue+Warlock anomaly cleared (53.2 → 48.8 vs
  competitive); Mage+Priest new #1 2v2 comp (83.9); draws and durations
  unchanged (2v2 0.7% / 50s, 3v3 0.2% / 54s).
- **Regression matchups for future cycles** (the cells that prove the
  mechanisms; N=100/side): Warrior+Priest vs Warrior+Rogue 59% (train
  survival), Rogue+Priest vs Mage+Shaman 43% (squishy-partner revival),
  Rogue+Warlock vs Mage+Priest 0% (shields blunt burst), Mage+Priest vs
  Mage+Shaman 87% (the concentration watch), Mage+Rogue+Warlock vs
  Warrior+Priest+Shaman 68% in 3v3 (anomaly stays dead).

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
- **Shield-scaling horizon (new):** absorb magnitudes now scale with SP, so
  any future SP change (gear cycles, Flametongue-style buffs, new casters)
  moves shield strength too — a max-mana or SP item tweak is no longer
  heal-only. Budget-check SP items with shields in mind.
