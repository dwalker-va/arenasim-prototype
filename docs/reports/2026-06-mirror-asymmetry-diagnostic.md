# Mirror-Matchup Side-Bias Diagnostic

**Date:** 2026-06-06
**Plan:** `docs/plans/2026-06-06-001-feat-healer-posture-movement-ai-plan.md`, U1 (R1)
**Baseline:** `design-docs/balance/matrix_baseline_2026-05-23_pet_dispatch_1v1_post_n100.md` (N=100, seed base 0)
**Verdict:** Root cause identified with direct trace evidence. **No fix lands in this slice** — the cause is the sequential-resolution architecture of same-frame decision races, and every localized candidate only flips or relocates the bias. **Side-symmetrized cell deltas are the standing measurement protocol for U4/U9.**

---

## 1. Bias magnitude per mirror

T1 winrate from the N=100 baseline, with exact two-sided binomial tests against 50/50 over **decisive** (non-draw) matches:

| Mirror | T1 win | Draw | Decisive split | p (two-sided) | Verdict |
|---|---|---|---|---|---|
| Rogue | **13%** | 0% | 13/100 | 1.3e-14 | Extreme T2 bias |
| Paladin | **73%** | 0% | 73/100 | 4.7e-6 | Extreme T1 bias |
| Priest | **61%** | 0% | 61/100 | 3.5e-2 | Mild T1 bias |
| Hunter | 43% | 29% | 43/71 | 9.6e-2 | Not significant |
| Warrior | 51% | 6% | 51/94 | 0.47 | Healthy |
| Warlock | 45% | 13% | 45/87 | 0.83 | Healthy |
| Mage | 19% | **64%** | 19/36 | 0.87 | Healthy decisive split; the draw wall is itself a symmetry artifact (§3.4) |

Diagnostic method: 10 seeded matches per skewed mirror (seeds 1–10) with `--trace-mode on`, plus 5-seed control runs on Warrior and Mage. All evidence below cites those runs; every claim is reproducible via the commands in §7.

---

## 2. Shared root cause

A mirror matchup is **perfectly symmetric**: spawn positions are mirrored, loadouts identical, and class AI is deterministic given the same inputs — so both sides arrive at every decision point **on the exact same frame** for the entire match (e.g. in Priest mirror seed 3, every `[CAST]` line from 0.00s to OOM appears in same-timestamp pairs). The match outcome is then decided entirely by (a) independent crit RNG rolls and (b) **how the engine resolves same-frame races**.

Same-frame races resolve **sequentially in ECS query iteration order** — Bevy archetype/table order. That order is:

- **deterministic** (the seeded determinism gates stay green),
- **side-correlated** (Team 1 spawns first in `src/headless/runner.rs:168` and starts first in every table), and
- **unstable across the match**: every component insert/remove (`ActiveAuras` insert at first aura / remove-on-empty at `src/states/play_match/auras.rs:86,666`, `CastingState`, `DamageTakenThisFrame`, …) migrates the entity between tables via swap-remove + append, permanently reordering subsequent query iteration.

Three **winner-take-all** resolution mechanisms convert that ordering into side bias:

1. **Instant-CC first-mover silencing** — `decide_abilities` (`src/states/play_match/combat_ai.rs:418`) dispatches actors sequentially; an instant CC landed by an earlier-dispatched actor is reflected into the shared snapshot via `same_frame_cc_queue` → `reflect_instant_cc` (combat_ai.rs:481–488) so the later-dispatched victim is *already incapacitated at its own dispatch* and silently loses its turn (combat_ai.rs:503–543). In a mirror where both sides want the same CC on the same frame, the first-dispatched side wins the race 100% of the time.
2. **Orb-pickup tie-break** — `check_orb_pickups` (`src/states/play_match/shadow_sight.rs:134`) awards an orb to the *first combatant in query iteration order* within pickup radius (`break` after first hit). Mirrored approach paths put both combatants inside the radius on the same frame.
3. **Lethal-swing suppression** — `combat_auto_attack` (`src/states/play_match/combat_core/auto_attack.rs`) queues all swings in iteration order, then applies them in queue order with `died_this_frame` suppression (auto_attack.rs:241): at a simultaneous-lethal frame, the first-processed swing kills and the victim's queued swing is dropped.

