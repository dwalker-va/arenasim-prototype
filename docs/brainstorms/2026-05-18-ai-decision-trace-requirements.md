---
date: 2026-05-18
topic: ai-decision-trace
status: requirements
related:
  - docs/ideation/2026-05-18-open-ideation.md (item #7)
  - design-docs/balance/matrix_baseline_2026-05-16.md
---

# Requirements: AI Decision Trace + Inspector Overlay

## Problem

When a class is misbehaving in the matrix baseline (Hunter at 7%, Paladin > Rogue at 100%/100%, healer mirror stalls), the cheap-to-answer question is *"what did the AI consider, and why did it pick what it picked?"* — and there's currently no good way to ask it.

Today's instruments:

- `info!("Team X Warrior casts Y on ...")` — tells us what was chosen, not what was rejected.
- `CombatLog` (`match_logs/*.txt`) — definitive for damage/healing/CC, blind to AI deliberation.
- The matrix runner — surfaces *that* a matchup is broken, not *why*.

The gap is rejection visibility. When Hunter loses 0% vs Warrior, the question we can't answer cheaply is: "Did Hunter never try to Concussive Shot? Did Concussive Shot keep getting rejected for X reason? Did Aimed Shot keep getting interrupted into another priority? Did the pet keep targeting the wrong enemy?" Re-reading combat logs and grepping `info!()` lines doesn't surface rejected candidates or target-pick reasoning because both are implicit in the if-chain structure of the AI functions.

## Users and value

**Primary user:** the developer (single-user project), iterating on class AI and balance.

**Value:** turn "I think Hunter never uses Concussive Shot because of X" from a hypothesis requiring a code-read into an observation requiring a `jq` filter. Pairs directly with the matrix runner — when a cell looks wrong, the trace for that match is already on disk; no replay needed.

**Secondary value:** during graphical-mode visual review, an in-match overlay answers "why didn't they cast that?" without leaving the client.

## Counterfactual (today's workflow)

1. Suspect AI bug in matchup → re-run with graphical client.
2. Watch a match, notice anomaly, suspect a class AI branch.
3. Open `class_ai/<class>.rs`, read the if-chain top-to-bottom, mentally simulate the predicate state.
4. Add `info!()` lines, recompile, re-run, remove `info!()` lines.

That loop is slow and the print-debug residue gets committed by accident. The trace replaces step 3-4 with a `jq` query.

## Scope

### In scope (Phase 1)

Three instrumentation surfaces, one shared JSONL stream:

1. **Ability decisions** — one event per actor per call into `decide_<class>_action` (all 7 classes).
2. **Target acquisition** — one event per actor per `acquire_targets` tick, listing all enemy candidates with score / rejection reason (range, stealth visibility, kill_target priority, immunity).
3. **Pet AI decisions** — one event per pet per `pet_ai::decide_pet_action`, same shape as ability decisions. Critical for Hunter rebuild and Warlock Felhunter dispel behavior.

All three emit through a single `DecisionTrace` builder. Event types are discriminated by a `kind` field: `"ability_decision" | "target_acquisition" | "pet_decision"`.

### Event payload

Every event carries:

- `frame: u64`, `sim_time: f32` (seconds since gates opened), `seed: u64` (match seed for cross-ref)
- `actor: { entity_id, team, slot, class, hp_pct, mana_pct, position }`
- `kind` + a `kind`-specific payload (see below)

**`ability_decision`** payload:

- `target: Option<{ entity_id, class, hp_pct, position, distance }>` (current target)
- `candidates: Vec<{ ability: AbilityType, status: Chosen|Rejected, reason: Option<RejectionReason> }>`
- `outcome: ActionTaken { ability, target_id, was_instant } | NoAction { primary_reason }`

**`target_acquisition`** payload:

- `previous_target: Option<entity_id>`, `new_target: Option<entity_id>`, `changed: bool`
- `candidates: Vec<{ enemy_id, class, score: f32, status: Chosen|Rejected, reason: Option<TargetRejectionReason> }>`

**`pet_decision`** payload:

- Same shape as `ability_decision`, plus `owner: entity_id`, `pet_type: PetType`.

### Reason taxonomy (closed enum + structured fields)

`RejectionReason` is a Rust enum where each variant carries the relevant numbers. Adding a variant requires a code change + an `expected_reasons` audit-test pass (mirror of `expected_abilities`). Grep stays clean (`.reason.OutOfRange.distance`), and the writer never lies about *what number the predicate saw*.

Initial variants (additive):

- `OutOfRange { distance: f32, max: f32 }`
- `OnCooldown { remaining: f32 }`
- `InsufficientMana { have: f32, need: f32 }`
- `InsufficientResource { resource: ResourceKind, have: f32, need: f32 }`
- `SilencedOrLocked { school: SpellSchool }`
- `TargetImmune` / `TargetAlreadyCCd { cc_type: AuraType }` / `DRImmune { category: DRCategory }`
- `FriendlyBreakableCC` / `SelfIncapacitated`
- `LowerPriorityThanChosen { chosen: AbilityType }`
- `PreconditionUnmet { note: String }` — catchall; rare and explicit.

A separate `TargetRejectionReason` enum covers target-pick rejections (`OutOfRange`, `Stealthed`, `Dead`, `Immune`, `LowerScoreThanChosen`, `KillTargetOverride`, etc.).

### Trace emission

**Headless single match:**
- `--trace` flag enables emission. Writes `match_logs/match_<timestamp>_trace.jsonl` alongside the existing `.txt` log.

**Headless matrix:**
- **Always on by default.** Writes to `match_logs/traces/match_<seed>_trace.jsonl` (separate subdirectory keeps the .txt log dir browsable).
- `--no-trace` opt-out if matrix runs become unacceptably slow.
- Writer must be batched/buffered — at 4,900 matches × ~6 actors × ~50 ticks, we're looking at ~1.5M events per matrix run. Use `BufWriter`, serialize one line at a time, no pretty-printing.

**Graphical:**
- In-memory ring buffer (e.g., last 200 events per primary combatant, configurable). No disk write by default; manual export hotkey deferred to Phase 2 polish.

### Two detail modes

- **`--trace` (default).** Minimal payload — just the actor + target view + reason codes described above. Smallest files, fastest writer.
- **`--trace-verbose`.** Adds: full aura lists on actor and target; all alive enemies with position/hp/aura. Lets you reconstruct kite/range decisions without re-running. Roughly 6-10× the size; only use it for deep dives. Implemented as one branch in the event-builder.

### Out of scope (Phase 1)

- **F-key overlay** — Phase 2 (see Phasing below).
- **Movement/positioning decisions** — kiting paths, LOS breaks, retreat thresholds aren't in `decide_<class>_action`; instrumenting them is a separate effort.
- **Trace-driven replay** (regenerate the match from a trace file). Determinism+seed already gives this for free.
- **Persistent overlay history scrollback** — Phase 2 polish if needed.
- **Cross-frame queries** ("why didn't this ability *ever* come up?") — answer with `jq` over the JSONL.

### Explicit non-goals

- **Trace must not alter behavior.** No new RNG draws. No state mutation. Adding the trace must keep the matrix baseline byte-identical at the same seed (regression test — see Success criteria).

## Success criteria

1. **Reproducible diagnosis cycle.** After a matrix run, I can `jq` over `match_logs/traces/match_<seed>_trace.jsonl` to enumerate every Hunter ability decision in a known-broken matchup, group rejection reasons, and identify the top blockers — without re-running.

2. **Pet attribution.** I can join `pet_decision` events to their owner via `actor.entity_id == pet_decision.owner` and see how often the Felhunter chose to Spell Lock vs cleanse vs auto-attack. Same for Hunter pets and target switches.

3. **Target-pick visibility.** I can answer "did the Rogue ever consider attacking the Priest, and if so, why was the Paladin scored higher?" from `target_acquisition` events alone.

4. **Determinism preserved.** Matrix runner with and without `--no-trace` produces byte-identical match outcomes for the same seeds. Add this as an integration test (extension of the PR #48 determinism tests).

5. **Low instrumentation cost going forward.** A new ability added per CLAUDE.md's "Adding a New Ability" checklist requires zero trace-specific changes — the AI dispatch helpers handle it.

6. **Performance acceptable in matrix mode.** With trace always-on, a `--matrix 100` run is no more than 2× slower than `--no-trace`. (The writer is the bottleneck; aim for batched flushes per match.)

7. **Overlay round-trip (Phase 2).** Launch graphical client, run Warrior v Mage, hit F4, see the decision panel populate with the Warrior's per-tick candidate evaluation. Toggle off, panel disappears.

## Approach choice

**Locked in: instrument the existing if-chains in each class AI file** (Approach A).

Thread `&mut DecisionTrace` into `decide_<class>_action`, `acquire_targets`, and `pet_ai::decide_pet_action`. Each predicate that gates a cast pushes a `Rejected` entry carrying its numeric context; the chosen branch pushes `Chosen`. The existing `is_spell_school_locked`, `is_silenced`, cooldown, and mana checks become reason-emitting call sites.

*Why not refactor to a candidate-list scoring pattern (Approach B)?* It bundles in the "trim large class_ai files" refactor (#4 from the ideation doc), inflates blast radius, and risks behavior shift / determinism regressions. Approach A keeps the brainstormed feature contained. If the per-class instrumentation gets repetitive enough that B becomes obviously right, do it as its own refactor PR — but don't gate the trace work on it.

## Phasing

**Phase 1 — trace plumbing + emission (headless + graphical hot-path):**
- `DecisionTrace` builder API + `RejectionReason` / `TargetRejectionReason` enums.
- Instrument `decide_<class>_action` for all 7 classes.
- Instrument `acquire_targets` (target picks).
- Instrument `pet_ai::decide_pet_action`.
- CLI: `--trace` (single match opt-in), `--trace-verbose` (any mode), `--no-trace` (matrix opt-out).
- Matrix mode: always-on by default, writes to `match_logs/traces/`.
- Determinism integration test extension.
- Audit test: `expected_reasons` mirror of `expected_abilities`.

**Phase 2 — F-key overlay:**
- `GameAction::ToggleDecisionTraceOverlay` bound to **F4**.
- egui panel showing the currently-followed combatant's last-N decisions (target + candidates + reasons).
- In-memory ring buffer fed by the same `DecisionTrace` events.

**Phase 3 (deferred, optional):** click-to-select; trace-driven movement decisions; overlay export-to-disk hotkey.

Phase 1 is independently valuable — even with no overlay, `jq` over the JSONL plus the matrix runner unlocks Hunter rebuild (#10) and Paladin > Rogue diagnosis (#11) from the ideation doc.

## Dependencies and assumptions

- **Verified:** `bevy_egui` is already a workspace dep (used in `combat_ai.rs`, `rendering/hud.rs`, etc.). Adding another egui panel in Phase 2 is a known pattern.
- **Verified:** F1-F12 are mapped in `keybindings.rs::parse_key` but no `GameAction` currently uses an F-key — F4 is free.
- **Assumption:** `serde` + `serde_json` are acceptable to depend on. `serde` is already in the tree via RON loading (`ability_config.rs`); `serde_json` is the only marginal addition. Fallback: hand-rolled JSONL writer — events are shallow.
- **Verified:** the `CombatContext` snapshot already exposes everything the verbose mode needs (`combatants`, `active_auras`, `dr_trackers`) as `BTreeMap`s for determinism (per PR #48). No new data plumbing.
- **Constraint (from MEMORY.md):** Any new combat-affecting system has to be registered in BOTH `add_core_combat_systems` (headless) and `StatesPlugin::build` (graphical). The trace builder system (Phase 1) and overlay system (Phase 2) each need both registrations. The registration audit test will enforce this.

## Risks

- **RNG drift.** Highest risk. Mitigation: the trace builder owns no RNG state; reason determination uses only the values predicates already read. The determinism integration test (criterion 4) catches violations on every CI run.
- **Writer becomes the bottleneck in matrix mode.** Mitigation: `BufWriter`, batched flushes per match, no pretty-print. Profile a matrix N=100 run with and without trace; if >2× slowdown, revisit `--no-trace` as the matrix default.
- **Reason-code drift.** Mitigation: closed enum + `expected_reasons` audit test (mirror of `expected_abilities`). Refactor renames fail the build.
- **Pet AI instrumentation pulls in unfamiliar state.** Mitigation: `pet_ai::decide_pet_action` already has the same shape as class AI dispatch — same builder API works.

## Open questions (small, defer to implementation)

1. JSONL field naming — snake_case to match Rust idioms, or camelCase for `jq` ergonomics? *Lean snake_case; `jq` is fine either way.*
2. Should `ActionTaken` distinguish "started cast" from "instant cast that landed"? *Yes — single bool `was_instant`.*
3. Overlay default state at launch (Phase 2): on or off? *Off — match the existing aura-icon toggle.*
4. Ring buffer size for graphical mode: 200 per actor a good default, or memory pressure concerns? *Defer — profile first.*

## Handoff

Ready for `/ce-plan` to break Phase 1 into ordered tasks. Likely cut: writer module → builder API + reason enums → audit test → `decide_<class>_action` instrumentation (warrior first as a template, then propagate) → `acquire_targets` instrumentation → `pet_ai` instrumentation → CLI wiring → determinism test extension. Phase 2 (overlay) can be planned separately.
