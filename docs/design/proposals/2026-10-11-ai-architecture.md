# Proposal: testable AI iteration, and AI that adapts to the situation

**Card:** AS-250. **Status:** PROPOSAL, awaiting a decision (see the last section).
**Date:** 2026-10-11.

## The recommendation, on one page

Parts 1 and 2 share one root cause. **The AI has no first-class notion of "the
situation".** The class AI recomputes situations ad hoc inside each class, and
the tests stand in a seed for them. One new layer fixes both parts.

**Part 2: build a Situation layer, and keep the priority lists.**
- A deterministic, per-team read model (`Situations`) is computed once per frame
  from the snapshot every unit already decides from.
- It names a small, closed set of situations. Each has an onset time, a duration
  and hysteresis: *enemy healer locked*, *kill window*, *ally in danger*,
  *pressing / trailing*, *match phase*.
- Class AI keeps its hand-ordered `try_*` chains. Each class chooses its chain by
  the situation it is in. Call that pairing a *play*.
- Utility scoring is allowed **only inside one step of a play**: which ally to
  heal, which direction to step, which aura to dispel. Even there it must pass an
  explicit admission test: the quantities are commensurable and predictable.
- Not a global utility AI. Not behaviour trees. Not GOAP/HTN.

**Part 1: specify AI behaviour as scenes, not seeds.** This is a testing pyramid.
Tier 1 exists in pieces today; tiers 2–4 are new:

| Tier | Cost | What it is |
|---|---|---|
| 1 | ms | **Scene tests.** A constructed situation runs the real class decision for one tick and asserts what it chose and why. |
| 2 | s | **Decision properties.** Invariants checked over a generated grid of scenes. |
| 3 | s | **Staged matches.** The real schedule runs from a constructed start, for behaviours that need time (walks, chases, escapes). |
| 4 | min | **Situation-conditional probes.** These run over a seed *bank*. The classifier finds the situation; the probe asserts a response *rate* in it. They replace seed pins. |

Paired sweeps stay. Their job narrows to magnitude, at the milestone.

