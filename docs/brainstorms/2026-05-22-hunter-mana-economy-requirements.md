---
date: 2026-05-22
topic: hunter-mana-economy
---

# Hunter Mana Economy Tuning

## Summary

Tune Hunter's mana economy to match the pattern of the other four mana classes (zero regen, fixed pool per fight). `max_mana` rises from 150 to 240, `mana_regen` drops from 3.0 to 0.0, and Hunter ability mana costs are cut ~15% uniformly. Validation is a Hunter+healer vs each-class+healer 2v2 matrix sweep; 1v1 matrix is a quick diagnostic, not the balance target.

---

## Problem Frame

Hunter sits at ~7% winrate across the 4,900-match 1v1 baseline at `design-docs/balance/matrix_baseline_2026-05-16.csv`, losing 0% of matches in 6 of 7 matchups. Diagnostic tracing of a Hunter v Warrior match (27.77s) surfaces ~1,767 `InsufficientMana` rejections in the AI decision trace — 945 FrostTrap, 388 ArcaneShot, 261 Disengage, 173 ConcussiveShot. The Hunter executes only 6 ability casts in the entire match. The AI wants to act and cannot afford to.

The cause is structural rather than tactical. Hunter is the only class in the roster with non-zero mana regen (3.0/s) AND the smallest mana pool (150). Hunter's full rotation costs 180 mana, so Hunter cannot afford a single full rotation from the starting pool and depends on continuous regen to function. Other mana classes (Mage 200, Warlock 180, Paladin 160, Priest 150) operate on a "one bar per fight, no regen" model — they accept hard OOM as a feature of the resource and balance ability budgets accordingly. The original Hunter brainstorm (`docs/brainstorms/2026-02-22-hunter-class-brainstorm.md:25`) called out OOM pressure as an intended feature; in practice that intent collided with a pool too small to support one rotation, leaving the AI mana-gated for most of the fight.

This work brings Hunter into structural parity with the other mana classes, with a pool sized to afford ~1.6 full rotations (240 mana ÷ ~150 post-cuts) — comparable to Mage's burst-window-then-wand pattern.

---

## Requirements

**Resource model**
- R1. Hunter `max_mana` is raised from 150 to 240 in `src/states/play_match/components/combatant.rs:215`.
- R2. Hunter `mana_regen` is reduced from 3.0 to 0.0 in `src/states/play_match/components/combatant.rs:215`.
- R3. Hunter `current_mana` starting value stays at `max_mana` (full pool at match start), matching every other mana class's starting behavior.

**Ability mana costs** (in `assets/config/abilities.ron`)
- R4. Aimed Shot mana cost reduced from 40 to 34 (~15%).
- R5. Arcane Shot mana cost reduced from 25 to 21 (~16%).
- R6. Concussive Shot mana cost reduced from 15 to 13 (~13%).
- R7. Disengage mana cost reduced from 20 to 17 (~15%).
- R8. Freezing Trap mana cost reduced from 50 to 43 (~14%).
- R9. Frost Trap mana cost reduced from 30 to 26 (~13%).

**Validation**
- R10. 2v2-with-healer matrix is the primary balance signal. Hunter+healer (Priest) vs each of the other six classes paired with the same healer, N=100 matches per matchup, deterministic seeds. Compared against the same matrix run with pre-change values.
- R11. 1v1 matrix sweep at N=100 deterministic seeds is run as a secondary diagnostic. Expected directional lift: Hunter v Warrior and Hunter v Rogue move noticeably; Hunter v Mage (~10s defeats) does not move; Hunter v Paladin (48s) does not move materially (different binding constraint).
- R12. Decision-trace audit of one Hunter v Warrior 1v1 match post-change confirms the `InsufficientMana` rejection count drops by ≥50% from the pre-change ~1,767 baseline.

---

## Success Criteria

