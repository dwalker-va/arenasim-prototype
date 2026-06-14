---
date: 2026-06-13
topic: warlock-movement-ai
---

# Warlock on the Posture Movement System (Planted Caster That Kites)

## Summary

Migrate the Warlock off legacy target-pursuit onto the shared ENGAGE/KITE
posture machine, reconfigured as a *planted caster that kites under duress*:
it plants to hardcast Shadow Bolt and Fear by default, and creates distance
(downshifting to instant curses/DoTs) when a melee focuses it. This is a
reconfiguration of the existing DPS-kiter machinery — no new state machine —
and the Warlock's existing `is_being_kited()` ability logic already moves in
lockstep with the KITE posture.

---

## Problem Frame

The posture movement system (context-steering scorer + two posture machines)
now powers Priest, Paladin, Mage, and Hunter. Warlock, Warrior, and Rogue are
still on the legacy pursuit path in `move_to_target`
(`src/states/play_match/combat_core/movement.rs`): with a target, walk toward
it and stop at `preferred_range`. For the Warlock, `preferred_range` is 20
yards, so a focused Warlock plants at 20 yards and face-tanks whatever closes
on it — cloth armor with no repositioning.

The mismatch is that the Warlock is the one ranged class whose damage is
*range-agnostic*: Corruption, Unstable Affliction, Immolate, and the curses
tick regardless of distance, and Shadow Bolt has a generous range with no
minimum. So unlike the Mage and Hunter, the Warlock has no shooting ring to
orbit — its movement problem is "survive the melee train while keeping DoTs
ticking," not "hold a firing band." The ability AI already understands this:
`is_being_kited()` (`src/states/play_match/class_ai/warlock.rs:41`) detects
slowed-and-out-of-range and suppresses interruptible casts (Immolate, Drain
Life, Shadow Bolt) in favor of instants. What's missing is the movement half —
nothing makes the Warlock actually create the distance that logic assumes.

---

## Key Decisions

- **Reuse the DPS-kiter ENGAGE/KITE machine, reconfigured — not a new posture
  set.** The Warlock's two needed states (planted-and-hardcasting vs
  flee-and-instant-cast) map exactly onto ENGAGE/KITE. This reuses the shared
  `evaluate_dps_posture` path, typed transitions, hysteresis, commit windows,
  and `movement_decision` trace events, with a new `warlock` config block
  following the `mage`/`hunter` pattern. The existing `is_being_kited()`
  ability downshift is the ability-side complement and needs no change.

- **Proximity-gated KITE entry, like the Hunter — not aura-gated like the
  Mage.** The Warlock has no self-applied root/slow to key off, so KITE entry
  derives from a melee threat within the kite-entry radius. The threat set
  mirrors Hunter's `melee_within` (Warrior/Rogue; ranged classes, healers,
  pets, and stealthed enemies excluded).

- **No range-band dead zone.** Mage and Hunter use a `range_band` with a
  non-zero minimum (a dead zone they cannot shoot inside). The Warlock casts
  at any range, so its band has no inner dead zone. The band's *outer* bound
  (≈Shadow Bolt range) still matters — but only in the split case below.

- **KITE is a gap-creation cycle when the chaser is the kill target.** When the
  melee chasing the Warlock is also its kill target, the Warlock does not orbit
  at the edge of cast range. It kites until it opens a gap (chaser past the
  kite-sustain radius), then drops to ENGAGE, **plants, and hardcasts Shadow
  Bolt** — accepting that the same melee will re-close and re-trigger KITE.
  "Kite to buy a window, plant and hardcast in the window," driven by the
  standard entry/sustain hysteresis the machine already has.

- **The flee-while-staying-in-cast-range behavior governs only the split
  case.** When the chaser and the kill target are *different* entities (e.g.
  a Warrior chases while the Warlock nukes the enemy healer), the Warlock flees
  the chaser while keeping the kill target inside cast range — `flee` from the
  nearest threat plus the `range_band` outer bound toward the kill target.

- **Reduced burst while kiting is intended counterplay, not a defect.** Moving
  means no hardcast Shadow Bolt; a sustained kite drops the Warlock to DoT +
  instant pressure only. A melee that successfully trains the Warlock denying
  its nuke uptime is the correct outcome, not something to engineer around.

---

## Requirements

**Posture machine**

