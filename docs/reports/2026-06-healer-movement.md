# Healer Posture-Based Movement AI — Validation Report

**Date:** 2026-06-07
**Branch:** `feat/healer-posture-movement-ai`
**Plan:** `docs/plans/2026-06-06-001-feat-healer-posture-movement-ai-plan.md` (U9; R13, R4)
**Mirror protocol:** `docs/reports/2026-06-mirror-asymmetry-diagnostic.md`

---

## Executive Summary

This slice gave the Priest and Paladin a posture-based movement AI (FREE / PRESSURED / ESCAPE / DIP over a multi-term position scorer), built harness-first: probes, `movement_decision` trace events, and KPI scripts landed before any behavior change, with all weights data-driven in `assets/config/movement.ron`. Two headline findings emerge from N=100 1v1 and 2v2 validation. **(1)** The U4 snapshot casting-visibility fix moved three 1v1 cells far more than its size suggests (Mage v Rogue +47.5, Rogue v Priest −52, Warrior v Paladin −35 symmetrized) — these are artifacts of a since-fixed AI blind spot (attackers skipped their entire decision tick when their target was mid-cast), **not class-kit balance signals**. **(2)** Healer movement made healers materially harder to kill: Priests now roam (post-gate path 0→1344 units in the Priest mirror), survive focus longer, and hold distance — at the cost of draws. The **R13 draw-rate watch metric tripped** (1v1 draws 2.3%→6.0%, all in healer-mirror cells; 2v2 Paladin+Priest a 100% draw wall at cap), formally triggering the deferred offensive-punish slice as the next unit. A Paladin 1v1 regression surfaced during validation and was fixed surgically (commit `877054a`).

---

## What Shipped

| Unit | Commit | Summary |
|---|---|---|
| U1 | `1723478`, `cce7aa2` | Mirror-asymmetry diagnostic + pre-change N=100 1v1 baseline. Root cause = sequential same-frame race resolution in ECS iteration order; no fix lands, side-symmetrized deltas become the standing protocol. |
| U2 | `2a5d901` | Behavior-probe observer (read-only, non-perturbing) + KPI helpers; `scripts/movement_kpis.sh`. |
| U3 | `82c238d` | `movement_decision` trace event kind + builder + audit wiring (transition-only emission). |
| U4.1 | `b14b577` | `CombatSnapshot` includes casting/channeling combatants. Landed alone, matrix-measured in isolation. |
| U4.2 | `da73e06`, `85f7504` | `CombatContext` threat predicates (`enemies_targeting`, `primary_attacker`, `attacker_escape_window`, `is_closing`); `positions` snapshot → `BTreeMap`. |
| U5 | `7f0a2d1`, `ad448a4` | `MovementDirective` + `HealerPosture` components, executor ladder branch, pure position scorer, `movement.ron`. |
| U6 | `57fc4cd` | Priest FREE/PRESSURED postures (formation anchoring + threat-aware repositioning). |
| U7 | `5dda7f6` | Priest ESCAPE windows + cast-vs-move urgency rule. |
| U8 | `b03eec1` | Paladin postures + HoJ DIP (melee identity preserved in FREE). |
| Fix | `877054a` | Gate Paladin PRESSURED retreat on a living non-pet teammate (R5 no-ally rule carried to Paladin). Regression probe `paladin_1v1_never_retreats` (seed 4100). |
| U9 | `96120c5`, `c66c24b` | Final N=100 1v1 (fixed binary) + 2v2 healer sweeps; this report. |

---

## 1v1 Matrix — Three-Way Symmetrized Deltas (N=100)

Cells are **side-symmetrized** per the mirror protocol: row-vs-col winrate = avg of the (row,col) and (100 − (col,row)) cells, cancelling first-mover bias to first order. **Mirror cells (diagonal) are reported raw** and are never tuning targets. Two transitions are shown: **pre → post-U4** isolates the snapshot-visibility effect; **post-U4 → final** isolates the movement effect.

Baselines: `matrix_baseline_2026-06-06_1v1_n100_pre.csv`, `matrix_2026-06-06_1v1_n100_post_u4_snapshot_fix.csv`, `matrix_2026-06-07_1v1_n100_final.csv`.