- 2v2-with-healer matrix shows Hunter+healer matchups become competitive (target: ≥30% Hunter-team winrate in at least 3 of 6 paired matchups, up from near-0% baseline). Exact threshold is exploratory — the goal is "Hunter has matchups where the comp is viable" not "Hunter wins everywhere."
- 1v1 Hunter winrate moves from 7% to ~15-20% in aggregate. Hunter v Warrior and Hunter v Rogue specifically move from 0% to ≥10%.
- AI decision trace shows `InsufficientMana` is no longer the dominant rejection class. `OnCooldown` or `OutOfRange` become the binding constraints, indicating the AI is now gated by tactical decisions rather than resource starvation.
- No regression in non-Hunter matchups (e.g., Warrior v Mage 100% should remain unchanged; other classes' winrates within ±5 percentage points of baseline).
- Downstream agent (ce-plan) can execute this brainstorm without inventing product behavior, target stat values, or validation methodology.

---

## Scope Boundaries

- Pet engagement, pet ability tuning, Felhunter Devour Magic counter, predictive trap placement, team-comp-aware target selection, and Disengage follow-through are all separately tracked Hunter rebalance survivors (see `docs/ideation/2026-05-22-hunter-rebalance-ideation.md`) — out of scope for this work.
- Approach C (Auto Shot returns mana on hit) is deferred. May be revisited after this baseline ships if matrix results suggest the linear tuning is insufficient.
- Changes to other classes' mana stats are out of scope (e.g., normalizing regen across classes).
- New resource types (Focus, Energy) for Hunter are out of scope. Resource model stays as `ResourceType::Mana`.
- Hunter base health, AP, crit, movement speed, and other non-mana stats are out of scope.
- Pet mana costs and pet ability mana economy are out of scope (pets have their own mana pools and effectively no cost-per-cast issues today).
- Loadout / equipment changes are out of scope.
- AI logic changes are out of scope (the AI's existing decision tree should suffice once mana is no longer the binding constraint).

---

## Key Decisions

- **Match the other mana classes' structural pattern (regen 0, fixed pool):** Rationale — the original Hunter brainstorm chose mana to reuse "OOM pressure" infrastructure. In practice, the asymmetric regen made Hunter neither a true OOM-burst class (it has regen that the others don't) nor a sustained-resource class (the regen wasn't enough). Bringing Hunter into structural symmetry with Mage/Priest/Warlock/Paladin removes the special case and makes class balance easier to reason about.
- **Approach B (generous pool + cost cuts) over Approach A (pool raise only):** Rationale — A's 1.1-rotation headroom is mathematically equivalent to today's mana-with-regen but offers no cushion for unexpected events (multiple Charges to escape, Mage Nova interrupting a cast cycle). B's 1.6-rotation headroom gives the AI room to react.
- **Approach C (Auto Shot returns mana) deferred:** Rationale — more elegant coupling of Hunter's "Auto Shot is the sustained damage" identity to resource recovery, but touches the auto-attack system (medium refactor). Ship the linear baseline first; if matrix data shows tuning alone doesn't land the right kiting tempo, revisit C as a follow-up.
- **2v2-with-healer is the validation target, not 1v1:** Rationale — 1v1 matrix is fast diagnostic signal but doesn't reflect the actual balance target. Healer presence changes which abilities matter (Hunter's heal-reduction debuff has no impact in 1v1 if opponent doesn't heal) and creates the CC-chain pressure points that justify Hunter's trap kit.
- **Cost cuts are uniform ~15% across all six Hunter abilities:** Rationale — cleanest signal for the matrix sweep. Any winrate change can be attributed to the mana economy itself rather than selective ability emphasis. Easier to reason about and revert if the matrix shows over-correction. Selective cost cuts (e.g., keeping Freezing Trap expensive) were considered and deferred as a possible follow-up if uniform cuts under- or over-perform.

---

## Dependencies / Assumptions

- **Assumes 2v2 matrix tooling can be produced cheaply.** Today's `--matrix N` is hardcoded to 7×7 1v1 matchups. The validation plan needs either (a) a small shell/Python wrapper script around `--headless` that loops the six Hunter+healer-vs-class+healer matchups, or (b) an extension to the matrix runner accepting team-comp templates. Option (a) is sufficient for this brainstorm's validation needs and is the assumed path. Building (b) is a separate decision that should not gate this work.
- **Assumes Priest as the canonical 2v2 healer.** Paladin is also a healer but is currently dominant (Paladin v Rogue 100% per matrix baseline) — using Paladin as the partner would conflate Hunter improvements with Paladin's existing advantage. Priest is a cleaner partner. Optional: re-run a subset with Paladin partner as a secondary check.
- **Assumes the AI's existing decision tree will function once mana is no longer the binding constraint.** This is the user-visible bet of this brainstorm. If matrix results show the AI does something unexpected (e.g., burns full pool in first 5 seconds then sits idle), AI changes become an in-scope follow-up.
- **Assumes Auto Shot's existing damage scaling is appropriate** as the sustained DPS source between ability windows. Not retuning Auto Shot in this work.
- **Assumes the matrix-trace audit pattern survives this work** — the audit (separate ideation survivor #7) is what verified the diagnosis; we'll re-run it to verify the fix.

---

## Outstanding Questions

### Deferred to Planning

- [Affects R10][Technical] **2v2 matrix tooling implementation:** wrapper script around `--headless` (simplest) vs. extension to the matrix runner. Planner picks based on how reusable the tooling needs to be for future balance work.
- [Affects R10][Technical] **Seed selection and run count for 2v2 validation:** N=100 per matchup matches the 1v1 baseline cadence but may be over- or under-sampled for the 6 paired matchups; planner picks based on variance observed in early runs.
- [Affects R11][Needs research] **Whether ±5 percentage points is the right regression tolerance for non-Hunter matchups,** or whether some matchups have noisier baselines that need wider bands. Planner consults `design-docs/balance/matrix_baseline_2026-05-16.csv` variance.
