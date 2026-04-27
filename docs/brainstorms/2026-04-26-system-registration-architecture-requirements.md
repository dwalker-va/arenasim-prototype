---
date: 2026-04-26
topic: system-registration-architecture
status: requirements
---

# System Registration Architecture (Narrowed Scope)

## Problem Frame

This document originally proposed a sweeping refactor of every system registration in `StatesPlugin`. Document review surfaced two strong objections: (1) the original "#1 silent-failure source" was already mitigated when `add_core_combat_systems()` landed, so the broad scope was not grounded in current pain; (2) UI states, lifecycle hooks, and icon loaders never had headless-classification ambiguity and were swept in for cosmetic completeness. The doc has been rewritten to address only the two pieces that are grounded in current evidence.

Two real, current pain points remain:

1. **The historical silent-failure bug class is mitigated by convention, not enforcement.** `src/states/play_match/systems.rs::add_core_combat_systems()` is the shared entry point for headless and graphical combat systems. But nothing prevents a future contributor from registering a new `process_*` or `*_system` combat function directly in `StatesPlugin::build()` and forgetting `add_core_combat_systems`. The pattern that prevents the bug is a written rule in `CLAUDE.md`/`MEMORY.md`, not a test.

2. **Visual-effect registration is fragmented and gotcha-laden.** `src/states/mod.rs` contains roughly 10 separate `.add_systems()` blocks for visual effects (healing light columns, dispel bursts, UA glow + backlash burst, drain-life beams, traps, ice block + slow zone, disengage trail + charge trail, polymorph cuboid, flame particles, etc.) — each repeating `.after(CombatSystemPhase::CombatResolution).run_if(in_state(GameState::PlayMatch))`. The 14 visual effects in `src/states/play_match/rendering/effects.rs` (~42 functions: spawn / update / cleanup) all follow the same lifecycle but are wired by hand, with 10 documented authoring gotchas (`docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`) — none of which are enforced structurally. Adding a 15th effect today re-pays the registration cost from scratch.

## Goals

- **G1.** Make the silent-failure bug class structurally enforced: a future contributor cannot add a Bevy system function under `src/states/play_match/` without registering it in either `add_core_combat_systems` (Both-mode) or `StatesPlugin::build()` (graphical-only) or the non-system allowlist. Enforced by a signature-based source-scan test (`cargo test`) that walks system-shaped `pub fn` items and asserts each appears in a registered set. The test is name-agnostic (does not depend on `process_*` / `*_system` naming conventions) so it catches combat systems regardless of how they are named.
- **G2.** Reduce visual-effect registration to a single declaration per effect, encoding the registration-shape gotchas (phase placement, run condition, cleanup marker, tuple-size batching) as defaults the author cannot forget. Authoring-body gotchas (alpha mode, `Res<Time>` choice, `try_insert`, `Without<T>` filter, positioning) remain documented conventions; the registration mechanism does not enforce them.
- **G3.** Preserve all existing combat behavior, visual behavior, and test outcomes. This is a refactor, not a rewrite of gameplay.

## Non-Goals

- **NG1.** Not introducing a `RunMode` tag on every system, a registration audit registry, or build-time/runtime classification panics. The original silent-failure bug class is closed by G1's narrow lint test, not by tagging infrastructure.
- **NG2.** Not migrating UI state systems (`MainMenu`, `ConfigureMatch`, `ViewCombatant`, `Results` Update systems), `OnEnter`/`OnExit` lifecycle hooks, or icon loaders to a new registration model. They exhibit none of the documented pain.
- **NG3.** Not changing how phase ordering or `add_core_combat_systems` is structured. The `.chain()`-based ordering and inline comments stay as-is. `CombatSystemPhase` remains the three-set enum.
- **NG4.** Not introducing a custom DSL, proc-macro, or new dependency. NG6 (no new dependencies beyond `bevy` and existing project crates) from the original draft is preserved.
- **NG5.** Not touching the headless runner's call site (`src/headless/runner.rs`). It continues to call `add_core_combat_systems` directly.
- **NG6.** Not pursuing a line-count target for `StatesPlugin::build()`. The aesthetic argument was scope-creep.
- **NG7.** Not changing Bevy's underlying scheduler or schedule semantics. The new layer sits on top of `app.add_systems(...)`.

## Requirements

### Part A — Combat Registration Audit Test (G1)

