# Healer crowd-control avoidance (AS-362): design

Status: **SIGNED OFF (team AI only) and BUILT** — see §10 for what was built
and measured. §§1-9 are the proposal as signed off; their measurements are on
`main` @ `5e13a22a`. The user's ruling (2026-10-18): `TeamPlan` only, `Legacy`
stays byte-identical, hard CC only, out-of-reach above cover and below anchor
sight, the Paladin reached only in PRESSURED; the `Legacy` Priest dip abort and
`last_stand_cover`'s projection are their own cards.

## 1. The problem in one paragraph

A healer's position decides which enemy crowd control (CC) can reach it, and
nothing in either AI profile positions by that. Twice now a change that fixed
something else turned out to have been CC avoidance by accident, and removing
it cost the healers:

- **AS-249** removed the `Legacy` range-edge bob, which had kept healers near
  the edge of a closing caster's range. That cost the Priest -2.1pt on
  BasicArena and the Paladin -1.7pt on Nagrand.
- **AS-292** (parked, PR #350) removes the false pressure an enemy healer
  exerted from 28-30yd, which had kept the `TeamPlan` healer backing off.

This card replaces both accidents with a deliberate rule.

## 2. Measurements on current main

The bank is every 2v2 team with a healer against every 2v2 team (no double
healers), on both maps, seeds 1-3, with both teams on the same profile:
3,600 matches and 6,000 healer-matches per profile. The harness is
`tests/healer_cc_exposure.rs` (ignored; a measurement). The inputs, rows and
scripts are under `docs/design/balance/sweeps/2026-10-18-as362-*`. Every
figure is per healer-match.

### 2.1 Time CC'd, per healer

Hard CC means stun, fear, polymorph, incapacitate or cyclone, as observed frame
by frame, so a fear that broke on damage counts only for as long as it held.

| profile | map | Priest | Paladin | Shaman | Druid | all | % of alive time |
|---|---|---|---|---|---|---|---|
| Legacy | BasicArena | 3.82s | 3.12 | 3.74 | 3.03 | 3.43 | 6.4% |
| Legacy | PillaredArena | 3.98 | 3.46 | 3.71 | 3.22 | 3.59 | 5.5% |
| TeamPlan | BasicArena | 3.99 | 3.27 | 3.28 | 2.57 | 3.28 | 6.4% |
| TeamPlan | PillaredArena | 3.39 | 2.96 | 3.58 | 2.80 | 3.18 | 4.2% |

By aura type, all healers (seconds per healer-match):

| profile | map | Stun | Fear | Poly | Incap (trap) | Cyclone | Root | interrupt lockout |
|---|---|---|---|---|---|---|---|---|
| Legacy | BasicArena | 1.64 | 0.96 | 0.64 | 0.21 | 0.01 | 0.67 | 1.46 |
| Legacy | PillaredArena | 1.82 | 0.95 | 0.67 | 0.17 | 0.01 | 0.69 | 1.66 |
| TeamPlan | BasicArena | 1.47 | 1.03 | 0.56 | 0.22 | 0.02 | 0.60 | 1.55 |
| TeamPlan | PillaredArena | 1.46 | 0.93 | 0.61 | 0.19 | 0.01 | 0.82 | 1.62 |

The Nagrand camp comp (`camp_sweep`, Warrior+Priest v Warlock+Priest, seeds
1-12) matches AS-249: the `Legacy` Priest spends 8.1s per match hard-CC'd or
rooted and heals its Warrior 243. The `TeamPlan` Priest spends 6.7s and heals
170.

### 2.2 By CC type and source

A landing is a `[CC]` log line on the healer. **src** is the nearest living
enemy of the ability's class at that moment; **src-ally** is that enemy's
distance to the healer's nearest teammate; **step** is how many yards inside
the ability's reach the healer stood. All three are medians.

| profile | map | ability (reach) | landings per healer-match facing it | src | src-ally | step |
|---|---|---|---|---|---|---|
| Legacy | BasicArena | Fear (30) | 0.74 | 22.0 | 25.0 | 8.0 |
| Legacy | BasicArena | Polymorph (30) | 0.45 | 21.6 | 27.5 | 8.4 |
| Legacy | BasicArena | Psychic Scream (8) | 0.09 | 4.8 | 17.6 | 3.2 |
| Legacy | BasicArena | Hammer of Justice (10) | 0.58 | 9.6 | 14.8 | 0.4 |
| Legacy | BasicArena | Kidney Shot (2.5) | 0.61 | 2.0 | 17.4 | 0.5 |
| TeamPlan | BasicArena | Fear | 0.79 | 24.1 | 27.9 | 5.9 |
| TeamPlan | BasicArena | Polymorph | 0.43 | 23.7 | 27.5 | 6.3 |
| TeamPlan | PillaredArena | Fear | 0.64 | 19.2 | 25.0 | 10.8 |
| TeamPlan | PillaredArena | Polymorph | 0.44 | 20.7 | 25.0 | 9.3 |

PillaredArena under `Legacy` reads like BasicArena. The full table, with every
ability and both maps, is in the reducer's output.

Nominal CC seconds (the sum of the logged, DR-scaled durations) split into two
families:

- **Chaser:** melee-range stuns (Kidney Shot, Cheap Shot).
- **Reach:** everything that has a reach disc a healer can stand outside.

| profile | map | chaser | reach | of which caster CC (Fear, Poly, Death Coil, Cyclone, Roots) | Scream | HoJ |
|---|---|---|---|---|---|---|
| Legacy | BasicArena | 1.17 | 3.48 | 2.36 | 0.14 | 0.69 |
| Legacy | PillaredArena | 1.37 | 3.26 | 2.22 | 0.13 | 0.68 |
| TeamPlan | BasicArena | 1.18 | 3.14 | 2.25 | 0.10 | 0.47 |
| TeamPlan | PillaredArena | 1.18 | 2.86 | 1.98 | 0.12 | 0.42 |

How much of a healer's alive time it stands inside an enemy CC's reach (in
range and in line of sight, cooldown ignored), measured against the healers
whose enemy fields that class:

| profile | map | Fear | Polymorph | Cyclone | HoJ | Scream |
|---|---|---|---|---|---|---|
| Legacy | BasicArena | 45% | 41% | 38% | 35% | 7.7% |
| Legacy | PillaredArena | 34% | 33% | 31% | 30% | 6.1% |
| TeamPlan | BasicArena | 57% | 40% | 33% | 36% | 8.3% |
| TeamPlan | PillaredArena | 30% | 25% | 16% | 10% | 3.4% |

### 2.3 What the numbers say

1. **About 70% of a healer's nominal CC is reach-type**, and caster CC alone
   accounts for about half of it (2.0-2.4s of 4.3-4.7s). Melee stuns are about
   1.2s, and no static position avoids them: avoiding a chaser is kiting,
   which the posture machines already do.
2. **Caster CC lands from a caster standing 20-24yd off, with the healer's
   teammate 25-28yd from that caster.** The teammate is *not* fighting at the
   caster's feet: the caster sits at its own preferred range (28yd) from its
   kill target. A spot 6-11yd further from the caster, still well inside heal
   range of the teammate, would have been out of reach. That is the AS-249
   finding, quantified. It also identifies the culprit: the FREE formation
   point sits `formation_offset` behind the teammate, *away from the enemy
   nearest that teammate*. That enemy is the melee in contact, not the caster,
   so the offset often points the healer toward the caster.
3. **Psychic Scream is rare** (0.07-0.09 landings per healer-match). The
   BasicArena seed-14 park that opened this card is not a Scream case: in that
   window (AS-292's build, combat 8.6-21.2s) the enemy Priest's Scream was on
   cooldown (used at 5.7s), and from 13.7s our Priest hard-cast Mind Blast and
   four Flash Heals back to back. A cast plants the caster, and that, not the
   solve, is what reads as a 0.22 u/s statue. A readiness-aware rule would
   rightly not move it. The statue probe needs a better measurement (§6).
4. **AS-292 removes accidental avoidance.** The same `TeamPlan` bank, with
   AS-292's rule merged onto this `main`:

   | map | hard CC | reach-type nominal | Scream | heal to teammates |
   |---|---|---|---|---|
   | BasicArena | 3.28 → 3.41s | 3.14 → 3.50 | 0.10 → 0.19 | 318 → 295 |
   | PillaredArena | 3.18 → 3.64s | 2.86 → 3.50 | 0.12 → 0.24 | 322 → 299 |

   Wins in the bank are unchanged (1546 → 1549, 1561 → 1562), as in AS-292's
   own head-to-head. AS-292 should land with or after this card, as already
   ruled: this card gives the healer the place to stand that the false
   pressure used to give it by accident.
5. **Both profiles bob more than AS-249's target.** Heading reversals per
   minute in this bank are `Legacy` 6.2 (BasicArena) and 5.3 (PillaredArena),
   and `TeamPlan` 8.2 and 6.8. The Druid alone runs at 9-11 per minute in
   every cell. The metric is AS-249's (a 0.5s step turning back within 45°),
   but the bank is wider than AS-249's 48 matches. These are the anti-strobe
   baselines this card must not exceed (§5).

## 3. Which CC counts

**One rule, derived from data, never a hardcoded list.** A **CC threat** is a
pair (enemy unit E, ability A) where:

- A is in E's kit: `AbilityDefinitions::own_abilities_for_class` for a non-pet,
  and `pet_abilities_for_class` filtered by `pet_type` for a pet. Its centre is
  E's own position, so a Boar's Charge is centred on the Boar.
- A applies a **hard CC** aura (`Stun`, `Fear`, `Polymorph`, `Incapacitate`,
  `Cyclone`): the auras that stop heals. The predicate already exists in spirit
  as `is_ccd`; this card names it once (`AuraType::takes_control`) and uses it
  everywhere.
- **Reach** = A's `range`. A trap adds its trigger radius, because Freezing
  Trap is thrown up to `range` and springs within its radius of where it lands.
- **Ready** = E could start A within the horizon `H` (`movement.ron`
  `shared.cc_ready_horizon`, default 1.5s):
  - A's cooldown remaining, category included, read from
    `CombatContext::ability_cooldowns`, is at most `H`;
  - E can pay A's cost;
  - E is not itself hard-CC'd, silenced or locked out of A's school;
  - the healer is neither DR-immune to A's category (`is_dr_immune`) nor immune
    (`entity_is_immune`; Berserker Rage's fear immunity counts for a fear).
  - This mirrors `cast_guard::unreached_reason`'s order, asked from E's side.
    It is a new pure function, `cc_reach::could_cast(ctx, E, A, H)`, so the
    two cannot drift.
- **Seen**: E is in `ctx.combatants`, which is the deciding unit's perception,
  so a stealthed Rogue contributes nothing.
- **Sight, where the ability needs it.** A targeted CC (Fear, Polymorph,
  Cyclone, Hammer of Justice, Death Coil) covers a spot only when E has line of
  sight to it, because the cast gate refuses it otherwise. That makes cover
  and distance two answers to the same threat, which is what lets the rule
  cooperate with `OccupyCover` rather than fight it. An area effect centred on
  E (Psychic Scream, Frost Nova: `area_victims` takes no sight) and a trap
  cover their disc regardless of sight. The distinction is read from the same
  config fields the cast gate reads, not from a list. (The exposure figures in
  §2.2 required sight for every ability, so on PillaredArena they slightly
  understate Scream and Frost Nova.)
- **Weight** = A's duration × the healer's DR multiplier for its category: the
  control-seconds that landing would cost. A fresh Polymorph (10s) outranks a
  Death Coil (3s), and a third fear (25% DR) barely registers.

**Deliberately out:**

- Roots: a rooted healer still heals.
- Interrupt lockouts: Spell Lock reaches 30yd with the same disc logic, but it
  only bites a healer that is casting. That is a follow-up card if the bank
  shows it is worth it.
- The melee-range stuns are not excluded by name. Their discs (2.5yd plus the
  margin) are simply small, so in practice they only add to the threat
  repulsion that already exists.

**Margin.** A disc is `reach + cc_reach_margin` (`movement.ron`
`shared.cc_reach_margin`, default 3yd). That is about a commit window of a
caster walking in at base speed, so the healer is not caught between
re-evaluations. A disc carries hysteresis: a spot counts as *covered* inside
`reach + margin`, but a healer already outside `reach + margin/2` is not moved.
This is a Schmitt pair, the same shape as AS-249's onset gap.

## 4. How it enters positioning

One module, `class_ai/cc_reach.rs`, owns the threat set and three queries:

- `threats(ctx, healer) -> Vec<CcThreat>`
- `covered(p) -> f32`: the summed weight of the threats covering spot `p`
- `deficit(p) -> f32`: how many yards `p` sits inside the nearest covering
  disc, which gives the gradient out

Every consumer reads these three queries. None of them re-derives a threat,
which is the "read a situation, never re-derive one" rule applied to space.

### 4.1 TeamPlan PRESSURED: a constraint in `OccupyCover` (`team_solve`)

A new bit, **`C_CC_REACH`**, is set when a candidate is covered. It is scored
as `W_CC × covered + W_STANDOFF × deficit`, with `W_CC` between sight and
cover. The proposed value is 200: below `W_SIGHT` (400), above `W_OCCLUDED`
(100 per caster). The order becomes:

> owed dispel ≫ dying teammate's reach ≫ **sight of the anchor** > **out of CC reach** > occluded from casters > leash > cohesion

- **Heal reach comes first by construction.** A candidate beyond `heal_range`
  of the anchor scores as covered whatever its true distance. Leaving heal
  range therefore never earns CC credit, which is the card's "a healer that
  avoids all CC by standing out of range has failed", written as arithmetic.
- **A cover spot satisfies both** the occlusion and the CC constraint against
  a caster, because a targeted CC needs sight. On a pillar map the solve's
  equilibrium is unchanged wherever it was already hidden. The exception is an
  enemy Priest or Mage within its area disc, whose Scream or Nova cover
  ignores; only distance answers those. The new term binds where cover is
  missing or partial: BasicArena, or a Nagrand pillar whose shadow sits inside
  Fear range.
- **It ends the statue the danger key was written for**, wherever a CC threat
  is ready. A covered stand-still candidate costs more than a step out, so the
  field is no longer flat and the gradient points out. The flat-field danger key
  stays as it is for the melee-only case, where no disc of any size applies.
- **Candidates:** for each threat, one point on the disc's rim at
  `reach + margin`, on the ray from E through the healer. These are anchored to
  world geometry, as `candidates_for` requires, not to a lattice around the
  mover. They are projected before scoring (AS-201).

### 4.2 FREE formation point (Priest, Shaman, Druid; both profiles if ruled, §7)

The repair is a **reach repair**, the same shape as AS-206's sight repair: a
constraint with a nearest-feasible repair, not a push.

- **Feasible:** within `heal_range − HEAL_REACH_MARGIN` of every living
  teammate the raw point was within range of, and not covered.
- **Candidates:** the raw point, plus each covering disc's rim points nearest
  the raw point and nearest the healer, plus the intersections of a disc's rim
  with the heal circle. Take the feasible candidate nearest the raw point; the
  kept candidate counts `KEEP_MARGIN` nearer.
- **The repair composes with the sight repair** rather than running after it,
  which could undo it. The candidate set is the union of both repairs'
  candidates, and feasibility is both predicates. `formation::repair(raw,
  &[Sight, Reach])` generalises `sighted_formation_point` so the two cannot be
  sequenced wrong.
- **No feasible spot:** the heal discs of the teammates lie wholly inside the
  union of covering discs, for example a 3v3 with casters on both flanks. The
  raw point then stands: heal first. Feasibility itself has hysteresis: the
  repair resumes only when a feasible spot has 1yd of slack.
- **Why this is not AS-249's jittering standoff.** AS-249 pushed the point
  radially from the caster through the teammate, and that direction swings
  wildly when the teammate fights at the caster's feet. The repair picks the
  nearest point of a region instead. The region is an annulus segment round
  the caster, clipped by the heal circle, and its nearest point slides
  continuously as the caster and the teammate move. Only a change of
  feasibility or of the kept candidate is a step, and the band and the slack
  hysteresis cover both.
- **AS-370**, the continuous direction of the formation point, changes the raw
  point. This repair runs on whatever raw point AS-370 produces, so the two
  compose without touching each other.

### 4.3 Legacy PRESSURED scorer: an interest term

A new per-class weight, **`cc_reach`**, in `movement.ron` (`priest`, `paladin`,
`shaman` and `druid` blocks). Each of the 16 lookahead candidates scores
`−cc_reach × (covered(c) + deficit(c))`.

- It is a soft term, so the masks still win: the heal-range anchor mask
  (AS-249's `close_when_out` rung included) and the boundary mask outrank it.
  Heal reach first, again by construction.
- It sits beside `threat_repulsion`, which already pushes away from whoever
  targets the healer. The difference is that `cc_reach` names the casters that
  can take control *and are ready to*, and it cares where the healer stands
  relative to their reach, not how near they are.
- The Paladin's band-hold parks at `fallback_range`. Under this design it parks
  at the nearest uncovered spot on the band instead, when one exists.

### 4.4 Options weighed

| shape | verdict |
|---|---|
| **Posture input** (a ready CC in reach flips PRESSURED) | Rejected. PRESSURED means "retreat and hold the hysteresis floor". A caster sitting at its reach edge would strobe the posture, which is the exact AS-249 bob. Reach belongs inside each posture's goal, not in the posture choice. |
| **Radial push of the formation point** away from the caster | Rejected. This is AS-249's tried and failed shape: ill-conditioned when the teammate fights at the caster's feet. |
| **One global scored field replacing the formation point and the solve** | Rejected for now. It is the utility-AI rewrite the 2026-10-11 architecture ruling declined. |
| **Constraint plus nearest-feasible repair in FREE, a constraint in the `TeamPlan` solve, an interest term in the `Legacy` scorer, all reading one threat set** | **Recommended.** Each consumer already has this shape (AS-206's repair, the solve's bit ladder, the scorer's terms), so each change is small. One threat set means one definition of "which CC counts". |

## 5. Not strobing

| source of strobing | guard |
|---|---|
| A threat's readiness flips (cooldown) | The `H` horizon counts a cooldown about to end as ready, so the healer does not walk back in just before it comes off. Cooldown CC cycles at most once per cooldown (15s or more). The casters' Fear, Polymorph and Cyclone have no cooldown, so their discs only appear or vanish with mana, CC or DR. |
| The healer hovers on a disc's rim | Schmitt pair: covered at `reach + margin`, not moved while outside `reach + margin/2`. |
| The FREE point jumps between repairs | AS-206's `KEEP_MARGIN` and `KEEP_RADIUS` on the kept candidate; feasibility slack. |
| The teammate fights at the caster's feet | Nearest point of a region, not a direction (§4.2). |
| The solve chases a moving optimum | Unchanged: nearest-satisfying tie-break plus commit window. A satisfied healer stands still. |

**Measured, not argued.** The bank's reversals per minute (§2.3 item 5) are
the baselines, and the build must not raise them in any healer × profile ×
map cell. A staged pair holds the AS-249 shape: a caster closing on a healer
whose teammate fights at the caster's feet, scored for reversals in a 10s
window against the same stage with the rule withheld.

## 6. AS-292's three probes, and the statue guard

The `TeamPlan` probes AS-292 moved are still at their original thresholds on
`main`. This design should let AS-292 land without rebaselining them:

- **`teamplan_healer_is_not_a_statue_on_basicarena`** gets a new measurement
  that guards the flat-field key itself, as the card asks. The proposed
  measurement:

  > On frames where the healer is PRESSURED, **not casting**, not CC'd, and a
  > melee enemy stands within `danger_radius`, the healer's movement directive
  > aims it at a spot at least 0.5yd away.

  - The share of such frames is the probe's number.
  - Both solve-level mutants fail it by construction: disarming the key
    leaves the goal on the spot, and so does holding the solve.
  - A healer planted by its own cast, as in seed 14, is excluded rather than
    excused.
  - This needs `casting: bool` on `ObservedCombatant` (read-only, like
    `point_goal`), and the posture from the trace.
  - The build plants both mutants and shows each fails before the floor is
    fixed. The floor is derived from the shipped distribution with the
    headroom stated, per the probe's own doc. The old path-speed figure stays
    as a printed context line.
- **`teamplan_healer_keeps_its_heal_line_on_nagrand`** (ceiling 35%) and
  **`juke_bounded_over_the_seed_bank`** (every dance) are re-measured with this
  card's rule plus AS-292. They are restored if green. If not, the build
  reports what the new behaviour does instead and argues it. Neither is
  rebaselined silently.

## 7. Which profile

**`TeamPlan`:** all of it (§4.1, §4.2, and the solve term), behind
`ai_profile.is_team_plan()` like AS-206.

**`Legacy`:** this is the decision in §9. The ruling folding AS-328 in asks
for "a single crowd-control-avoidance design [that] covers both AI profiles",
and AS-328's done-when is that "the Legacy Priest and Paladin costs come back".
That cannot happen while `Legacy` stays byte-identical. The recommendation is
to land it in both profiles, keeping the gate per profile so each is measured
as its own arm:

- The `TeamPlan` arm is built and measured first.
- The `Legacy` arm (§4.2 and §4.3) is then switched on as a deliberate
  `Legacy` change, like AS-249. The `Legacy` baseline and
  `determinism_pin` are re-recorded in that commit.

## 8. Build plan (for after sign-off)

1. **`cc_reach.rs`:** threats, `could_cast`, covered and deficit, with unit
   tests for:
   - cooldown, mana, a silenced or CC'd enemy, the healer's DR immunity and
     fear immunity;
   - stealth (the unit is not perceived);
   - line of sight;
   - a pet's ability centred on the pet;
   - a trap's reach including its trigger radius.
2. **Scenes,** each paired with its control (one condition removed):
   - an enemy Priest with Scream ready, 3yd from a PRESSURED healer with no
     cover (control: Scream on cooldown);
   - a Warlock at 24yd with Fear ready (control: the Warlock silenced);
   - a Paladin at 9yd with HoJ ready (control: HoJ on cooldown).
3. **Staged matches (no pinned seeds):**
   - the same three situations over time;
   - the closing caster with the teammate at its feet (the anti-strobe pair);
   - a melee with a stun ready;
   - each against the same stage with the rule withheld.
4. **Measure:**
   - this bank before and after, per profile changed: CC time per healer per
     match, heal to teammates and reversals per minute;
   - `camp_sweep`;
   - a DIRECTIONAL head-to-head scored from the healer side per profile
     changed (`gen_sweep.py --affects Priest,Paladin,Shaman,Druid`, both
     maps);
   - AS-292's three probes (§6).
5. **The `Legacy` arm** as its own commit and measurement, if ruled.

## 9. Decisions for the user

1. **Does `Legacy` change?** *Recommended: yes.* Land the FREE reach repair and
   the PRESSURED scorer term in both profiles, `TeamPlan` first, each gated and
   measured as its own arm, with `Legacy` re-baselined deliberately as AS-249
   was. Without this, AS-328's done-when (the `Legacy` Priest and Paladin costs
   return) cannot be met. The alternative is `TeamPlan` only: `Legacy` stays
   byte-identical and keeps AS-249's cost.
2. **Which CC counts?** *Recommended: hard CC only,* derived from the config
   (§3). Roots and interrupt lockouts are left for a follow-up card if the
   bank shows they matter.
3. **Rank against cover in the solve?** *Recommended: out of CC reach above
   occlusion, below sight of the anchor (§4.1).* Losing the healer for 6-10s
   costs the team more than one caster's damage line, and against a caster
   cover satisfies both anyway.
4. **The Paladin.**
   - *Recommended: its FREE melee pursuit is left alone* (melee identity).
     The rule reaches it in PRESSURED (scorer, band-hold, solve).
   - On the open question (a healing Paladin counts as melee pressure from
     30yd): *recommended no change.* A FREE Paladin really does walk to its
     target. Its stun is now represented by its Hammer of Justice reach,
     which is the threat that matters.
5. **The `Legacy` Priest dip abort.** Between the trigger and the onset, the
   dip neither aborts nor turns PRESSURED: `dip_aborts` requires `!trigger`.
   So a caster in the onset gap suspends the budget and the dying-teammate
   abort. *Recommended: fix it in this card* by evaluating the aborts whenever
   onset has not pre-empted. It is a one-line, deliberate `Legacy` change that
   is otherwise unrelated, so it rides with the `Legacy` arm.
6. **AS-201's `last_stand_cover` projection.** *Recommended: a separate small
   card.* TakeCover's solver is untouched by this design.

## 10. As built (2026-10-19)

`TeamPlan` only, as ruled. `Legacy` builds no field.

### 10.1 What differs from the proposal

- **The threat set** is §3 as written: `class_ai::cc_reach` (`CcField`), with
  `holds` (stun, fear, polymorph, incapacitate, cyclone) as the hard-CC
  predicate and `cc_reach::aim`, an exhaustive match, deciding sight (a unit
  target) against an area round the caster or a thrown trap.
- **The solve.** `C_CC_REACH` as §4.1, plus one rule measurement forced: sight
  outranks cover and CC *together*. With 200 for CC beside 100 per caster, a
  spot that saw the Warrior but was seen by two casters and covered cost more
  than a blind, hidden one, and a Priest stood 8s blind behind Nagrand's camp
  pillar. While a field is in play a blind spot pays the CC ceiling on top of
  `W_SIGHT`. The anti-statue key is armed on the flatness of everything but the
  CC term, so a melee's 5.5yd stun disc does not disarm it.
- **The FREE repair** (Priest, Shaman, Druid; never the Paladin) is §4.2's
  nearest feasible spot, composed after the sight repair (sight no worse than
  the sight-repaired point is part of feasibility), with three additions the
  strobe stage needed:
  - a spot the healer would walk *through* a disc to reach ranks after every
    spot it would not (the far rim of a caster at the teammate's feet);
  - with no other spot, a healer already out of reach, in heal reach and sight,
    stays where it stands;
  - the spot last chosen stands while it is still a repair (sticky), instead of
    following the caster's every step.
  The repair's spot is taken in full to 24yd and fades out by 30yd
  (`REACH_REPAIR_FULL` / `REACH_REPAIR_LIMIT`), wider than the sight repair's
  10/16: with the teammate fighting near the caster the nearest spot out of a
  30yd reach is 20-25yd off.
- **A parked `TeamPlan` healer keeps its formation walk alive.** It used to
  lapse every directive TTL, hand the legs to target pursuit for a step, and
  come back: about a heading reversal a second. Healers the rule keeps FREE
  longer (a closing caster held out of reach no longer pressures them) showed
  it most; it was the whole of the reversal rise the first build measured.
- **The Paladin's band-hold** steps aside while the spot it would park on is
  covered (inner disc).

### 10.2 Measured

`TeamPlan` CC-exposure bank (§2's bank, 6,000 healer-matches per arm; base =
`main` @ `55849194`, rows `balance/sweeps/2026-10-19-as362-teamplan-*`), per
healer-match:

| map | healer | hard CC | reach-type nominal | heal to teammates | reversals/min | wins |
|---|---|---|---|---|---|---|
| BasicArena | Priest | 3.93 → 2.78s | 3.38 → 2.10 | 201 → 259 | 7.8 → 6.8 | 356 → 386 |
| BasicArena | Paladin | 3.26 → 3.09 | 4.05 → 3.78 | 436 → 425 | 8.7 → 8.3 | 475 → 461 |
| BasicArena | Shaman | 3.28 → 2.17 | 3.12 → 1.69 | 135 → 217 | 3.4 → 3.1 | 318 → 346 |
| BasicArena | Druid | 2.60 → 1.85 | 1.96 → 1.27 | 506 → 560 | 10.8 → 4.4 | 397 → 353 |
| BasicArena | all | 3.27 → 2.47 | 3.13 → 2.21 | 320 → 365 | 8.2 → 5.8 | 1546 → 1546 |
| PillaredArena | Priest | 3.44 → 2.19 | 3.05 → 1.44 | 234 → 298 | 5.3 → 4.4 | 384 → 385 |
| PillaredArena | Paladin | 3.02 → 3.03 | 2.78 → 2.92 | 367 → 361 | 6.1 → 6.9 | 478 → 474 |
| PillaredArena | Shaman | 3.62 → 2.18 | 3.45 → 1.72 | 189 → 268 | 4.7 → 4.3 | 330 → 344 |
| PillaredArena | Druid | 2.84 → 2.44 | 2.34 → 1.93 | 484 → 545 | 10.4 → 5.6 | 369 → 365 |
| PillaredArena | all | 3.23 → 2.46 | 2.90 → 2.00 | 319 → 368 | 6.8 → 5.4 | 1561 → 1568 |

The Paladin is the one cell whose reversals rose (Nagrand 6.1 → 6.9). In the
matches that rose most, a Paladin left alone runs the TakeCover play and
`last_stand_cover` flips its goal between two projected spots on one pillar
face: the `last_stand_cover` projection defect split out of this card, which a
Paladin living longer shows more.

DIRECTIONAL head-to-head, `TeamPlan` against `Legacy`, both seat assignments,
16,160 configs per map, scored from the `TeamPlan` side where it fields a
healer (n = 12,000 per map; `2026-10-18-as362-h2h.py`):

| map | all | Priest | Paladin | Shaman | Druid |
|---|---|---|---|---|---|
| BasicArena | 52.1 → 53.8% (+1.8pt, z = +4.5) | +1.0 (+1.2) | +0.2 (+0.3) | +5.4 (+6.5) | +0.5 (+0.6) |
| PillaredArena | 54.0 → 55.1% (+1.1pt, z = +3.0) | +2.5 (+2.9) | -0.8 (-1.5) | +0.7 (+0.8) | +2.1 (+3.3) |

The 4,160 matches per map whose `TeamPlan` side fields no healer are identical
in winner and duration. Directional: which way, roughly how much; not a
class's standing.

`camp_sweep` (Warrior+Priest v Warlock+Priest, Nagrand, seeds 1-12), `TeamPlan`
Priest: hard CC 5.7 → 3.6s per match, heal to the Warrior 181 → 241, Warlock
denied after contact 12.1 → 14.3s, heal line occluded 25 → 23%. `Legacy`
unchanged (8.1s, 243).

`Legacy`: 4,848 match logs (1,616 per map on BasicArena, TwinPillars and
PillaredArena, 2v2s a healer change reaches plus controls) byte-identical to
the base, carrying 370k damage lines, 30k crits, 85k CC lines, 13.7k deaths and
2,595 distinct durations.

### 10.3 AS-292's three probes

On this card's base AS-292 is not merged, and its probes keep their original
thresholds. On this card alone all three pass: the statue floor (minimum 0.92
u/s, median 1.86; base 0.92 / 2.09), the heal-line ceiling (pinned seeds 7, 2,
12 at 3, 25, 15%; seeds 1-12 at 3-33%, median 18%; base median 22%) and the
juke bound. Solve-level statue mutants (the flat-field key disarmed, the
PRESSURED solve held in place) both fail the original 0.75 floor on this base,
so it still guards the key and no new measurement was needed for it.

With AS-292's situation rule applied on top, the statue floor and the juke
bound pass at their original thresholds (statue minimum 1.16 u/s against
AS-292's own 0.22-0.40). The heal-line ceiling does not: seed 7 runs 44%
against 35%, and seeds 1-12 run 16-49% (median ~32%, AS-292 alone 8-43%, median
25%). AS-292 needs that ceiling re-examined when it lands.

