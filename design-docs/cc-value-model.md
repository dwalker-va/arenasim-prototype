# CC as a priced resource (`CcValue`)

Status: **STEPS 0-3 IMPLEMENTED. No default flip recommended yet.**
Step 1 (CC targeting) is a large win on an obstacle-free map and thins to nothing
on PillaredArena — and the obvious explanation (feared healers hiding behind
pillars) is **measured and refuted**; PillaredArena barely has line of sight in
play at all under `Legacy`. Step 2 (interrupt targeting) measured negative
outright. Both
sit behind their own per-team flag, both default to today's behaviour, and
`Identity` on either axis is byte-identical to the behaviour that shipped before
this document existed. The predictor is consumed by the Warlock's
healer-Fear gate behind the per-team `CcPolicy::Priced` switch; `CcPolicy::Identity`
is the default and is byte-identical to prior behaviour. Steps 2-6 remain proposed.
Date: 2026-08-07.

Read *Step 0 result* before building step 1. The harness has run and the
predictor is on its second revision: bias is now small (mean signed error
−0.09s), the dispel term is calibrated from measurement, and the **break
classifier remains the open problem at 60% precision** — see *Predictor v2*.
Three plausible corrections written along the way were refuted by measurement;
they are recorded so they are not re-proposed.

Supersedes the CC half of *CC targeting is stance-derived, not static* in
[team-level-positioning-ai.md](team-level-positioning-ai.md) — see
*Relationship to `TeamPlan`* below. That section's diagnosis was right and its
remedy (a stance → CC-target table) is a special case of what follows.

---

## Why this exists

The Warlock's healer-lockout Fear (`class_ai/warlock.rs:238`, the priority-1.75
gate that measured +3.4pt vs healer comps on 2026-06-28) is gated on one line:

```rust
// Don't Fear the target we're actively trying to kill.
.filter(|&healer| healer != kill_target)
```

That line is the entire coordination model for the Warlock's best anti-healer
tool. Measured consequences, on `Warlock+Warrior` vs `Priest+Mage`, 5 seeds,
identical in every respect except the kill-target assignment:

| Warlock's assigned kill target | Fears cast on the enemy Priest |
|---|---:|
| the Mage (a DPS) | **3 per match** (t≈14s, 29s, 38s, every seed) |
| the Priest (the healer) | **0. Ever.** |

The fall-through is worse than "no lockout". With the healer as kill target,
priority-4 Fear retargets to `cc_target`, which `select_cc_target_heuristic`
correctly inverts to the enemy DPS — but the Warlock's *position* follows its
kill target, so the DPS is never inside Fear's 30yd. From the decision trace of
that match:

```
338 rejected OutOfRange   ← every Fear consideration, distance ~70yd vs max 30
```

**Zero CC cast for the entire match.** This is not an exotic configuration:
`team_plan::choose_kill_target` explicitly prefers the healer, the Configure
screen exposes a kill target, and the in-match HUD call (commit `7751965`)
writes `MatchConfig.team{1,2}_kill_target` on click. Calling the enemy healer
silently disables the Warlock's healer lockout. In the comp above it also
happens *by accident* — the Priest is simply nearest at the gates, so plain
nearest-target acquisition lands the Warlock on the healer and the lockout never
runs.

The converse fails too. When the Warlock is not on the healer it fears the
healer regardless of who else is hitting it. Captured instance
(`Warlock+Mage` vs `Mage+Priest`, seed 2):

```
15.42  Mage Frostbolt → Priest        (the Mage's kill target IS the Priest)
15.63  Warlock's Fear lands on Priest (8.0s)
15.65  Warlock casts Curse of Agony on the same Priest   ← next GCD, curse spread
16.78  Mage Frostbolt 84 dmg
18.12  Mage Frostbolt 83 dmg
18.13  Fear broke from damage (166/100)
```

An 8s CC lasted 2.5s, at full Fears DR, and two hardcasts were spent pushing a
fleeing target. Diagnostic sweep, 48 matches (4 Warlock comps × 4 enemy comps ×
3 seeds) — mechanism measurement, **not** a balance claim:

| Fear target | n | mean duration | broke early |
|---|---:|---:|---:|
| healers | 21 | 6.61s | **33%** (mean 3.82s, breaking at 116–170 dmg vs a 100 budget) |
| DPS | 22 | 8.00s | 0% |

The asymmetry is the proof: fears break precisely when the feared unit is the
one the team is burning. Fears on the off-target never broke.

---

## The reframe: identity is a proxy for value, and it is the wrong one

Every CC decision in the codebase is keyed on `class.is_healer()` — 15 call
sites across `class_ai/` and `combat_ai.rs`. `select_cc_target_heuristic` scores
"healer +100"; `pick_healer_to_fear` filters on it; the Shaman's Wind Shear
prefers healers; the Felhunter's Spell Lock prefers heal casts.

Role identity is standing in for *how much it costs the enemy to have this unit
removed for the next N seconds*. That proxy is right often enough to have
measured +3.4pt, and wrong in every case the design discussion surfaced:

- it cannot see a second DPS as an off-target when no healer exists
- it cannot see that a healer at 5% mana denies less than a Mage with a full bar
- it cannot see that a fear on a burning target is correct for 1.5s and wrong for 8
- it has no notion of a CC being worth more *now* than later
- it cannot see that CCing a non-threatening healer can be worth it *because* it
  makes a teammate's CC stick

The proposal: stop assigning CC by role, start pricing it.

---

## The value model

For a candidate `(ability, target)` pair, evaluated per-unit per-decision:

```
value  =  I  +  D × T_eff  +  E  −  C
```

### `I` — interrupt value

What the target is doing *right now* that this CC cancels. A one-shot payoff,
independent of duration. A Flash Heal on the unit we are three seconds from
killing sits at the top of the scale; a self-buff sits near zero.

This is what makes an offensive fear on a fully-DoTed target correct *at that
instant and not otherwise* — the pseudo-interrupt case — which is exactly the
discrimination the blanket `healer != kill_target` filter cannot make.

`CombatantInfo.casting_ability` is **already** exposed to class AI and no CC
decision reads it. Remaining cast time is not exposed and must be added.

### `D` — denial rate

Value per second of removing this unit, in our currency. Not a role constant:
the expected value of the actions they would otherwise take.

Crucially this includes **utility throughput, not just healing and damage**. A
dispel denies our CC; a purge denies our buffs; an interrupt denies our cast.
A healer who is dispelling rather than healing is still high `D` — which is the
answer to "it might be worth CCing the healer even if they aren't especially
threatening."

Gated on delivery, so all of these price near zero: an OOM caster, a melee
kited out of range, a healer with nobody hurt and nothing to dispel, a pet.

### `T_eff` — expected effective duration

```
T_eff = min( nominal × DR_multiplier(target, category),
             break_budget ÷ predicted_friendly_damage_rate(target),
             expected_time_to_dispel(target) )
```

Three caps, each load-bearing:

- **DR** — verified mechanics in the appendix. Collapses to 0 at immune, which
  is what makes rotation emergent (below).
- **Break budget** — this is where the measured bug dies *structurally* rather
  than by special case. Predicted friendly damage on the target is the thing
  today's model assumes away; Fear's 100-damage budget is treated as infinite.
- **Dispel latency** — a function of whether an enemy dispeller is *free to
  act*. This is the load-bearing term for cross-CC (below).

### `E` — enabling value

The marginal uplift this candidate creates in **teammates'** best currently
available actions. Depth 1, no recursion. Discounted by whether the teammate can
actually deliver: off cooldown, in mana, in range and line of sight, not CC'd.

Justified and bounded in *Non-additivity* below.

### `C` — cost

- **Time-to-availability**: `max(cooldown_remaining, resource_wait, travel_time)`.
  Value is evaluated at that future moment, not now.
- **Opportunity cost**: the GCD and cast time priced at the damage or healing
  forgone. This is the term that decides *whether to CC at all*; it is developed
  in its own section below, because it is the one that changes the architecture.
- **Mana**, weighted by mana pressure.
- **DR spend**: applying now forfeits the category for up to 15s.
- **Displacement**: feared and horrified units flee. Dragging a melee's target
  out of reach is a real cost the sim models and the AI ignores entirely.

### Travel time is interception, not distance

```
travel_time = distance / (my_speed − their_speed_away_from_me)      → ∞ if unable to close
```

One term explains without special-casing why Hammer of Justice lands on a
planted Priest and never on a kiting Mage. Note the repo already contains a
hand-coded instance: `evaluate_dip_entry` gates the Paladin's dip on the healer
being within `HoJ range + dip_budget × effective speed`.

### Comparing CC against the damage rotation

Everything above prices CC against **other CC**. Whether to CC *at all* is still
decided by the ability's position in each class's priority ladder — a constant,
independent of state. So a fear worth almost nothing still outranks a Shadow
Bolt, and a fear worth a great deal still loses to a Corruption refresh.

That is the same defect as `is_healer()`, one layer up: a fixed answer standing
in for a situational one. It is also why **this design does not, by itself, fix
the starvation bug**. The 2026-06-28 findings measured Fear at priority 4 casting
*zero* times across many seeds, because every earlier gate returns true on some
GCD; the remedy was to hardcode it to 1.75. Better targeting does not remove the
positional assumption underneath it.

**The currency is already commensurable.** `D` is throughput denied per second,
and throughput is healing and damage — so `I + D × T_eff + E` is natively in
expected-damage-equivalent units, the same units a damage ability is worth.

**The bounded mechanism** — not the unified action scorer, which stays out of
scope. Only the ability the ladder *would have picked* needs a price:

1. CC scorer → best CC candidate, value `V_cc`
2. Price the ladder's chosen damage action → `V_dmg`
3. Take the CC if `V_cc > V_dmg`

One extra evaluation per decision. The ladder keeps its internal ordering and all
its rotation logic; it only has to state a number for its output. If it turns
out that sensible comparisons require pricing *every* damage ability, that is
evidence the ladder's ordering is itself wrong — a finding, not scope creep.

Consequence worth stating plainly: **priority 1.75 becomes deletable.** The
"where in the ladder" question exists only because the order is fixed.

#### Pricing damage: one part is free, four parts are not

A DoT's value has the **same shape as a CC's** — magnitude × expected surviving
duration, where survival is threatened by dispel for both and additionally by
incoming damage for CC. Corruption is not worth tick × duration; it is worth
expected damage before dispel or death. So the step-0 prediction-error harness
generalises to DoTs at no conceptual cost and validates both sides of the
comparison with one mechanism.

The parts that need care, each state-dependent in a way a fixed ladder cannot
express:

- **Absorbs defer damage; they do not waste it.** An earlier draft said a tick
  into a fresh PW:Shield "is worth nothing," reading the combat log's
  `Curse of Agony ticks for 0 damage (14 absorbed)` at face value. That is
  wrong. **A shield is temporary effective HP**, and damage into it consumes
  that pool exactly as damage into real HP does — so the model must price the
  *effective-HP delta*, never the log's damage number, which reads 0 and is
  actively misleading. PW:Shield runs 30s in matches that typically resolve at
  ~37s, so shield capacity is nearly always consumed rather than expiring.
  Two real discounts remain, and neither is anywhere near total: **urgency**
  (EHP removed later does not advance a kill happening now, so it is worth less
  inside a burst window) and **overshield** (capacity that expires unspent — the
  mirror of overheal, and rare at these durations).
- **Purge is the efficient answer to a shield, and that is a cross-class
  comparison neither ladder can make.** One Purge GCD removes the whole
  remaining pool; grinding ~70 EHP down with Curse of Agony at 14/4s takes five
  ticks and twenty seconds. And because PW:Shield applies a 15s Weakened Soul
  (`class_ai/priest.rs:645`, checked at `:578`), a purged target cannot simply
  re-shield — so the purge buys the rest of that window too. Dispel and purge
  therefore price in the *same* effective-HP currency as damage, and belong at
  the same seam as CC-vs-rotation rather than in a separate scheme.
- **Dampening cuts the value of denying heals.** `ArenaDampening` ramps healing
  and absorbs to zero from `DAMPENING_START_SECS`. A healer's `D` must scale
  through it, or the model systematically over-values CCing healers late in long
  matches.
- **Overkill and overheal** need the same "did it actually matter" discount on
  both sides of the comparison.
- **Damage feeds Warrior rage** at 15% of damage taken, so some damage carries a
  negative component. Already recorded as a DoT design constraint.

### The law that falls out: `I` does not survive a walk

Interrupt value attaches to a cast already in flight, so a positional CC can
only claim `I` when `travel_time + cast_time < remaining_cast`. From 15 yards,
nothing beats a 1.5s cast.

This is why Kick and Pummel are reactive-from-inside-melee while HoJ is a
*planned dip*, and it is a structural fact rather than a tuning knob:

| | claims `I` | claims `D × T_eff` |
|---|---|---|
| instant / ranged CC | yes | yes |
| positional CC | only from inside range already | yes — stable across a walk |

---

## Non-additivity: cross CC breaks the argmax

The model above scores actions independently and picks the maximum. CC value is
**not additive across units**, so that is wrong in a way worth stating plainly.

Polymorph on a DPS while the healer is free lasts about as long as it takes to
dispel. The same Polymorph while the healer is feared lasts its full duration.
So `value(poly)` depends on `CC(healer)` — and, better, it can be worth CCing a
healer who is not threatening *purely* to raise `T_eff` elsewhere. Stacked
further: stunning the kill target while the healer is polymorphed produces a
window with no counterplay at all, which is worth more than the two CCs
separately.