The previous draft used a name-based regex scan (`fn process_*` / `fn *_system`). Review pass 2 demonstrated that approach silently misses real combat systems whose names don't match the prefix/suffix convention (`update_countdown`, `regenerate_resources`, `apply_pending_auras`, `acquire_targets`, `move_projectiles`, `combat_auto_attack`, etc.). Part A is now signature-based and reverse-directional.

- **A1.** Add a `cargo test` (in `tests/registration_audit.rs`) that:
  - **(a)** Reads `src/states/play_match/systems.rs` and extracts the set of system identifiers registered inside `add_core_combat_systems`. Call this the **core-registered set**.
  - **(b)** Reads `src/states/mod.rs` and extracts identifiers passed to `app.add_systems(...)` inside `StatesPlugin::build()` — including identifiers behind the `play_match::` re-export path. Call this the **graphical-registered set**.
  - **(c)** Walks `src/states/play_match/**/*.rs` and identifies every `pub fn` whose argument list contains at least one Bevy system parameter type (`Query`, `Res`, `ResMut`, `Commands`, `Local`, `EventReader`, `EventWriter`, `Time`, `Assets`, `AssetServer`, `EguiContexts`, etc.). Call this the **candidate set**.
- **A2.** For each function in the candidate set, the test asserts it appears in the core-registered set OR the graphical-registered set OR an explicit allowlist (A3). The check is name-agnostic; renaming a system without updating its registration is caught because the function still matches the system signature.
- **A3.** The test must include an explicit allowlist of `pub fn` items that match the system-parameter signature but are NOT registered as systems (for example, helper functions called from inside system bodies, AI decision functions invoked indirectly, or systems registered via a future `VisualEffectPlugin` that the source-scan does not directly see). Each allowlisted entry must include a one-line justification comment naming where the function is invoked.
- **A4.** The test must fail with a clear error message naming the offending function, the file path, and instructing: "register `<function>` via `add_core_combat_systems` in `systems.rs` (for systems that run in both headless and graphical modes), or via `StatesPlugin::build()` in `states/mod.rs` (graphical-only), or add to the allowlist in `tests/registration_audit.rs` with a justification." Reference this requirements doc.
- **A5.** Update `CLAUDE.md` (under "Common Tasks" or a new "Adding a New Combat System" subsection) to point at the test and explain when each registration path is the correct one.
- **A6.** The test must be implementable without adding new project dependencies. Source scanning may use `std::fs` plus simple regex parsing (the project already has `regex` transitively via Bevy). It does not need to parse Rust syntax exhaustively — the goal is detection of orphan systems, not perfect AST analysis. Comment-stripped text scanning is sufficient.

### Part B — Visual Effect Plugin (G2)

#### B0 — Pre-Implementation Prototype (Q2 elevated)

- **B0.** Before B1's API is finalized, implement healing light columns (the cleanest existing spawn/update/cleanup trio in `rendering/effects.rs`) under each of three candidate API shapes — (i) one Bevy `Plugin` per effect type with `impl Plugin for HealingLightPlugin`, (ii) a generic `VisualEffectPlugin<T: VisualEffect>` driven by an associated-type trait, (iii) a builder-style helper such as `register_visual_effect(app, ...)`. Each prototype is ~30 lines of real code. Confirm composition with Bevy 0.15's `IntoSystemConfigs<Marker>` (the marker generic is the failure mode reviewers flagged), measure boilerplate-vs-overrides ergonomics, and commit the chosen shape into B1 before any further migration. This step is NOT deferrable to planning — B4, B5, and SC1 are unverifiable until the API shape is fixed.

#### B1-B8 — Plugin Mechanism