- R1. The Warlock runs the shared two-posture machine (ENGAGE, KITE) on the
  existing DPS posture machinery: typed transition triggers, hysteresis, commit
  window, and `movement_decision` trace events, matching the Mage/Hunter
  integration.
- R2. KITE entry derives from world-state: a qualifying melee threat
  (Warrior/Rogue, excluding ranged, healers, pets, stealthed) within the
  kite-entry radius. KITE exits when no qualifying threat remains within the
  kite-sustain radius. The Warlock issues no writes to any legacy
  `kiting_timer` channel.
- R3. In ENGAGE the Warlock plants (no posture-driven movement) so the ability
  AI can hardcast Shadow Bolt and cast Fear. In KITE the Warlock moves to
  create distance from the threat, and the existing `is_being_kited()` logic
  governs the ability downshift to instants.

**Scoring / positioning**

- R4. The Warlock's `range_band` has no inner dead zone (minimum 0 or melee
  range); its outer bound is approximately Shadow Bolt cast range.
- R5. When the chasing threat is the kill target, the entry/sustain hysteresis
  produces a plant-once-gap-opened cycle: KITE until the threat is past the
  sustain radius, then ENGAGE and hardcast. The plant window must be long
  enough to land a Shadow Bolt at the shipped tuning.
- R6. When the chasing threat and the kill target are different entities, the
  Warlock flees the threat (`flee` term) while keeping the kill target within
  the `range_band` outer bound, so DoT/instant pressure on the target
  continues.
- R7. Warlock movement weights and posture parameters live in a `warlock` block
  in `assets/config/movement.ron` with struct defaults and startup validation,
  following the existing per-class pattern.

**Diagnosis parity**

- R8. Warlock movement is diagnosable with the same tooling as the other
  posture classes: `movement_decision` events with `scorer_terms` breakdowns
  and posture transitions appear in traces, and `scripts/movement_kpis.sh`
  covers the Warlock without modification.
- R9. A `warlock_postures` probe module pins KITE entry on melee proximity,
  the plant-once-gap-opened cycle (R5), and the split-case flee geometry (R6)
  at fixed seeds, using the observed-run harness.

---

## Key Flows

- F1. Focused by its own target (chaser == kill target)
  - **Trigger:** A Warrior/Rogue closes to within the kite-entry radius; that
    same enemy is the Warlock's kill target.
  - **Steps:** Warlock enters KITE, flees, casts instants (curses/DoTs) per
    `is_being_kited()`. Once the chaser falls past the kite-sustain radius, the
    Warlock returns to ENGAGE, plants, and begins a Shadow Bolt hardcast. The
    chaser re-closes; KITE re-triggers.
  - **Outcome:** A repeating kite-then-plant cycle; Shadow Bolt lands in the
    gaps, DoTs tick throughout.
  - **Covered by:** R2, R3, R5

- F2. Focused while nuking a different target (chaser != kill target)
  - **Trigger:** A Warrior chases the Warlock while the Warlock's kill target
    is the enemy healer.
  - **Steps:** Warlock enters KITE; the scorer flees the Warrior while the
    `range_band` outer bound keeps the healer in cast range. The Warlock keeps
    instant DoT/curse pressure on the healer while moving.
  - **Outcome:** Warlock maintains distance from the chaser without dropping
    out of cast range of its actual target.
  - **Covered by:** R6

- F3. Not focused
  - **Trigger:** No qualifying melee within the kite-entry radius.
  - **Steps:** Warlock stays in ENGAGE, plants, hardcasts Shadow Bolt, and uses
    Fear per the existing ability AI.
  - **Outcome:** Maximum damage / peel uptime when safe.
  - **Covered by:** R3

---

## Acceptance Examples

- AE1. Plant window is castable.
  - **Covers R5.** Given the chaser is the kill target, when the Warlock opens
    a gap and returns to ENGAGE, then the plant window at shipped tuning is long
    enough to complete at least one Shadow Bolt cast before KITE re-triggers
    (asserted in the `warlock_postures` probe).

- AE2. Split-case stays in cast range.
  - **Covers R6.** Given a chaser distinct from the kill target, when the
    Warlock kites, then it remains within `range_band` outer bound of the kill
    target for the duration of the chase window (proximity KPI on the kill
    target, not the chaser).

