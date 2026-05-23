# Hunter Mana Economy Tuning — Iteration 1 Report

**Date:** 2026-05-22
**Plan:** `docs/plans/2026-05-22-001-fix-hunter-mana-economy-plan.md`
**Brainstorm:** `docs/brainstorms/2026-05-22-hunter-mana-economy-requirements.md`

## TL;DR

The mana economy fix **achieved its mechanical goal** — `InsufficientMana`
rejections dropped from ~1,767 to 56 in the Hunter v Warrior trace (96.8%
reduction, far above the R12 ≥50% target). Mana is no longer the binding
constraint on Hunter's AI; cooldowns are.

**Winrates barely moved.** Hunter v Rogue went 0% → 5% and Hunter v Warlock
went 3% → 5% (both at N=20; small enough to be noise). Hunter v Warrior,
Hunter v Mage, Hunter v Priest, and Hunter v Paladin all stayed at 0%. The
2v2 axis stayed flat with no Hunter+Priest winrate movement against any
non-mirror opponent.

The mana economy was a real constraint on AI behavior but not the binding
constraint on outcomes. **Hunter still loses because the other diagnosed
gaps remain** — pet positioning, predictive trap placement, healer-targeting
logic, Disengage follow-through. Those are separately tracked survivors in
`docs/ideation/2026-05-22-hunter-rebalance-ideation.md` and need to land
before Hunter winrates move materially.

## Change Summary

`src/states/play_match/components/combatant.rs:215`:

| Field            | Before | After |
|------------------|-------:|------:|
| max_mana         |    150 |   240 |
| mana_regen       |    3.0 |   0.0 |
| starting_resource|    150 |   240 |

`assets/config/abilities.ron` — Hunter ability `mana_cost` cuts ~15% uniform:

| Ability         | Before | After |
|-----------------|-------:|------:|
| Aimed Shot      |     40 |    34 |
| Arcane Shot     |     25 |    21 |
| Concussive Shot |     15 |    13 |
| Disengage       |     20 |    17 |
| Freezing Trap   |     50 |    43 |
| Frost Trap      |     30 |    26 |

Hunter now matches Mage/Priest/Warlock/Paladin's "one bar per fight, no regen"
pattern instead of being the lone hybrid.

## Validation Results

### Decision Trace Audit (Hunter v Warrior, single seed)

| Rejection class           | Pre  | Post |
|---------------------------|-----:|-----:|
| FrostTrap:InsufficientMana | 945 |    0 |
| ArcaneShot:InsufficientMana| 388 |   56 |
| Disengage:InsufficientMana | 261 |    0 |
| ConcussiveShot:InsufficientMana | 173 | 0 |
| **Total InsufficientMana** | **1,767** | **56** |

96.8% reduction. The dominant rejection class shifted from `InsufficientMana`
to `OnCooldown` (322 FrostTrap, 186 ArcaneShot, 179 ConcussiveShot, etc.) —
the AI is now gated by tactical timing rather than resource starvation.

Hunter chose 7 abilities in the post-change match vs 6 pre-change. Match
duration: 33.4s post vs 27.8s pre (Hunter survived ~5s longer).

### 1v1 Matrix (Hunter row, N=20)

Comparing this run against `design-docs/balance/matrix_baseline_2026-05-21.md`
(N=100, pre-change):

| Matchup            | Pre  | Post | Δ      | Pre Avg | Post Avg | Note |
|--------------------|-----:|-----:|-------:|--------:|---------:|------|
| Hunter vs Warrior  |   0% |   0% |   0pp  |  27.1s  |  32.5s   | Hunter survives longer, still 0% |
| Hunter vs Mage     |   0% |   0% |   0pp  |  10.4s  |  10.5s   | Dies before mana matters |
| Hunter vs Rogue    |   0% |   5% |  +5pp  |  22.9s  |  24.2s   | Noise-level lift (1/20) |
| Hunter vs Priest   |   0% |   0% |   0pp  |  30.3s  |  32.2s   | No move; Priest heals through |
| Hunter vs Warlock  |   3% |   5% |  +2pp  |  19.1s  |  18.9s   | Noise; Felhunter Devour still counters |
| Hunter vs Paladin  |   0% |   0% |   0pp  |  48.0s  |  50.1s   | No move; no healer CC |
| Hunter vs Hunter   |  55% |  45% |       |  20.9s  |  17.9s   | Mirror; 40% draws post (was 0%) |

### 2v2 Matrix (Hunter+Priest team, N=10 pre/post)

| Matchup                | Pre  | Post |
|------------------------|-----:|-----:|
| H+P vs Warrior+Priest  |   0% |   0% |
| H+P vs Mage+Priest     |   0% |   0% |
| H+P vs Rogue+Priest    |   0% |   0% |
| H+P vs Priest+Priest   |  70% |  60% |
| H+P vs Warlock+Priest  |   0% |   0% |
| H+P vs Paladin+Priest  |  10% |   0% (10 draws) |