### Symmetrized winrate (row vs col), FINAL

| row\col | War | Mag | Rog | Pri | Wlk | Pal | Hun |
|---|---|---|---|---|---|---|---|
| **War** | 51\* | 0 | 4 | 86 | 68 | 16 | 73 |
| **Mag** | 100 | 19\* | 61 | 100 | 100 | 100 | 100 |
| **Rog** | 96 | 39 | 13\* | 48 | 100 | 0 | 98 |
| **Pri** | 14 | 0 | 52 | 0\* | 0 | 60 | 100 |
| **Wlk** | 32 | 0 | 0 | 100 | 43\* | 21 | 92 |
| **Pal** | 84 | 0 | 100 | 40 | 79 | 52\* | 100 |
| **Hun** | 27 | 0 | 2 | 0 | 8 | 0 | 43\* |

`*` = mirror cell (raw T1 winrate, not symmetrized).

### U4 effect (snapshot-fix): pre → post-U4, symmetrized

| Matchup | pre | post-U4 | Δ |
|---|---|---|---|
| Mage v Rogue | 13.5 | 61.0 | **+47.5** |
| Rogue v Priest | 100.0 | 48.0 | **−52.0** |
| Warrior v Paladin | 50.5 | 15.5 | **−35.0** |
| Warlock v Paladin | 2.0 | 21.0 | +19.0 |
| Priest v Warrior | 0.0 | 6.0 | +6.0 |

All three headline moves are mechanism artifacts, not class signals — see the U4 Mechanism section below.

### Movement effect: post-U4 → final, symmetrized (non-mirror)

| Matchup | post-U4 | final | sym draw post→final |
|---|---|---|---|
| Warrior v Priest | 93.5 | 85.5 | 0% → 0% |
| Priest v Paladin | 0.0 | 60.0 | 0% → **34%** |

The Priest v Paladin cell goes from a 0-win wall to a draw-laden split (raw final `Pri,Pal` = 39W/25L/36D; `Pal,Pri` = 19W/49L/32D), i.e. neither healer can finish the other inside the cap. Non-healer cells outside the healer columns/rows are byte-stable across post-U4 → final (the U5 byte-identity guard: non-healers carry no directive).

### Mirror cells (raw, never tuning targets), post-U4 → final

| Mirror | post-U4 | final |
|---|---|---|
| Priest | 59W / 0D | **0W / 100% draws** |
| Paladin | 65W / 0D | 52W / **15% draws** |

The Priest mirror flips from a decisive 59/41 split to a **100% draw wall** — two mobile, distance-holding healers OOM-grind to the 300s cap. The Paladin mirror retains a decisive majority but accrues 15% draws.

### Total 1v1 draw rate

| | pre | post-U4 | final |
|---|---|---|---|
| Mean cell draw rate | **2.3%** | **2.3%** | **6.0%** |

The snapshot fix is draw-neutral; the entire 2.3%→6.0% rise is the movement effect, concentrated in healer-mirror cells (Priest mirror 100%, Priest v Paladin 34%, Paladin mirror 15%).

---

## 2v2 Healer Sweep — Movement Effect (N=100)

Hunter+Priest vs (each class)+Priest. 120s cap (durations reported reach 130s where matches consistently time out). Sources: `matrix_2026-06-07_2v2_n100_post_u4.csv`, `matrix_2026-06-07_2v2_n100_final.csv`.

| Matchup (T1 = Hunter+Priest) | post-U4 (W/D, dur) | final (W/D, dur) |
|---|---|---|
| vs Warrior+Priest | 1% / 0% — 74.3s | 3% / 2% — 93.1s |
| vs Mage+Priest | 0% / 0% — 35.3s | 0% / 0% — 39.4s |
| vs Rogue+Priest | 0% / 0% — 62.0s | 0% / 0% — 65.1s |
| vs Priest+Priest | 87% / 0% — 84.9s | 74% / **23%** — 121.4s |
| vs Warlock+Priest | 0% / 0% — 53.9s | 0% / 0% — 54.9s |
| vs Paladin+Priest | 1% / **39%** — 105.1s | 0% / **100%** — 130.0s |