**Why this converges where the scoring attempt did not.** The CC value model
stalled for three reasons:
- it priced quantities that are either not commensurable (a cooldown's option
  value against mana) or not predictable (Polymorph's real duration);
- each hypothesis could only be judged by an n=300 win-rate sweep;
- its flags stacked.

The Situation layer answers each one:
- It asks discrete questions the sim can answer: is the enemy healer locked, and
  until when?
- A scene test checks each change in milliseconds before any sweep runs.
- Plays ride the one existing `AiProfile` gate, under a graduate-or-delete rule.

**First card:** "Situation v0". Build `Situations` holding two situations:
*enemy healer locked*, with its `until`, and *pressing/trailing*. Port the four
places that already compute them ad hoc onto it, byte-identically under
`Legacy`. Land the scene harness alongside.
- **Success:** `Legacy` byte-identical. Every ported site is pinned by a scene
  test that kills a mutant the default `cargo test` lets survive today. For the
  Warrior's site this is already measured: deleting its burst window survives
  all 2,085 default tests, and leaves 300 paired matches byte-identical (§3.3).
- **Debuggability check:** one `jq` query answers "what did each class do while
  the enemy healer was locked".
- **Failure modes:** it disproves the approach if the port cannot be made
  byte-identical without contortion, or if a scene costs more to write than
  the seed probe it replaces.

**Decisions asked of you** are at the end. There are four. The main one is
whether to adopt the Situation layer as the direction for 0.8's AI work.

---

## 1. What exists today

This section is short and factual. It covers what the proposal builds on.

**Decision shape.** Each class exposes a free function: `decide_warrior_action`,
`decide_priest_action` and so on, in `src/states/play_match/class_ai/<class>.rs`.
- Each is a hand-ordered chain of `if try_<ability>(...) { builder.finish();
  return true; }`.
- `decide_abilities` (`combat_ai.rs:726`) calls them through an exhaustive class
  `match` (`combat_ai.rs:1109-1507`). It runs every fixed tick for every unit not
  casting.
- Gates are shared through `cast_guard::pre_cast_ok`. Every gate that refuses
  records a typed `RejectionReason` on the trace builder.
- A decide function **decides and executes in one pass**. It does all of the
  following:
  - mutates `&mut Combatant` (mana, GCD, cooldowns);
  - spawns `CastingState` / `AuraPending` through `Commands`;
  - writes `CombatLog`;
  - for Warrior, Mage, Rogue and Hunter, rolls crits on `GameRng`.

**Situational logic exists, scattered and recomputed per caller.** Examples:

| Situation | Where it is computed today |
|---|---|
| Enemy healer locked | `CombatContext::enemy_healer_is_cced` (`class_ai/mod.rs:403`). Read by the Warrior burst window (`warrior.rs:129/172`), the Hunter burst window (`hunter.rs:456`), and the Priest Mana Burn CC window (`priest.rs:1336`). |
| Pressing / trailing | `pressing_when_ahead(team_hp_advantage, press_advantage_margin)`, read by the healer postures and `evaluate_warrior_reset`. |
| Ally in danger | `teammate_needs_saving`, `another_teammate_dying`, `lowest_health_ally_below`, `compound_pressure_trigger`, `druid::focused_ally`. Each uses its own threshold. |
| Match phase | The Druid mana governor reads dampening time. `TeamPlans.contact` latches first contact. |

None of these is:
- named in the trace;
- latched with hysteresis, except where a class hand-rolled it (`KitePosture::wand_oom`);
- tested as a classification separately from what it drives.

`tests/movement_probes.rs:3186` does test `enemy_healer_is_cced` as a predicate.
**No test pins what the Warrior does with it.** §3.3 measures this.

**The layers above the unit.**
- `TeamPlan` / `team_solve` (`team_plan.rs`, `team_solve.rs`) is a constraint
  solve over positions.
- It is live only for healer `OccupyCover`, plus `DispelGoal` and `C_DYING`.
  Its `kill_target` is computed and read by nothing.
- `AiProfile::{Legacy, TeamPlan}` (`ai_profile.rs`) is the per-team gate.
  No ability-priority code branches on it today.

**Testing today.**
- `tests/movement_probes.rs`: 8,121 lines, 122 tests, about 48 literal seeds, 11
  seed constants, and 12 `scan_*seed*` scanners.
- `tests/druid_control_probes.rs`: 22 `(comp, map, seed)` tuples.
- `determinism_pin`, re-recorded six times.
- 65 in-code re-pin/re-seed comments across `tests/`.

Cheap single-decision tests already exist, but there are few of them and no two
share a fixture:
- `reach_fixture::ReachScene` (`class_ai/mod.rs:~2420`, `#[cfg(test)]`, so
  invisible to `tests/`) runs one `try_*`;
- `tests/druid_decisions.rs` runs the *whole* `decide_druid_action`, at about 90
  lines of setup per scene;
- `tests/class_ai_decisions.rs` has 75 predicate tests on a hand-built
  `CombatSnapshot`.

## 2. What was tried, and why it stalled

Four scoring-shaped attempts were found. Two shipped and two stalled. The two
that shipped tell us as much as the two that stalled.

1. **Context-steering movement scorer.** Shipped and live:
   `combat_core/movement_scoring.rs`, with weights in `movement.ron`.
   - **Why it works:** every term is in one currency, which is how good a step is
     in a direction. The candidates are 16 directions re-scored every commit
     window. Hard constraints are boolean masks, not weights (PR #69).
   - **Where it stopped:** `cover_pull` is a 2yd local gradient, not navigation.
     Warlock ENGAGE/KITE cost about 7pt and was reverted (PR #72).
2. **Gate-and-rank utility re-sort for ability choice.** Ideation #1 in
   `docs/ideation/2026-06-12-combatant-ai-ideation.md`, ranked first at 85%
   confidence.
   - It was never built.
   - Its named downside was "Large migration; needs a
     weights-reproduce-old-ordering equivalence stage before tuning."
3. **The CC value model.** This is the scoring system the card means. Its shape:
   - Branch `feat/cc-value-model`, PR #105. Files: `cc_value.rs` (1,345 lines)
     and `design-docs/cc-value-model.md` (2,273 lines).
   - It priced crowd control as `I + D x T_eff + E - C`, gated by
     `CcPolicy::{Identity, Priced}` per team, plus `interrupt_policy`, plus
     `MAGE_PRICED_POLYMORPH`.

   Results:
   - Warlock: +9pt (z=2.43).
   - Mage, across seven matchups: +83 / +3 / +1 / -1 / -5 / -12 / -9.
   - Frost Nova: three pricing attempts all lost to "cast it when something is
     on me".
   - Interrupt pricing: -5pt against "interrupt the healer".

   The PR was closed unmerged on 2026-09-06: *"had some promise but need to find
   a more scalable way to address the issue at scale, plus it was getting too
   complicated stacking multiple levels of feature flags."*
4. **The TeamPlan positional solve.**
   - **Healer half:** shipped, and worth +36pt to Warlock+Priest at n=100.
   - **Kiter half:** a ring constraint cost about 17pt and was reverted. A
     kiter's `flee` maximises distance, and a ring constraint is satisfied the
     moment the kiter reaches it.
   - **Step 5's held team kill target:** refuted. "EVERY static held call was
     catastrophic for at least one side of a matchup while helping its mirror."

**My reading: four causes, and only the first is about scoring.**

1. **No common currency.**
   - Utility AI needs every option priced on one scale.
   - Movement has one scale (step quality) and the scorer works there.
   - Ability choice in this sim does not have one. Mana is consumable and
     priceable. A 25s cooldown's option value is not. Neither is damage
     *avoided* as opposed to deferred (the Frost Nova finding), nor the
     superlinear value of a chain CC (`cc-compounds-not-adds`).
   - Forcing these onto one axis is where the weight-tuning swamp comes from.
2. **Estimates with high variance, picked by argmax.**
   - The value model's own admission criterion found the cause: Polymorph's real
     duration is decided by things the model cannot see, since 45% end in an
     instant, free Devour Magic.
   - Taking the argmax of a noisy estimate selects for the cases where the
     estimate is most over-optimistic. That is the optimizer's curse, the
     doc's own diagnosis.
   - A crude rule keyed on a *fact* ("something is on me", "the healer is
     sheeped") has no estimate to be wrong about.
3. **The only oracle was a win-rate sweep.**
   - Six Mage attempts, each "individually justified", were each judged at
     n=300. Days of wall clock went into answering questions that a scene
     test could have answered first.
   - For example: *in this situation, does the priced Mage sheep the healer or
     the Warrior?* That is a millisecond scene test, not a sweep.
   - Nothing between "the code compiles" and "the sweep came back" said whether
     a change did what its author meant. This is the Part 1 problem, and it was
     the rate-limiter on the Part 2 attempt.
4. **Flags multiplied.**
   - `CcPolicy` x `interrupt_policy` x `MAGE_PRICED_POLYMORPH` x `AiProfile`,
     with a +12pt Warlock change bundled with a -7pt Mage change under one flag.
   - The user named this directly.
   - It is a process failure, but the architecture invited it. Each priced
     decision was a separate model, so each needed its own switch.

**What won, every time, was a crude adaptive rule keyed on a discrete fact.**
- burst while the healer is CC'd;
- interrupt the healer;
- Nova when something is on me;
- per-unit target acquisition re-evaluated every tick (step 5's finding: it
  ADAPTS, and held calls do not).

This proposal stops fighting that result and builds on it. Make the facts
first-class, make them adapt, and make them testable.

## 3. Part 1: making AI iteration testable

### 3.1 The problem in one sentence

**A seed is a proxy for a situation, and proxies drift.**
- A movement probe does not want "Warrior+Paladin vs Mage+Druid seed 1". It
  wants a Druid whose Mage is dying behind a pillar.
- AS-247's re-seed commit (`f621050`) says it outright: the repro moved to "the
  one seed found that still walks the Druid to its dying Mage round a pillar".
- Every balance-relevant change moves the RNG stream. That moves which seeds
  contain the situation, which means a re-pin. AS-240 alone re-pinned three
  suites.

AS-116's catalogue is the symptom list:
- floors fitted to the seed set (`WALL_PIN_YARDS = 6` fails 2 of 70 windows
  outside its sample);
- a 10s time-to-heal ceiling that "holds at the seeds it is pinned to and
  nowhere in particular";
- a dead seed in `SHIFT_MATCHES`;
- `druid_dying_heal` silently no longer guarding what it names after a seed
  moved;
- mutants that survive whole suites. AS-200's flee-on-every-escape mutant
  survives all 16 Druid probes yet changes 169 of 200 grid matches.

A seed-pinned probe has two weaknesses:
- It cannot say what situation it is about, so a reviewer cannot check that it
  still contains one.
- It samples the situation once or twice, so its floor is fitted to a handful of
  windows.

The fix is to **construct** the situation (tiers 1–3) or **detect** it (tier 4).
Never stand in a seed for it.

### 3.2 The pyramid

**Tier 1: scene tests.** This is the specification of a decision.

- **What it is.** A `Scene` is a constructed situation:
  - units with class, team, position, health, resource;
  - auras, cooldowns, DR, obstacles, and the AI profile.

  `scene.decide(unit)` runs **the same class dispatch `decide_abilities`
  uses**, for one tick. It returns a typed `DecisionView`: the chosen ability
  and target, plus every candidate with its `RejectionReason`.
- **How it reads:**
  ```rust
  let mut s = Scene::new()
      .unit(WARRIOR, Team1, Warrior).rage(80.0)
      .unit(MAGE, Team2, Mage).at(2.0, 0.0)
      .unit(PRIEST, Team2, Priest).at(0.0, 20.0).aura(PRIEST, Polymorph, 6.0);
  assert_eq!(s.decide(WARRIOR).chose(), Some((MortalStrike, MAGE)));
  s.clear_auras(PRIEST);
  assert_eq!(s.decide(WARRIOR).chose(), Some((Rend, MAGE)));
  ```
- **Every scene test is a pair.** The situation, and its control with the
  situation's one condition removed. That is `druid_control_probes`' pairing
  discipline, made the default shape. A paired scene cannot pass vacuously: if
  the rule stops firing, the pair collapses.
- **It is one source of truth.** Today a scene keeps the `Combatant` and its
  `CombatantInfo` row in sync by hand (`druid_decisions.rs:87-91` copies mana
  across). The builder derives the roster from the units, so they cannot
  disagree.
- **It has no seed.** A scene draws no rolls that affect the decision.
  `GameRng` is consumed only by execution, which tier 1 does not assert on.

**Tier 2: decision properties.** These are invariants over a scene grid.

- **What they are.** Properties that must hold in *every* situation:
  - the chosen ability is in range and in sight of its target;
  - never on an immune target;
  - never breaks a friendly breakable CC;
  - never chooses an ability whose own gate the trace shows rejected;
  - a healer with a castable heal and an ally below the urgency threshold does
    not cast damage.
- **How they run.** A deterministic grid generator per class: distances
  straddling every range edge, health bands, resource bands, each situation on
  and off. No `proptest` dependency to start; a nested-loop grid is enough and
  keeps failures readable. The classes are trusted with nothing: one property
  file drives all nine.
- **What they catch.** This is where the "bent guard" class of defect gets
  caught. One property covers every class and every future ability, so a new
  `try_*` that forgets a gate fails without anyone writing a test for it.

**Tier 3: staged matches.** These are for behaviours that need time.

- **Why it exists.** Walks, chases, escapes and posture transitions play out
  over seconds. A one-tick scene cannot assert "the Druid walks round the
  pillar to its dying Mage".
- **What it is.** `run_headless_match_staged(config, stage)` in
  `src/headless/runner.rs`. It spawns as usual, skips the countdown, applies a
  `stage: impl FnOnce(&mut World)` that places units and sets health, auras and
  cooldowns, then runs the real `add_core_combat_systems` schedule under the
  existing frame observer.
- **What it replaces.** The seed still governs rolls, but the situation holds
  *by construction*. A change that moves the RNG stream no longer removes the
  thing under test. The hand-built worlds in `druid_control_probes.rs` are this
  idea done locally; this makes it one shared entry point.

**Tier 4: situation-conditional probes.** These replace seed pins.

- **What they are.** A probe runs a **seed bank**, for example 40 seeds across
  the cells that can produce the situation. It does three things:
  1. finds every window of the situation **with the same classifier the AI
     uses**, read off the trace (§4.4);
  2. asserts a response *rate* inside those windows. For example: "in ≥ 85% of
     *enemy healer locked* windows where Mortal Strike was ready, the Warrior's
     next GCD was Mortal Strike";
  3. reports the window count, and fails on a thin bank with a message saying
     *the bank is thin*, not *the behaviour broke*.
- **Why they survive drift.** RNG drift changes *which* windows exist, not the
  assertion. A stale probe needs a bigger bank, never a new pinned seed.
- **What it relates to.** This is `camp_sweep`'s "measure the mechanism, per
  frame" stance, given a detector so the mechanism is found rather than assumed.

**Tier 5: paired sweeps.** These stay exactly as `sweep-tiers.md` defines them.
They answer *how much*, at the milestone, and they are no longer the only oracle.

**One working rule makes the migration self-propelling.** When a seed-pinned
probe goes stale, it is migrated to tier 3 or 4 instead of re-pinned. No
migration project is needed: the drift that costs us re-pins today pays for the
migration instead.

### 3.3 The prototype

Measured on 2026-10-11 in a scratch copy of `main` at `7905152`, dev profile.
The prototype is one throwaway integration test file and is not committed; the
first card rebuilds it properly as `class_ai/scene.rs`.

Three questions were asked: does tier 1 work today with no refactor, does tier 2,
and does the gap they fill actually exist?

**1. A whole-decision scene test works on `main` as it stands.**
- **What it ran.** A 60-line fixture builds a roster: Warrior, enemy Mage at
  2yd, enemy Priest at 20yd. It calls the real `decide_warrior_action` through
  a throwaway `World` + `CommandQueue`, the way `ReachScene` does, and reads the
  choice off `trace.pending_events`.
- **The paired test.** Priest polymorphed → `MortalStrike`. The control, the
  Priest free → `Rend`. It passes in 0.1s.
- **No class decision consumes `GameRng`.** Only execution's damage and crit
  rolls do (`warrior.rs:822`, `mage.rs:532`, `rogue.rs:507/812`). So a scene
  needs no seed.

**2. A decision property over a grid works too.**
- **The grid.** 288 situations: 9 distances straddling melee range, Charge's
  8yd minimum and its 25yd maximum; 4 target health bands; 4 rage levels; the
  enemy healer CC'd or free.
- **The run.** The whole decision ran in each one.
- **The assertions.** Every chosen ability was in range of its target, and
  Charge was never chosen inside 8yd.
- **Non-vacuity.** 168 situations produced a choice: Charge 96, Rend 48,
  Mortal Strike 24.

**3. The gap is real, and it is worse than "untested".**
- **The mutant.** `let burst_window = false && ...` at `warrior.rs:129`, which
  deletes the Warrior's whole burst-window behaviour.
- **It survives the entire default `cargo test`: 2,085 tests.** The one other
  failure, `script_fixture_suites`, was environmental: the scratch copy is not a
  git repository.
- **The scene test kills it.**
- **At match level it is invisible.** 300 paired matches (Warrior+Mage vs
  Warlock+Priest, Warrior+Warlock vs Rogue+Priest and Warrior+Rogue vs
  Mage+Priest; seeds 1–100; kill target on the DPS; BasicArena) gave
  **byte-identical logs in all 300**.
- **That is not vacuous.** 128 of those matches put a hard CC on the enemy Priest,
  728 applications in all.
- **Why.** The rule reorders Mortal Strike ahead of Rend. The log shows Rend
  already ticking on the kill target by the time the Polymorph lands (seed 1:
  Rend at 16.42s, Polymorph at 17.80s), so the reorder never changes a choice.

That third result carries the argument. A situational rule can be:
- correct in its scene;
- shipped, and documented in its own comments as "burst-during-CC";
- and **never once change a decision in 300 matches that contain its situation
  728 times.**

No seed probe can pin such a rule, because no seed exercises it. No sweep can
measure it, because it does nothing. Only a constructed scene can say what it
does. Only a trace that records *whether the situation changed the decision*
(§4.4) can say whether it matters. Today we have neither, and we could not
have known.

### 3.4 What changes (Part 1)

| Change | Where |
|---|---|
| Factor the class `match` out of `decide_abilities` into `pub fn dispatch_class_decision(...)`. Production and `Scene` call the same entry point, so a scene cannot test a path a match never takes. | `combat_ai.rs:1109-1507` → a new fn in the same file |
| `Scene` builder, `DecisionView` (typed, read from `DecisionEvent`, not `serde_json::Value`), and named scene constructors. It is `pub` and `#[doc(hidden)]` so `tests/` can use it. `ReachScene` becomes a thin alias, then goes. | new `src/states/play_match/class_ai/scene.rs` |
| Property grid and the cross-class invariants. | new `tests/decision_properties.rs` |
| `run_headless_match_staged`. | `src/headless/runner.rs` |
| Situation windows read from the trace, plus the rate-assertion helper (`assert_response_rate(label, windows, responded, min_rate, min_windows)`). | `tests/common/situation_probe.rs` (or a `headless::probe` module) |
| The migrate-not-re-pin rule and the pyramid, added to the probe sections of `CLAUDE.md` and `.claude/agents/tester.md` item 4. | docs |

A later step, worth doing but **not** a prerequisite, splits *decide* from
*execute*:
- `decide_<class>` returns a `Decision { ability, target, .. }`, and one
  executor applies it.
- This removes `Commands`, `CombatLog` and `GameRng` from the decision
  signature, so tier 1–2 tests no longer need a throwaway `World`.
- §3.3 shows tier 1 works without it. It is therefore a cleanup that earns its
  place when the 9–15-argument signatures get in the way. Druid's `DruidTurn`
  struct is the intermediate form.

## 4. Part 2: AI that adapts to the evolving situation

### 4.1 The options, judged

Five criteria, each scored good, mixed or poor:
- **ECS / determinism:** fits this ECS codebase and keeps matches deterministic.
- **Trace:** debuggable through the decision trace.
- **Testable:** fits the Part 1 pyramid.
- **Incremental:** can be adopted step by step with no big-bang rewrite.
- **Converges:** would make progress where the scoring attempt did not.

| Option | ECS / determinism | Trace | Testable | Incremental | Converges? |
|---|---|---|---|---|---|
| **Utility AI** (IAUS: considerations, response curves) | good, if in-house; `big-brain` is archived with no determinism guarantee | poor: "0.62 beat 0.59" is not an explanation | poor: every weight retune flips scene expectations, so tests pin numbers, not intent | mixed: needs a weights-reproduce-the-old-order stage first | **No.** It is the CC value model's failure generalised: no common currency, argmax over noisy estimates, and a sweep-only oracle for the weights. |
| **Behaviour trees** | good | good | good | good | **No gain.** A selector of guarded `try_*` calls *is* a behaviour tree, written as Rust control flow. A BT library adds indirection and a second language. It adds no situational awareness, since the conditions are the same predicates. |
| **GOAP / HTN planning** | mixed: search cost per unit per tick, so it needs caching | poor: plans are hard to explain post hoc | mixed | poor: replaces the class AI | **No.** A 1.5s-GCD arena with about 8 abilities per class has no long action chains to plan. Plans also rest on predicting the opponent, which is the T_eff variance problem with a longer horizon. The ideation doc already judged it overkill. |
| **Hierarchical intent** (team plan → role → action) | good | good | good | good: `TeamPlan` exists | **Partly.** Right shape, but step 5 shows the failure mode: an upper layer that *commits* ("hold this kill call") loses to one that re-evaluates. Viable only if the upper layer holds nothing longer than the facts that justify it. |
| **Situation classifiers feeding the priority lists** | good: a pure fn of the snapshot, no RNG | **good: a situation is a word you can grep for** | **good: scenes construct situations, probes detect them** | **good: one site at a time, byte-identical ports** | **Yes.** It is the shape of everything that has won here: discrete facts, adaptive rules, re-evaluated every tick. |
| **Current approach, improved** | good | good | mixed | good | **Slowly.** It already grows situations (the burst window), but ad hoc, untraced, per class, with diverging thresholds. It does not compound. |

### 4.2 Recommendation: situations, plays, and utility only where it is admitted

These are **situation classifiers feeding the priority lists, arranged as a thin
hierarchy**: team situation → the unit's play → its existing priority chain.
Nothing in the hierarchy holds a decision. Each layer is re-derived every tick
from facts, with hysteresis only on the *facts*.

**Layer 1: `Situations`.** This is the read model. Add a new module
`src/states/play_match/situation.rs`.

```rust
#[derive(Resource, Default)]
pub struct Situations { pub teams: [TeamSituation; 2] }

pub struct TeamSituation {
    pub phase: MatchPhase,                    // Opener | Engaged | Attrition (dampening past a set share)
    pub tempo: Tempo,                         // Even | Pressing | Trailing (team_hp_advantage, latched)
    pub enemy_healer_locked: Option<Window>,  // who, since, until (remaining CC on the healer)
    pub kill_window: Option<Window>,          // kill target low AND its healer locked, out of sight, or OOM
    pub ally_in_danger: Option<Window>,       // a focused teammate below urgency_hp_threshold
}

pub struct Window { pub subject: Entity, pub since: f32, pub until: Option<f32> }

/// Pure: the snapshot, the previous frame's situation, and the clock.
pub fn classify(snap: &CombatSnapshot, team: u8, prev: &TeamSituation, now: f32,
                cfg: &SituationConfig) -> TeamSituation
```

- **Where it runs.** An `update_situations` system in Phase 1, after
  `update_team_plans`, registered in `add_core_combat_systems`.
  `CombatContext` gains `situation: &TeamSituation`, read by `ctx.situation()`.
- **Two properties make it safe:**
  - **Pure and RNG-free.** It only reads. Adding it changes no `Legacy` output
    until a site reads it. That is the byte-identity gate on every port.
  - **Built from what the team perceives.** It uses the same `stealth_visible`
    rule `CombatContext::new` applies, so a situation never leaks a hidden Rogue.
- **Hysteresis is per situation, with onset and release thresholds** in a
  `situations:` block of `movement.ron`, or a sibling `situations.ron`. That
  generalises what `KitePosture::wand_oom` and `kite_hold` hand-roll today.
- **`since` and `until` are the adaptivity.** They let a rule say "the lock
  ends in 1.2s, so do not start a 2.5s cast for it", which a boolean cannot.
  The Hunter already hand-rolls exactly this ("a shot that would land after the
  hunter's own trap catches its target is held").

**Layer 2: plays.** A play is a named reordering, nothing more.

```rust
// warrior.rs, replacing the duplicated burst-window calls at :129 / :172
match ctx.situation().play_for(Role::MeleeDps) {
    Play::Burst  => /* MortalStrike, Rend, HeroicStrike */,
    _            => /* Rend, MortalStrike, HeroicStrike */,
}
```

- **The mapping.** A short, exhaustive `play_for(role) -> Play` maps situations
  to plays: `Play::{Default, Burst, Peel, Recover}`. Roles come from the
  `CompProfile` that `team_plan.rs` already classifies.
- **What does not change.** Each class keeps its `try_*` functions and gates.
  What changes is that the ordering *depends on* a named fact instead of a local
  boolean.
- **Exhaustive by design.** The `match` is exhaustive, so a new play is a
  compile error in every class until each one answers it. Where membership is
  a judgement, make the compiler ask.

**Layer 3: utility, admitted locally.** Scoring stays where it already works,
and new scoring must pass an admission test written into the module docs.
- **The test:** a step may rank candidates by score only when **(a)** every term
  is in one currency, and **(b)** the score's inputs are observable now rather
  than predicted.
- **What passes:** heal target selection (HP deficit and range), dispel and purge
  priority (`dispel_priority`), and movement direction.
- **What fails:** pricing a CC against a cooldown, or choosing whether to
  Polymorph on a predicted duration.

This generalises the value model's own admission criterion from "a class joins
if its T_eff is predictable" to "a *comparison* joins if it is commensurable
and observed".

### 4.3 How this answers "dynamic to the evolving situation"

The current AI already re-decides every tick. What it lacks is *awareness of
what changed*. Situations supply it in three ways:
- **Onset.** A situation that has just begun is a trigger. *The healer just got
  sheeped: swap to the burst play now.* Today each class infers this from a
  per-tick boolean, if at all.
- **Remaining time.** `until` lets decisions fit inside windows: burst only what
  lands before the lock ends, or hold a stun to chain into the lock's end. This
  is how chain CC, a defining arena strategy (`cc-compounds-not-adds`), becomes
  expressible *without* pricing it. A rule like "if *enemy healer locked* ends
  within 1s and I have a stun, stun the healer" encodes compounding as a fact
  about time, not a superlinear value function.
- **Phase and tempo.** Opener, engaged and attrition, and pressing or trailing,
  change *which* play is default. The Druid's mana governor and
  `press_advantage_margin` are two hand-rolled cases that would become one.

Cross-class plays become possible for the first time:
- *ally in danger* → **Peel**. Every class with a stun, root, fear, Nova or
  Concussive spends it on the attacker of the ally in danger.
- This is exactly the "damage avoided" value the CC model could not price, and
  a discrete rule can state it.

### 4.4 Debuggability

Three trace additions, mirroring how `movement_decision` already works:
- every `ability_decision` event carries a compact `situation` field: the
  active situation names and the play;
- under a non-default play, the event records `play_changed_choice`: whether
  the default chain would have chosen differently. §3.3 shows why this matters:
  without it, a play that never changes anything is indistinguishable from one
  that works. Answering it means evaluating the default chain's *gates*
  without executing, which is why the decide/execute split (§3.4) moves up the
  plan once plays exist. Until then, a play records which of its reordered
  steps produced the choice, a weaker but still non-vacuous signal;
- a new `situation_change` event kind fires **on transitions only**, carrying
  the window's subject, `since` and `until`.

"Why didn't the Rogue Kidney Shot during the kill window?" becomes one `jq`
query joining the window to the Rogue's candidates. "How often does a kill window
open per match, per comp?" becomes a histogram. Tier 4 probes read their windows
from this same event, so **the AI and its tests cannot disagree about what the
situation was.**

### 4.5 Gating, and the flag problem

- **Ports** of existing situational logic onto `Situations` land **ungated**.
  They are byte-identical by construction (§5 step 1), so there is nothing to
  gate.
- **New plays** land under the **one existing gate, `AiProfile::TeamPlan`**,
  read through `ctx.ai_profile`. No per-class or per-play flags.
- **Graduate or delete.** At each milestone sweep, every play under the gate is
  either made unconditional or deleted. A play does not survive two milestones
  gated. That bounds the gate to one milestone's worth of work, and bounds the
  combinatorics at one bit per team.

## 5. Migration plan

Each step ships alone. Steps 1–4 are infrastructure plus byte-identical ports;
step 5 is the first behaviour change.

1. **Situation v0 + scene harness.** This is the first card; see §6.
2. **Ally in danger and phase, ported.** Move `teammate_needs_saving`,
   `another_teammate_dying`, `compound_pressure_trigger` and the Druid governor's
   phase onto `Situations`.
   - Their thresholds are reconciled *explicitly*. A port that must change a
     threshold to unify it is a separate, gated behaviour change, not part of
     the port.
   - `Legacy` byte-identical.
3. **Decision properties (tier 2)** across all nine classes. Any invariant that
   fails on `main` becomes a correctness card, not a test exemption.
4. **Staged matches (tier 3)**, then migrate the medic-walk and dying-heal probes
   (`druid_dying_heal`, `medic_chase_hold`) off their seeds. These are AS-247's
   re-seeded ones, so the first migration retires a known pain point.
5. **First new play: Peel**, gated under `TeamPlan`.
   - Measured by a tier 4 probe: the share of *ally in danger* windows answered
     by a CC on the attacker within one GCD, and the ally's survival time in those
     windows.
   - Then a DIRECTIONAL paired sweep with `--expect improvement` on comps with
     peel tools.
   - This is the step that tests the *value* claim. Steps 1–4 test the fit.
6. **Kill window → Burst for every DPS**, with `until`-aware cast fitting. Same
   measurement shape.
7. **Chain play.** Hand off a stun into the end of a lock. Its acceptance metric
   is `cc_lifecycle_probe`'s counterplay-free seconds, the instrument
   `cc-compounds-not-adds` says nothing has scored against yet.
8. **Decide/execute split** (§3.4), class by class, when the signatures justify it.

The TeamPlan positional solve is untouched. It stays the 0.8 pillar work, and
reads `Situations` where it now recomputes contact and tempo itself.

## 6. The first card

**Title:** Situation v0: a traced *enemy healer locked* / *tempo* read model,
four sites ported, scene harness.

**Scope:**
1. Add `situation.rs`, holding `Situations`, `TeamSituation`, `Window`,
   `classify` and `update_situations`.
   - Two situations only: `enemy_healer_locked` (with `until` = the longest
     remaining hard CC on the healer) and `tempo`.
   - Register it in `add_core_combat_systems`. `CombatContext::situation()`.
2. Port four sites to read it:
   - Warrior burst window (`warrior.rs:129/172`), as a `Play::Burst` match;
   - Hunter burst window (`hunter.rs:456`);
   - Priest Mana Burn CC window (`priest.rs:1336`);
   - `pressing_when_ahead` (`healer_postures.rs:97`).
3. Add the `situation` field to `ability_decision` and the `situation_change`
   event. Extend `decision_trace_audit`.
4. Add `class_ai/scene.rs` (the `Scene` builder, `DecisionView`,
   `dispatch_class_decision`). Add one paired scene test per ported site.

**Success measure.** All three must hold:
- **Fit.** `Legacy` byte-identical on the standard determinism batch, and on a
  TeamPlan batch where only the trace's new fields differ. Report non-vacuity:
  count the burst windows the batch contained.
- **Testability.** For each ported site, a mutant that removes the site's
  situational branch:
  - **survives** the default `cargo test` on `main` today;
  - is **killed** by the new scene test.

  §3.3 measured the Warrior's case already. The card also records, per ported
  site, how often its situation changed the decision over a seed bank. A site
  that never does, like the Warrior's in §3.3, is reported as a finding for its
  own card, not fixed in this one.
- **Debuggability.** The `jq` recipe "per class, chosen ability while
  `enemy_healer_locked`" is added to CLAUDE.md's trace recipes and runs on a
  real match trace.

**What would disprove the approach:**
- the classifier cannot be made byte-identical without special-casing per class
  (the facts are not shared after all);
- a paired scene costs more lines than the seed probe it would replace;
- the trace field cannot be added without perturbing event order.

Any of these means stop and report, not push through.

## 7. What would make this recommendation wrong

- **If the important behaviours are not discrete.** The bet is that arena play
  is mostly *recognise the situation, run the play*, plus local scoring. If the
  wins turn out to sit in continuous trade-offs between plays, a utility layer
  *over plays* becomes the next step. A handful of plays is a far smaller
  scoring problem than about 70 abilities. Steps 5–6 will show this: watch for
  plays that fight each other.
- **If plays multiply without bound.** Each play is a judgement. The exhaustive
  match keeps them visible, and graduate-or-delete keeps them few; if neither
  holds, that is the signal.
- **If tier 4's rate thresholds get fitted the way seed floors were.** The
  defence is the paired control from tier 1, applied to probes: assert the rate
  *against the same bank with the play disabled*, not against an absolute
  number.

## 8. Decisions asked of you

1. **Direction.** Adopt the Situation layer (situations → plays → priority
   lists, with admitted local utility) as the AI architecture for 0.8. The
   alternative is to revisit a global utility scorer.
2. **Test policy.** Adopt the migrate-not-re-pin rule: a stale seed probe moves
   to a staged match or a situation-conditional probe instead of being re-pinned.
   This changes what the Tester accepts.
3. **Gating.** New plays ride the existing `AiProfile::TeamPlan` under
   graduate-or-delete per milestone, rather than a new profile. The trade-off:
   TeamPlan already carries the positional solve, so a sweep of the TeamPlan arm
   measures both. The alternative is one new `AiProfile` variant, still under
   graduate-or-delete.
4. **First card.** Approve "Situation v0" (§6) as the card that proves or
   disproves the approach before any behaviour work.