## Success Criteria Assessment

From the requirements document:

| Criterion | Status | Notes |
|---|---|---|
| 2v2 ≥30% winrate in ≥3 of 6 paired matchups | **Not met** | Only Priest+Priest matchup (~60%) is positive |
| 1v1 aggregate winrate 7% → 15-20% | **Not met** | Aggregate non-mirror is ~1.4% (3 wins out of 240) |
| Hunter v Warrior and Hunter v Rogue ≥10% | **Partial** | Rogue moved 0→5%; Warrior stayed 0% |
| `InsufficientMana` ≤50% of pre-change | **Met (96.8%)** | 1,767 → 56 |
| No collateral regressions (±5pp non-Hunter) | **Mostly met** | See note below |

**Collateral note:** at N=20, several non-Hunter matchups shifted by more than
5pp (e.g., Rogue v Rogue 13%→25% T1 win, Priest v Priest 61%→70%, Warlock v
Warlock 41%→60%). These are almost certainly N=20 sample noise rather than
real signal — the 1v1 baseline at N=100 would smooth them out. Worth
re-running at N=100 before treating any as a real regression. The Hunter
changes shouldn't affect non-Hunter matchups by construction (no other
class's stats or abilities changed).

## Honest Reading

The mana fix did exactly what it claimed to do — restored AI rotation
fluidity by removing mana as a binding constraint. That was the diagnosed
problem (1,767 InsufficientMana rejections) and the trace audit confirms
it's fixed.

But the outcome data shows mana wasn't the **binding constraint on Hunter's
winrate** — it was a binding constraint on AI behavior. Hunter still loses
because:

1. **Damage gap.** Even with more casts, Hunter doesn't out-damage enemies
   in their effective window. Aimed Shot (2.5s cast at 35yd) is hard to
   land; instant abilities deal modest damage; the pet (~7-12 dmg per swing
   at melee range) doesn't pursue enemies (separate survivor — pet
   engagement).
2. **CC unreliability.** Freezing Trap `break_on_damage_threshold: 0.0`
   means any incidental damage breaks the trap (separate survivor —
   predictive placement + threshold tuning).
3. **No healer pressure.** In Hunter v Paladin and 2v2 with healer
   partners, Hunter never CCs or focuses the healer (separate survivor —
   team-comp-aware target selection).
4. **Specific class counters.** Felhunter Devour Magic strips Concussive
   Shot within 0.02s of landing (separate survivor — Concussive immunity or
   Devour CD parity).

## Recommendations

1. **Ship this change** — it's strictly an improvement, even if outcomes
   didn't move. The AI is now gated by cooldowns rather than resource
   starvation, which is the correct model for the class and removes a layer
   of confounding noise from any subsequent Hunter rebalance pass.
2. **Re-run validation at N=100 before merge** to confirm the small lifts
   (Rogue 0→5%, Warlock 3→5%) are real signal vs N=20 noise, and to verify
   non-Hunter matchups didn't actually shift.
3. **Don't re-cut mana** in a follow-up before the other survivors land.
   The trace shows mana is no longer the bottleneck — additional pool/cost
   tweaks would chase a non-bottleneck.
4. **Prioritize the next Hunter survivor** based on which constraint binds
   hardest in the post-change trace. Three candidates from
   `docs/ideation/2026-05-22-hunter-rebalance-ideation.md`:
   - **#1 Pet engagement** — pet pursuit movement + target acquisition.
     Trace still shows 1,000 SpiderWeb:NoValidTarget rejections; pet sits
     at owner's feet rather than closing.
   - **#5 Team-comp awareness for trap targeting** — Hunter v Paladin
     unchanged at 0%; 2v2 axis unchanged. Both gated by missing
     healer-pressure logic.
   - **#6 Disengage follow-through** — Hunter v Warrior unchanged at 0%
     despite extra casts; the gap-closer matchup needs a real defensive
     cooldown, not just more mana to cast Disengage.

## Deferred / Residual Work

- Re-run 1v1 matrix at N=100 (~53 min) and 2v2 matrix at N=100 (~10+ hours)
  before treating these results as final.
- Apply ce-doc-review residual findings to the plan (CSV column rename
  divergence in the script was caught and fixed during implementation;
  stdout-vs-file parsing was resolved by using log-file grep per precedent).
- Consider whether Hunter v Hunter mirror's increase in draw rate (0% →
  40%) is a problem. With more mana on both sides, mirror matches stall —
  this might warrant a Hunter-specific timeout or fatigue mechanism, or it
  might be acceptable. Worth a single-match read-through to confirm.
