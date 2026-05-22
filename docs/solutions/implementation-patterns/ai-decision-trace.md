---
title: "AI Decision Trace: builder pattern + closed-enum audit + determinism discipline"
tags:
  - observability
  - ai
  - class-ai
  - decision-trace
  - jsonl
  - determinism
  - bevy-resource
category: implementation-patterns
module: states/play_match/decision_trace
symptoms:
  - "Need to add a new ability or class AI branch and have it automatically traced"
  - "Need to diagnose why an AI didn't cast a specific ability"
  - "Need to add a new RejectionReason variant"
  - "Trace events seem to differ between runs at the same seed (non-determinism)"
severity: low
date: 2026-05-21
---

# AI Decision Trace

## What it is

A JSONL stream of AI decisions, captured per actor per AI tick. Each event
records who decided, what they targeted, what abilities they considered, and
a typed rejection reason (with numeric context) for every candidate that
lost. Three event kinds emit through the same `DecisionTrace` Resource:

- `ability_decision` — emitted from `decide_<class>_action` for all 7 classes
- `target_acquisition` — emitted from `acquire_targets` on target changes
- `pet_decision` — emitted from `pet_ai_system` for Felhunter / Spider / Boar / Bird

Enable with `--trace-mode on` (single match) or rely on the matrix default
(`on`). See `CLAUDE.md` → "Diagnose AI behaviour with the decision trace" for
common `jq` recipes.

## Architecture

`src/states/play_match/decision_trace/` is a 4-file module:

- `events.rs` — serializable event types: `DecisionEvent`, `EventKind`,
  `RejectionReason`, `TargetRejectionReason`, `ResourceKind`, plus the
  `ActorView`/`TargetView`/`AbilityCandidate`/`TargetCandidate` value types.
- `builder.rs` — the `DecisionEventBuilder` (for ability + pet decisions)
  and `TargetEventBuilder` (for target_acquisition). Both borrow
  `&mut DecisionTrace` and push the final event on `.finish()`.
- `writer.rs` — `TraceWriter` (BufWriter over the JSONL file) with a
  canonical sort on `(frame, actor.entity_id, kind)` before flush;
  `flush_decision_trace_system` runs in Phase 3 each frame.
- `mod.rs` — the `DecisionTrace` Resource, builder constructors,
  `ActorView::from_info` / `from_raw`, `TargetView::from_info` helpers.

`DecisionTrace` is registered via `app.init_resource::<DecisionTrace>()`
inside `add_core_combat_systems`, so both headless and graphical modes have
it. The writer is installed only when `--trace-mode` is non-`off` (single
match) or in matrix mode by default; without a writer the builder remains
active but events are drained and discarded each frame.

## Pattern for class AI instrumentation

Every `decide_<class>_action` takes a `&mut DecisionTrace` and follows this
shape (Warrior is the canonical reference):

```text
1. GCD short-circuit returns false without emitting (emission gate — no
   decision was produced this tick).
2. Build ActorView from ctx.self_info(), TargetView from combatant.target.
3. Start a builder: decision_trace.start_ability_decision(actor, target).
4. Thread `&mut builder` into every try_* helper.
5. Each try_* helper:
   - At every predicate gate that returns false: call
     `builder.reject(ability, RejectionReason::...)`.
   - For pre_cast_ok failures, call
     `classify_pre_cast_failure(ability, def, ...)` from cast_guard.rs to
     get the typed reason.
   - On the success branch: call
     `builder.choose(ability, Some(target), was_instant_bool)`.
6. After every priority chain (or branch), `builder.finish()` commits the
   event. Empty builders are dropped — no event emitted (emission gate).
```

`finish_no_action(NoActionReason)` is the variant when a top-level skip
(e.g., TargetImmune at the outer dispatch) decides the outcome explicitly.

## The closed-enum audit discipline

`RejectionReason` and `TargetRejectionReason` are closed enums. `Cargo
test --test decision_trace_audit` runs 6 reference matchups (4 × 1v1 across
all 7 classes + 2 × 2v2 for multi-actor variants) and asserts:

1. Every variant in `EXPECTED_REJECTION_REASONS` is emitted at least once
   (catches dead variant declarations).
2. Every variant emitted in production appears in the expected list
   (catches typos and out-of-band emissions).

**To add a new variant:**

1. Add the variant to `RejectionReason` in `events.rs`.
2. Emit it from at least one class AI's reject site.
3. Run `cargo test --test decision_trace_audit`. If a reference matchup
   doesn't reliably produce the condition that fires your variant, either
   add a matchup that does, or document it in the `// Variants NOT in this
   list` comment block in `tests/decision_trace_audit.rs`.

## Determinism discipline (critical)

The trace must not perturb gameplay. Two integration gates protect this in
`tests/headless_tests.rs`:

- `trace_on_matches_trace_off_outcomes` (always-run): Warrior v Mage × 3
  seeds, asserts MatchResult byte-equality with vs without trace.
- `trace_on_matches_trace_off_all_class_pairings` (`#[ignore]`'d): same
  assertion across all 49 1v1 pairings.
- `trace_file_deterministic_all_class_pairings` (`#[ignore]`'d): two
  trace-on runs at the same seed must produce byte-identical JSONL files.

Run via `cargo test --release -- --ignored`.

### Lesson learned: iteration order matters

Rust's default `HashMap`/`HashSet` uses `RandomState` (entropy-seeded
hasher). Iteration order varies across runs even within the same process.
Any collection in the combat hot path that gets **iterated** must be a
`BTreeMap`/`BTreeSet`, not `HashMap`/`HashSet`. Lookup-only collections
(`.get()`, `.contains_key()`) can stay HashMap.

PR #48 (`CombatSnapshot` HashMap → BTreeMap) addressed AI decisions but
missed the combat-resolution path. This work's all-pairings determinism
sweep exposed two more sites that needed conversion:

- `combat_core/auto_attack.rs::frost_armor_procs` — drove
  `commands.spawn(AuraPending)` calls, where call order determined entity
  ID allocation and rippled into downstream query iteration. Fixed by
  switching to `BTreeSet<Entity>`.
- `auras.rs::apply_pending_auras::new_auras_map` — drove ActiveAuras
  component insertion order. Fixed by switching to
  `BTreeMap<Entity, Vec<Aura>>`.

When adding new combat collections that get iterated, default to
BTree variants. Add a comment naming the rationale (see existing comments
in `auto_attack.rs` and `auras.rs` for the canonical phrasing).

## Variant-to-predicate map (selected)

Use this as a starting point when adding instrumentation to a new ability:

| Predicate | Variant | Notes |
|---|---|---|
| `combatant.global_cooldown > 0` | _no event_ | Outer GCD check — emission gate |
| `ability already applied (DoT / shield / aura)` | `AlreadyApplied` | Corruption, UA, Immolate, Ice Barrier, Power Word: Shield, etc. |
| `target distance > def.range` | `OutOfRange { distance, max }` | classify_pre_cast_failure handles this |
| `target distance < def.min_range` | `WithinDeadZone { distance, min }` | Hunter Aimed Shot at <20 yards, Warrior Charge < CHARGE_MIN_RANGE |
| `ability_cooldowns.get(ability).is_some()` | `OnCooldown { remaining }` | Read remaining from the map directly |
| `combatant.current_mana < def.mana_cost` (Warrior) | `InsufficientResource { resource: Rage, ... }` | classifier picks variant by `caster.class` |
| `combatant.current_mana < def.mana_cost` (Rogue) | `InsufficientResource { resource: Energy, ... }` | classifier picks variant by `caster.class` |
| `combatant.current_mana < def.mana_cost` (mana classes) | `InsufficientMana { have, need }` | Default for Mage/Priest/Warlock/Paladin/Hunter |
| `is_spell_school_locked(def.spell_school, auras)` | `SilencedOrLocked { school }` | classify_pre_cast_failure handles this |
| `is_silenced(combatant, auras) && def.mana_cost > 0` | `SilencedOrLocked { school }` | UA backlash, Spell Lock |
| `ctx.has_friendly_breakable_cc(target)` | `FriendlyBreakableCC` | Charge / Frostbolt / Corruption against own team's Polymorph |
| `ctx.is_dr_immune(target, category)` | `DRImmune { category: "Incapacitates" \| "Stuns" \| "Fears" }` | Mage Poly, Rogue Kidney Shot, Warlock Fear |
| `auras has target's CC aura` | `TargetAlreadyCCd { cc_type: AuraType }` | Don't waste Polymorph on already-stunned targets |
| `is_rooted (Root aura on self)` | `Rooted` | Warrior Charge, Hunter Disengage |
| `no target available` | `NoValidTarget` | Fortitude needs an ally, Dispel needs a debuffed ally, etc. |
| `pre_cast_ok returned false for unknown reason` | `PreconditionUnmet { note }` | Fallback when no specific variant fits — note explains |

## Surfaces you must touch when adding a new combat system

The trace flush system is dual-registered (see `tests/registration_audit.rs`).
If you add a new system that runs combat logic, follow the existing dual
registration pattern in `add_core_combat_systems` — see
`docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`.

## Related

- `docs/plans/2026-05-18-001-feat-ai-decision-trace-plan.md` — the
  implementation plan, with Implementation Units U1-U12.
- `docs/brainstorms/2026-05-18-ai-decision-trace-requirements.md` — origin
  requirements, with the closed-enum + structured-payload decisions.
- `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`
  — the dual-registration discipline this work uses.