### Half of it is already emergent

`T_eff`'s dispel cap does the work. The moment the healer is feared,
`expected_time_to_dispel` for the Polymorph jumps, the poly's score rises on its
own, and the Mage casts it. **No coordination, no shared plan, no sequencer** —
just re-scoring against the new state each decision. What a player would call a
chain is what a greedy argmax does once `T_eff` is honest about the enemy's
ability to answer.

The zero-counterplay case rides the same mechanism from the other side: it is CC
uplifting *damage*, not CC uplifting CC. Burst delivered into a healing-denied
window is worth more, so once the poly lands, stunning the kill target scores
higher without anyone planning it.

`acquire_targets` already reflects instant CC landing within a frame into the
shared snapshot (`snapshot.reflect_instant_cc`), so this composes at frame
granularity rather than lagging a tick for instants.

### What does not emerge: the initiator is never paid

The chain's *first* CC is a pure externality. The Warlock's fear creates the
entire uplift and captures none of it, so it systematically undervalues the
action that starts the chain. That is what `E` exists to price, and nothing
else in the model needs to know about chains.

**Depth 1, hard cut.** If `A`'s enabling value could read `B`'s already-uplifted
value, two units can inflate each other into CCing on the strength of a follow-up
neither will make, and the fixed point may oscillate. `E` is always measured
against teammates' *un-uplifted* scores.

### The Rogue/Priest case falls out of `E` × interception

A stunned healer has zero escape speed, so the Priest's travel time collapses
and its Fear moves from undeliverable to deliverable. The Rogue's stun then
scores that Fear's value as enabling uplift — so the Rogue prefers stunning the
healer over the nearer kill target, *for the reason a player would give*, and
only when the Priest can actually follow up.

The property worth protecting: `E` is near zero for a team with no second CC.
`Warlock+Warrior` never attempts chains it cannot finish; `Rogue+Priest` does.
**Cross-CC turns on per-composition with no composition-specific code.**

---

## Positional CC is a commitment, not an action

A CC that requires walking is not a one-frame score. It is a plan with a budget
and abort conditions — which the repo already has, hand-coded, for exactly one
ability on one class. `paladin_postures` enters DIP on a reach envelope, holds a
`dip_budget`, and aborts (`DipAbort`) on teammate HP dive, target dead-or-immune,
or budget exhaustion.

Generalizing that machinery — reach envelope, budget, abort predicates — is the
migration path for the Rogue's stuns, and it means half the work is already
shipped and measured.

---

## Position is the contested resource

The deeper form of the measured bug. `try_spell_lock` range-gates from the
**pet's** position (30yd), and the pet's position comes from its damage
assignment: it inherits the owner's target and pursues it. So the Felhunter can
only lock the school of a caster it happens to be standing near — structurally
identical to the Warlock's 338/338 `OutOfRange`.

Damage assignment picks position; position decides which CC is deliverable at
all; the CC layer never gets a vote.

The eventual answer is a `cc_pull` term in the movement scorer, alongside
`wand_pull` / `range_band` / `cover_pull`, per-class weighted. **Deferred out of
the first cut**: Warlock and Mage need no repositioning to deliver their CC, and
the movement scorer is where the measured healer-solve gains live (`Legacy` 0.0s
→ `TeamPlan` 28.1s occlusion; +36pt head-to-head on `Warlock+Priest`). Cut one
prices CC from where the unit already stands.

---

## Playstyles are weight presets, not code paths

If CC is scored with per-class weights in RON, a playstyle *is* a named weight
vector. Focused, Spread and Lockdown become three presets, the same move
`movement.ron` made for healer postures, and they drop into the per-unit config
surface that already exists (`warlock_curse_prefs`, `rogue_openers`,
`rogue_poisons`, `hunter_pet_types`, `warrior_shouts`, `mage_armors`,
`paladin_auras`).

Proposed shape, `assets/config/cc.ron`, mirroring `movement.ron` — additive
interest terms as weights, hard constraints as masks in the scorer:

```ron
(
    shared: (
        dr_spend_penalty: 1.0,
        displacement_penalty: 1.5,
        enabling_discount: 0.6,      // depth-1 uplift is worth less than own denial
        min_t_eff: 0.5,              // below this, a CC is not worth a GCD
    ),
    warlock: (
        weights: ( interrupt: 2.0, denial: 1.0, enabling: 1.0, opportunity: 1.0 ),
        presets: {
            "Focused": ( ... ),
            "Spread":  ( ... ),
        },
    ),
)
```

**Cost to name honestly:** every preset multiplies the balance surface. A
Warlock with three playstyles is three Warlocks in every matrix cell, and the
canonical baselines are already expensive to regenerate. So presets ship as
**opt-in configuration with one designated default** that the canonical
baselines are measured against. The AI does **not** select a playstyle
situationally — that is a second scoring problem stacked on the first, and it is
how `TeamPlan` step 5 earned its −48pt.

---

## Relationship to `TeamPlan`

The existing design doc identified the same class of defect from the stance
side, and proposed a table:

| Layer input | CC target |
|---|---|
| `Press` | enemy healer / off-target |
| `Recover` | the enemy applying pressure, kill target included |
| Obligation: ally is kill target and in danger | that ally's pursuer |

Every row of that table is a special case of the pricing model, which is the
argument for pricing rather than tabulating:

- `Press` → the team is dealing damage, so `predicted_friendly_damage` on the
  kill target is high, its `T_eff` collapses, and the off-target wins.
- `Recover` → the team is deliberately not dealing damage, so the kill target's
  `T_eff` is full again and its `D` (it is the one applying pressure) is high.
  Polymorphing our own kill target becomes correct, exactly as that section
  argues, without a stance branch.
- The obligation row is `D` reading "damage that would land on us."

### This model does not depend on stance, and must not

An earlier draft of this section said "stance remains a useful *input* to `D`
and to the kill-window term." That was wrong twice over, and correcting it makes
the design simpler rather than harder.

**First, the stance machinery it leaned on does not exist.** `Stance` has three
variants, and production code assigns exactly two: `Hold` when a plan has an
anchor, `Press` otherwise or after the contact latch downgrade
(`team_plan.rs:554` and `:638` are the only non-test write sites). **`Withdraw`
is never produced outside tests** — `team_solve.rs` handles it in `focal_point`
and `assign_intents`, but nothing ever assigns it. So the `Recover` row above,
which is exactly the row that made "polymorph our own kill target" correct, is
unreachable today. A design that consumed it would be dead on arrival.

**Second, stance is a worse proxy for something this model already measures.**
`T_eff`'s break-budget cap needs predicted friendly damage on the target, specced
above as a trailing-window measurement of *actual* recent damage. That is
revealed behaviour, and it subsumes the entire `Press`/`Recover` distinction: if
the team is not damaging the target, the measured rate is ~0, `T_eff` is full,
and polymorphing the kill target scores well — with no stance branch, and
correctly even in the states no stance variant describes. Declared intent is
strictly less reliable than measured behaviour here, which is the same lesson
`TeamPlan` step 5 recorded when a held call lost to per-unit re-acquisition.

Every other term reads per-unit observables — positions, auras, casts, mana,
cooldowns, recent damage — all of which exist identically under both profiles.
**The CC value model is therefore profile-independent, and is gated on its own
switch rather than on `AiProfile`.** See *Migration plan*.

The one piece that genuinely touches `TeamPlan` is `cc_pull` movement feedback,
which is deferred for that reason among others.

The `TeamPlan` doc's own closing observation — "a hard-coded guard turning out to
be a stance assumption in disguise is the sort of thing to look for elsewhere in
the class AIs" — is what this document is, applied to all of them at once.

---

## The larger question this opens

Recorded here so it is not lost, and deliberately **not** pursued in this
document. Pricing CC against the rotation, and then pricing purge against
grinding a shield, are two instances of a general problem: how to arbitrate
between damage, CC, dispels, buffs, heals, and *movement*.

### Two contended resources, coupled by one mechanic

- **The GCD / cast-time timeline** — contended by damage, CC, dispels, heals,
  instant buffs.
- **Position** — contended by safety, line of sight, range bands, cover, and
  CC delivery range.

They are not independent: **movement cancels casts**, so choosing to move is
choosing not to hardcast. That single mechanic is the entire coupling, and it
means the answer is not one scorer but two schedulers with an explicit exchange
rate between them.

### One currency, three value shapes

Everything except movement prices in **expected effective-HP swing**, plus
throughput swing over time:

| Shape | Actions | Priced as |
|---|---|---|
| Direct swing | damage, heals, shields | EHP removed / added |
| Denial | CC, offensive dispel, purge, interrupts | throughput denied × expected duration |
| Enabling | movement, `E` | change in what teammates can *deliver* |

Offensive dispel and purge fall out as denial or direct swing depending on what
they strip. Movement is the outlier: almost purely enabling, and its cost is
cast time forgone.

### The same anti-pattern, a third time

This document opened on `is_healer()` — a fixed answer to "who is worth CCing."
*Comparing CC against the damage rotation* found priority-1.75 — a fixed answer
to "is CC worth a GCD." The movement layer has the third:
`urgency_hp_threshold: 0.5` defers non-critical casts during ESCAPE/DIP unless
an ally is below that HP fraction. That is **a hardcoded exchange rate between
movement value and cast value**, the same shape one layer further out.

Three layers, one pattern. That is the argument that the general problem is real
and not an aesthetic preference.

### Why it waits on step 0

Every design in this family — this one included — assumes expected value is
*predictable* in this sim: expected CC duration, expected damage before a dispel,
expected shield consumption, expected delivery. Step 0's `T_eff` prediction-error
harness measures precisely that, read-only, on the quantity the rest depends on.

- **Large prediction error** → expected-value pricing is unsound here, and a
  unified action scorer would be built on sand. Far better to learn that from one
  read-only harness than from a seven-class rewrite.
- **Small prediction error** → positive evidence to expand, and the same harness
  generalises directly to DoT survival and shield consumption.

So the sequencing is not timidity: **step 0 adjudicates the whole philosophy, not
just the CC part of it.** Revisit this section when it reports.

---

## What this does NOT solve

- **Chains deeper than one step.** `E` at depth 1 catches "my CC makes my
  teammate's next action deliverable." It will not see CC → position → CC →
  kill. Accepted rather than building a planner: the repo's own evidence is that
  a held team-level call measured −48pt while continuous per-unit re-evaluation
  adapted.
- **Cast cancelling.** Stance or valuation can flip while a cast is in flight;
  only interrupts exist. Same accepted suboptimality the `TeamPlan` doc records
  for Polymorph, and the cost is slightly worse than a wasted GCD because a
  self-broken CC still applies DR.
- **A unified action scorer.** The logical endpoint is damage, healing and CC
  priced in one currency with a single argmax over every ability. That is a
  rewrite of all seven class AIs and stays out of scope. Note the boundary moved
  once already: *Comparing CC against the damage rotation* prices the ladder's
  **chosen** action so CC and rotation can be compared at one seam. That is a
  deliberate step toward the endpoint, not arrival at it — the ladder still owns
  which damage action is best, and nothing prices healing.
- **Over-CCing.** If uplift terms are over-weighted, every unit talks itself
  into CC because someone might follow up. DR spend and opportunity cost are the
  brakes. This is the weight set most likely to need walking back.

---

## Step 0 result (2026-08-07): the philosophy holds; the break term needs a classifier, not a clock

**Built and run.** `src/states/play_match/cc_value.rs` (the pure predictor,
unconsumed by the simulation) and `tests/cc_lifecycle_probe.rs` (the read-only
lifecycle harness). **150 CC applications over 36 matches** — four 2v2 comps and
two 3v3 comps across six seeds; 139 scored after excluding match-end censoring,
128 judged after also excluding targets that died under the CC.

```bash
cargo test --release --test cc_lifecycle_probe -- --ignored --nocapture
```

### Verdict: proceed

Mean absolute error **1.16s against a mean observed duration of 3.83s — 30%**,
and the error decomposes into named, fixable parts rather than noise. A large
*unstructured* error would have been the signal to abandon expected-value
pricing; this is not that.

The motivating case is predicted almost exactly: **Fear on healers, n=13,
predicted 4.17s vs actual 4.18s (+0.01s)**. The term the Warlock defect is about
— break budget under real team damage — works.

### The real finding: the break term's timing is excellent, its *discrimination* is not

Aggregate error hid this, because predicting a break that never comes and
predicting a real break late are opposite errors that partly cancel. Split apart:

| | n | error |
|---|---:|---:|
| correctly predicted a break | 21 | **−0.17s** |
| false alarm (predicted a break, none came) | 15 | **+1.27s** |

**Precision 58%, recall 72%.** So the model answers *when* a break lands almost
perfectly and answers *whether* one lands only moderately well. That inverts the
obvious remedy: do not tune the rate estimate to shift the timing — improve the
yes/no call. Concretely, a continuous damage rate is the wrong shape for a
discrete event. A 100-point budget against 40-damage hits breaks on the third
hit, and when that lands is governed by swing and cast timers, not by a smooth
average. Modelling *time to the next N landed hits* is the candidate fix.

### Two hypotheses raised and killed by the data

Both were plausible, both were written into an earlier draft of this section on
an n=16 sample, and both are **refuted** at n=36:

1. **"The friendly-CC guard stops our damage after a break-on-any CC lands."**
   Refuted. Gross damage during CC is consistently *higher* than the trailing
   rate extrapolates, in every category (break-on-any 25.4 actual vs 13.3
   extrapolated; damage-budget 60.2 vs 44.3; never-breaks 92.7 vs 63.6). The
   Polymorph error that motivated the hypothesis was **+2.50s at n=8 and is
   +0.62s at n=22** — the first sample was noise, exactly as this repo's
   sample-size rule predicts.
2. **"Shields are refreshed mid-CC, adding buffer the predictor never counted."**
   Refuted outright: **0 of 55** breakable CC applications were re-shielded while
   active. Weakened Soul's 15s lockout appears to prevent it.

Recording these because the next person will reach for the same two explanations.

### Correction that survives: the dispel term is real, and now parameterised

`Duration`-bound predictions are biased **−0.76s**: CC dies earlier than
predicted. v1 set `expected_dispel_delay: None` rather than invent a latency, and
the measurement supplies one:

- **27 of 128** judged applications ended `Removed`; 16 had a free dispeller at cast.
- Latency when a dispeller was free: **mean 1.95s, median 1.40s**, range 0.02–6.07s.
- Base rate: of 89 applications cast into a free dispeller, **18% were removed**.

Death Coil is the clean demonstration — its horror never breaks on damage, so it
is correctly predicted at a full 3.00s and actually lasts **2.21s** (n=24),
ended by dispels. Psychic Scream is the worst single case at **−2.16s**.

### Baseline behaviour, for the steps that come later

- **Chains essentially do not happen today.** Simultaneous control is **0.21s per
  match** against **13.93s** of total control denied — under 2% of denial time
  overlaps. That is the number step 5 has to move.
- Simultaneous-control and counterplay-free are equal. That is *forced* at 2v2,
  but the survey includes 3v3 where it is not, so the equality is a finding:
  every simultaneous-control window in this survey included a healer.
- **CC is cheap in time**: 2.65s of 40.77s hard-casting per match (6%).
- **31 of 150 applications cancelled a cast**, most often Flash Heal (9), Mind
  Blast (6) and Flash of Light (5) — direct evidence that the `I` term has
  something real to price.
- **29 of 150 broke early**, costing 4.36s of CC per match. On healers
  specifically, **13 of 39 (33%)** — matching the independent combat-log sweep at
  the top of this document to within a point.

### Hazard worth recording

Damage sites record the break accumulator through a *deferred* `Commands`
insert, so `auras.rs` acts on it a frame or more after the health drop. The
harness's first classifier tested for damage on the disappearing frame alone and
found **2 breaks where there were 11** — it silently reported that friendly
damage almost never breaks CC, the exact opposite of the truth, and was caught
only because it contradicted the combat-log sweep. Anything attributing cause
across frames in this sim needs a lookback window, not an equality test.

### Predictor v2 (2026-08-08): bias fixed, discrimination not

Both prerequisites were implemented and re-measured on the same 150 applications.

**Fix 2 — probabilistic dispel — landed and works.** `expected_dispel_delay` is
now a blend rather than a cap: `(1-p)·T + p·min(latency, T)` with `p = 0.18` and
`latency = 1.40s`, both taken from the step-0 measurement. Capping at the latency
was rejected on the data — only 18% of CC cast into a free dispeller is removed,
so a hard cap would be a far larger error than ignoring dispels. Mean **signed**
error improved from **−0.45s to −0.09s**, and the `Duration`-bound bias from
−1.13s to −0.30s.

**Fix 1 — the break classifier — did NOT achieve its goal.** Three iterations,
each evidence-led, and precision moved **58% → 60%** with recall flat at 72%:

1. *Split incoming damage by delivery mode and discount melee when the CC
   displaces its target.* Justified by measurement (displacing CC realizes 17.0
   dmg/s against 25.0 trailing; stationary CC 9.0 → 36.6). Kept — it is correct
   and it improved bias — but it barely moved discrimination.
2. *Gate on attacker presence.* Strongly justified: of 32 applications landed on
   a target with **zero** enemies pointed at it, **31 did not break**. Kept, and
   `expected_incoming` now returns zero damage in that case. It did not move
   precision, because those cases already had a ~zero trailing rate and were
   already predicted to survive.
3. The remaining 14 false alarms all **had** attackers at cast and still saw
   damage collapse — trailing 17.9 dmg/s at cast versus **3.5 dmg/s realized**.

So the open problem is sharper than before but unsolved: *attackers are pointed
at the target, damage has been arriving, and it stops anyway.* Candidates not yet
tested — the attacker switches target immediately after, the attacker is itself
CC'd or killed, or melee retention under displacement is far below the 0.35 first
estimate. **Stopping here deliberately** rather than fitting a fourth variant to
150 samples; the next iteration should start by measuring what those 14 attackers
actually did.

### What this means for step 1

Step 1 can proceed, because it does not depend on the unsolved part. The gate it
replaces is a binary identity filter that uses *no* information; a 60%-precision
break call plus a well-calibrated duration is strictly more informed. And the
motivating case is the one the model gets right: **Fear on healers, n=13,
predicted 4.17s vs actual 4.18s.**

One design consequence, though, and it changes the plan: **step 1 probably does
not need a trailing-damage tracker in the simulation.** The whole point of v2's
measurement is that the trailing rate is a weak signal. Attacker composition —
who is pointed at the target, and whether they are melee — is already available
in `CombatContext` today, needs no new per-frame state, and carries the signal
that actually separated the outcomes. A class-based DPS estimate over the
attacker mix is the candidate estimator, and it should be scored against this
same harness before it is consumed.

Still missing: `damage forgone to CC` in *value* terms. The harness reports the
time (6% of hard-casting) but pricing those seconds needs step 4's cost model.

---

## Step 1 result (2026-08-08): the measured defect is fixed, behind a gate

**Shipped.** `cc_policy.rs` (the per-team `CcPolicy` axis), `RecentDamage` (the
trailing gross-damage window), `CombatContext::dr_multiplier` (read-only DR
lookup), and `pick_healer_to_fear` rewritten to price candidates through
`predict_t_eff` rather than filter them by identity.

### The defect, before and after

`Warlock+Warrior` vs `Priest+Mage`, `team1_kill_target = 0` (the healer) — the
configuration at the top of this document, where the identity filter silently
disabled the Warlock's healer lockout because the healer *was* its kill target:

| Policy | Fears cast on the enemy Priest, 5 seeds |
|---|---:|
| `Identity` (today's behaviour) | **0** |
| `Priced` | **11** |

The Warlock now asks how long a Fear would actually last and fears the healer
when the answer is good, whether or not it is also the unit it is damaging.

### What the priced path evaluates

For each living, non-immune, non-DR-immune, not-already-CC'd enemy healer, it
computes expected effective duration from: Fear's base duration times the
target's **current** DR multiplier (read-only — pricing a CC must not advance
DR), the absorb pool sitting in front of the break budget, the attacker mix
pointed at that target split melee/ranged, the target's trailing gross-damage
rate, and whether their side has a free dispeller. Candidates below
`MIN_FEAR_T_EFF` (1.5s) are declined; the best survivor wins, ties broken on
entity id for determinism.

`MIN_FEAR_T_EFF` is deliberately low. With the break classifier at 60% precision
(*Predictor v2*), a mis-predicted break should cost a **declined** Fear rather
than a wasted one — the asymmetry matters because a wasted Fear also burns Fears
DR, which then blocks the Fear that would have stuck.

### Gating and safety

- **`Identity` is the default and is byte-identical.** All 26 test binaries pass,
  including the 97 fixed-seed movement probes, `camp_sweep`, and the trace audits.
  `tests/cc_policy_gate.rs` additionally pins that naming `Identity` explicitly
  produces bit-identical results to a config that never mentions the policy.
- **The gate is not inert.** The same file asserts `Priced` diverges somewhere in
  the survey, so a wiring mistake cannot pass as success.
- **`Priced` is deterministic** at a fixed seed — asserted, because the priced
  path ranks by float score and needed an explicit tie-break.
- **Per-team**, so head-to-head measurement is possible: `cc_policy`,
  `team1_cc_policy`, `team2_cc_policy` in the headless config, validated at load.
- `RecentDamage` is maintained every frame in both modes but read **only** under
  `Priced`, so it cannot move a `Legacy`/`Identity` seed. It reconstructs gross
  damage the same way the probe harness does — health lost plus absorb consumed —
  so the shipped estimator and the measured one cannot drift apart.

### Measured (2026-08-08): `Priced` wins where it fires, and is inert elsewhere

Head-to-head via `scripts/headtohead_sweep.py --axis cc_policy`, per-team
policies, `AiProfile` pinned to `Legacy` so positioning is not a confound,
BasicArena so line-of-sight is not one either. CSVs in `design-docs/balance/`.

**Healer called as the kill target** — the scenario the defect is about, and what
the in-match HUD call produces:

| comp | n | `Identity` | `Priced` | effect | z |
|---|---:|---:|---:|---:|---:|
| `Warlock+Priest` vs `Priest+Rogue` | 300 | 37% | **48%** | **+10pt** | **+2.56** |
| `Warlock+Priest+Mage` vs `Priest+Warrior+Rogue` | 150 | 65% | **83%** | **+19pt** | **+3.69** |

**Free targeting** — the same comps with no kill call, where the Warlock usually
picks a DPS and both policies already agree:

| comp | n | `Identity` | `Priced` | effect | z |
|---|---:|---:|---:|---:|---:|
| `Warlock+Priest` vs `Priest+Rogue` | 150 | 43% | 50% | +7pt | +1.27 |
| `Warlock+Priest` vs `Warrior+Priest` | 100 | 19% | 19% | **+0pt** | 0.00 |

That last row is worth reading carefully: **all three cells were bit-identical**,
because with free targeting in that comp the Warlock's kill target is the
Warrior, so the identity filter and the priced gate reach the same answer on
every seed. The change is scoped to exactly the case it was built for.

**The null control held in every sweep.** Each cell also gives the variant to the
side with no Warlock, and that side moved **+0pt, z=0.00, in all four** — so the
gains above are attributable to the Warlock's Fear decision and not to sweep
noise or a leak into shared code.

### PillaredArena (2026-08-08): the win does NOT replicate

The BasicArena numbers above were taken on an obstacle-free map to isolate the
effect from line of sight. Re-run on **PillaredArena** — the current default for
the sweep tooling, and the Nagrand replica the project is building toward:

| condition | BasicArena | PillaredArena |
|---|---:|---:|
| 2v2, healer called (n=300) | **+10pt** (z=2.56) | **+2pt** (z=0.44) |
| 3v3, healer called (n=150) | **+19pt** (z=3.69) | **-7pt** (z=-1.54) |
| 2v2, free targeting (n=150) | +7pt (z=1.27) | +3pt (z=0.61) |

Null controls were exactly `+0pt (z=0.00)` in every PillaredArena cell, so this
is not a measurement artefact — the effect really does thin out, and the 3v3 cell
flips sign (not significantly, but in the wrong direction).

**It is not that the gate stops firing.** Fears actually cast on the enemy healer
over 8 matched seeds, healer called:

| map | `Identity` | `Priced` |
|---|---:|---:|
| BasicArena | 6 | **16** |
| PillaredArena | 4 | **16** |

The AI makes the same decision on both maps. The extra Fears simply stop
converting to wins once there are pillars.

### The displacement hypothesis: MEASURED AND REFUTED (2026-08-08)

The hypothesis was that on an obstacle map, fearing a healer sends it running
*behind a pillar*, where it is occluded from our own team and can heal
unmolested — so the Fear that opens a kill window on open ground hands the
healer cover on a pillared one. If true, `T_eff` would not be the term at fault:
the model would be missing a **displacement cost that depends on map geometry**.

`tests/feared_healer_cover_probe.rs` measures it directly. Same cell as the
sweep (`Warlock+Priest` vs `Priest+Rogue`, healer called, PillaredArena), 24
seeds per policy, read-only. Two comparisons, because they answer different
questions: within-match, each Fear window against the **equal-length window
immediately before it** (isolating the Fear's own effect from the healer's
baseline tendency to hide when pressured); and between-policy, since `Priced`
Fears the healer ~2x as often as `Identity` and any real effect should scale.

| | `Identity` | `Priced` |
|---|---:|---:|
| Fear windows on the healer | 11 | 24 |
| occluded fraction, before -> during | 0.028 -> 0.009 (**-0.019**) | 0.032 -> 0.043 (**+0.011**) |
| fully hidden, before -> during | 0.000 -> 0.000 | 0.024 -> **0.018** |
| healing delivered during the Fear | **0.0** | **0.0** |
| whole-match healer occlusion | 2.64s | 2.77s |

**Refuted, four ways.** The within-match effect disagrees in *sign* between the
two policies and is tiny in both. "Fully hidden" — the state the hypothesis is
actually about — never rises; it falls under `Priced`. Doubling the number of
Fears moves whole-match healer occlusion by 5% (2.64s -> 2.77s), not by anything
like 2x. And the healer delivers **zero healing while feared** under both
policies, which is the Fear working exactly as intended rather than handing the
healer a free window.

### The more important finding: PillaredArena barely has line of sight in play

The context line the probe prints to make a zero interpretable:

| | `Identity` | `Priced` |
|---|---:|---:|
| ALL cross-team pairs occluded, per match | **1.32s** | **1.45s** |
| healer's closest approach to a pillar | 8.1yd | 7.8yd |