- AE3. No dead-zone retreat.
  - **Covers R4.** Given a melee adjacent to the Warlock, when the Warlock
    kites, then it does not path *toward* the kill target to satisfy a minimum
    range (no inner dead zone) — it only ever opens distance from the threat.

---

## Scope Boundaries

**Deferred for later (fast-follow / v2)**

- The "kite when a teammate CCs my chaser" opening trigger — proactively
  extending the kite window when an ally lands Frost Nova / Spider Web / a trap
  on the melee focusing the Warlock. Deferred to a fast-follow once the core
  is validated. (Base behavior degrades gracefully without it: a CC'd chaser
  stops following, so proximity-gated KITE relaxes and the Warlock gains
  distance anyway.)
- Fear-aware pathing — making movement account for where a feared target will
  flee so the Warlock doesn't kite into it. v2 refinement.
- Low-HP Drain Life planting — deliberately creating a safe window to channel
  the Drain Life self-heal. v2; today's `is_being_kited()` already suppresses
  Drain Life while kited, and the Warlock will Drain when planted and safe.

**Outside this work**

- Enemy melee-pet kiting — the threat set excludes pets, consistent with the
  Hunter's existing known limitation; folding pets in is a shared follow-up,
  not Warlock-specific.
- Line-of-sight / pillar play — a whole-system gap affecting every posture
  class, not part of the Warlock migration.
- Warrior / Rogue migration — separate work; Rogue in particular needs new
  scorer vocabulary (stealth approach) the current terms don't express.

---

## Success Criteria

- Survivability: the Warlock's time spent within melee range while focused
  drops materially versus the legacy-pursuit baseline (post-gate proximity
  KPIs from `scripts/movement_kpis.sh`), and post-gate path length rises when
  focused (it actually repositions).
- Damage posture: the Warlock still lands Shadow Bolt hardcasts in the plant
  windows (F1) and maintains DoT/instant pressure on its kill target in the
  split case (F2) — survival does not come at the cost of zero damage.
- No collateral regression: a matrix sweep shows the Mage, Hunter, Priest, and
  Paladin unchanged (they share machinery but not config), and Warlock matchup
  shifts are explainable by the new movement rather than incidental.
- Tuning: the kite-sustain radius and `directive_ttl` are the knobs that set
  how often the Warlock gets to hardcast; they are settled in the matrix sweep,
  not pre-committed here.

---

## Dependencies / Assumptions

- Builds directly on the shipped context-steering scorer and DPS-kiter posture
  machine (`evaluate_dps_posture`, `DpsMovementConfig`, `MovementDirective`).
  No scorer-term additions are required — `flee` and `range_band` already
  exist; this is configuration plus a Warlock posture-evaluation entry point.
- Assumes the existing `flee` + `range_band` outer-bound combination resolves
  the split case (R6) without a new term. If probes show the two terms fight in
  a way tuning can't settle, that becomes an Outstanding Question for planning.
- Assumes Shadow Bolt's cast time and the achievable plant window are
  compatible — i.e. a gap large enough to be worth planting is also large
  enough to land a Shadow Bolt. R5 / AE1 verify this; if false, the tuning
  target (sustain radius, TTL) absorbs it.

---

## Sources / Research

- `src/states/play_match/class_ai/warlock.rs` — current Warlock ability AI;
  `is_being_kited()` at line 41 is the ability-side complement to KITE.
- `src/states/play_match/class_ai/dps_postures.rs` — shared ENGAGE/KITE
  `evaluate_dps_posture`; Hunter's `melee_within` proximity gating is the
  pattern R2 follows.
- `src/states/play_match/combat_core/movement_scoring.rs` — `score_directions`,
  the `flee` and `range_band` terms, and the boundary/anchor masks.
- `src/states/play_match/movement_config.rs` — `DpsMovementConfig` struct and
  per-class config loading/validation.
- `assets/config/movement.ron` — `mage` and `hunter` blocks are the template
  for the new `warlock` block.
- `src/states/play_match/combat_ai.rs` — per-class movement dispatch (~line
  591+) where the Warlock branch wires in.
- `docs/brainstorms/2026-06-12-context-steering-masks-requirements.md` — the
  Mage pilot that established this migration pattern.
- `scripts/movement_kpis.sh`, `tests/movement_probes.rs` — diagnosis and
  probe harness for R8 / R9.
