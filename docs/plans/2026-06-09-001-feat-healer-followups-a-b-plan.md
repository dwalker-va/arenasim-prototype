# Healer-movement follow-ups — buckets A + B (implementation plan)

Source: `design-docs/roadmap.md` §"Follow-ups from healer movement AI" (PR #63).
Scope chosen by user: **buckets A (offensive-punish) + B (PR#63 code-review residuals)**.
Conflict-aware ordering produced by the `healer-followups-research` workflow (8 agents).

## Decisions on open questions
- **B2 shared-helper shape**: option (a) — `fallback_range: Option<f32>` param, inline band-hold gate (Priest passes `None`).
- **"Softer" target metric**: `current_health` proxy (no effective-HP system exists).
- **Swap trace schema**: reuse `Chosen` + note string — no `TargetRejectionReason` variant / schema migration.
- **Burst-during-CC**: real but conservative priority elevation — burst ability jumps ahead of filler **only** when the enemy healer carries a hard-CC aura, applied to the kill target only. Matches roadmap "priority tweak, not new machinery".
- **B8 validation isolated** from bucket A: B8 (stale-directive) is a documented latent balance lever, so it gets its own attributable matrix pass before bucket A's pass.
- **Acceptance**: bucket A → draw_rate DOWN + avg_duration DOWN with non-overlapping Wilson CI (side-symmetrized). B8 → Priest matchups within variance band (no regression).

## Ordered steps (green `cargo test` after each)

### Bucket B — strict serial chain (priest.rs / paladin.rs / healer_postures.rs)
- [x] **B1** Move `escape_window` + `escape_distance_gained` → healer_postures.rs (`pub`, re-exported from priest for the public test path); repoint paladin import. ✅ 8 escape tests green.
- [x] **B2** Extract `healer_pressured_tick_shared(.., weights, wand_kill_target, fallback_range: Option<f32>)` into healer_postures.rs; both class ticks now thin wrappers. ✅ 49 probes + full suite green (behavior-identical).
- [x] **B3** New `paladin_postures.rs` (492 lines): moved `evaluate_paladin_posture`, `evaluate_dip_entry`, `dip_should_abort`, the 3 ticks; `dip_target_candidate`/`hoj_target_eligible`/`rotation_hoj_allowed` stay in paladin.rs (used by both). Re-exported `evaluate_paladin_posture`+`dip_should_abort` for combat_ai/test paths. paladin.rs 1626→1063. ✅ full suite + registration_audit green.
- [x] **B4** Perf: `compute_formation_point` single-pass scalar accumulators (dropped 2 Vec collects/FREE tick). ✅ priest_postures bit-identical.
- [x] **B5** Perf: streaming `escape_window_from` helper (drops `cc_remaining` Vec/tick in both healers, `escape_window` delegates) + deduped doubled `alive_allies()` in paladin posture eval. Deeper full single-pass-over-enemies hoist deferred (higher-risk-per-benefit vs bit-identical contract; the dominant per-tick Vecs are addressed). ✅ 49 probes.
- [x] **B6** corner_penalty default 4.0 → 6.0 (matches shipped Priest RON; Paladin overrides). ✅ validate tests.
- [x] **B7** Migrated 3 priest FREE-directive consts → `PriestMovementConfig` + movement.ron (the roadmap's "four" was three) + 2 new validate() rules. ✅ RON loads/validates, full suite.
- [x] **B8** Clear stale `MovementDirective` on Priest FREE entry (mirror paladin) + unconditional formation re-anchor. **Behavior change:** Priest PRESSURED time ~40.5%→~49.5% (mirror, side-symmetrized) — user chose KEEP + matrix-arbitrate. Recalibrated the time-in-FREE probe to side-symmetrized 50% ceiling. → authoritative check is VAL2a.

### Bucket A — burst-during-CC + target-swap (mostly independent files)
- [x] **A1** `CombatContext::enemy_healer()` + `enemy_healer_is_cced()` (cast-preventing CC subset — excludes Root, which doesn't stop heals); deduped hunter `find_enemy_healer`. ✅ class_ai_decisions.
- [x] **A2** Burst-during-CC: Warrior Mortal Strike jumps ahead of Rend during a healer-CC window (trace-clean single-attempt). Hunter no-op (Aimed already > Arcane); Mage (no held burst), Rogue (combo-point entangled), Warlock (DoT-identity reorder, large swing) deferred — mechanism in place for extension. ✅ decision_trace_audit.
- [x] **A3** RON `melee:` block + `MeleeMovementConfig` (swap_range 4 / swap_hysteresis 2 / swap_hp_margin 0.15) + validate(). ✅ shipped RON validates.
- [x] **A4** `Combatant.last_target_swap_time` + `last_kill_target`; pure `select_softer_melee_target` utility — redefined context-free (tuple-friendly: `(entity,distance,current_health)` iter; margin is a fraction of the kill target's CURRENT HP, since the acquire-targets tuples lack max_health). ✅ builds.
- [x] **A5** Hysteresis-gated swap in `acquire_targets`: melee + configured kill target kited out of `swap_range` + chased ≥ hysteresis → swap to softest in-melee enemy. Re-force gated by `swap_sticky` (no ping-pong); same-tick so the intermediate write is unobserved; existing target_acquisition trace covers it (no schema change). ✅ full suite (13 binaries).

### Validation
- [x] **VAL1** `bucket_a_unit` module (5 tests): `enemy_healer_is_cced` (cast-preventing CC, excludes Root; dead/healthy/missing) + `select_softer_melee_target` (softest-in-range, margin, range, emptiness, deterministic tie-break). time-in-FREE probe recalibrated covers B8. Target-swap/burst-during-CC integration probes SKIPPED — see finding below (would go vacuous).
  - **Finding (target-swap):** correctly wired + unit-tested + RON-tunable, but fires rarely in 2v2 — even lenient (range 8 / margin 0 / hyst 0.5) produced 0 swaps across seeds. The trigger geometry (current target kited beyond range WHILE a softer enemy is within range of the melee) seldom co-occurs when enemies move as a group and off-targets take no damage. Likely needs design iteration (swap to nearest-reachable, not just softer) and/or matters mainly in 3v3/cleave. Logged for roadmap.
- [x] **VAL2a** Isolated B8 matrix (1v1 n100, branch vs **freshly-built main** — NOT canonical, which proved stale). Only **3/49 cells moved >5pt, all healer cells**; 45/49 byte-identical to main (confirms B1–B7 behavior-preserving + no nondeterminism). **B8 result:** Priest-mirror draw wall **100%→48% draws (300s→262s)** — exactly the R13 draw-wall metric this slice targets — and Priest-vs-Paladin shifts toward Priest. No non-healer regressions. (Note: comparing against the committed `canonical_1v1` falsely flagged 16 cells — it predates current main; always baseline against a fresh main build.)
- [ ] **VAL2b** Bucket A 2v2 sweep — DEFERRED (low priority): A5 target-swap is largely dormant in 2v2 (geometry), A2 is one-class; bucket A's measurable effect is small. Canonical 2v2 baseline is stale — needs fresh-main baseline if run.

## Cross-step conflict notes
- movement_config.rs shared by B6/B7/A3 → serialize those config edits.
- class_ai/mod.rs shared by A1/A4/B3 → serialize those.
- A2's five per-class files are mutually disjoint.
- Load-bearing suites: registration_audit, item budget, movement `validate()`, posture probes — green every step.