Across *every* living cross-team pair, total occlusion is **under 1.5 seconds per
match**. Combat clusters in the centre of a ~120yd bowl and the pillars sit at
(+/-40, +/-20) with a 6yd circumradius, so under `AiProfile::Legacy` — which has
no cover-seeking; that is what `TeamPlan` added — sightlines essentially never
cross one.

So the BasicArena/PillaredArena gap **cannot be a pillar effect at all**, because
pillars are barely in play. Whatever moves step 1 from +10pt to +2pt is a
different property of the map — its size, spawn separation, and the engagement
timings those produce — not line of sight. That reframes the next question:
compare BasicArena against PillaredArena on *distance and timing* metrics
(time-to-contact, mean engagement range), not on occlusion.

It also carries a caution for the CC model generally: any future term that prices
*cover* will be inert under `Legacy` and only bite under `TeamPlan`, so it must
be measured on the profile where cover exists rather than on the default.

### Why the map changes the answer: engagement geometry, not geometry-geometry (2026-08-08)

With cover refuted, the remaining structural difference between the maps is
**size**: BasicArena is a 73x43 octagon, PillaredArena a ~119yd-diameter bowl.
That suggested a second hypothesis — the healer *is* the called kill target here,
so step 1 makes the Warlock Fear the very unit its team is killing, and a Fear
displaces its target. In a 43yd arena a fleeing healer hits a wall; in a 119yd
bowl it does not.

**Also refuted.** Per Fear window, `tests/fear_effect_probe.rs`, 24 seeds:

| map | policy | n | net displacement | dist before -> during -> after | damage on kill target before -> during -> after |
|---|---|---:|---:|---|---|
| BasicArena | Priced | 10 | 10.5yd | 21.9 -> 18.3 -> 20.0 | 155 -> 96 -> **181** |
| PillaredArena | Priced | 16 | 12.2yd | 26.3 -> 24.5 -> 19.6 | 108 -> 99 -> **162** |

The healer ends up **12.2yd from where the Fear caught it on the big map versus
10.5yd on the small one** — essentially the same, in an arena three times the
size. And damage on the kill target *recovers* after the Fear on both maps. There
is no differential displacement cost.

### What is actually different

The whole-match baseline, measured with no reference to Fear at all:

| map | policy | match length | mean distance to kill target | damage on kill target /s | kill target HP at end |
|---|---|---:|---:|---:|---:|
| BasicArena | Identity | 39.9s | 38.5yd | 14.24 | 70.3 |
| BasicArena | **Priced** | 43.3s | 36.7yd | 11.89 | **57.6** |
| PillaredArena | Identity | 57.3s | 54.3yd | 12.52 | 53.4 |
| PillaredArena | **Priced** | 58.3s | 53.3yd | 10.92 | **42.1** |

PillaredArena keeps our team **~45% further from the kill target** (53yd vs 37yd)
and stretches matches **~40% longer** (57s vs 40s). That is the map difference,
and it is about engagement distance and pacing — not obstacles.

Note what `Priced` does *within* each map, which this is the first measurement to
make explicit: it **lowers raw damage per second on both maps** (14.24 -> 11.89,
12.52 -> 10.92) — Fear costs GCDs and pushes the target away — while still
leaving the kill target **lower on both** (70.3 -> 57.6, 53.4 -> 42.1). The trade
is real and it works. The difference is whether it converts.

### The term this points at

The Fear is doing its job on both maps: healing denied, kill target ends lower.
It converts to wins on the small map and not on the large one, because **a kill
window is worth what your team can deliver into it**, and delivery is lower when
the team is half again as far from its target for forty percent longer.

That is the *kill-window* term — the CC-uplifts-damage carrier already named in
*Non-additivity* and **not implemented**. The model currently prices denial
(`D x T_eff`) with no reference to whether anyone can exploit it, which is
precisely why one number (+10pt) did not survive a change of map.

Implementing it needs our team's own delivery rate onto the kill target — the
mirror of what `RecentDamage` already tracks for the enemy. **This is the highest
value next step in the plan, ahead of steps 3-5**, because without it every CC
score is map-dependent by construction and no amount of tuning `T_eff` fixes
that.

### The delivery-rate term: implemented, and it does NOT explain the map gap (2026-08-09)

`cc_value::kill_window_value(t_eff, delivery_rate)` is in, consumed by the
Warlock's Fear gate, which now **ranks candidate healers by the value of the
window a Fear would open** rather than by raw expected duration. Delivery comes
from `RecentDamage` on the kill target — enemies do not damage each other, so a
unit's incoming damage *is* our delivery onto it — and correctly uses the
post-displacement rate when the CC target is itself the kill target, since
fearing the thing you are killing pushes it away from your own damage.

The term is right in shape: **denying healing is worth exactly the damage it
fails to erase.** Three things measured along the way say it cannot carry the
weight that was hoped for.

**1. It cannot veto, because trailing delivery is zero when a Fear is most
wanted.** Gating on it — first against the displaced Shadow Bolt, then against a
bare `delivery > 0` — suppressed *every* opening Fear and collapsed `Priced` back
into `Identity`: Fears went 16 -> 6 (BasicArena) and 16 -> 4 (PillaredArena), and
all four sweep cells returned **+0pt**. A Fear is wanted *before* the burst it
opens, which is exactly when the trailing rate reads zero. A veto needs a
FORWARD delivery estimate, and step 0 measured composition-only estimation as
substantially worse than trailing (precision 60% -> 45%). No such estimate
exists yet.

**2. As a pure ranking signal it is inert in 2v2.** With one enemy healer there
is one candidate, so ranking changes nothing. It can only bite where there are
two healers to choose between, or once it can veto.

**3. The arithmetic rules it out as the map explanation anyway.** Delivery onto
the kill target is **14.24/s on BasicArena and 12.52/s on PillaredArena — a 12%
difference**. The win-rate effect differs by **8 points** (+10pt vs +2pt). A term
linear in delivery cannot turn a 12% input difference into that. Even a perfect
delivery estimate would not explain the map dependence.

So the map gap is still unexplained, and the remaining candidate is the one the
baseline table points at but this term does not touch: **match length**
(39.9s vs 57.3s). Longer matches give a denied heal more time to be re-cast, and
push toward the dampening onset at 75s. That is the next thing to measure — and
it is a property of pacing, not of any CC term, which would mean the CC model is
simply not where the map dependence lives.

### What a manual log read found that four aggregate metrics missed (2026-08-09)

Cover, displacement, delivery rate and OOM tail were all measured and none
explained the map gap. Reading one indicative match end to end did, in about ten
minutes.

**Seed 1, PillaredArena, healer called** — `Identity` wins at 74.2s, `Priced`
loses at 76.8s, the same seed flipped by the policy:

| | `Identity` (WIN) | `Priced` (LOSS) |
|---|---|---|
| enemy **Rogue** | **died**, took 386 | **survived** 197/386, took 189 |
| our Warlock | alive 250/402, took 152 | died, took 402 |
| enemy Priest (the called kill target) | died | **died** |

We killed the called kill target in **both** runs and lost one of them anyway.

**The correlation, across all 32 matches (2 maps x 2 policies x 8 seeds):**

| | we won | enemy Rogue died |
|---|---|---|
| BasicArena `Identity` | 3/8 | **3/8** |
| BasicArena `Priced` | 5/8 | **5/8** |
| PillaredArena `Identity` | 2/8 | **2/8** |
| PillaredArena `Priced` | 3/8 | **3/8** |

**The match is won if and only if the OFF-target dies — perfectly, in every
cell.** The called healer dies in essentially every match; it is focused by the
whole team and its death is not in question. What decides the game is whether
the *other* enemy also dies, and that happens only through incidental curse
spread.

So the CC model has been pricing the wrong thing. Every term built so far —
`T_eff`, the break budget, the dispel expectation, the kill window — prices the
lockout of a healer whose death is already assured. The quantity the outcome
actually turns on is **damage delivered onto the off-target**, which the model
does not represent at all. `Priced` improves it on both maps (Rogue damage taken
239 -> 268 on BasicArena, 170 -> 247 on PillaredArena), which is why it helps at
all; PillaredArena simply starts from a lower base and converts less often.

### Off-target diagnosis: a race, not a window

Following the "win iff the off-target dies" correlation through, over the same
32 matches.

**Almost all off-target damage arrives AFTER the called target dies.** The kill
call focuses the healer; nothing meaningful lands on the Rogue until the team
re-acquires:

| map | policy | healer dies | match end | window left | rogue dmg before | rogue dmg after |
|---|---|---:|---:|---:|---:|---:|
| BasicArena | Identity | 33.8s | 47.9s | 15.4s | 22 | **190** |
| BasicArena | Priced | 39.7s | 54.8s | 13.2s | 26 | **245** |
| PillaredArena | Identity | 58.3s | 64.8s | 7.5s | 66 | **105** |
| PillaredArena | Priced | 55.2s | 68.9s | 11.2s | 56 | **168** |

Seconds remaining after the healer dies also separate the results almost
perfectly — wins mean 15.6s (min 11.6s), losses mean 4.1s (median 2.4s), with a
single 11.6s threshold splitting 31 of 32 matches.

### That correlation is real and the causal reading of it was WRONG

`tests/first_kill_state_probe.rs` measures the state at the instant the called
healer dies, which is the check that should have been run before drawing any
conclusion from the window:

| result | n | **our team HP** | off-target HP | window | **our units alive** | our deaths after |
|---|---:|---:|---:|---:|---:|---:|
| WON | 13 | **0.97** | 0.92 | 15.6s | **2.00** | 0.38 |
| LOST | 11 | **0.35** | 0.92 | 7.1s | **1.27** | 1.27 |

In a typical loss we are at **35% team health with one unit already dead** when
the enemy healer finally falls. In a win we are at **97% with both alive**. The
match was already decided; the short window is a **symptom of having lost the
race**, not the resource that decides it. Only **3 of 24** matches saw the
off-target kill both of ours after its own healer died.

So "lengthen the post-kill window" is not a lever, and the tempo term this
section previously argued for is not supported by this evidence.

### What the evidence does support

The match is a **race**: our focused damage on the called healer against the
off-target's damage on us. The kill call puts both our units on the healer, which
leaves the Rogue **completely unpressured** — it deals 900-1150 damage per match
while taking 170-270, almost all of that incidental and almost all of it after
the healer is already dead.

Wins are races we win comfortably (97% health remaining); losses are races we
lose outright. The off-target's death and the window length are both downstream
of that, which is why they correlate so cleanly with the result while explaining
nothing about it.

That points the investigation at **peel and target selection**, not at CC
valuation: nothing in the comp answers an unpressured Rogue, and the CC model
cannot fix a race it does not participate in.

### Method note

The window correlation separated 31 of 32 matches on a single threshold, which is
exactly the kind of result that reads as a mechanism and is not one. The check
that caught it — measuring the state at the moment of the first kill — took one
probe and should have been the first thing run, not the second. A near-perfect
separator is evidence that something upstream is driving both variables.

### Other things the read surfaced

- **Action density is very low.** The Warlock casts **11 spells in a 77s match**.
  Everything else is DoT ticks, wand chip, and dead time. Both policies produce
  nearly identical timelines; the divergence is a single substitution (a Fear
  where a Curse of Agony would have gone) whose effects cascade.
- **Dead time is structural, not policy-driven.** A 6s Kidney Shot plus a long
  wand-only tail. The dry tail after the last real spell is **9-13s (16-20% of
  the match) in all four cells** — proportionally constant, so it is not the map
  differentiator either, though it is a large standing inefficiency.
- **Everyone finishes mana-dead**: 9-12 of 296 in every cell. These matches are
  decided after all four combatants are dry.
- The log's `Duration` includes the 10s countdown; `MatchResult.match_time` does
  not. The two clocks differ by ~7-10s and must not be compared directly.

### What this changes

**Scope correction first.** An earlier draft of this section asked whether "the
kill call should be what it is". That is not a coherent action item:
`team1_kill_target` is a **player input**, set from the Configure screen or the
in-match HUD call. The AI does not choose it and it is not something to fix.

What it does mean is a caveat on the headline. **The condition step 1 was
measured under — healer called as the kill target — was imposed by the
measurement, not observed.** It was chosen because it is when the Warlock's
defect fires. With free targeting the same change measures +7pt (z=1.27) on
BasicArena and +3pt (z=0.61) on PillaredArena — same sign, neither significant.
The +10pt figure is conditional on a configuration the player has to make, and
should always be quoted with that condition attached.

The in-scope question is not which target the player calls. It is: **given any
call, does the AI answer the threat that call leaves unattended?** It does not,
and the reason is specific and fixable.

**The Warlock's peel is self-only.** `pick_death_coil_peel` measures distance
from `my_pos` and gates on `info.target == Some(me)`, so it fires only for
threats on the Warlock itself. In the audited match the Rogue hit our **Priest 14
times and the Warlock 3**, and Death Coil — 30s cooldown, 3s horror that never
breaks on damage, the best peel in the kit — fired **once**, when the Rogue
happened to stray near the Warlock. The healer died. Nothing in the class AI has
a trigger for "my healer is being killed".

That is a **`D` defect, not a targeting one**. `D` is specified as the value of
the actions a unit would take, *including damage that would land on us* — and
"us" has to mean the team, not the caster. It lands in step 3 and is
independently testable without involving kill calls at all.

Methodologically: four aggregate probes agreed the effect was real and none
located it. One read of one match did. Aggregates are for confirming a mechanism,
not for finding one.