The movement effect is unmistakable in the healer-heavy cells: **Paladin+Priest becomes a 100% draw wall at the cap** (39% → 100%), and **Pri+Pri loses 13 decisive wins to draws** (87W/0D → 74W/23D). Durations rise across the board (+3s to +36s) as healers survive longer. The decisive Hunter-loss cells (vs Warrior/Mage/Rogue/Warlock+Priest) are essentially unchanged — Hunter+Priest's offensive shortfall, not healer movement, decides those.

**Paladin-fix note:** the committed `final` 2v2 sweep is the fixed-binary re-run (post commit `877054a`). It is byte-identical to the pre-fix fixed-binary run, because the Paladin no-ally PRESSURED gate never fires with a living teammate — every 2v2 Paladin always has a Priest alongside it. The fix is therefore invisible in 2v2 and is exercised only by the 1v1 matrix and the regression probe.

---

## U4 Mechanism — Why the Snapshot-Fix Deltas Are Large

### Why the U4.1 deltas are so large (mechanism attribution)

The U4.1 snapshot casting-visibility fix (b14b577) moved three 1v1 cells far more than its size suggests, and all three move through a single mechanism: before the fix, any combatant that was mid-cast was missing from `CombatSnapshot.combatants`, so a class AI whose *target* was casting failed its `target_info()` lookup and skipped its decision tick outright. The fix roughly doubled how often the attacker got to act against a casting opponent. In **Rogue v Priest (100%→48%)** this is decisive and clean — pre-fix the Rogue stun-locked the Priest with KidneyShot in 86 of 100 games; post-fix the realigned combo-point economy means KidneyShot never fires (0/100), the Priest is never stun-locked, and it heals the fight into a near-even OOM grind (avg duration 19s→34s). In **Mage v Rogue (13.5%→61%)** and **Warrior v Paladin (50.5%→15.5%)** the effect is a *timing cascade* rather than a behavioral change: ability selections are nearly identical pre/post, but removing the attacker's idle-while-target-casting ticks shifts combat timing by ~0.5–1s, which in these knife-edge fights flips who lands the last hit (Mage gains a 4th Frostbolt on net) or perturbs the Paladin's threshold-gated DivineShield/HolyShock timing in its favor. The takeaway: these are not balance signals about the classes' kits — they are artifacts of a since-fixed AI blind spot, and the magnitude reflects how many of these matchups were decided in their final second. WvP in particular should be flagged as a diffuse timing flip, not attributed to any single ability.

Supporting evidence (full detail in the investigation session): Rogue decision-event counts 47,240→82,603 (RvP cell) and 487→723 (single MvR seed); Warrior NoValidTarget-idle ticks 137k→113k (WvP); KidneyShot landed 86/100→0/100 with 45,803 post-fix InsufficientResource rejections (combo points now spent on SinisterStrike every tick instead of banked); Mage Frostbolt-count 3→4 on 10 seeds vs 4→3 on 4 (flipped seeds 946/947/958/963/978/992); Paladin mean first-DivineShield 26.8s→36.7s, self-heal HolyShock 23→3 (seed 520 pivot at ~20.7s).

---

## Paladin 1v1 Regression and Fix

Validation worked as designed: the **first** final-matrix run exposed a Paladin 1v1 collapse, which the no-ally degenerate rule from R5 was supposed to prevent.

**Symptom.** With no living ally to protect, the Paladin entered PRESSURED on any focus and **retreated permanently**, deleting its melee output for nothing:

| Cell | first-final | fixed-final |
|---|---|---|
| Paladin v Hunter | 100% → **0%** wins, 61.5% draws | restored to 100% |
| Priest v Paladin | flipped to 100% (Priest) | restored |
| Warlock v Paladin | flipped to 100% (Warlock) | restored |

Trace forensics showed **85 posture strobes per match** — the lone Paladin kiting the Hunter's pet back and forth, never committing.