By contrast, **queued instant attacks both resolve** (e.g. two Mortal Strikes on the same frame both land), which is why some mirrors are immune (§4).

---

## 3. Per-mirror causal chains (trace evidence)

### 3.1 Rogue mirror — 13% T1 (T2 wins 87%)

The full chain, identical in **10/10 diagnostic seeds** (timestamps are match-log time = sim time + 10s prep):

1. **Stealth standoff.** Both Rogues are mutually invisible, acquire no target, and walk to arena center (movement.rs no-target branch). Nothing perturbs either entity's archetype for 90s, so iteration order is still spawn order: **T1 first**.
2. **99.97s:** Shadow Sight orbs spawn at (0,±15). Both Rogues sit at z≈−0.4 → both path to the *same south orb* and enter pickup radius on the same frame.
3. **102.07s:** **Team 1 picks up Shadow Sight in 10/10 seeds** — mechanism 2, first-iterated-wins, and T1 iterates first. (Pickup grants mutual visibility: T1 holds Shadow Sight, and the holder is also revealed.)
4. **Archetype round-trip reorders T1 behind T2.** The Shadow Sight aura inserts `ActiveAuras` on T1 (auras.rs:666) — T1 leaves the base table, leaving it as `[T2]`. Both Rogues Ambush each other on the same frame at 102.09s; the damage breaks Shadow Sight ("broke from damage", 102.11s), `ActiveAuras` empties and the component is removed (auras.rs:86) — T1 is appended *back* to the base table: order is now **[T2, T1]** for the rest of the match.
5. **103.61s in 10/10 seeds:** the first post-opener GCD expires for both simultaneously. Both want Kidney Shot. **T2 dispatches first and wins the race** — trace (seed 10, `frame 6217`): Team 2 emits `KidneyShot → action_taken`; **Team 1 emits no event at all that frame**, the signature of the incapacitated-skip in combat_ai.rs:503 (both sides were verifiably off GCD — both Ambushed on frame 6126). The reflected stun (mechanism 1) silenced T1 before its dispatch.
6. 6.0s stunlock → free Sinister Strikes/autos → T1 dies (e.g. seed 10: dead at 107.11s without ever acting again).

Crit RNG is the only escape valve: in the single T1 win (seed 5 of the diagnostic set), T1's Ambush crit (209 vs 104), T1 survived the stunlock, and landed its own Kidney Shot at 109.63s. That ~1/10 escape rate matches the 13% baseline.

Note the inversion: the spawn-order advantage (winning the orb) is what *costs* T1 the match — the aura insert/remove round-trip is the reordering event. This is why naive reasoning ("T1 iterates first → T1 wins races") predicts the wrong sign here.

### 3.2 Paladin mirror — 73% T1

1. Both Paladins buff during prep and carry permanent auras; combat begins symmetrically (simultaneous Holy Shocks at 14.90s).
2. **16.42s in 10/10 seeds: Team 2 lands the first Hammer of Justice** — same mechanism-1 race as the Rogue KS. Direct trace evidence (seed 2, `frame 986`): Team 2 emits `HammerOfJustice → action_taken`; Team 1's only event that frame is the `DivineShield rejected: PreconditionUnmet` candidate from the *Divine-Shield-while-CC'd special path* (combat_ai.rs:514) — i.e. **T1 was already stunned at its own dispatch**. (Dispatch order here favors T2 because of earlier archetype churn from the first damage exchanges; order at any instant is an emergent artifact, cf. §2.)
3. **The HoJ "prize" backfires.** T2 spends its 6s free-cast window healing a small deficit — its first Flash of Light heals **58** (overheal-capped) — while T1, healing after eating the stun plus 6s of free damage, heals at full deficit (82, 81). Seed 2 totals: identical cast counts (14 FoL each, both mana-capped) but **effective healing T1 1071 vs T2 1002**.
4. The match decays into a ~200s OOM auto-attack attrition war (avg 207.5s in baseline). The ~5% effective-healing edge (~70 HP ≈ 10 auto-attacks) decides it: **T1 won 8/10 diagnostic seeds**, matching the 73% baseline. T1 healed more in 7/10 seeds (avg 1029 vs 976).