### Reading these numbers honestly

- This is **one class's one gate**. It is not "the CC value model is worth +10pt";
  it is "pricing beats identity for the Warlock's healer Fear, in the scenario
  where identity was measurably broken".
- BasicArena was chosen to isolate the effect, and the PillaredArena re-run
  above shows that choice mattered a great deal: the win is **map-dependent**.
- The break classifier is still at 60% precision (*Predictor v2*). These gains
  were obtained *despite* that, which suggests `MIN_FEAR_T_EFF` being
  conservative is doing useful work — a mis-predicted break costs a declined
  Fear, not a wasted one.
- **Canonical baselines are untouched**, because `Identity` remains the default
  and is byte-identical. Flipping the default would require regenerating them.

### Tooling added

`scripts/headtohead_sweep.py` grew two options, both general rather than
CC-specific: `--axis {ai_profile,cc_policy}` selects which per-team axis to
sweep (pinning the other one), and `--team1-kill-target` / `--team2-kill-target`
pass an explicit kill call through, so "what happens when the team calls X" is a
measurable condition rather than a manual config edit.

---

## Step 2 result (2026-08-08): `I` is built and correct; the interrupt half does NOT beat the heuristic

**Shipped, behind the same `CcPolicy::Priced` gate.** `cc_value::CastValue` +
`interrupt_value_with_lockout` (pure, 8 unit tests), and a priced interrupt
target selection in `check_interrupts` that scans every interruptible cast in
range and picks by value instead of reacting to whatever the unit's own kill
target happens to be doing.

### The headline: it measured negative, and it is not being recommended

`Shaman+Warrior` vs `Priest+Warlock`, n=150, BasicArena:

| | `Identity` | `Priced` | effect | z |
|---|---:|---:|---:|---:|
| interrupt policy | 53% | 49% | **-5pt** | -0.81 |

Not significant, but not a win, and two evidence-led iterations did not move it.
**The identity heuristic ("interrupt the healer") is encoding something the
magnitude model loses.**

### Why — and the model bug it exposed

The strategic value of an interrupt is mostly the **school lockout**, not the
cancelled cast. Locking Holy for 4s denies *every* heal in that window. The
2026-06-28 Warlock findings said exactly this about Spell Lock and I priced it
wrong anyway.

I added a lockout term — the interrupted cast's own value per second, times the
lockout duration — which correctly makes a fast repeatable Flash Heal outrank a
slow one-off nuke. It changed no rankings and the result was byte-identical.
The remaining bug is visible in the formula: **lockout value is derived from the
cancelled cast's value, so a cast worth ~0 contributes ~0 of lockout value.**
That is wrong. Interrupting a heal that would have been pure overheal *still*
locks Holy for 4 seconds. The lockout should be priced from the target's
throughput in that school, independent of the particular cast being cancelled —
which is a different input than any currently plumbed.

### What step 2 did deliver

- **Three blind spots closed, caught by measurement rather than review.** The
  first version priced only direct healing and damage, so it walked straight
  past Unstable Affliction, Fear, and Drain Life — a Shaman under `Priced` never
  interrupted any of them. Now DoT applications are valued at their full tick
  damage, incoming CC at `CC_DENIAL_PER_SECOND × duration`, and channels are
  scanned alongside casts. Had step 2 shipped on the first version it would have
  been a clear regression.
- **A structural finding about melee interrupts.** Pummel and Kick are **2.5yd**.
  A Warrior or Rogue can only ever interrupt the one caster it is already
  standing on, so "which cast to interrupt" is decided by positioning, not by
  policy — the priced and identity paths chose identically across every seed
  tested. Step 2's premise ("fixes Warrior/Rogue interrupt policy") is therefore
  **only true in principle**; at 2.5yd the choice set is almost always a
  singleton. Wind Shear at 30yd is the only interrupt with a real decision, which
  is why the Shaman comp is where it was measured.

### Consequence for shipping: the flag was split

`CcPolicy::Priced` originally **bundled step 1 and step 2**, which meant a
measured win (+10pt) and a measured non-win (-5pt) could only ship together.
That bundling was itself visible in the numbers: step 1's null control — the
side with no Warlock, which should be *exactly* unmoved — drifted from `+0pt` to
`-1pt (z=-0.17)` once the opposing Rogue's interrupts started changing too.

The flags are now split, and the drift is gone:

| | effect | z | null control |
|---|---:|---:|---:|
| step 1, bundled with step 2 | +10pt | +2.56 | -1pt (z=-0.17) |
| step 1, axes split (n=300) | **+10pt** | **+2.56** | **+0pt (z=0.00)** |

### The split is per DECISION SITE, not per migration step

This distinction is the load-bearing one, and getting it wrong in either
direction is costly.

- [`CcPolicy`] governs *which enemy to crowd-control*.
- [`InterruptPolicy`] governs *which cast to interrupt*.

Two different decisions, taken by different classes through different abilities,
whose code paths never consult each other — genuinely independent, and measured
with opposite signs, so they must be separately switchable.

**A flag per step would be wrong.** The remaining steps — `D` (denial rate),
`C` (cost), `E` (enabling) — are all *terms in the same CC-target score*. They
interact by construction, so measuring them behind separate flags would be
measuring nonsense, and six flags would be sixty-four configurations. They land
on `CcPolicy` and are measured incrementally against it.

Independence is asserted, not assumed. `tests/cc_policy_gate.rs` pins that
naming `Identity` on either axis is bit-identical to omitting it, that each axis
diverges somewhere on its own, and — the leak check that matters — that flipping
the CC axis leaves a match with **no Warlock in it** bit-identical.

### Recommended settings today

| axis | recommended | why |
|---|---|---|
| `cc_policy` | **`Identity`** — do NOT flip the default | +10pt (z=2.56) / +19pt (z=3.69) on BasicArena, but only +2pt (z=0.44) / **-7pt (z=-1.54)** on PillaredArena. A map-dependent win is not a win |
| `interrupt_policy` | **`Identity`** | `Priced` measured -5pt (z=-0.81); needs the lockout priced from target throughput first |

---

## Step 3 result (2026-08-09): `D` implemented generally; +13pt on BasicArena, still nothing on PillaredArena

**Shipped as a general primitive, not a Warlock patch.** `cc_value::denial_rate`
+ `DenialInputs` (pure, 4 unit tests), and three `CombatContext` helpers any
class can call: `enemy_damage_to_us`, `enemy_healing_capped`, `denial_rate_of`.

`D` answers "what is removing this unit worth, per second, **to the team**". Its
two components are the ones the log read demanded:

- **damage_to_us** — damage this enemy is landing on *any* of our units,
  attributed as its even share of the damage actually arriving on the unit it is
  pointed at. Prices to zero for an enemy targeting nobody, or whose target is
  taking nothing because it is kited or the attacker is dry.
- **healing_capped** — healing it would deliver, capped by our delivery onto its
  team, because denying healing is worth what it fails to erase. An out-of-mana
  healer prices at zero.

**Consumed in two places.** The Warlock's Death Coil peel now picks the highest-`D`
enemy inside Death Coil's real 30yd range instead of asking "is something within
8yd threatening *me*"; and the healer-Fear gate ranks by `D x T_eff` rather than
the kill-window proxy.

### Measured

| cell | step 1 alone | step 1 + `D` |
|---|---:|---:|
| BasicArena 2v2, healer called (n=300) | +10pt (z=2.56) | **+13pt (z=3.29)** |
| PillaredArena 2v2, healer called (n=300) | +2pt (z=0.44) | **+2pt (z=0.44)** |
| PillaredArena 2v2, free targeting (n=150) | +3pt (z=0.61) | +4pt (z=0.72) |

Null controls exactly `+0pt (z=0.00)` in all three. The peel works mechanically on
both maps — our Priest dies in 6/8 -> 5/8 (BasicArena) and 7/8 -> 6/8
(PillaredArena) — but on PillaredArena that does not convert, and the cell is
**numerically identical** to step 1 alone. The map dependence is untouched and
remains unexplained after five hypotheses.

### Three bugs found on the way, each of which silently zeroed everything

`D` returned 0 for every enemy on its first three attempts. None of these would
have failed a test; all of them produced plausible-looking behaviour.

1. **`RecentDamage` measured a HEALTH delta.** A unit healed as fast as it is
   damaged shows no health loss, so its incoming damage read as zero — and
   healed units are exactly the ones this term cares about. Fixed by deltaing
   `Combatant::damage_taken`, which is cumulative and monotonic.
2. **A clock mismatch.** Samples were timestamped with `Res<Time>` inside
   `FixedUpdate` (which is `Time<Fixed>`) and filtered against the generic
   `Time` read elsewhere. The two disagree, so every sample fell outside its own
   window. Fixed by making the window **frame-based** — the timestep is a fixed
   1/60, so a frame count is exact and cannot desynchronise.
3. **The component was never attached in headless at all.** `RecentDamage` was
   added to the two spawn sites in `play_match/mod.rs`; the headless runner has
   **six more of its own** in `headless/runner.rs`. This is the documented
   graphical/headless divergence hazard, and note what the existing guardrail
   does and does not cover: `tests/registration_audit.rs` enforces that *systems*
   are registered in both paths, and nothing enforces that *components* are
   attached in both.

The lesson is in `tests/recent_damage_tracker.rs`, which now asserts the tracker
observes real damage. A quantity that silently reads zero produces AI that looks
reasonable and is inert — the peel "worked" for three iterations while firing
zero times.

---

## The dispel exchange, and a second CC anti-synergy (2026-08-09)

Found by watching a match, not by a probe. Two observations from the manual
review, both confirmed at n=40 per policy.

### 1. CC on a healer suppresses Unstable Affliction's backlash — partially, and with caveats

Unstable Affliction punishes a dispel: removing it deals **138 Shadow damage**
and silences. Crowd-controlling the healer stops it dispelling, so it stops the
punish. Across 40 seeds per policy:

| policy | UA dispels / match | backlash damage / match |
|---|---:|---:|
| `Identity` | 1.27 | **130.9** |
| `Priced` | 1.05 | **100.8** |

**Two corrections to how strongly this can be read** (both raised on review, both
right, and the first materially weakens the claim):

1. **Which debuff a dispel removes is RANDOM.** `process_dispels` picks a buff
   instance at random, and that randomness is *intentional* — it is what makes
   purge an investment rather than a deterministic counter. So the backlash gap
   above is not purely "CC suppressed the dispel"; a meaningful share of it is
   which debuff the roll happened to select. The direction is right, the
   magnitude is not attributable to CC alone, and seed 1 is an illustration
   rather than proof.
2. **A suppressed dispel is not a pure loss.** If the healer cannot dispel, the
   DoT keeps ticking and the healer must spend mana healing through it. The Fear
   trades backlash damage for sustained DoT damage plus enemy mana drain — a
   worse trade, probably, but not the 276-damage cliff a single seed suggests.

What survives is the *shape*, which is what matters for the model: a CC on a
dispeller has a **negative component through our own kit**, and the model has no
way to represent it. That makes two:

- purging a shield shortens the `T_eff` of your own CC on that target;
- CCing a dispeller suppresses (some of) your own Unstable Affliction's punish.

Both are cases where CC value is negative *through one of our own mechanics*.
The model prices what a CC denies the enemy and never what it denies us.

**A design option this suggests, not implemented:** if CC can suppress a dispel,
the mirror is that a Priest or Paladin could choose to *withhold* a dispel when
Unstable Affliction might be the instance rolled. That is a real strategic layer
on the healer side and it does not exist today.

### 2. The matchup is a dispel exchange the Warlock loses

| | cost | cast | cooldown |
|---|---:|---|---|
| Warlock DoT (Corruption / Curse of Agony / Immolate) | 25 mana | GCD | — |
| Unstable Affliction | 30 mana | 1.5s cast | — |
| **Priest Dispel Magic** | **18 mana** | **instant** | **none** |

Measured over 80 matches: **9.5 DoT applications per match, 46% of them
dispelled, mean lifetime before removal 2.1 seconds.** Curse of Agony applied at
19.25s was gone at 19.28s; Corruption applied at 27.73s was gone at 27.75s.

The Priest removes a 25-30 mana DoT for 18 mana with no cast time and no
cooldown. The Warlock loses the trade on mana *and* on tempo, roughly four and a
half times a match, out of a pool of 296 that funds about **eleven casts total**.

### Why this matters for the model

The CC decision is a small perturbation on top of an exchange the model does not
represent at all. That is a better explanation for the map result than anything
tested before it: on BasicArena the fight is short enough that CC timing still
swings outcomes (+13pt, z=3.29); on PillaredArena, at ~40% longer, the dispel
exchange has time to run to its conclusion and the CC choice is noise on top of
it (+2pt, z=0.44).

It also means **`C` should price mana, not the GCD.** Mana is the binding
resource here — everyone finishes on ~10 of 296 — and its shadow price rises as
the expected remaining match duration exceeds what the pool funds. That is a
quantity that genuinely differs between a 40s map and a 70s one, which is more
than can be said for any of the six hypotheses tested before it.

---

## Steps 4 and 5 (2026-08-09): both implemented, neither earns its keep yet

### Step 4 — `C`, priced in mana. Calibrated, and the calibration is a negative result.

`cc_value::action_cost` + `CostInputs`, consumed by the Warlock's Fear gate. This
is the architectural change the plan called for: **whether to CC stops being the
ability's position in the priority ladder and becomes a comparison.**