**Root cause.** The Priest's R5 no-ally degenerate rule (FREE falls back to legacy hold; PRESSURED needs a team to protect) was implemented for the Priest but **not carried to the Paladin's retreat arms**.

**Fix (`877054a`).** PRESSURED is gated on a living non-pet teammate. With no team, there is no healing capacity to retreat for, so the Paladin keeps its melee identity. Dips, rotation HoJ, and ESCAPE are unaffected. Guarded by regression probe `paladin_1v1_never_retreats` (seed 4100).

**Verification.** The fixed-binary full matrix (`96120c5`) is surgical: only the **four intended movement cells** (the two Priest-relevant healer cells plus the Priest and Paladin mirrors) differ from post-U4 — every other cell is unchanged, confirming the fix touched nothing it shouldn't.

---

## KPI Section (R4)

Same-seed traces exist for post-U4 (`match_logs/traces/1780794175`) and final (`match_logs/traces/1780862672`). KPIs below are **medians across 10 seeds** per cell, computed with `scripts/movement_kpis.sh` on the Priest-relevant cells. (Path length is a sparse-sample underestimate — events are emitted on decisions, not per tick.)

### Focused-healer KPIs (median, 10 seeds/cell)

| Cell / set | post-gate path | avg nearest-enemy | min nearest-enemy | % within 4yd | % within 10yd |
|---|---|---|---|---|---|
| Warrior v Priest (Priest), post-U4 | **0.0** | 57.1 | 1.92 | 9.3 | 9.3 |
| Warrior v Priest (Priest), final | **66.5** | 53.9 | 1.99 | 14.6 | 14.6 |
| Priest mirror (Priest), post-U4 | **0.0** | 33.0 | 27.84 | 0.0 | 0.0 |
| Priest mirror (Priest), final | **1343.8** | 41.6 | 29.54 | 0.0 | 0.0 |

**Reading.**
- **The statue is gone.** Post-gate path length goes from **0.0** (the Priest never moved post-gate — exactly the ~21-unit-walk-only pathology the slice targeted) to 66.5 (1v1 vs Warrior) and **1343.8** (Priest mirror, where both healers roam the full 300s draw). The Priest mirror nearest-enemy distance also rises (33.0→41.6 avg, 27.8→29.5 min) — the two healers actively hold each other at range.
- **Honest caveat — 1v1 vs a lone melee.** In Warrior v Priest the Priest's % within 10yd actually *rises* (9.3→14.6). This is expected and correct: in 1v1 the Priest has no ally to anchor to, so movement is the legacy-degenerate hold plus PRESSURED repositioning, and a single uncontested Warrior simply re-closes the gap. Movement buys time (the cell drops 93.5→85.5 symmetrized) but cannot escape a lone melee — that is the offensive-punish/peel gap, not a movement defect. The escape value shows up where a teammate provides the CC window (see U6/U7 probes below).

### Probe numbers (already measured, U6–U8)

- **Statue probe** (forced-focus): post-gate path length **20.1 → 89.1 units**.
- **Focused-Priest survival** under forced focus: **14s → 32s**.
- **Escape separations:** ~**20 units** gained per qualifying window (teammate-CC-on-attacker).
- **Paladin identity probe:** melee uptime **61% preserved** (no material regression vs pre-change).

---

## R13 Draw-Rate Verdict and Follow-Up Recommendations

**Watch metric outcome (stated plainly):**

- **1v1 draws 2.3% → 6.0%**, entirely in healer-mirror cells: **Priest mirror 100%**, **Priest v Paladin 34%** (symmetrized), **Paladin mirror 15%**. Every non-healer cell is draw-stable.
- **2v2 Paladin+Priest: 100% draw wall** at the cap; **Pri+Pri 23%** draws (from 0%).

**VERDICT: the R13 watch metric is TRIPPED.** Per the plan's Scope Boundaries, this **formally triggers the deferred offensive-punish slice** as the next unit: target-swap responsiveness when a kill target kites out of reach, and burst-priority during enemy-healer CC windows. The current slice was scoped to make healers *survivable*; it succeeded, and the predicted consequence (draw inflation, the headline risk in the plan) materialized exactly where the plan said it would (the Paladin+Priest cell already drew 10/10 pre-slice). Offense is the correct next lever, not re-nerfing healer movement.