So in the Paladin mirror the first mover **loses**: winning the CC race forces your heal window to occur when your deficit is smallest. Same root cause as Rogue, opposite sign — further evidence that the bias direction is mechanism-specific, not a uniform "Team 1 advantage".

### 3.3 Priest mirror — 61% T1 (mild)

1. Cast timelines are **frame-identical for the entire match** (seed 3: every Mind Blast / Flash Heal / PW:Shield from both sides shares a timestamp pair). Priests have no instant CC, so mechanism 1 never fires.
2. Divergence comes only from independent crit rolls (damage and heal crits), which decide most seeds — diagnostic outcomes split 5/5 with crit counts the visible driver.
3. The residual symmetric seeds end in an OOM wand-attrition race where both sides reach lethal on the same frame, resolved by **mechanism 3**: seed 6 ends `[102.39s] Team 1 Priest's Wand Shot hits Team 2 Priest for 7 damage → DEATH`, with T2's same-frame queued wand shot suppressed by `died_this_frame`. The suppression goes to whichever side iterates first in `combat_auto_attack` — side-correlated, producing the mild 61% (p=0.035).

### 3.4 Why the healthy mirrors are immune

- **Warrior (51%):** all synchronized actions are *queued instant attacks that both resolve* — control run seed 1 shows simultaneous Charges at 14.40s and simultaneous Mortal Strikes at 17.43s with **both** hits landing (88 vs 92). No winner-take-all race exists; damage-roll/crit RNG desynchronizes the match within seconds and decides it (~25–29s). Iteration order never gets a decisive moment to act on.
- **Mage (19%/64% draw):** the symmetric race is "resolved" by **mutual destruction**. 4/5 control seeds end with both Mages eliminated on the same frame (e.g. both dead at 20.40s) — simultaneous lethal Frostbolts resolve in `projectiles.rs`, which has no cross-side suppression — recorded as draws. That is the 64% draw wall; the decisive remainder is crit RNG (19/36 ≈ 53%, p=0.87).
- **Warlock (45%, 13% draw) / Hunter (43%, 29% draw):** DoT/auto-shot attrition with the same double-KO draw escape valve and no synchronized winner-take-all CC race; decisive games are RNG-dominated (p=0.83 / p=0.096).

The pattern: **a mirror is biased exactly when it funnels into a synchronized winner-take-all race** (instant-CC reflection, orb tie-break, lethal-swing suppression) **and lacks a draw/mutual-resolution escape valve.**

---

## 4. Suspects cleared

- **(c) `move_to_target` pre-loop position snapshot** (`combat_core/movement.rs:91`): the snapshot is taken once before the loop, so *every* combatant reads pre-frame positions — symmetric, no half-frame asymmetry between sides. Not implicated.
- **(d) HashMap nearest-enemy tie-breaks** (`movement.rs:260`, kiting): in a 1v1 mirror there is exactly one enemy, so iteration order of the map is irrelevant; and HashMap iteration nondeterminism would show up as *same-seed* nondeterminism, which the determinism gates exclude. The bias is deterministic and side-correlated, which acquits hash ordering generally. (`auto_attack.rs` already converted its damage maps to BTreeMap for exactly this reason — see comments at auto_attack.rs:73–82.)
- **(b) auto-attack iteration order**: implicated, but only as the *minor* mechanism (Priest endgame tiebreak), not the headline Rogue/Paladin bias.

---

## 5. Fix-or-defer decision: **DEFER**

No localized fix lands in this slice, because every candidate examined removes none of the bias:

| Candidate | Effect |
|---|---|
| HashMap→BTreeMap swaps in movement.rs | Not implicated (§4); a determinism hygiene change, not a bias fix. No effect on mirrors. |
| Award orb to nearest combatant instead of first-iterated | Mirrored approach paths produce *exactly equal* f32 distances (sign-symmetric arithmetic), so a tie-break is still required — same problem relocated. |
| Insert `ActiveAuras` on all combatants at spawn (freeze archetype order) | Removes the order *instability* but locks dispatch order to spawn order: the Rogue mirror flips from 13% T1 to ~87–100% T1 (T1 then wins every KS race). Bias magnitude survives; only the sign changes. Also shifts every seeded outcome → full re-baseline for zero fairness gain. |
| Resolve simultaneous lethal swings as mutual kill | Converts the Priest endgame tiebreak into draws; touches a kill-credit path shared by all matchups; does nothing for Rogue/Paladin. Not worth the re-baseline alone. |