Cost is priced in **mana, not GCDs** — the correction the log read forced. The
GCD is not scarce (about eleven casts a match); mana is (everyone finishes on
~10 of 296). Scarcity is the *fraction of remaining mana* an action consumes,
which needs no forecast of match length.

Then it was calibrated by sweep, at n=300 per point:

| `MANA_POOL_DAMAGE_EQUIVALENT` | effect |
|---|---|
| 0 (gate off) | +10pt, z=2.46 |
| 150 | +10pt, z=2.38 |
| **300** | **+10pt, z=2.38** |
| 600 | +6pt, z=1.39 |
| 1000 | +5pt, z=1.31 |

**There is no value at which the gate helps.** At or below 300 it is
indistinguishable from having no gate; above that it declines Fears worth
casting. So mana scarcity *on its own* does not earn a veto. 300 is kept as the
largest measurably-neutral setting, which preserves the comparison structure for
when the other half of `C` exists — `displaced_value`, the price of the rotation
cast a CC displaces, is still 0 and is the part most likely to make this mean
something.

The original anchor of 1000 was wrong: it came from "a full pool is worth a
match's ~1000 damage", which includes wand chip and DoT ticks rather than only
mana-derived damage.

### Step 5 — `E`, enabling. Implemented; effect not demonstrated.

`cc_value::enabling_value` (depth 1, discount 0.6) plus
`CombatContext::dispel_denial_uplift`. The concrete mechanism is dispel denial:
a break-on-any-damage CC is worth almost nothing while a free dispeller can
remove it and its full duration once that dispeller is locked. So Fearing a
healer is worth extra *because it makes a teammate's Polymorph stick* — the
externality a chain's opener creates and otherwise captures none of.

Guards are as designed: depth 1 against teammates' **un-uplifted** capability, a
sub-unit discount so chains cannot inflate, and zero unless a teammate actually
owns a deliverable follow-up.

Measured on `Warlock+Mage` vs `Priest+Rogue` (the roster that can actually
chain), n=300, healer called: **+1pt, z=0.41.** Not significant. That comp's
baseline is 19%, so headroom is thin, and **win rate is the wrong instrument
here** — the doc's own measurement plan says chains should be scored on
*simultaneous-control seconds* (baseline 0.21s/match against 13.93s of total
denial), which has not been re-run since the term landed. That is the
outstanding measurement, not another sweep.

### Step 5 scored on the right metric, and `displaced_value` completed

**Step 5 works mechanically.** Scored on simultaneous-control seconds (the
measurement plan's metric, not win rate), 36 matches per policy:

| policy | denied/match | simultaneous | counterplay-free | overlap share |
|---|---:|---:|---:|---:|
| `Identity` | 14.05s | **0.21s** | 0.21s | 1.5% |
| `Priced` | 15.52s | **0.41s** | 0.23s | 2.6% |

Chain time **nearly doubles**, which is exactly what the enabling term was built
to do. But 0.41s out of 15.5s of denial is far too small to move a match, which
is why win rate showed +1pt (z=0.41). The mechanism is real and the effect size
is not — chains are rare in 2v2 with a single CC-capable partner.

**`displaced_value` is now implemented** (`dot_expected_damage`), completing the
CC-versus-rotation comparison. The action a Fear displaces is a DoT
re-application, not a nuke, and against a dispeller it is worth far less than
full duration — 46% of applications are removed after ~2.1s. The same free
dispeller is applied to BOTH sides, since it shortens the Fear and the DoT alike.

It does not change the outcome:

| cell (n=300) | `displaced_value` = 0 | implemented |
|---|---:|---:|
| BasicArena | +10pt (z=2.38) | **+9pt (z=2.13)** |
| PillaredArena | +2pt (z=0.44) | **-1pt (z=-0.17)** |

So with **both halves of `C` built**, the cost comparison is still neutral. The
architectural change stands — whether-to-CC is a comparison rather than a ladder
position — but it does not improve outcomes.

### Status after 4 and 5

Nothing changes the recommendation: `cc_policy` and `interrupt_policy` both stay
at `Identity`. Steps 1 and 3 are the only parts with a demonstrated effect
(+13pt BasicArena pre-Corruption-change, +10pt after), and the map dependence is
still unexplained.

---

## The map dependence: SOLVED (2026-08-09). It was the price of Fear.

Seven hypotheses were measured and refuted — cover, displacement, delivery rate,
OOM tail, post-first-kill window, tempo, mana scarcity as a gate. The answer was
none of them. It is the **mana cost of Fear relative to the DoT it displaces**.

Found by watching four matches. The read: the PillaredArena regressions are
slivers where "the Warlock is more OOM and unable to damage the Rogue... a result
that could flip with a single mana cost change."

Measured, n=300 per cell, varying only Fear's cost:

| Fear mana | BasicArena | PillaredArena |
|---|---:|---:|
| **30** (shipped) | +9pt (z=2.13) | **-1pt (z=-0.17)** |
| 22 | +8pt (z=2.06) | +6pt (z=1.57) |
| **16** | **+11pt (z=2.65)** | **+11pt (z=2.61)** |

**At 16 the map dependence vanishes**: both maps +11pt, both significant, and the
gap that survived seven investigations closes completely.

### Why it was invisible to every probe

The CC model prices what a Fear *denies*. Its cost in mana is real but is paid in
a different currency and shows up much later — as a Warlock that cannot finish
the off-target thirty seconds after the CC decision. Every probe measured the
denial side and none connected a 30-mana spend to a sliver loss half a minute
later.

PillaredArena runs ~40% longer, so it needs more casts from the same pool; an
over-priced Fear therefore costs more there. That is the map-dependent quantity
all along — not geometry, not distance, not line of sight.

**This project made it worse.** Cutting Corruption 25 -> 16 (a good change on its
own terms) left Fear at 30, moving the CC-to-DoT price ratio from 1.2 to **1.9**.
The PillaredArena cell went +2pt -> -1pt across that change. At Fear 16 the ratio
is 1:1 and both maps agree.

### What this implies

1. **A balance proposal, not a model change:** Fear 30 -> 16, matching Corruption
   and restoring a 1:1 CC-to-DoT price. NOT committed — it is a balance change to
   shipped data and needs its own review, plus the same canonical-baseline
   regeneration Corruption triggered. Note the baseline barely moves on
   PillaredArena (LL 128 -> 129) while BasicArena rises (149 -> 160), so this is
   mostly a policy-interaction effect rather than a flat class buff.
2. **`C` was pointing at the right thing and priced it wrong.** Step 4 concluded
   mana scarcity "does not earn a veto" — correct as far as it went, but the
   issue was never whether to veto on scarcity. It was that the *relative price*
   of CC versus its alternative was wrong in the DATA, which no gate can fix.
3. **The measurement standard rises.** Any future CC-model result should be
   checked at more than one relative CC/DoT price before being called
   map-dependent, since that ratio evidently dominates.

### A separate correction: some of the BasicArena win is cascade, not the model

Watching `basic-seed22` showed the flip was caused by **our Priest** landing a
Psychic Scream in the opener under `Priced` and not under `Identity` — and the
Warlock's own play "doesn't differ much and isn't too important".

`cc_policy` is read **only in `warlock.rs`**; the Priest AI never sees it. So
that flip is definitionally a **cascade**: the Warlock acted slightly
differently, the timeline shifted, and a different Priest opportunity
materialised. Some share of the BasicArena win is therefore chaotic
re-rolling rather than better CC decisions.

That does not erase the result — cascades are direction-random and should cancel
across 300 paired seeds, and the Fear-cost sweep moves the effect coherently in
both directions, which noise would not do. But it does mean **per-seed flips are
not evidence of the model deciding better**, and any future single-seed story
needs the same check.

Seed 6 is the counter-example worth keeping: there `Priced` Fears the enemy
healer at half HP while fully DoTed, denying the self-heal and killing it much
faster. That is the model working exactly as designed.

---

## Extending the model to a second class (2026-08-09): the Mage, and a negative result

Until now the entire model was exercised by **one class**. `cc_policy` was read
only in `warlock.rs`, which is also why per-seed stories were unreliable: a flip
could be the model deciding better or a cascade re-rolling someone else's
opportunity (watching `basic-seed22` showed the latter).

The Mage was the natural second class. Its Polymorph gate carried the same shape
of defect the Warlock's Fear gate did — a **hardcoded stance assumption**:

> `cc_target equals kill target — would break on damage`

Correct while we are damaging that unit and wrong otherwise, exactly as the
`TeamPlan` doc predicted for this family of guard.

### What shipped

`pick_polymorph_target` ranks candidates by `D x T_eff` against `action_cost`,
with Polymorph's zero break-budget, `Incapacitates` DR, absorb pool, attacker
mix and dispel exposure all fed through the same shared primitives. The
kill-target rule is now **derived** rather than asserted: a target under fire has
`T_eff` near zero and is rejected; a target nobody is hitting is allowed.

Behaviourally it works. Over 10 seeds the Mage went from **6 Polymorphs, all on
the enemy healer** to **15 — 12 on the healer and 3 on the Warlock**, the latter
being casts the identity guard forbade outright.

### It measures NEGATIVE for the Mage, and positive for the Warlock

`Mage+Priest` vs `Warlock+Priest`, BasicArena, n=300:

| side given `Priced` | effect |
|---|---:|
| team 1, `Mage+Priest` | **-7pt (z=-1.81)** |
| team 2, `Warlock+Priest` | **+12pt (z=+2.87)** |

The same policy helps one class and hurts the other, both substantially.

**The floor is not the lever.** Raising `MIN_POLYMORPH_T_EFF` makes it
monotonically worse — 1.0 → -7pt, 2.0 → -8pt, 3.0 → -8pt, 5.0 → -11pt — so the
priced Mage is not losing because it sheeps too eagerly. Being *more*
conservative hurts more. The cause is undiagnosed.

A plausible reading, untested: Polymorph is break-on-any-damage, and step 0 found
threshold-0 CC to be the model's **worst** prediction case (Polymorph predicted
0.00s against 2.50s actual at n=8; +0.62s at n=22). Extending the model to the
Mage puts decision weight on its least reliable term.

### This re-opens the bundling problem the flag split solved

`cc_policy: Priced` now bundles a **+12pt Warlock change with a -7pt Mage
change**. That is precisely the situation the `CcPolicy` / `InterruptPolicy`
split was created to prevent, arriving by a different route: not two decision
sites, but one decision site across two classes that measure with opposite signs.

The flag-per-decision-site rule does not resolve this, and a flag per class is
the combinatorial explosion that rule exists to avoid. **Unresolved.** The
options are to diagnose and fix the Mage regression, revert the Mage extension,
or accept a per-class opt-in with the measurement cost that implies. Nothing
should flip to `Priced` by default until this is settled.

---

## Pets are invisible to the model, and it costs a matchup (2026-08-09)

Three facts, each verified in the code, that together explain the priced Mage's
worst cell:

1. **`alive_enemies()` filters out pets** (`!c.is_pet`), so no CC valuation in
   any class can even see the Felhunter as a candidate.
