---
date: 2026-04-26
topic: open-ideation
focus: open-ended general improvements
---

# Ideation: ArenaSim — Open Improvement Set

## Codebase Context

**Project shape:** Rust + Bevy 0.15 WoW Classic-inspired arena autobattler. 7 classes (Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter), 28 abilities, two run modes (graphical client + headless sim). Data-driven config in RON files (`abilities.ron`, `items.ron`, `loadouts.ron`). 62 Rust modules.

**Key structural patterns:**
- **Data-driven design:** abilities/items/loadouts in RON enable balance changes without recompile.
- **Dual registration:** combat systems registered in BOTH `states/mod.rs` (graphical) and `systems.rs::add_core_combat_systems()` (headless). Forgetting one = silent failure (documented as #1 historical bug source).
- **Distributed damage sites:** 7 separate sites apply damage; new stats thread through all of them.
- **Per-class AI:** 7 hand-coded `class_ai/*.rs` decision trees using a shared `CombatContext` helper. Paladin refactor (-132 LOC while adding a class) is the gold standard.
- **Visual effects:** every effect is a hand-rolled spawn/update/cleanup trio with 10 documented gotchas.

**Pain points / gaps:**
1. Only 3 test files — combat invariants documented but unautomated.
2. `match_logs/` has 900+ folders with no archival or compression.
3. No balance metrics tooling despite headless sim being well-suited for batching.
4. AI quality is opaque — class strategies exist but no measurement.
5. "Tuple creep" (`instant_attacks`, `frost_nova_damage`, `hits_to_process`) tracked in `todos/005`.
6. Adding a new stat (haste, hit, parry/dodge) cascades through 7 damage sites.
7. Adding a new ability requires 7 manual steps including icon download and validation-list edit.

**Past learnings (institutional):**
- Friendly-CC-break prevention is a systemic AI quality pattern; new damage abilities can forget the guard.
- Critical-hit system: 7 distinct damage sites; deferred mechanics must snapshot caster stats at cast time.
- Visual effects use `AlphaMode::Add`, `Res<Time>`, `try_insert`, `Without<T>` filter — gotchas-as-tribal-knowledge.
- Bug-hunting workflow finds invariants ("dead units deal no damage", "no duplicate buffs") that aren't yet automated tests.

## Ranked Ideas

### 1. Single-source system registration
**Description:** Replace the parallel registrations in `states/mod.rs` (graphical) and `systems.rs::add_core_combat_systems` (headless) with a single declarative table or trait that fans out to both schedules. New systems register once and are tagged `Headless`/`Graphical`/`Both`.
**Rationale:** MEMORY.md flags dual-registration as the #1 historical silent-failure source — Divine Shield, Holy Shock, Dispels all hit it. A structural fix removes the bug class permanently. AI logs the cast and burns the cooldown, but `[BUFF]/[DMG]` entries are absent and Bevy doesn't error on unregistered systems.
**Downsides:** Moderate refactor across many call sites. The "headless ≠ graphical" distinction is real (visual systems live only in graphical) — the abstraction has to express that cleanly.
**Confidence:** 90%
**Complexity:** Medium
**Status:** Unexplored

### 2. Unified DamageEvent pipeline + typed Caster/Target handles
**Description:** Collapse the 7 distributed damage-application sites into one `apply_damage(DamageEvent)` funnel. Replace `(Entity, f32, f32, ...)` tuples with named structs (or Bevy events). Wrap entity references in `CasterId`/`TargetId`/`OwnerId` newtypes so attribution becomes type-checked rather than convention-checked.
**Rationale:** Recent commits 7ae08ae (pet attribution) and the UA-backlash fix were both "we passed the wrong Entity" bugs that recur with every new pet/summon. Adding a future stat (haste, hit, parry, dodge, resilience) currently threads through 7 sites — this turns N-site refactors into 1-site. Tracked tech debt in `todos/005`.
**Downsides:** Largest survivor by line count. Touches hot paths; needs careful test coverage during the move. Risks a long branch with merge conflicts.
**Confidence:** 85%
**Complexity:** High
**Status:** Unexplored

### 3. Deterministic seeded replays + win-rate matrix
**Description:** Audit RNG sources for full determinism, persist seed in match logs, add `--replay <log>` (visual playback of a headless match at adjustable speed) and `--matrix N` (run all 7×7 matchups N times → CSV/markdown heatmap, diff vs. baseline).
**Rationale:** Headless sim is built for this but exposes no batching primitive. Today balance work is anecdotal eyeballing. With this, every change becomes A/B-measurable. The replay→visual loop closes the gap between "find the bug in batch" and "watch what happened." Compounds with every future class/ability/stat.
**Downsides:** Determinism audit is tedious — every `rand` call, every `HashMap` iteration order, every system parallelism source. RNG drift bugs are subtle.
**Confidence:** 80%
**Complexity:** Medium
**Status:** Unexplored

### 4. Property-based invariant tests + runtime debug_asserts
**Description:** A `proptest` harness that fuzzes random valid match configs through the headless runner and asserts: dead units deal no damage, no duplicate buffs of same type, all CC events logged, attribution sums match damage applied, no negative resources, no orphan auras at match end. Mirror the strongest invariants as `debug_assert!` inside the central damage/aura funnels.
**Rationale:** Only 3 test files today. The bug-hunt skill already documents these invariants but they're checked manually. Property fuzz turns the headless runner into a 24/7 regression target. Compounds with every future ability — the next pet-attribution-style bug gets caught pre-commit.
**Downsides:** Property tests can be flaky if the sim isn't deterministic — depends on idea #3 landing first or in tandem.
**Confidence:** 85%
**Complexity:** Medium
**Status:** Unexplored

### 5. Declarative AI behavior trees in RON
**Description:** Replace the 7 hand-coded `class_ai/<class>.rs` decision trees with `assets/config/ai/<class>.ron` behavior trees. Nodes: priority-select / condition (has_aura, target_at_range, mana_above) / action (cast, move, switch_target). One generic decider consumes the data.
**Rationale:** abilities.ron and items.ron are explicitly "working well." AI is the last large hand-coded surface. Tuning kiting thresholds, interrupt priorities, healer panic points becomes a no-recompile loop. Foundation for AI-quality measurement and eventual search/optimization over policies. New classes drop in as data.
**Downsides:** Some class behaviors are nuanced enough that BT primitives may need to grow (Rogue stealth opener timing, Mage Polymorph + reset-Frostbolt sequencing). Risk of "expressed as data but special cases keep leaking back into Rust."
**Confidence:** 65%
**Complexity:** High
**Status:** Unexplored

### 6. AI decision trace + live inspector overlay
**Description:** Capture every AI decision (target, ability chosen, candidates considered, rejection reasons like "out of mana" / "on CD" / "friendly CC active") into structured JSONL alongside the combat log. Add a toggleable F-key egui overlay during graphical matches showing each combatant's current target, decision, auras, and CDs in real time. Click a combatant to pin.
**Rationale:** Class AI is opaque today — the only debugging tool is the combat log plus inference. Solves "why didn't Warlock fear?" without rerunning. Prerequisite signal for any AI-quality work and de-risks idea #5 by exposing what the imperative trees are actually deciding.
**Downsides:** Some perf overhead in headless if always-on; gate behind a flag. JSONL volume can be large for long matches.
**Confidence:** 80%
**Complexity:** Medium
**Status:** Unexplored

### 7. Codegen abilities + auto-fetch icons from RON manifest
**Description:** Build script (or proc macro) that reads `abilities.ron` at compile time and generates `AbilityType` plus the `expected_abilities` validation list. A `cargo xtask sync-icons` task scans abilities.ron and items.ron, downloading any missing icons via the Wowhead MCP / known URL pattern.
**Rationale:** Steps 1, 2, and 5 of the "Adding a New Ability" CLAUDE.md workflow vanish. Same for items. Asset/data drift becomes structurally impossible — if it's in RON, it's on disk. Small, contained, high-frequency win.
**Downsides:** Build-script icon-fetch needs an offline mode (CI without network). Codegen complicates IDE jump-to-definition for the enum.
**Confidence:** 90%
**Complexity:** Low
**Status:** Unexplored

## Quick Wins (Worth Mentioning)

- **Invert match-log retention** — default to NOT saving 900+ folders; save only on assertion failure, balance-test divergence, or `--save-log`. Trivial change, immediate relief from `match_logs/` pile-up.
- **RON hot-reload for abilities/items** — Bevy's asset system already supports it; wire into the match-running state for sub-second balance iteration.

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| R1 | Player-as-Coach (real-time intervention layer) | Big product pivot — out of scope for combat-sim improvement |
| R2 | Tournament/Season as atomic unit | Scope explosion for solo project |
| R3 | Drafting/Auto-Drafter metagame | Different product entirely |
| R4 | User-authored abilities via Rhai/Lua/WASM | Modding platform pivot; not grounded in current direction |
| R5 | Headless-First, Graphics as spectator plugin | Interesting reframe but a rearchitecture with no clear payoff vs. survivor #3 |
| R6 | Reframe as Balance Research Lab | Already implicit in survivors #3 + #4 |
| R7 | Teaching Simulator ("WoW PvP, annotated") | Different product, not an improvement |
| R8 | Loadout/Talent egui editor | UX polish, low leverage relative to engine ideas |
| R9 | Class health dashboard CI | Folded into survivor #3 |
| R10 | Drop pre-match mana-restore loop | Too small; not worth a top slot |
| R11 | Auto-run Hunter/Fixer bug dance | Better as a /loop invocation than built-in tooling |
| R12 | StatModifier system | Strong but largely subsumed by survivor #2's funnel |
| R13 | Macro-driven visual effects | Nice cleanup; lower urgency than combat-side ideas |
| R14 | Live match inspector (standalone) | Folded into survivor #6 |
| R15 | Tuple-only refactor (standalone) | Folded into survivor #2 |
| R16 | Combat log replay-from-file (standalone) | Folded into survivor #3 |
| R17 | Snapshot-diff regression suite (standalone) | Folded into survivor #3 |
| R18 | Headless batch matrix (standalone) | Folded into survivor #3 |

## Session Log

- 2026-04-26: Initial ideation — 44 candidates generated across 4 frames (user pain, inversion/automation, assumption-breaking, leverage), 7 survived after dedupe + adversarial filtering.