- **B1.** Define a single registration mechanism (the API shape chosen in B0) that bundles the spawn / update / cleanup systems for one visual effect and registers them with default phase placement (`after(CombatSystemPhase::CombatResolution)`), default run condition (`in_state(GameState::PlayMatch)`), and the `PlayMatchEntity` cleanup marker. The mechanism must support effects whose lifecycle is NOT a clean spawn/update/cleanup trio: floating combat text spawns per damage tick and self-expires (no separate cleanup); compound effects like "UA glow + backlash burst" or "trap visuals + trigger bursts + launch visuals" have multiple spawns and multiple updates. The API must accept N spawn / M update / K cleanup systems per effect, not exactly one of each.
- **B2.** The mechanism must internally batch its registrations to dodge Bevy 0.15's 20-element `IntoSystemConfigs` tuple limit. Author-facing API does not need to know about this batching.
- **B3.** Migrate all existing graphical-only visual effects currently registered in `StatesPlugin::build()` to use the new mechanism. The known groups span both `src/states/play_match/rendering/effects.rs` (~50 functions) and `src/states/play_match/shadow_sight.rs` (orb animation systems): healing light columns, dispel bursts, UA glow + backlash burst, drain-life beams, trap visuals + trigger bursts + launch visuals, ice block + slow zone, disengage trail + charge trail, polymorph cuboid, flame particles, shadow sight orbs, shield bubbles, spell impact effects, floating combat text. The exact effect-to-function mapping table — listing which spawn / update / cleanup functions belong to each effect, where they live, and whether they need non-default ordering — must be produced as a planning prerequisite (Q2 below) and committed to the requirements doc before migration begins. Systems that affect game state (`check_match_end`, `trigger_death_animation`, `update_victory_celebration`, `update_speech_bubbles`, `update_stealth_visuals`) are NOT visual effects and stay in their current registration form.
- **B4.** A migrated visual effect's registration site must read as a single declarative call — no inline `.after()` / `.run_if()` / `.in_set()` chains visible in `StatesPlugin::build()` for the default case.
- **B5.** Effects that need non-default ordering must express it via `SystemSet` references only, never via direct symbol references to functions in the core combat module. The current case (`spawn_projectile_visuals` running between `process_channeling` and `move_projectiles` inside `CombatAndMovement`) is resolved by introducing a finer-grained sub-set inside `CombatAndMovement` — e.g., `CombatSystemSubPhase::ProjectileSpawning` placed between casting and movement — and having the visual-effect plugin's override target that set, not the function symbols. This avoids the cross-module symbol coupling that previously made R12 cross-mode ordering infeasible.
- **B5a.** Define any new sub-sets needed to resolve B5 in the same source file as `CombatSystemPhase` (`src/states/play_match/systems.rs`). Configure their ordering inside `configure_combat_system_ordering`. The visual-effect plugin's override API takes `impl SystemSet`, not function pointers.
- **B6.** The visual-effect mechanism must NOT attempt to enforce body-of-system gotchas (`AlphaMode::Add` over `Blend`, `Res<Time>` over `Time<Real>`, `try_insert` over `insert`, `Without<T>` filter on second `Transform` query, chest-height positioning). These remain documented authoring conventions in the existing `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`. The mechanism enforces only the registration-shape defaults listed in B1.
- **B7.** After migration, `StatesPlugin::build()` must contain at most one `app.add_plugins((VisualEffect1, VisualEffect2, ...))` call covering the visual-effect family, replacing the ~10 fragmented `.add_systems()` blocks. Bevy `add_plugins` also has a 20-tuple limit; multiple `add_plugins` calls are acceptable if needed.
- **B8.** Update `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` to (a) describe the new plugin API as the canonical authoring path, (b) keep the body-of-system gotchas as conventions the author still must follow, (c) note which gotchas are now enforced as defaults.
- **B8a.** The plugin API must carry a rustdoc comment that surfaces the body-of-system gotchas at the call site — `AlphaMode::Add` not `Blend`, `Res<Time>` not `Time<Real>`, `try_insert` not `insert`, `Without<T>` filter on the second `Transform` query, chest-height positioning. The rustdoc must explicitly state that the plugin does NOT enforce these and link to `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`. The goal is to prevent false confidence — a contributor who reads only the API docs at the call site must still see the gotcha list.

### Behavior Preservation (G3)

