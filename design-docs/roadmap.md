# Project Roadmap

## Current Status

- **Core gameplay loop**: COMPLETE
- **Classes**: Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter (7)
- **Abilities**: 28 across all classes
- **Headless testing**: COMPLETE
- **Results screen**: Enhanced with WoW Details-style breakdown

## Active TODOs

### High Priority

- [ ] Combat log filter (HP changes only)

### Medium Priority

- [ ] Diminishing returns for CC
- [ ] Team summary totals on results screen
- [ ] Silence CC type (prevent casting)
- [ ] DoT stat scaling engine support — snapshot AP/SP into aura magnitude at
      application time. DoT ticks are currently flat for every class; Serpent
      Sting's origin requirement asked for AttackPower scaling and was amended
      to flat pending this (see
      docs/plans/2026-06-11-001-feat-serpent-sting-hunter-plan.md, Scope
      Boundaries). Affects Corruption/UA/Curse of Agony tuning too.
- [ ] Remaining-duration-aware Freezing Trap guard — trap anyway when the
      blocking friendly DoT expires within ~1s (deferred from Serpent Sting v1,
      reactive-binary guard). Pull forward if sweep data shows costly trap
      suppression.

---

## Follow-ups from healer movement AI (PR #63, merged 2026-06-08)

The healer posture-movement slice shipped with four buckets of deferred work.
This is the consolidated, durable list — a new session can start from here.
Context: `docs/plans/2026-06-06-001-feat-healer-posture-movement-ai-plan.md`
(the completed plan), `docs/reports/2026-06-healer-movement.md` (validation),
and the three learnings under `docs/solutions/` (casting-visibility blind spot,
mirror-asymmetry measurement protocol, bevy macOS exit).

### A. Offensive-punish slice (triggered)

The R13 draw-rate watch **tripped** as designed (1v1 draws 2.3%→7.0%, all
healer-mirror cells; Paladin+Priest 2v2 became the worst comp at 12.2% — the
draw wall resolves into losses at the 300s cap). Healers got harder to kill with
no offsetting pressure, so this slice is the agreed answer:

- [ ] Target-swap responsiveness: when a melee's kill target kites out of reach
      and a softer target is in range, swap instead of chasing forever (with
      hysteresis to avoid ping-pong).
- [ ] Burst-during-CC: DPS prefers burst when the enemy healer is CC'd (priority
      tweak on existing aura tracking, not new machinery).
- Both also reduce the #1 "bot tell" (tunnel-vision chasing). Scope mirrors the
  healer-movement slice: probes + matrix/sweep validation, side-symmetrized
  deltas, draw-rate as the success metric (should come back down).

### B. Code-review residuals from PR #63

Not auto-applied during review because they touch the sim path or need judgment;
one matrix re-validation covers the behavioral ones. Source: PR #63 Known
Residuals table + `/tmp` review artifact (now only here).