2. **CCing an owner does not stop its pet.** `pet_ai` gates on the *pet's own*
   auras (`is_incapacitated(auras)` where `auras` is the pet's), which is
   WoW-faithful — pets act independently.
3. Therefore **Devour Magic is unanswerable**. It strips a Polymorph 0.03s after
   it lands, the pet cannot be crowd-controlled, and CCing the Warlock does not
   stop it. There is no play any class can make.

Measured across matchups, `Mage+Priest` with `cc_policy: Priced`, n=300 each:

| opponent | Felhunter? | baseline | effect |
|---|---|---:|---:|
| `Warlock+Priest` | **yes** | 60% | **-10pt (z=-2.46)** |
| `Priest+Rogue` | no | 43% | **+4pt (z=+0.90)** |
| `Warrior+Priest` | no | 95% | -5pt (z=-2.46) |
| `Paladin+Rogue` | no | 16% | -6pt (z=-2.03) |

The Felhunter cell is the worst by a clear margin, and the one Felhunter-free
matchup with a mid-range baseline is positive. Discount `Warrior+Priest` at a 95%
baseline — near a ceiling almost any behaviour change loses ground. So Devour
Magic aggravates the strategy badly, but `Paladin+Rogue` at 16% has room to
improve and does not, so it is not the whole cause.

### Pets as CC targets: implemented, and Polymorph-on-pet measured NEGATIVE

`alive_enemies_including_pets` was added (a separate accessor, so `alive_enemies`
and therefore the whole `Identity` path is untouched). Pets already carry their
own `DRTracker`, so diminishing returns land on the pet rather than its owner,
and they cannot be re-summoned, so CC spent on one is not wasted.

The Mage immediately used it — 6 of 19 Polymorphs went to the Felhunter, casts it
previously could not even consider. **And the matchup got much worse: -10pt ->
-17pt (z=-4.17).**

The valuation arithmetic explains it:

| sheep target | can it be removed? | `T_eff` | value |
|---|---|---:|---:|
| Felhunter | nothing on their side removes CC from a pet | **~10s** | ~100-150 |
| enemy Priest | devoured in 0.03s | **~2s** | ~25 |

So the model prefers the pet by a wide margin — and that preference is wrong.
The gap is in `D`: it prices "damage this unit would deal us" and "healing it
would deliver" **symmetrically per second**, but denying a healer *compounds into
a kill* while denying a pet only slows their offense. A 10-second lock on a
melee pet is not worth four times a 2-second lock on the healer, and the model
cannot currently say so.

**Scoped back**: pets are excluded from Polymorph candidates pending a `D` that
distinguishes those two kinds of denial. The accessor stays, and the Warlock's
Death Coil peel still uses it — peeling a pet off our healer is what that ability
is for, and it is a defensive choice rather than a ranking against a healer.
(That path is untested: the comp measured has no pets on the Warlock's side.)

### What pets should be worth

A pet carries damage, dispels and crowd control of its own. The value model
already has the terms — `denial_rate` prices "damage this unit lands on us"
without caring whether the unit is a pet — so this is a **filter** problem, not a
modelling one. Removing pets from the candidate filter would let a Frost Nova on
a pet, or a sheep on a Felhunter, be valued on the same footing as anything else.

Open questions before doing it: whether pets should carry DR at all, whether CC
on a pet should share the owner's DR bucket, and whether the AI should ever spend
a long-cooldown CC on a unit that can be re-summoned.

## Divine Shield: the avoidance half already works, the exploitation half does not

Checked because an immune target is the clearest case of wasted effort. Over 12
`Mage+Priest` vs `Paladin+Rogue` matches, Divine Shield fired in **all 12**, and
of **200** actions we aimed at the Paladin, **zero** landed while it was bubbled.
The `entity_is_immune` guards work.

What does not exist is the *positive* play. An 8-second bubble is an 8-second
window in which the enemy healer cannot be touched, and the right response is to
spend that window on the partner — pressure or crowd control that the Paladin
cannot answer by dying. Nothing in the model represents "this window is
temporarily unwinnable against target X, so redirect", and the immunity guards
only make units skip the Paladin rather than actively capitalise elsewhere.

That is a **stance** question of the kind the `TeamPlan` doc anticipated, arriving
in a new form: not "what is my team doing" but "what is temporarily impossible".

---

## The admission criterion (2026-08-10): a class joins the model only if its `T_eff` is predictable

Six attempts to make the priced Mage beat the identity heuristic, each
individually justified, each neutral or negative (n=300, `Mage+Priest` vs
`Warlock+Priest`, BasicArena):

| change | result |
|---|---|
| `D x T_eff` ranking | -7pt |
| + `I` interrupt value | -8pt |
| + GCD floor on value | -8pt |
| + `forgone_damage` (our own damage stops on a sheeped target) | -9pt |
| + Felhunter counted as a dispeller | -10pt |
| + pets as CC candidates | **-17pt** |
| + correct per-attacker damage attribution | **-15pt** |

The **same model, same sweep, same seeds** is **+12pt (z=+2.87)** for the
Warlock. So the problem is not the model and not the class — it is the
**predictability of the inputs**:

- **Fear**: 100-damage break budget, 8s duration, nothing on the enemy side
  removes it from a player. `T_eff` is a well-posed estimate, and step 0
  measured Fear-on-healers at **+0.01s** error.
- **Polymorph**: breaks on ANY damage, and a Felhunter devours it **0.03s** after
  it lands. Step 0 measured threshold-0 CC as the model's **worst** prediction
  case (0.00s predicted against 2.50s actual).

Pricing a decision on an unpredictable quantity loses to a crude heuristic that
does not try. That is the general lesson, and it was already in the step-0 data
before any of these six attempts.

### The criterion

> **A class joins the priced model only once its CC's `T_eff` prediction error
> has been measured on the step-0 harness and found comparable to Fear's.**

Cheap, objective, and it would have predicted this outcome for free. It also
tells us what to fix first if the Mage is wanted: not another value term, but
`T_eff` for break-on-any-damage CC.

`MAGE_PRICED_POLYMORPH` is `false`. The priced path stays compiled and tested, so
re-enabling is one line once threshold-0 `T_eff` is trustworthy. With it off, the
Mage cells return to **exactly +0pt** while the Warlock keeps **+12pt** in the
same sweep and **+9pt (z=2.43)** in its own.

### Two fixes kept, because they are correct regardless

- **Per-attacker damage attribution.** `enemy_damage_to_us` now reads a unit's
  own trailing damage-DEALT rate instead of an even split of its victim's
  incoming damage. Measured, a Warlock dealt 1180 damage in a match while its
  Felhunter dealt 170 — the even split credited the pet with ~7x its real
  contribution. Neutral for the Warlock, and correct.
- **`alive_enemies_including_pets`.** Pets carry damage, dispels and CC, cannot
  be re-summoned, and hold their own DR. The Warlock's Death Coil peel uses it
  (untested — the measured comps have no pets on that side). Polymorph does not,
  for the reason above.

---

## Fixing `T_eff` for break-on-any-damage CC (2026-08-10)

The admission criterion above names threshold-0 `T_eff` as the blocker for every
class after the Warlock. This is the attempt to clear it. It did not clear it —
but it found a much larger defect on the way, and the predictor is substantially
better for it.

### The headline

| | before | after |
|---|---|---|
| mean absolute error | 1.33s | **1.12s** (29% relative) |
| **skill vs a constant predictor** | **−2%** | **+41%** |
| never-breaks slice (stuns, horror), n=76 | −17% | **+82%** |
| damage-budget slice (Fear), n=36 | +16% | +16% |
| break-on-any slice (Polymorph), n=22 | −30% | **−33%** |

**Live behaviour is unchanged.** The canonical Warlock cell is still +9pt
(z=+2.43) and every test binary is green: the one live-model change is inert
because Fear's break budget (100) sits far above the new floor and the Mage is
gated off. This was a predictor-accuracy change, measured on the step-0 harness.

### The skill score, and why it belongs in the report

A low absolute error is not evidence a model works. If the thing being predicted
barely varies, a constant scores well too. The step-0 report now prints the
model's error beside the error of *always predicting the in-sample mean* — a
baseline that is deliberately flattered, since the model never sees that mean.

Running it for the first time said the whole predictor was **worse than a
constant** (−2%). That is what turned up the real bug.

### The real bug: the dispel term was never gated on dispellability

`TEffInputs::dispel_exposed` is documented as "a dispeller is free to act **and
the aura is dispellable**", with the caller owning the second half. The step-0
probe only ever checked the first. So every stun in the survey — Cheap Shot,
Kidney Shot, Hammer of Justice, none of them removable by any dispel in this
sim — carried a shortening it could never earn.

Gating on `AuraType::is_magic_dispellable` is the entire improvement above: the
never-breaks slice went from −17% to **+82%** skill, and it alone took the
overall model from worse-than-constant to +41%.

Worth naming the failure mode, because it will recur: this was a **contract
split across a boundary**, where one side silently did half the job. The
`predict_t_eff` docs now state the caller's obligation explicitly.

### The threshold-0 fix that was kept: a break needs an EVENT, not a rate

`time_to_break = (absorb + budget) / rate` with `budget = 0` is a degeneracy —
it predicts **0.0s** for any nonzero trailing damage. Real Polymorphs last
2.21s.

The mechanic the rate model loses is that CC breaks on a *landed attack*, and
attacks are discrete. Flooring the budget at the size of one attack
(`TYPICAL_DAMAGE_EVENT`, recovered as trailing-rate × observed
time-to-first-damage: 19.4 with one attacker, 11.1 with two or more, pooling to
14.4) converts a division-by-a-zero-budget into a real waiting time. It never
binds on a positive budget, so Fear is untouched.

**Reported honestly: on this survey the floor does not improve threshold-0
accuracy.** It trades an under-prediction for an over-prediction — mean
prediction 1.47s → 3.20s against a 2.21s actual, so signed error moves +0.74s →
−0.99s and absolute error 2.45s → 2.49s. Both are bad; neither is bad in a way
the other fixes.

It is kept anyway, for two reasons that are not accuracy on n=22:

1. Predicting **exactly 0.0s** is not a calibration error, it is a degeneracy —
   it says a CC is worth nothing the instant any damage is trailing, which would
   make every future threshold-0 pricing decision impossible regardless of how
   the constant is tuned.
2. It is a mechanical statement (a break requires a landed attack), not a fit,
   and it is **live-inert today** — Fear's budget of 100 sits far above it and
   the Mage is gated off. So it carries no behavioural risk while the class that
   needs it is parked.

Tuning the constant downward would improve the fit on these 22 samples. That is
exactly the overfitting the admission criterion exists to prevent, so it was not
done.

### Break-on-any is an ARRIVAL process, not an erosion one (2026-08-11)

A zero budget does not deplete — there is nothing to deplete. The aura ends on
the FIRST landed attack, so the governing quantity is the *waiting time* for
that attack. Treating it as erosion with a tiny budget is the wrong functional
form, and the three regimes are genuinely different mechanics:

| threshold | mechanic | shape |
|---|---|---|
| `< 0` (stuns, horror) | damage is irrelevant | no break term at all |
| `== 0` (Polymorph) | ends on the first landed attack | **arrival** |
| `> 0` (Fear = 100) | a budget that depletes | **erosion** |

`predict_t_eff` now branches accordingly. The arrival branch models damage
events at `lambda = rate / TYPICAL_DAMAGE_EVENT` per second and returns the
expected time to the first, capped by the aura's duration — which for an
exponential waiting time is the closed form `(1 - e^(-lambda*D)) / lambda`, and
*is* the mixture "P(nothing lands) x duration + P(something lands) x E[when]"
without needing the branches separately. Absorb still sits in front, since
absorbed damage never reaches the break accumulator.

It degrades correctly at both ends (lambda -> 0 gives the full duration; large
lambda gives ~1/lambda) and reduces to the old erosion form when `lambda*D >> 1`,
which is why the existing threshold-0 unit tests still pass unchanged.

**It did not improve accuracy.** Threshold-0 went from -33% to **-39% skill**
against a constant predictor; overall the model went 1.12s -> 1.15s absolute
error. Kept anyway, on the same grounds as the constant it uses: the alternative
predicts exactly 0.0s for any nonzero trailing damage, which is a degeneracy
rather than a miscalibration, and the branch makes a real mechanical distinction
explicit in the code. Both are live-inert (Fear's budget is 100; the Mage is
gated off).

#### The decisive negative: the break model fails on its own turf

The obvious hypothesis was that the Felhunter's instant dispel was masking a
sound break model. Decomposing threshold-0 by ENDING PROCESS refutes it:

| threshold-0 cases | n | model | constant | skill |
|---|---|---|---|---|
| **break process only** (Expired + Broke) | 12 | 2.58s | 1.47s | **-76%** |
| dispel process (Removed) | 10 | 2.61s | 2.09s | -25% |

On the cases decided purely by damage — the break term's own subject — the model
is *worse* than on the ones it cannot see at all. So the problem is not the
shape, and not the dispel. **It is that the trailing damage rate carries almost
no forward signal for threshold-0.**

The reason is specific and it is "correction 1" from step 0 turned fatal:
applying CC changes our OWN team's behaviour. At the Polymorph call site both
inputs — `trailing` and `mix` — measure *our* damage on the sheep target, and
the friendly-CC guard exists precisely to suppress it. Fear can absorb 100
damage of our own team's mistakes; **Polymorph can absorb none**, so a single
uncancelled attack ends everything.

And the guard empirically does not do its job: step 0 extrapolated 13.3 damage
onto threshold-0 targets and **25.4 actually arrived**. DoTs already ticking do
not consult it, and neither do pets.

**So `T_eff` for break-on-any is not really a prediction about the enemy — it is
a statement about our own team's coordination.** That reframes the fix: it is
not a better estimator, it is (a) making the friendly-CC guard actually hold,
and (b) cross-CC so the Felhunter cannot answer. Both are mechanism work, and
both are the same items already queued for the Mage's admission.

### The dispel term was mis-specified, and fixing it is the largest single win (2026-08-11)

Widening the survey (a Mage beside every partner against four opponent shapes,
224 matches, 286 dispellable applications) showed the shipped dispel term was
wrong twice over:

- **Wrong population.** It counted any `is_healer()` unit. The **Shaman cannot
  dispel an ally at all** — `try_purge_enemy` skips its own team — so a third of
  the "dispellers" it counted were incapable. Counting only Priest, Paladin and
  Felhunter sharpens the split from **46%-vs-24%** to **61%-vs-11%**.
- **Wrong magnitude.** A flat 18%. Measured: **61%** with one real dispeller
  free, 11% with none.

Displacement is the second axis and it is mechanical rather than fitted: a
FEARED ally runs away from the teammate who would cleanse them, so one dispeller
catches only **21%** of displacing CC against **68%** of stationary CC. A second
dispeller covers the gap (57%).

`dispel_expectation(free_dispellers, displaces_target)` now returns a measured
(probability, latency) pair per cell, and `TEffInputs::free_dispellers` is an
`Option<u32>` so that **"undispellable" (`None`) cannot be confused with
"dispellable but unanswered" (`Some(0)`)** — collapsing those is what once gave
every stun a discount it could never earn.

| slice | before | after |
|---|---|---|
| **break-on-any (threshold 0)** | −39% skill | **−7%** (abs 2.59s → 2.01s) |
| never-breaks | +82% | **+83%** |
| damage budget (Fear) | +16% | +14% |
| **overall** | +40% | **+45%** (1.15s → 1.06s) |

In absolute seconds threshold-0 (2.01s) is now *better predicted than Fear*
(2.19s). The Warlock's canonical cell is unchanged at +9pt (z=+2.43).

### Why better prediction still did not rescue the Mage: the optimizer's curse

With the recalibrated term the priced Mage was re-measured and came in at
**−19pt (z=−4.74)** — the eighth failed attempt, and no better than when the
predictor was far worse. Scoring it on DENIAL rather than win rate, which is the
instrument this work should use, says the same thing more clearly (20 matches,
`Mage+Priest` vs `Warlock+Priest`):

| policy | casts | lasted | dispelled | **total denial** |
|---|---|---|---|---|
| Identity | 18, all on the Priest | 3.29s of 7.78s | 67% | **59.2s** |
| Priced | 19 (16 Priest, 3 Warlock) | 1.26s / 2.11s of 10.00s | **94%** | **26.5s** |

The priced Mage casts slightly MORE Polymorphs and gets **less than half the
denial**. Its sheeps land at full 10s duration — no diminishing returns, so it is
spacing them out — and are then dispelled 94% of the time within 1.26s.

**The mechanism is selection bias, not calibration.** A decision takes the
`argmax` of an estimate, so it systematically selects the cases where that
estimate is most *over-optimistic*. The error that matters is therefore not the
mean error but the upper tail, and improving average accuracy does nothing for
it. Fear survives this because its `T_eff` is low-variance, so the argmax is
safe; Polymorph does not, because a 10s prediction and a 1.26s outcome sit inside
its ordinary spread.

That reframes the remaining work. The lever is **variance reduction, not better
point estimates** — either shrink an estimate toward the base rate in proportion
to its uncertainty (a per-CC confidence weight, and exactly the kind of thing
that belongs in `cc.ron`), or remove the variance at its source by making Devour
Magic answerable. It also generalises the admission criterion: a class joins the
priced model when its CC's `T_eff` is *low-variance*, not merely unbiased.

### The threshold-0 fix that was measured and REMOVED: pet dispel

Chasing the removals turned up a clean mechanism. This sim *does* have a pet
dispel — the Felhunter's **Devour Magic**, instant, 0 mana, 30yd, 8s cooldown —
and the split is essentially perfect:

| | removed | mean actual |
|---|---|---|
| Felhunter present | **10 / 16** | 2.35s |
| no Felhunter | **0 / 6** | 1.82s |

Because exposure was computed over non-pet healers only, the model's dispel term
was associated with removal **backwards**: 0 of 6 removed when a free healer was
flagged, 10 of 16 when none was.

A `pet_dispel_available` input with its own probability (0.75, from 9 of 12
Polymorphs on a Warlock) was built, unit-tested and measured. **It was removed
again**, because it did not pay for itself:

- predictor accuracy: 1.13s → 1.12s, **+1% skill**. The dispellability gate was
  the whole gain; a second dispeller added nothing.
- live decisions: the Warlock mirror cell went **−4pt → −7pt**. Not significant
  alone, but the wrong direction on both assignments.

The deciding argument is calibration scope: 0.75 was measured on **Polymorph**
and the live term applied it to **Fear**, an unsupported extrapolation — and the
mirror cell (both teams holding a Felhunter) is exactly where that extrapolation
bites. The Felhunter is still counted in the Mage's parked `dispel_exposed` at
the shared 0.18. Re-derive from Fear-specific data if the Mage is ever admitted.

### The verdict on threshold-0: still below the bar, now quantitatively

Threshold-0 is the **only** slice where the model loses to a constant (−33%).
And the ceiling is low even if it were perfectly calibrated: a constant predictor
scores 1.87s absolute error against a 2.21s mean — **85% relative error**, versus
Fear's 50% and the model's 29% overall.

That is the admission criterion answering its own question. Polymorph's duration
in this sim is decided by processes the model does not observe — 45% of them end
in a Devour Magic that is instant and free — and no amount of re-weighting the
observable inputs recovers it. `MAGE_PRICED_POLYMORPH` stays `false`.

**What would actually move it**, in order of expected value:

1. Make Devour Magic *answerable* rather than merely predictable. The value model
   correctly wants to cross-CC the Felhunter first; `pet_ai` gates on the pet's
   own auras, so this is a mechanism question, not a scoring one.
2. A dispel-bait debuff (non-DoT, dispellable) to dilute the roll, which is the
   design direction already recorded under the dispel exchange.
3. Only then, re-derive a pet-dispel term — from Fear data, not Polymorph data.

### Pre-existing finding, surfaced by this work

Measuring the mirror cell for attribution turned up something the canonical cell
hides: **the Warlock's priced Fear is negative when the enemy holds a Felhunter**
(−4pt / −13pt, n=300, both assignments) and negative under a forced
healer kill-call versus `Mage+Priest` (−13pt, z=−5.15). Both predate today's
work — they reproduce exactly with the new terms disabled.

The +9pt headline is a free-kill-target, no-enemy-pet cell. That is a real gain
and it replicates, but it is **narrower than "the priced Warlock is better"**,
and the flip-the-default decision should be made against the full baseline
sweep rather than that one cell.

---

## Migration plan

### Gating: a new axis, not `AiProfile`

This work must **not** ride `AiProfile`. `Legacy` is `#[default]`, is what the
graphical client runs, and is what every balance baseline and movement probe is
calibrated against — so a `TeamPlan`-gated CC model would leave the measured
defect at the top of this document unfixed in the profile that actually ships,
indefinitely, waiting on a build-out that is currently paused. Worse, it would
inherit a dependency on stance transitions that are never produced.

Instead: **per-team policy switches, orthogonal to `AiProfile`**, defaulting to
today's behaviour and flipped once measured — one per *decision site*
(`cc_policy`, `interrupt_policy`), not one per migration step. That gives:

- **Paired A/B on the profile that matters.** Measure the CC change under
  `Legacy`, with no positioning confound, then confirm `TeamPlan` × new-CC has no
  bad interaction as a separate cell.
- **Per-team, for the same reason `AiProfiles` is per-team** — a uniform A/B
  compares two internally consistent worlds and cannot answer "is this better."
- **An eventual default flip that actually fixes the bug**, with canonical
  baselines regenerated at the flip.

Changing default behaviour deliberately is normal here — the Paladin's while-CC
Divine Shield explicitly changed BasicArena behaviour, "intended, like the melee
tempo reset." The byte-identity rule exists to stop `TeamPlan` work from
*incidentally* drifting the baseline, not to freeze `Legacy` forever.

Each step is measured before the next starts.

| # | Step | Behavioural? | Why here |
|---|---|---|---|
| 0 | **`T_eff` harness + CC accounting** | No — read-only | **DONE 2026-08-07/08** — see *Step 0 result* and *Predictor v2*. Dispel term calibrated; break classifier still 60% precision and left open on purpose |
| 1 | **`T_eff` honesty in the existing gates** | Yes | **DONE + MEASURED 2026-08-08** — see *Step 1 result*. +10pt (z=2.56) 2v2 and +19pt (z=3.69) 3v3 when the healer is called; inert under free targeting; `Identity` byte-identical |
| 2 | **`I` — interrupt value** | Yes | **DONE + MEASURED 2026-08-08, NOT RECOMMENDED** — see *Step 2 result*. -5pt (z=-0.81); now on its OWN flag (`interrupt_policy`) so it cannot drag step 1. Needs the lockout priced from target throughput |
| 3 | **`D` — delivered throughput** | Yes | **DONE + MEASURED 2026-08-09** — see *Step 3 result*. +13pt (z=3.29) BasicArena, unchanged on PillaredArena. Team-wide peel replaces self-only |
| 4 | **`C` — cost model, incl. CC-vs-rotation** | Yes | **DONE + CALIBRATED 2026-08-09.** Mana-priced. No setting of the gate helps; kept at the neutral 300 pending `displaced_value` |
| 5 | **`E` — enabling, depth 1** | Yes | **DONE 2026-08-09.** Dispel-denial uplift. +1pt (z=0.41); needs scoring on simultaneous-control seconds, not win rate |
| 6 | **Positional CC as commitment** | Yes | Generalize the Paladin dip to the Rogue |
| — | Playstyle presets | Config | Any time after 4 |
| — | `cc_pull` movement feedback | Yes | **Deferred** — re-measure against the camp probes when taken |

Step 0 is deliberately first. The entire model rests on `T_eff` being honest,
and step 0 answers whether it is without changing behaviour or running a single
win-rate sweep.

Step 2 is the highest-confidence, cheapest win in the list: no team model, no
prediction, and it resolves the investigation's conflict without touching the
healer-lockout gate at all.

---

## Measurement plan

**Per-frame mechanism metrics are primary; win rate is secondary and needs
n≈100 per cell with per-team profiles.** Aggregate control-time cannot
distinguish a chain from three disjointed CCs, which is the entire thing this
design is about, so it is not the headline metric.

Primary:

- **`T_eff` prediction error** — predicted vs actual duration per application.
  Self-validating: a systematically wrong model is visible per application over
  thousands of samples.
- **Overlap seconds** — enemy-team time with ≥2 units simultaneously controlled.
- **Counterplay-free seconds** — healer locked *and* kill target locked at once.
- **CC seconds wasted to friendly damage** — duration lost to our own break
  budget. Today's baseline is in the table at the top: 33% of healer fears, mean
  3.82s each.
- **Casts actually cancelled**, split by what was cancelled (heal / nuke / buff).
- **Damage forgone to CC** — GCDs spent on CC, priced at the rotation action they
  displaced. **Mandatory pairing:** every metric above is denominated in
  CC-units, so on its own the suite cannot see what the CC cost and every step
  will look like an improvement by construction. Report denial delivered and
  damage forgone together or neither.

Secondary: `scripts/headtohead_sweep.py`, per-team `CcPolicy`, both assignments,
with `AiProfile` held at `Legacy` so positioning is not a confound. A uniform
A/B compares two internally consistent worlds and cannot answer "is the new AI
better." The `TeamPlan` × new-CC interaction is a separate cell, run once the
`Legacy` result is settled — not a substitute for it.

**Goodhart warning, stated explicitly because it is the likeliest failure of
this whole document:** control-time is farmable. Fearing a pet, an OOM Mage, or
a kited melee all score seconds and win nothing. `D` must be a *utility* rate or
the AI will farm the metric — and so will the metric's author. Every mechanism
number above is reported alongside the outcome check, never instead of it.

---

## Open questions

1. **Predicting friendly damage rate.** `T_eff`'s break-budget cap needs
   "damage my team will land on this target over the next N seconds." A
   trailing-window measurement of actual recent DPS on that target is the cheap
   version and is probably enough; a predictive model is not obviously worth it.
   Step 0's harness answers this empirically.
2. **Dispel latency.** Needs a model of dispeller availability (GCD, mana, range,
   own CC state, dispel cooldown). Start with binary free / not-free and let the
   harness say whether that is sufficient.
3. **`enabling_discount` starting value.** Unknown. Depth-1 uplift should be
   worth less than own denial; how much less is a measured weight.
4. **Whether rotation pays here at all.** The DR economy makes chain-fearing one
   target look strong on paper — 14s of control for 4.5s of casting (appendix) —
   but Fear is a 1.5s hardcast in matches that resolve at ~37s. Whether the
   rotation-heavy weight set beats the focus-heavy one is exactly the sort of
   question that measured −48pt when assumed. It is a weight, and it gets
   measured per class.

---

## Appendix: verified mechanics

Read from the source on 2026-08-07; several of these differ from what the
balance docs and memory record.

**Diminishing returns** (`components/auras.rs`, `DRTracker`): a component on the
*target*, so DR is **shared across all casters**. Per category, 4 levels with
multipliers 100% / 50% / 25% / immune. The reset timer is **15s from the last
application**; applications while immune do not restart it.

Chain arithmetic for one target with an 8s base CC: 8s + 4s + 2s = **14s of
control for 3 casts (4.5s of casting)**, then immune until ~13s after the last
application. Good value *if* the CC survives — which, per the measurement at the
top, it does not on a target the team is burning (mean 3.82s when broken).

**Fear**: 1.5s cast, 30 mana, no cooldown, 8s, `break_on_damage: 100.0`.

**Death Coil**: instant, 60 mana, **30s** cooldown, 3s horror,
`break_on_damage: -1.0` (never breaks), and a dedicated `Horror` DR bucket so it
does not share DR with Fear. The only CC in the Warlock kit whose `T_eff`
survives our own burn — which is why it is the tool for denying control on a
target we are killing.

**Polymorph**: `break_on_damage_threshold: 0.0` (breaks on any damage).

**`has_friendly_breakable_cc`** (`class_ai/mod.rs:508`) only fires on
`break_on_damage_threshold == 0.0`. Fear's 100-damage budget is therefore
invisible to the friendly-CC guard — a graded resource treated as infinite.

**Interrupts**: Warrior and Rogue interrupt whatever their own kill target is
casting, with **no value judgement** — a Frostbolt as readily as a Flash Heal.
The Shaman scans for any casting enemy in Wind Shear range, preferring healers.
The Felhunter's Spell Lock is instant, **30s** cooldown, **30yd from the pet's
own position**, 3s school lockout, preferring heal casts. (Prior docs record
Spell Lock at 24s; `abilities.ron` says 30.)

**Enemy cast visibility**: `CombatantInfo.casting_ability: Option<AbilityType>`
is already available to class AI. Remaining cast time is not.

**No combo points** in this sim — Kidney Shot is gated on energy and cooldown,
so the "setup time" cost class is thinner here than in WoW.

**Fear and horror produce fleeing movement**, excluded from tangent steering
(`map_geometry`), so displacement is real and physical, not a status flag.