- **C1.** All existing tests must pass without modification: `tests/ability_tests.rs`, `tests/combat_log_tests.rs`, `tests/headless_tests.rs`, and the system phase ordering test in `src/states/play_match/systems.rs::tests`.
- **C2.** A representative graphical match must visually pass a manual eyeball check covering every visual effect touched by B3's migration: gate animation, all four resource bars, floating combat text, dispel burst, drain-life beam, hunter trap (placement + arming + trigger + burst), frozen target with ice block, polymorphed cuboid, UA glow + backlash burst, healing light columns, disengage trail, charge trail, slow zone disc, flame particles, shadow sight orbs (pulsing + consumption), shield bubbles, spell impact effects, death animation, and victory celebration. Earlier draft of this requirement covered only ~11 visuals; expanded to match B3 because every migrated effect carries silent-regression risk if its registration drops a system.
- **C3.** No statistical baseline / reference-suite comparison this round. Without seeded determinism (idea #3 in `docs/ideation/2026-04-26-open-ideation.md`), tolerance-based comparisons either flake or hide regressions.

## Open Questions (defer to planning)

- **Q1.** Implementation detail of A1's signature detection — which Bevy parameter types comprise the "is a system?" predicate? Start with `Query`, `Res`, `ResMut`, `Commands`, `Local`, `EventReader`, `EventWriter`, `Time`, `Assets`, `AssetServer`, `EguiContexts`, but walk the actual `add_core_combat_systems` body during planning and verify every registered system is detected by the predicate. Tune the parameter list once based on what's actually used.
- **Q3.** How does B5's `SystemSet`-based override API compose with B1's defaults — fields on a config struct, builder methods, or per-effect `impl` overrides? Determined by B0's chosen API shape.

(Q2 was elevated to B0 — pre-implementation prototype — because it is load-bearing for B1, B4, B5, and SC1.)

## Success Criteria

- **SC1.** Adding a 15th visual effect requires editing one file and writing one declaration; no new `.add_systems()` block in `states/mod.rs`.
- **SC2.** Renaming an existing `process_*` combat system without updating `add_core_combat_systems` causes the A1 test to fail in `cargo test`. Verified by deliberately seeding the regression once during development.
- **SC3.** Removing the call to `add_core_combat_systems` from `StatesPlugin::build()` (simulating an accidental delete) causes A1 to fail. Verified by deliberately seeding once.
- **SC4.** All C1 tests pass.
- **SC5.** The C2 manual eyeball check passes — all listed visuals appear and behave as before.

## Scope Summary

- **In scope this round:** Combat registration audit test (A1-A6) + visual-effect plugin mechanism (B0-B8a) + behavior preservation (C1-C3).
- **Explicitly deferred:** RunMode tagging, registration audit registry, UI/icon-loader/lifecycle migration, declarative phase ordering, cross-mode ordering semantics, Bevy version-coupling design. None of these address current evidence-grounded pain.
- **Out of scope this round (separate ideas):** seeded determinism (idea #3), unified DamageEvent pipeline (idea #2), property-based invariant tests (idea #4), declarative AI behavior trees (idea #5), AI decision trace + inspector (idea #6), codegen for AbilityType (idea #7).

## Delivery Plan

Part A and Part B have no technical coupling — A scans source for orphan systems, B refactors visual-effect registration. They should ship as **independent PRs**, in order:

1. **PR 1 — Part A only.** Implement A1-A6 + C1 (existing tests pass). No behavioral change. Small surface, high confidence, immediate guard against the silent-failure bug class. Roughly half a day.
2. **PR 2 — Part B prototype (B0).** Implement healing light columns under all three candidate API shapes, choose one, commit B1's API into the requirements doc. Half a day.
3. **PR 3 — Part B migration.** B1-B8a + C2 manual eyeball + C3 statement. Multi-day.

Bundling A and B into one PR is explicitly disallowed — the smaller, lower-risk Part A should not be blocked on the larger Part B's planning prototype.

## Session Log

- 2026-04-26: Initial brainstorm. Original scope was "every system in `StatesPlugin`" with `RunMode` tagging, registration audit, and full migration of UI + icon loaders.
- 2026-04-26: Document review pass 1 (5 personas) flagged the scope as unjustified by current pain — the historical bug class was already mitigated by `add_core_combat_systems`, and UI/icon-loader migration had no pain evidence. Multiple reviewers recommended a narrower split.
- 2026-04-26: Rewritten to focus on two evidence-grounded pieces: a `cargo test` lint and a visual-effect plugin mechanism. Roughly 5% of the original scope, addressing close to 100% of the documented current pain.
- 2026-04-26: Document review pass 2 confirmed the strategic narrowing succeeded but flagged five implementation gaps: A1's regex missed real combat systems, Q2's plugin API shape was load-bearing not deferrable, B5's non-default ordering risked re-introducing cross-module symbol coupling, body-of-system gotcha enforcement was invisible at the call site, and Part A + Part B should ship independently.
- 2026-04-26: Refined to address all five: A1 rewritten as signature-based reverse-direction scan (A1-A6); Q2 elevated to B0 pre-implementation prototype; B5 constrained to `SystemSet`-only references with sub-set introduction (B5a); B8a added for rustdoc gotcha surfacing; Delivery Plan section added separating PR 1 (Part A) from PR 2/3 (Part B prototype + migration).