- [ ] P1 `class_ai/paladin.rs` — file crossed 1k lines (1626); extract `paladin_postures.rs`.
- [ ] P1 `class_ai/paladin.rs:1136` — ~7 per-tick Vec allocations in posture eval; hoist to single-pass scalars (~65M allocs/matrix).
- [ ] P2 `class_ai/priest.rs:914` — stale PRESSURED Direction directive survives FREE transition (~1s TTL; Paladin path removes it, Priest doesn't); **needs matrix re-validation**.
- [ ] P2 `class_ai/priest.rs:776` — threat set computed twice per PRESSURED tick.
- [ ] P2 `class_ai/priest.rs:967` — `compute_formation_point` allocates 3 Vecs every FREE tick.
- [ ] P2 `class_ai/priest.rs:561` — shared escape helpers live in priest.rs, imported by paladin.rs; move to `healer_postures.rs`.
- [ ] P2 `class_ai/paladin.rs:1454` — ~45-line pressured-tick duplication vs Priest; extract shared helper.
- [ ] P2 `class_ai/priest.rs:594` — four movement constants hardcoded despite the RON-first policy; move to `movement.ron`.
- [ ] P3 `assets/config/movement.ron:56` — Priest `corner_penalty` 6.0 vs struct default 4.0 silent divergence.

### C. Movement-AI extensions (build on the posture skeleton)

- [ ] Line-of-sight / pillar play — the structural counter to Mage team dominance
      (Mage is clear #1 in 2v2/3v3 with no counterplay). LoS terms plug into the
      existing scorer term list.
- [ ] CC danger radii — cooldown-aware avoidance of enemy CC ranges (new scorer term).
- [ ] Cast-juking — step out of range of an incoming CC cast (new trigger).
- [ ] Migrate Mage/Hunter `kiting_timer` onto the `MovementDirective` system (unify the two movement mechanisms).
- [ ] Psychic Scream (short-range Priest CC) — the Priest DIP predicate is already
      built ability-agnostic; Scream plugs in when it ships.

### D. Infrastructure / methodology

- [ ] Early-draw heuristic — declare a draw when neither team has dealt meaningful
      damage in N seconds. Draw-wall healer mirrors now dominate matrix wall time;
      this reclaims most of it without touching balance. (The parallel in-process
      batch runner already landed via PR #62.)
- [ ] Mirror-asymmetry root fix — same-frame action races resolve in ECS iteration
      order (side bias up to ~18%). Mechanism documented
      (`docs/reports/2026-06-mirror-asymmetry-diagnostic.md`); fix is a
      same-frame-resolution redesign, deferred. Until then, the side-symmetrized
      measurement protocol is the standing workaround
      (`docs/solutions/implementation-patterns/mirror-asymmetry-side-symmetrized-measurement.md`).
- [ ] Manual naturalness pass — watch seeded replays in the graphical client (the
      one validation loop with no automation): statue comp seed 20260606, escape
      comp Priest+Mage seed 1, dip comp Pal+War seed 1. Look for zigzag,
      indecision, robotic geometry.

---

## Follow-ups from Hunter movement migration + pet fixes (worktree-ai-tuning, 2026-06-13)

The Hunter ENGAGE/KITE migration (commits `fe9acac..f8f5ff3`) and three
follow-on fixes — `03387f4` (melee-only kite filter), `1a41deb` (melee-pet
dead-zone), `c0dc2af` (pets don't break friendly CC) — are on branch
`worktree-ai-tuning`, NOT yet PR'd. The mask refactor + Mage pilot from the same
plan already shipped as PR #69. Plan:
`docs/plans/2026-06-12-001-refactor-context-steering-masks-plan.md`.

Headline post-fix Hunter winrates (symmetrized, N=50/side):
- **1v1**: Warrior 100, Rogue 84, Warlock 92, Priest 92, Mage 0, Paladin 0.
- **2v2 (Hunter+Priest vs each+Priest)**: Priest 100, Paladin 61 (39 draw),
  Rogue 50, Warrior 2, Mage 0, Warlock 0.

**Critical context for ANY Hunter balance work:** every Hunter matrix predating
`1a41deb` was computed with a damage-dead pet — the ranged dead-zone silently
cancelled every melee-pet auto-attack swing. All Hunter baselines in
`design-docs/balance/` are stale; re-sweep before tuning.

### A. Hunter 2v2 holes (diagnosed, NOT pet-related)

- [ ] **Warrior 2v2 ~19%** — re-diagnosed 2026-06-20 against the Psychic Scream
      build (PR #73). The original framing ("Priest heals *itself* instead of
      peeling the focused Hunter", target-selection bug) does **not** hold: the
      Warrior trains the *enemy Priest*, not the Hunter — across 16 seeds the
      first Mortal Strike always lands on the team1 Priest, which dies first
      (~47s), then the Hunter loses 1v2. Self-healing is therefore *correct*
      targeting. The real loss mechanism (matches the [[hunter-2v2-warrior-loss]]
      memory note): (1) the focused Priest can't out-sustain Mortal Wounds
      healing reduction (Flash Heals decay 66→43) under repeated interrupt
      lockouts (Holy locked 4s ×3); (2) the Hunter generates near-zero kill
      pressure (~609 dmg/match, never lands Aimed Shot — all GCDs spent on peels
      Freezing Trap/Concussive/Frost Trap/Disengage that get DR'd and out-paced).
      Psychic Scream is a real but *capped* lever: the defensive scream fires
      reliably (~17s, once PRESSURED) but the enemy double-healer comp dispels the
      Fear within ~1.7s every time (16/16 casts dispelled), and the 30s CD makes
      it once-per-fight — it lifts the comp 0%→~19%, not a fix. **Leverage is on
      Hunter offensive uptime (the kill-race lever, see bucket B "plant when
      safe" + Hunter burst-during-CC), not healer target-selection.** Repro:
      `{"team1":["Hunter","Priest"],"team2":["Warrior","Priest"],"random_seed":N}`.
- [ ] **Mage 2v2 0%** — control matchup (Polymorph / Frost Nova / kiting).
      LoS / pillar play is the structural counter (see healer bucket C — shared
      scorer term).
- [ ] **Warlock 2v2 0%** — dispel-war + DoT/Fear sustain out-grinds Hunter+Priest
      (confirmed in the graphical client 2026-06-13: Hunter dies ~27s in to Fear
      + Corruption/UA/Agony while the two Priests trade dispels).

### B. Hunter movement refinements (deferred from the melee-only kite filter)

- [ ] **Enemy melee-pet kiting** — `melee_within` (`class_ai/dps_postures.rs`)
      excludes pets (`!is_pet`), so the Hunter does not kite an enemy
      Voidwalker / Felhunter / Spider chasing it. Fold enemy melee pets into the
      kite-threat predicate. (Surfaced as the survivability gap vs Warlock+Priest,
      whose pet beats on a now-stationary Hunter.)
- [ ] **"Avoid CC" movement input** — the Paladin is excluded from kite threats
      (its melee isn't pressure), but its Hammer of Justice is; avoiding incoming
      stuns/HoJ is a cooldown-aware CC-danger-radius / cast-juke scorer term
      (overlaps healer bucket C).
- [ ] **Strategic "plant when safe"** — match-state-aware planting for Hunter
      damage uptime (root-duration vs re-engage-time, team HP delta). A naive
      root-aware plant regressed Rogue 1v1 9→1 and was reverted; this needs the
      strategic layer, not a reactive predicate.

### C. Pet AI (surfaced by the pet-damage fix)

- [ ] **Pet retarget under friendly CC** — the new guard (`c0dc2af`) makes the
      pet hold fire on a target carrying its own team's Freezing Trap / Web, so it
      idles in melee through the CC window instead of switching to a valid
      secondary target. Add retargeting so the pet stays useful during the peel.
- [ ] **Pet melee-commitment pass** — committing to melee was the original
      Web-self-break source and pulls the Hunter's formation inward. Revisit when
      a pet should commit vs hang back (hybrid hold/peel behavior).

### D. Ship + re-baseline

- [x] ~~**Tier-2 review** the `main..HEAD` range on `worktree-ai-tuning`.~~ DONE
      2026-06-13 — 8-persona review (commit `0a4a93f`). No code defects; verdict
      Ready with fixes. Applied the safe nits + the 3 missing regression tests
      below. PR still open (see remaining item).
- [x] ~~**Open the PR** for `main..HEAD` on `worktree-ai-tuning` (Hunter migration
      + pet/kite fixes + rebaseline).~~ DONE 2026-06-13 — PR #71.
- [x] ~~**Re-sweep** the full 7×7 1v1 + 2v2/3v3 matrices with the pet-damage fix
      live, replacing the stale `design-docs/balance/` Hunter baselines.~~ DONE
      2026-06-13 — `canonical_{1v1_n100,2v2_full_n100,3v3_full_n50}_300s.csv`
      regenerated + `canonical_baselines_summary.md` rewritten. Hunter 1v1
      20.7→59.4; team formats +3-4; Mage+Paladin meta unchanged (Hunter-isolated).

### E. Code-review residuals (from the 2026-06-13 Tier-2 review)

Deferred from the review — none block the PR. The 3 P1 regression-test gaps were
fixed in `0a4a93f`; these are the lower-priority remainder.

- [ ] **P2 `combat_ai.rs` crossed 1k lines (1,313)** — the Mage and Hunter
      `evaluate_dps_posture` dispatch arms are near-identical 10-line scaffolds.
      Extract a `dispatch_dps_posture(...)` helper and call it from both.
- [ ] **Doc: name the auto-attack CC-guard site** — update
      `docs/solutions/ai-decision-patterns/friendly-cc-break-prevention.md` to
      list `combat_core/auto_attack.rs` as a second guard site alongside
      `pre_cast_ok` (the doc predates the auto-attack path).
- [ ] **Agent-native: pet CC-suppressed swing is untraced** — when the friendly-CC
      guard makes a pet hold fire, no trace event records it; an agent diagnosing
      "why did the pet stop swinging?" must infer it. Consider a `suppressed_by_cc`
      field on `pet_decision`.
- [ ] **Nit: `incap_cc_team`/`root_cc_team` use `HashMap`** where the surrounding
      determinism-sensitive maps use `BTreeMap`. Lookup-only today so it's safe;
      switch for consistency if iteration is ever added.

---

## Milestone 2: Visual Polish

- [ ] Procedural character meshes (distinct silhouettes per class)
- [ ] Ability visual effects (AoE indicators, ground effects)
- [ ] Death animations
- [ ] Arena environment details (pillars, decorations)
- [x] ~~Victory celebration animations~~ (basic version done)
- [x] ~~Spell projectile visuals~~ (Frostbolt, etc. done)

## Milestone 3: Depth

- [ ] Full ability roster per class (currently ~4 per class)
- [ ] Talent system (simplified)
- [ ] Additional maps (only Basic Arena functional)
- [ ] Imbalanced matchups (1v2, 2v3, etc.)
- [x] ~~Detailed results breakdown (WoW Details-style)~~ DONE

## Milestone 4: Polish

- [ ] Audio implementation
- [ ] Font styling (fantasy theme)
- [ ] Gamepad support
- [ ] SteamDeck testing and optimization
- [x] ~~Options menu expansion (keybinds)~~ (Keybindings menu done)
- [x] ~~Settings persistence~~ (settings.ron saves/loads)

---

## Technical Debt

### Aura System Architecture
Currently auras are separate entities. May need to reconsider as child entities or components on the combatant for better performance and simpler queries.

### Combat Log Performance
If matches get long, may need to limit log size or virtualize the display to prevent memory growth and UI slowdown.

---

## Completed Features

### Core Gameplay Loop (Milestone 1)

- [x] Tech stack decision (Bevy/Rust)
- [x] Project structure scaffolded
- [x] Data schemas (RON config files)
- [x] UI system (bevy_egui for menus)
- [x] Main Menu Scene
- [x] Options Menu Scene
- [x] Configure Match Scene
- [x] Play Match Scene
- [x] Results Scene
- [x] Camera system (zoom, pan, follow)

### Combat System

- [x] Auto-attack combat with attack speed
- [x] Health/Mana/Resource bars
- [x] Cast bars during spell casting
- [x] Win/lose detection with victory celebration
- [x] Pre-match countdown (10s) with gates
- [x] Mana restoration during countdown (pre-buffing phase)
- [x] 28 abilities across 6 classes
- [x] Ability cooldowns
- [x] Cast time handling (interruptible)
- [x] Resource cost/generation (Mana, Rage, Energy)
- [x] Spell school lockouts on interrupt
- [x] Killing blow tracking

### AI System

- [x] Target selection (nearest enemy, lowest HP ally)
- [x] Ability usage logic with priorities
- [x] Movement towards targets
- [x] Kiting behavior (Mages)
- [x] Interrupt logic (Warriors)
- [x] Defensive cooldown usage
- [x] Strategic CC targeting (separate from kill target)
- [x] CC target heuristics (healer priority, context-aware inversion)

### Simulation Controls

- [x] Pause/Play toggle (Space)
- [x] Speed buttons (0.5x, 1x, 2x, 3x)
- [x] Keyboard shortcuts (1-4)

### Auras and Buffs

- [x] Aura system (Root, Stun, Slow, DoTs, buffs)
- [x] Duration tracking
- [x] Visual labels with duration countdown (ROOT 5.2s, STUN 3.1s, etc.)
- [x] Pre-match buff phase (Fortitude)
- [x] Absorb shields (Ice Barrier, Power Word: Shield)

### Crowd Control

- [x] Root (Frost Nova) - prevents movement
- [x] Stun (Kidney Shot, Charge) - prevents all actions
- [x] Fear (Warlock) - target runs randomly, breaks on damage (100 threshold)
- [x] Polymorph (Mage) - target wanders slowly, breaks on ANY damage
- [x] CC indicators on combatants
- [x] CC breaks (Fear breaks on damage threshold, Polymorph on any damage)
- [x] Strategic CC targeting (separate cc_target from kill_target)
- [x] Heuristic CC target selection (healer priority, inverted when killing healer)

### Data-Driven Configuration

- [x] abilities.ron - All 28 ability definitions
- [x] AbilityDefinitions Bevy resource
- [x] Runtime balance changes without recompilation