The actual root cause — sequential resolution of same-frame decision races in `decide_abilities` (the `same_frame_cc_queue` first-mover structure) plus iteration-order-resolved ties in `check_orb_pickups`/`combat_auto_attack` — requires an **ordering redesign** (explicit dispatch order, e.g. alternating by `(team, slot)` round-robin per frame, or two-phase simultaneous resolution of instant CC). That changes every seeded outcome in every matchup, forces a full matrix re-baseline, and is explicitly out of scope per the plan ("Deferred to Follow-Up Work"). It is recorded there as the standing follow-up.

**No code changed in this unit. Seeded outcomes are unshifted; all existing baselines remain valid.**

---

## 6. Standing measurement protocol for U4/U9

While the bias remains unfixed, **all matrix-based before/after deltas in this plan (U4 snapshot-visibility delta, U9 validation) must be computed on side-symmetrized cells**:

```
sym(A,B) = ( winrate_T1(A,B) + (1 − winrate_T1(B,A)) ) / 2
```

i.e. average the (A,B) cell with the complement of the (B,A) cell. This cancels first-mover/side bias to first order. Mirror cells (A,A) cannot be symmetrized this way — report them raw, flag them per this document, and never use a mirror cell as a tuning target while the ordering artifact stands. Draw rates symmetrize the same way (average of the two cells).

---

## 7. Reproduction

```bash
cargo build --release

# Rogue mirror, any seed 1..10 — watch: T1 takes the orb, T2 lands KS at 103.61s
echo '{"team1":["Rogue"],"team2":["Rogue"],"random_seed":10}' > /tmp/rr.json
./target/release/arenasim --headless /tmp/rr.json --trace-mode on
grep -E "Shadow Sight|Kidney|DEATH" match_logs/$(ls -t match_logs | grep -m1 'txt')

# The silenced-victim signature at the KS frame (no Team-1 event on the frame
# where Team 2 chooses KidneyShot):
T=match_logs/$(ls -t match_logs | grep -m1 trace)
jq -c 'select(.kind == "ability_decision" and (.outcome.ability == "KidneyShot"))
       | {frame, team: .actor.team}' $T

# Paladin mirror — Team 2 lands first HoJ at 16.42s in every seed; Team 1's
# same-frame event is the Divine-Shield-while-CC'd path (already stunned):
echo '{"team1":["Paladin"],"team2":["Paladin"],"random_seed":2}' > /tmp/pp.json
./target/release/arenasim --headless /tmp/pp.json --trace-mode on
jq -c 'select(.sim_time > 6.4 and .sim_time < 6.45)' match_logs/$(ls -t match_logs | grep -m1 trace)

# Mage mirror double-KO draw (4/5 seeds):
echo '{"team1":["Mage"],"team2":["Mage"],"random_seed":2}' > /tmp/mm.json
./target/release/arenasim --headless /tmp/mm.json
grep DEATH match_logs/$(ls -t match_logs | grep -m1 'txt')
```

Caveat on within-frame trace ordering: the trace writer sorts events by `(frame, entity_id, kind)` (`decision_trace/writer.rs:71`), so within-frame event order in the JSONL is **not** dispatch order. Dispatch order must be inferred from *absence* (the silenced victim emits nothing) or from special-path signatures (Divine-Shield-while-CC'd), as done above.

---

## 8. Follow-up recommendations (out of scope here)

1. **Ordering redesign** (the real fix): explicit, side-fair dispatch order in `decide_abilities` — alternate first-mover by frame parity over a `(team, slot)`-sorted list, or two-phase commit for instant CC (collect intents, resolve simultaneously, mutual CC both lands). Requires full matrix re-baseline; pairs naturally with the planned parallel in-process matrix runner.
2. Same treatment for `check_orb_pickups` (nearest-wins with explicit fair tie-break) and simultaneous-lethal resolution in `combat_auto_attack`/`projectiles` (mutual kill = draw), in the same re-baseline window.
3. Until then, treat all mirror-cell winrates in any matrix as measurements of the ordering artifact, not of balance.