**Wall-time follow-ups (recommended, discussed in-session).** Draw-wall cells now dominate matrix wall time — the N=100 1v1 matrix went **3,612s (pre) → 4,864s (final)** as more cells run to the 300s cap. Two mitigations:

1. **Parallel in-process matrix runner** (already noted in project memory as planned) — would cut the now-~80-min N=100 wall time by running cells concurrently in one process.
2. **Early-draw detection heuristic** — detect a stalemate (e.g. both teams above an HP floor with no net damage trend over a window) and cut the match early rather than burning the full cap, which is where the new wall-time cost concentrates.

---

## Naturalness Pass (recommended manual step)

The plan's outer tuning loop calls for human replay review. This was **not** run as part of U9 (it requires the graphical client and a human watcher); it is recommended as a manual follow-up before the next slice. Watch these three seeded replays via `cargo run --release` and check for the tells the postures are meant to remove:

| Comp | Seed | What to watch for |
|---|---|---|
| **Statue comp** — Rogue+Warrior vs Priest+Warrior (Rogue forced onto Priest) | 20260606 | Priest visibly repositions under focus instead of standing still; no zigzag/strobe; legible PRESSURED retreat that holds heal range of the ally. |
| **Escape comp** — Priest+Paladin vs Warrior+Mage | 1 | When the Mage Novas the closing Warrior, the Priest's ESCAPE reads as an intentional break for distance (≈20 units gained), not a wander; heals defer only while the ally is healthy. |
| **Dip comp** — Paladin+Warrior vs Priest+Warrior | 1 | The Paladin's walk-to-enemy-healer → Hammer of Justice → return reads as a single intentional play (dip-cast-return), aborts cleanly if the teammate dives, and never face-tanks when focused. |

Look specifically for: no per-frame direction flapping (R11 commitment windows), legible posture transitions, and no corner-pinning.

---

## Tuning Changelog (`movement.ron`)

Values changed during U6–U8 implementation/validation, each justified by a probe or KPI. Authoritative current values from `assets/config/movement.ron`; history from the git log of that file (`ad448a4` U5 → `57fc4cd` U6 → `b03eec1` U8).

| Key | Block | Change | Unit | Rationale |
|---|---|---|---|---|
| `corner_penalty` | priest.weights | 4.0 → **6.0** | U6 | Stronger corner avoidance for the Priest after corner-probe pressure; Paladin block stayed at 4.0. |
| `threat_intent_radius` | global | added = **30.0** | U6 | PRESSURED intent gate (closing/melee/pet) so a distant caster targeting the healer doesn't flip the posture (AE5). |
| `pressured_hold` | global | added = **1.5** | U6 | Hysteresis hold on the PRESSURED↔FREE boundary so a threat hovering at the danger radius doesn't strobe the posture. |
| `healing_heavy_hp` | paladin | added = **0.6** | U8 | Paladin-specific PRESSURED trigger: pulls to fallback range on healing-heavy team-HP state even before being focused. |

For reference, related fixed values in the current config: `danger_radius` 12.0, Paladin `fallback_range` 15.0, Priest `corner_penalty` 6.0 vs Paladin `corner_penalty` 4.0.

---

## Measurement-Protocol Note

All non-mirror cell deltas in this report are **side-symmetrized**: row-vs-col winrate = avg of the (row,col) and (100 − (col,row)) raw cells. This cancels the first-mover / iteration-order side bias to first order. **The mirror bias is unfixed** — the U1 diagnostic identified it as the sequential-resolution architecture of same-frame decision races (Team 1 spawns and iterates first), where every localized candidate fix only flips or relocates the bias, so a fix was deferred. **Mirror cells are therefore reported raw and are never used as tuning targets** (their absolute value carries the residual bias; their *change* across builds is still informative for naturalness, e.g. the Priest mirror's flip to a 100% draw wall). See `docs/reports/2026-06-mirror-asymmetry-diagnostic.md` for the mechanism, per-mirror magnitudes, and the standing protocol.
