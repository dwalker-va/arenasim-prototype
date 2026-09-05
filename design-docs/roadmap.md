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

- [x] ~~**Warrior 2v2 ~19%**~~ — LARGELY ADDRESSED in this PR. NOT a healer
      target-selection bug (the old framing was wrong: the Warrior trains the
      enemy Priest, so self-healing is correct); the hole was zero Hunter kill
      pressure. Root cause traced to the **Hunter mana economy** — it gets zero
      mana from gear (mail itemization), the smallest effective pool of any mana
      class. Fix: +60 gear mana + Freezing Trap 43→26 + smarter Concussive/trap
      AI → Hunter+Priest 2v2 25%→~35%, overall Hunter 2v2 +4.7pt, 3v3 +5.1pt, no
      1v1 regression. Full re-diagnosis:
      `design-docs/balance/2026-06-20-warrior-2v2-rediagnosis.md`. (Remaining
      Hunter holes — Mage control, deeper melee-pin — are separate, below.)
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

## Animation tier walk: the next candidates (2026-08-26)

The tiered animation walk has shipped its pilot and three more signatures —
Polymorph (#103), Fear (#107), Lightning Bolt (#111), Mortal Strike (#112) —
plus `animate_body_lean` (#113), which lifted the melee ceiling for the whole
tier at once, and the Frostbolt / Shadow Bolt missiles and impacts (#116).
Procedure and its amendments:
`docs/solutions/implementation-patterns/signature-ability-animation-procedure.md`.

This candidate list came out of **removing the in-combat ability speech
bubbles**. Those bubbles announced their caster's ability in text above its
head, which is exactly where an animation plays; deleting them was the right
call, but it also removed the only cue several abilities had. An audit of all
13 removed bubble sites against `rendering/` produced the gaps below.

**The structural finding, which should drive the ordering.** There are exactly
two generic caster-side hooks: `CastingState` drives the casting orb, and
`QueuedInstantAttack` drives `InstantAttackLanded`. An ability that is
**instant AND aura-only** has neither, so it falls through both and renders
nothing whatsoever. Compounding it, the receiver side has treatments for
`Fear`, `Polymorph`, `DamageOverTime`, `Absorb`, `DamageImmunity` and
`Incapacitate` — but **none for `Root`, `Stun` or `MovementSpeedSlow`**, which
is precisely what the silent abilities apply. One receiver-side treatment
therefore lights up six abilities at once, which is why it led.

**A third hook has since surfaced, and the framing above missed it.** Those two
are both CASTER-side; a projectile's *landing* is a hook of its own, and it is
barely implemented. Section D has the detail — it is the highest-value remaining
item on this list.

Section A has since shipped and closed the `Root`/`Stun` half of that gap.
**`MovementSpeedSlow` remains untreated** — Frostbolt, Crippling Poison,
Concussive Shot, Frost Shock and Frost Armor's chill all apply it and
none of them shows it on the victim. (An earlier revision of this list also
named Kick, Arcane Shot and Lightning Bolt — fact-checked against
`abilities.ron` 2026-09-04: none of the three applies a slow.) It is a softer effect than hard CC and did
not belong in the same treatment (a slowed unit is still moving and acting), but
it is now the largest remaining receiver-side hole.

### A. Root/Stun receiver treatment — SHIPPED (#115)

- [x] ~~**A shared treatment for `Root` and `Stun`.**~~ `rendering/effects/hard_cc.rs`,
      25 probes. It covers **six** abilities, not the four originally listed here:
      the audit missed Hammer of Justice and Boar Charge, both equally silent.
- **The grammar is spatial** — feet for Root, head for Stun. Root grows ice
  crystals (Frost) or a webbed sheet (Nature), then holds completely still; Stun
  turns a hueless bead whirl overhead at exactly 1 Hz. A per-victim apply flare
  marks the landing, which is what gives Frost Nova's AoE an AoE read with no
  caster hook of its own.
- **Three calls that are easy to undo by accident.** There is no ground ring — a
  dark disc already means "AoE landing here" in this genre, and the apply flare
  does that job. The body is never touched (no tint, mesh swap, pose or gait
  suppression), which sidesteps the `OriginalBodyMaterial` contention family and
  keeps Cheap-Shot-on-a-stealthed-Rogue a non-event. And the web stops at the
  shins deliberately: enclosing the whole unit is this game's **Incapacitate**
  language (Freezing Trap's `IceBlockVisual`), while Root leaves the torso free
  to act.

### A2. The applier side of the four silent instants — SHIPPED

- [x] ~~**Frost Nova, Cheap Shot, Kidney Shot and Hammer of Justice have no
      actor-side animation.**~~ All four now do. `InstantAttackLanded` was
      generalised to `InstantAbilityFired` (`caster` plus `target: Option<Entity>`,
      `None` for caster-centred), with `is_spawned_for()` as the single list both
      combat code and the sandbox derive from so the two cannot drift.
- **Researching the client data reversed three of the four designs.** The table
  and its lessons live in the procedure doc; what matters here is the two
  consequences a later change could quietly break. The Paladin's mace
  deliberately does **not** swing — `swing_style_for_ability` returns `None` for
  Hammer of Justice on purpose. And the two rogue stuns are byte-identical on
  the receiver side and differ ENTIRELY on the caster side, so collapsing them
  into one shared stroke would discard the only thing separating them.
- **One ordering dependency to preserve.** Frost Nova's propagated freeze — each
  victim's crystals grow as the wave reaches *them* — means the nova systems must
  stay chained ahead of the hard-CC treatment, or the delay lands too late and
  propagation degrades silently to an instant freeze.
- **Left undone:** the source puts a red glow on both the Rogue's hands during
  Kidney Shot. We have weapon sockets but no hand attachment points, so placing
  it would be inventing anatomy.

### B. Interrupts — an actor-side gesture (melee half SHIPPED)

- [x] ~~**Pummel and Kick actor gestures.**~~ Shipped 2026-09-04. The client
      data reversed the "shallow weapon jab" sketch above: Kick's caster anim
      is the literal `Kick` (95) and Pummel's is `SpecialUnarmed` (118) — both
      `HasMissile = 0`, no caster effect model, **the weapon does not swing in
      the source**. Kick ships source-faithful as `SwingArc::Unarmed`
      (body-only: torso loads forward, ROCKS BACK at extension, weapon rides
      rigidly). Pummel deliberately deviates — a limbless capsule has no punch
      — as `SwingArc::PommelStrike`: the weapon flips tip-back over the grip
      while thrusting, the haft's BUTT slamming into the target (a
      bench-reviewed call, not an over-read). Both are faster than Cheap
      Shot's 0.63s; the opposite body directions are the glance-level
      distinction — pinned by `the_two_interrupts_disagree_on_body_direction`.
      Markers spawn at the interrupt system's committed-use site in
      `combat_ai.rs` (runtime `interrupt_ability`, invisible to the literal
      audit scan — covered by `VIA_INTERRUPT` +
      `the_interrupt_gestures_are_spawned_and_gated`). Bench: the Interrupt
      Gesture Bench artifact (sliders emit the consts).
- [ ] **Counterspell / Wind Shear / Spell Lock** are casts or pet abilities and
      want a different answer — likely a brief effect at the *victim*, timed to
      the orb's sputter, rather than an actor stroke. Wind Shear is
      deliberately excluded from the gesture guard (no weapon sockets); a test
      pins the exclusion.

### C. Leftovers

- [ ] **Devour Magic** (Felhunter) — the dispelled target gets a `DispelRibbon`;
      the pet itself plays nothing. Lower value than B: the outcome is legible
      even though the actor is not.
- [ ] **Frost / Mage / Molten Armor** (Mage) — self-buff auras
      (`FrostArmorBuff`, `ManaRegenIncrease`, `CritChanceIncrease`), none keyed
      in rendering. Lowest value on the list: they fire once during the
      pre-match buff rotation, where nothing is competing for attention and the
      banter is the intended focus.

**Already covered — the bubble was pure occlusion, nothing owed.** Ice Barrier
(`ShieldBubble`), Psychic Scream (`ScreamBurst`), Fear (receiver husk in
`fear.rs`, plus the caster's orb — it is a hardcast), Boar Charge (charge trail
in `movement_trails.rs`), Master's Call (`DispelBurst` + `DispelRibbon`).

### D. Impact: the third hook — SHIPPED

- [x] ~~**A shared, school-coloured impact for projectiles.**~~
      `rendering/effects/school_impact.rs`, 19 probes in
      `tests/school_impact_visual_probes.rs`. One marker, `SchoolImpact`,
      spawned at four resolution sites (`process_projectile_hits`; the
      instant-effect landing in `process_casting` for Mind Blast;
      `process_holy_shock_damage`; `process_mana_burn`, only when mana actually
      burned); one per-school row in `impact_style`, with `landing_style` as
      the per-ability override seam (Mana Burn is its only user: Shadow's
      colour, a chest fan pulled upward, none of Mind Blast's smoulder — the
      client gives it its own `manaburn_chest.m2`); `SchoolImpact::anchor_for`
      is the single list of what lands through it, and the probe
      `every_projectile_in_the_config_reaches_a_landing` fails the moment a new
      `projectile_speed` reaches neither it nor a bespoke burst. The legacy
      `SpellImpactEffect` is gone, and Concussive Shot no longer borrows
      `DispelBurst`. Bench: the School Impact Bench artifact (ports the shipped
      math; every slider is a constant).
- **The client data changed the list.** Aimed Shot, Arcane Shot and Concussive
  Shot share ONE impact model in the source, which is the shared-tier premise
  confirmed. Mind Blast is not a burst at all but a two-second smoulder on the
  HEAD (attachment 20), hence `ImpactAnchor::Head`. Web has NO impact kit —
  only the root state `hard_cc.rs` already draws — so it is deliberately
  absent from `anchor_for`; a generic burst on it would double the landing.
- **Three calls a later change could quietly undo.** Physical is hueless
  (bone-white splinters, no ring) rather than `SpellSchool::Physical`: that tan
  is within 0.07 of the arena floor in RGB, so an additive tan burst is
  invisible on sand — `colour_comes_from_the_school_authority` pins the
  exception. Every live school carries a piece that can DARKEN (lit splinters,
  alpha droplets, Shadow's blot), the Shadow Bolt lesson made structural. And a
  ring is magic language only: Physical and Nature throw material, Arcane
  expands a band, which is what tells an Arcane Shot from an Aimed Shot.
- **Magnitude is now visible in world space.** `SchoolImpact::magnitude` is
  damage over the victim's max health; the flash and spray scale from
  `IMPACT_MAGNITUDE_FLOOR` (aura-only) to full at `IMPACT_MAGNITUDE_FULL`
  (0.20), and a crit multiplies both and throws more debris.
- **The anchor fixed a #116 defect.** The bolt bench drew the unit with its
  feet at the transform; Bevy centres the capsule on it, so the bolt bursts
  shipped at +1.45 — 0.2yd above the top of the head. Both tiers now share
  `IMPACT_CHEST_Y` (+0.55, the upper torso), and `bolt_impact_origin`
  delegates to it. When benching anything anchored to a unit, draw the capsule
  centred. **The dispel ribbon had the same defect twice over:** anchored at
  +1.9 it played entirely in the air above the head, and its coil radius
  (0.35) was INSIDE the 0.5 body capsule, so even on the body it was hidden
  until it cleared the crown — which is why only Purge, starting at chest
  height, ever read. A third: the ribbon was alpha-BLENDED, like the body
  capsule (stealth), and Bevy sorts blended meshes by distance with no depth
  writes — so the capsule painted over the ribbon even where the ribbon was
  nearer the camera, and only the parts outside the body's silhouette showed.
  It is now OPAQUE — and since an opaque strip cannot fade, it PLAYS OUT: at
  `PLAYOUT_FRACTION` of its life the top end fixes in place and the bottom end
  keeps rising, consuming the strip from below like a ribbon drawn through a
  ring, while sparks stream off the fixed top until the bottom has caught up.
  It coils at 0.92 (outside the body), starts at the
  FEET and climbs (the client anchors Dispel Magic, Cleanse and Devour Magic
  at BASE attachment 19) except Purge, which starts around the chest
  (`purge_new_impact_chest.m2`); a FOLD rolls along it from the held end (a
  derivative-of-Gaussian wavelet — lift then pull — launched at birth,
  travelling to the top and attenuating, with a smaller echo behind it) and it
  IGNITES (an emissive spike settling in ~0.3s) to mark the instant. **Not a
  standing wave:** the first ripple was a sinusoid over the whole strip with
  width flutter, and it read as tessellation; a flicked ribbon carries ONE
  local fold, and the band's width never changes. **Do not give a dispel a flash-and-band beat:** one was
  tried and it overpowered the ribbon — a dispel read as a hit with a strip
  behind it. The instant belongs in the ribbon's own vocabulary.
  Graphical-only; 7 probes in `tests/dispel_visual_probes.rs`.
- **Palette, partly settled.** `SpellSchool::color()` is the authority as a
  `const` Bevy colour; the new tier and `FROST_IMPACT_COLOR` read it. The
  bespoke highlight tints (root ice, nova core, the shard and ribbons) are
  deliberate pale cores, not drift, and stay. `NOVA_EDGE_COLOR` is the one
  remaining hand-copy of the Frost authority.

### E. Leftovers, second pass

- [ ] **Nothing flinches when it takes a hit.** No reaction on the victim for
      ordinary damage, so every attack in the game lands soundlessly on the
      body. Potentially the highest leverage on this whole list — one
      receiver-side system covering every damage source, the way
      `animate_body_lean` lifted the entire melee tier — but it lands straight
      in the Transform-channel contention documented in A: the gaits own
      `translation.y`, the death fall owns rotation, the lean owns the
      horizontal step. Answer the channel-ownership question *before* designing
      it, not during.
- [ ] **Shadow-caster hygiene is unaudited.** Before #116, `NotShadowCaster`
      appeared nowhere in the repo — the omission only surfaced because a bolt
      trail painted a dotted black line across the arena floor beside itself.
      Nova crystals, stun beads, charge trails, totems and traps all spawn
      meshes on open ground with the same exposure. Cheap sweep.
- [ ] **The sandbox preview window is only verified for one family.** #116
      fixed projectiles, whose window ended at `cast_time` and so cut off the
      flight and the impact entirely. Whether a `Residue` or `Entity` entry's
      window covers its full visual — a trap arming and later triggering, a
      totem's life — is unchecked. **A worse cousin has since been found and
      fixed:** every dispel entry previewed NOTHING, because the staged aura
      was a buff `can_be_dispelled` rejects, the dummy had no `ActiveAuras`
      container to push into (only a Rogue's weapon poison gets one at spawn),
      and Purge fired unfiltered. No test noticed, because the boot test only
      asked whether the state could be entered.
      `every_dispel_entry_strips_something_in_the_sandbox` now drives each
      entry the way the panel does and asks for the ribbon. **The general
      hole remains:** an entry can be listed as playable and resolve to
      nothing, and only a human at the panel sees it. A per-family "did the
      visual it exists to show actually spawn" test is the right closure.

- [ ] **Nit: `rendering/effects/slow_zone.rs` is misnamed** — it contains the
      Disengage trail, not a slow zone, and there is no slow-zone visual at
      all. Fallout from the effects.rs split (#106). Rename when next touched.

---

## Milestone 2: Visual Polish

- [ ] Procedural character meshes (distinct silhouettes per class)
- [ ] Ability visual effects (AoE indicators, ground effects) — the live list is
      *Animation tier walk: the next candidates* above
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
