# Team-Level Positioning AI (`TeamPlan`)

Status: **AGREED** — design settled in review; no code landed yet.
Date: 2026-07-25, revised 2026-07-26 after review.

Open questions from the first draft are resolved and folded into the body below;
see *Resolved during review* at the end for the list, and *Deferred* for the three
items deliberately left open. Implementation order is in *Migration plan*.

## Why this exists

The Nagrand Arena rework (four octagonal pillars at `(±40, ±20)` in a ~120yd
bowl) produced a map where the pillars are geometrically correct but never used:
measured occlusion across a whole match is **0.00s**, and the pressured Priest's
`cover_pull` term fires 2 times per match against 14–19 on the old map.

The first diagnosis was that the pillars sit too far from where combat happens.
That is true but it is a *consequence*, not the cause. Combat converges on the
arena centre **because nothing gives either team a reason to be anywhere else** —
both sides walk at each other and meet in the middle. Moving the pillars to the
centre makes cover work (verified: the `u8_healer_cover` probe passes with pillars
at `(±10, ±6)` and fails at `(±18, ±10)` and beyond), but it treats the fight
location as immovable when it is really an output of the AI.

The actual gap is that **all pillar behaviour we have is individual and
reactive**, and real pillar play is neither.

### What exists today

| Mechanism | Shape | Limitation |
|---|---|---|
| `cover_pull` (scorer weight) | Per-frame interest term rewarding a candidate step that is *already* occluded, at `SCORER_LOOKAHEAD` = 2yd | A local gradient. Flat — and therefore inert — whenever cover is more than a step or two away. |
| `cover_seek` (2026-07-25) | Direct `Point` walk to the nearest hiding spot when no local step is occluded | Fires **only when PRESSURED** (an enemy within `danger_radius` = 12yd). Cannot produce an opening position, because at the opening nobody is pressured. |
| `medic_chase` | Direct `Point` walk toward an occluded, low-HP ally | Single-objective: regain sight of an ally. Mutually exclusive with `cover_pull` by construction (`teammate_needs_saving` zeroes the weight, which also disables `cover_seek`). |

All three are *reactions to the current frame's threat picture*. None can express
an intent held across seconds, and none coordinate between teammates.

### What real pillar play requires

Worked examples, and what each needs that we lack:

1. **Melee/healer camps a pillar at the opening vs double ranged.** The team
   takes the pillar *before contact*, forcing the ranged team to close — which
   closes the distance *for* the melee team, more effectively than a mutual
   charge would. Needs: a plan chosen from the **comp matchup**, active from
   `gates_opened`, with no threat nearby.

2. **Healer pinned to the pillar.** Holds a position with line of sight to its
   partner **and** no line of sight to the enemy casters. Needs: a **dual
   constraint** satisfied simultaneously. Our two behaviours each own one half
   and are wired to be mutually exclusive, so the healer is either hiding *or*
   seeking sight of an ally — never positioned so both hold.

3. **Defensive retreat to a pillar to bait, then counter-attack.** Needs: a team
   **tempo state** (defensive / neutral / committed) and a flip trigger, rather
   than a per-unit HP threshold.

4. **Declining to close distance.** A team that wants to be approached stands
   somewhere advantageous and waits. Needs a primitive we have *none* of: every
   current posture approaches, orbits, or flees. **None of them hold ground.**

## Three layers, not two

An early draft of this doc had a single team layer sitting above unit postures.
That is too coarse. **Peeling is the counter-example**: CCing an ally's pursuer is
not a team strategy — the team's strategy might be anything — but it is also not
an individual reaction to one's own threat picture. It is a *duty one unit takes
on because of another unit's state*.

That points at three layers, distinguished by what they are a function of and how
fast they change:

| Layer | Function of | Cadence | Examples |
|---|---|---|---|
| **Strategy** (`TeamPlan.stance`) | comp matchup, team HP, resources, match clock | discrete triggers (seconds) | Hold the pillar, press an advantage, bait, recover |
| **Obligation** (`RoleIntent` + duties) | *another unit's* state | on state change (sub-second) | Peel the kill target's pursuer, screen the healer, trade a defensive cooldown |
| **Execution** (existing postures + positioning solve) | own threat picture, geometry | per frame | Which step to take, which ability to fire |

The middle layer is the one we have no representation for at all. Today a Priest
Psychic Screams because *it* is being attacked, or a Warlock Fears because it has
a rotation slot — never because a teammate is the kill target and needs a window.
CC targeting is per-unit and self-interested.

Crucially, obligations are **instrumental to the layer above**: `Recover` cannot
work without peels, because a melee train on the heal target defeats any amount of
healing. So strategy generates obligations, obligations generate execution intents.
That directionality is what makes three layers rather than three parallel systems.

#### Arbitrating competing duties needs a lethality model

DECIDED: competing duties are arbitrated by **prediction, not priority order**. The
governing example: if the called target dies to one more cast and my partner will
survive the next two seconds, casting wins; otherwise the defensive play wins.

That is the right way for the AI to reason, and it is also the most expensive
answer in this document, because it requires something we have nothing for —
**time-to-die estimates on both sides**: incoming damage rate, heal throughput,
and absorb remaining.

This is best understood as a **fourth subsystem the obligation layer queries**, not
a fourth layer. Layers are about cadence and scope; this is a shared predicate.
Staging:

- **v1 (crude, shippable):** linear extrapolation of damage taken over the last
  ~2s, ignoring cooldowns, absorbs, and pending casts. Wrong in known ways, but
  cheap and testable — and a wrong-but-measurable estimate beats a correct one
  that never lands.
- **later:** account for shields, in-flight casts, and known enemy burst
  cooldowns.

An obligation also needs to be *bounded and revocable* — "peel for my ally" must
expire, or the peeler abandons its own job indefinitely. Likely shape: a duty with
an owner, a beneficiary, a trigger condition, and a deadline.

## Proposal

### `TeamPlan` — one per team, recomputed on a slow cadence

```rust
/// DECIDED: a resource holding a fixed two-element array, indexed `team - 1`.
///
/// Teams are always exactly two, so an array is deterministic by construction —
/// there is no map iteration order to get wrong, which is the failure this
/// codebase keeps paying for. It also matches the established pattern
/// (`ActiveMapGeometry`, `ArenaDampening`, `MovementConfig` are all resources,
/// and no team marker entities exist), and teardown reuses the existing
/// `commands.remove_resource::<T>()` idiom on state exit.
///
/// Components were considered for Bevy change detection on the recompute
/// triggers; that is not actually a component-only feature (`Res<T>` has
/// `is_changed()`), so it did not favour them.
#[derive(Resource)]
struct TeamPlans { plans: [TeamPlan; 2] }

struct TeamPlan {
    /// Where this team wants the fight to happen. `None` = open field.
    anchor: Option<Anchor>,
    /// How it intends to get there / what it does once there.
    stance: Stance,
    /// The team's called kill target. DECIDED: the plan owns this, replacing
    /// per-unit selection — a coordinated target call is the same class of gap as
    /// coordinated positioning. Held CONSTANT for the whole match in v1: mid-match
    /// switching is strategically better but is deferred (see Deferred below).
    kill_target: Option<Entity>,
    /// Per-role position intent, resolved against `anchor`.
    intents: BTreeMap<Entity, RoleIntent>,
}

enum Anchor { Obstacle(usize), Point(Vec2) }   // index into ActiveMapGeometry.volumes
enum Stance {
    Hold,
    Press,
    /// Bait and Recover unified: they share their entire exit path (into `Press` on
    /// conversion or enemy overextension) and differ only in WHY the team retreated.
    /// One stance with a reason code, not two peers.
    Withdraw(WithdrawReason),
}

enum WithdrawReason {
    /// From strength — retreat to draw an overcommit (the old `Bait`).
    Draw,
    /// From weakness — retreat because we must (the old `Recover`).
    Recover,
}
enum RoleIntent { OccupyCover, ScreenPartner, PressTarget, HoldRange, StackAnchor }
```

`BTreeMap` for `intents` (not `HashMap`) for the same determinism reason as
everywhere else in this codebase — iteration order must be stable at a fixed seed.

**Cadence.** Recomputed on discrete triggers, never per-frame: gates open, a
combatant dies, stance flip conditions met, anchor becomes untenable, a plan-
critical unit is CC'd in a way we cannot dispel, or a dispel we were relying on
goes on cooldown. A per-frame plan is just the current scorer with extra steps;
the whole point is an intent that *persists* long enough to shape where the fight
happens.

**Note the fixed kill target caps the obligation layer.** "This enemy dies to one
more cast" cannot be acted on if the killable unit is not the called target. The
arbitration below will therefore look under-powered until mid-match switching
lands — that is expected, not a defect in the arbitration.

### Comp → plan selection

The opening plan is a function of the matchup, evaluated once at `gates_opened`:

| Own comp | Enemy comp | Anchor | Stance |
|---|---|---|---|
| melee + healer | double ranged | nearest pillar | `Hold` — make them come |
| double ranged | melee + healer | open space, max range | `Press` |
| mirror | mirror | nearest pillar | `Press` |
| any | any (no obstacles on map) | `None` | current behaviour |

That last row is the **BasicArena inertness argument**: with no obstacles there is
no `Anchor::Obstacle` to choose, so `anchor` is `None` and every role intent
degrades to today's posture. This must be a *provable* no-op, asserted by a test,
not merely likely — the existing balance baselines depend on it.

The honest caveat: `Stance::Hold` and `HoldRange` are **not** obstacle-dependent,
so if we let them apply on obstacle-free maps they *will* change BasicArena. The
proposal is to gate stance selection on `anchor.is_some()` initially, and treat
open-field tempo as a separate follow-up with its own balance pass.

### Plan validity under hard CC

A CC'd unit cannot serve its intent, so CC is a plan-level event, not just an
execution-level one. The discriminator is **whether we can remove it right now**:

- **Removable** (Polymorph, and anything our dispel covers, with a dispel actually
  off cooldown): the plan does not change. Dispel and carry on — the CC cost the
  enemy a cast and cost us a dispel.
- **Not removable** (a Rogue stun, or a dispellable CC while our dispel is down):
  continuing may be impossible or actively bad, and the plan re-solves without
  that unit.

Two consequences that are easy to miss:

- Plan validity depends on **our own cooldown state**, not just the enemy's action.
  "Just dispel it" is only true while a dispel is available, so *dispel became
  unavailable* is itself a re-evaluation trigger.
- **Diminishing returns change the threat.** A target already DR-immune to
  Incapacitates is not facing the same CC as a fresh one, and `DRTracker` already
  carries that state — the removability test should read it rather than treating
  every stun as equivalent.

### Reading the enemy: inference from observables only

DECIDED: **the code never reads the opposing `TeamPlan`.** Not "reads it with a
delay" — never. A team forms an *estimate* of the enemy's intent from what is
observably true: positions, and who is attacking whom. Making the plan
structurally unreachable is what keeps this honest; a delay constant would be a
discipline problem forever.

What that yields, and it maps onto how a real player reads a game:

- **Formation is immediately obvious.** A team stacked on a pillar at the opening
  is visible from position alone, and can be responded to at once.
- **The kill target is not inferable from position** — but it *is* inferable from
  damage, so it becomes known shortly after they commit. The inference delay we
  wanted therefore falls out of the observable itself rather than being a tuned
  number.
- **Resources and cooldowns are not observable**, so an enemy healer's mana can
  only be estimated from its casting behaviour over time.

#### Counter-formations

Reading an enemy formation is only useful if we can respond in kind, which needs
an intent family the list above lacks: a response *shaped by their shape*. A team
camped on a pillar can be answered by piling onto one side, or by a **pincer** that
approaches from two angles so the pillar cannot cover both.

The pincer matters architecturally because it deliberately **splits** the team —
the exact opposite of `StackAnchor` — which confirms the positioning solve has to
support both convergent and divergent team shapes rather than assuming units
cluster. Choosing between pile-on and pincer is deferred.

### `Stance::Withdraw` — recover and bait

"We are behind; disengage, deny damage, and let the healer restore us without
exposing itself." Two execution modes — and the choice is driven by **our own
composition, not by what the map offers** (an earlier draft of this doc had that
wrong):

- **Pillar stack.** The whole team goes to *the same* pillar —
  `RoleIntent::StackAnchor` — so the healer heals from a position where the
  enemy's casters have no line to it and their melee must commit around the
  obstacle to reach anyone. This is the case where the team *shares* an anchor
  rather than distributing around it, which is why it needs its own intent
  variant. **Correct for melee-containing teams**, for whom retreating into the
  open is essentially never right.
- **Open-field kiting.** Retreat into open space and make the enemy melee spend
  its gap-closers to keep up. **Correct for caster/ranged teams**, and only for
  them — the objective is not distance for its own sake but *draining a finite
  resource*.

That second mode names an objective the AI has no representation for at all:
**enemy cooldowns as a target.** Exhausting a Charge or a gap-closer is a real
strategic gain, distinct from dealing damage and from holding position, and a
caster team may correctly accept damage to force one. Deferred, but it is the
reason open-field kiting is a *plan* and not just fleeing.

`Recover` is the natural mirror of `Press`. The codebase already has the signal:
`pressing_when_ahead(team_hp_advantage(), press_advantage_margin)` fires on a
+0.2 team-HP lead. `Recover` is the same reading on the negative side, so stance
selection over `team_hp_advantage` is a single axis: `Recover | neutral | Press`.

**Peels are load-bearing here, not optional.** A melee train on the heal target
defeats any amount of healing, so `Recover` must generate peel obligations
(middle layer) or it is strictly worse than fighting — the team gives up damage
uptime and gets nothing. `Recover` without peels should not be enterable.

#### Arena dampening puts a clock on it

This is the constraint most likely to make `Recover` a trap. `ArenaDampening`
ramps **all** healing and absorbs linearly to zero from `DAMPENING_START_SECS`
(75s) over `DAMPENING_RAMP_SECS` (120s), so healing is dead at 195s after gates.
Recovering at t=150s restores ~40% of normal, and at t=190s essentially nothing —
while the disengage still costs full damage uptime.

So `Recover` is only rational in a window, and stance selection **must** read
`ArenaDampening::reduction`. A dampening-blind `Recover` would reliably throw
late-game positions: the team disengages, heals for nothing, and loses the
attrition race it was trying to win. There is probably a reduction threshold above
which the stance is simply unavailable.

#### Entry needs a mana *trajectory*, not a snapshot

"The healer has mana" is the wrong test. A healer at zero mana with no path back
to mana cannot convert time into HP, so `Recover` buys nothing while still
conceding damage uptime — it is a pure loss. The entry condition is whether the
healer's mana is *going up fast enough to matter* within the dampening window:
current mana, regen rate, and the cost of the heals it intends to cast. A team
with a dry healer and no regen should fight, not hide.

This generalises: every `Recover` entry test is about a rate, not a level. Team HP
must be recoverable faster than the enemy can re-apply damage, which is why the
peel obligations below are part of the entry condition rather than a nicety.

#### Exiting into a counterattack is where the value is

`Recover` is not a defensive terminal state — it is the **setup half of a
two-part play**. The enemy team chases to maintain pressure, closing the gap on
our terms, and we re-engage healthier than when we disengaged. The HP swing is
only realised at the transition *out*; a `Recover` that never converts has simply
donated tempo.

So the stance graph matters as much as the stance set, and `Recover → Press` is
the payoff edge:

```
        ┌──────────────── converts ────────────────┐
        │                                          ▼
   Recover ──── healed / enemy overextended ───► Press
        ▲                                          │
        └────────── behind & recoverable ──────────┘
              (dampening window still open)
```

Exit triggers, in rough priority:

- **Converted** — team HP restored above the re-engage threshold.
- **Overextension** — the chasing enemy has strung itself out (its healer left
  behind, out of its own cover), which is a *better* trigger than our own HP
  because it is the moment the counter is cheapest.
- **Aborted** — healer out of mana, dampening threshold crossed, or the disengage
  is failing (we are being chased and healing nothing). Must exist; see the open
  question about graceful failure.

DECIDED: **`Bait` and `Recover` are one stance with a reason code**, modelled as
`Stance::Withdraw(WithdrawReason)`. Both retreat in order to counterattack and both
exit into `Press`; they differ only in why. `Draw` retreats *from strength* to
provoke an overcommit, `Recover` *from weakness* because it must. Since the exit
logic is identical, splitting them would duplicate the part that carries the value.

The reason code still matters in two places, which is why it is not simply erased:
entry conditions (a `Draw` needs no dampening or mana check, a `Recover` needs
both), and how much risk the withdrawal will absorb before aborting.

#### Mana longevity caveat (raised in review)

`Recover` presupposes the healer can convert time into HP faster than the enemy
converts it into damage, and that it has the mana to do so. Current pools may not
support that — the Hunter has the smallest *effective* pool of any class (zero
gear mana on mail), and healer pools are tuned around a 75s-to-195s dampening
window rather than around sustained recovery. Two implications:

- `Recover` may be measurably *bad* on current numbers even when implemented
  correctly. Worth measuring before concluding the AI is wrong.
- If we want recovery to be a real strategic option, mana longevity is a
  prerequisite balance change, not a detail. That is a separate decision from
  this layer and should not be smuggled in with it.

### Positioning as constraint satisfaction

The three existing behaviours collapse into one solve. Instead of summing
single-objective interest terms, a unit scores candidate positions against the
constraints its `RoleIntent` implies:

- `OccupyCover` — occluded from enemy casters; within `heal_range` of the anchor ally.
- `ScreenPartner` — has LoS to the partner; lacks LoS to the enemy kill target.
- `PressTarget` — in ability range of the kill target; has LoS to it.
- `HoldRange` — outside enemy threat range; retains LoS to the kill target.
- `StackAnchor` — same side of the anchor as the rest of the team; occluded from
  enemy casters; within `heal_range` of the healer. The only intent where
  teammates deliberately *converge* instead of distributing.

#### The solve is a team problem, rooted at a focal unit

Positioning is not per-unit with a coordination patch on top; it is one team-level
solve. DECIDED: **the stance names the focal unit, and everyone else solves
relative to it.**

| Stance | Focal unit | Others orient to |
|---|---|---|
| `Press` | the called kill target | damage range + LoS on it; CC positions on the off-targets (the existing Paladin dip composes here) |
| `Withdraw` (either reason) | our own healer | stay within `heal_range`, share its cover, screen its approaches |
| `Hold` | the anchor | cover the approaches to it |

This supersedes an earlier proposal to break ordering ties by slot index. A
focal-rooted dependent solve is deterministic *and* meaningful, where slot order
was merely deterministic — and it removes the need for an arbitrary convention in
the one place the units are not independent.

That `cover_pull`, `cover_seek`, and `medic_chase` all fall out as special cases
of a single solve is the main evidence this is the right shape — three mechanisms
that currently have to be kept from fighting each other by hand become three
constraint sets.

Note the mutual-exclusion hack disappears: "LoS to my ally, no LoS to their
caster" is one position query, not two behaviours arbitrated by an HP threshold.

### CC targeting is stance-derived, not static

The layer model's first concrete payoff outside positioning. CC target selection
is currently a fixed per-unit rule, and the Mage's Polymorph is the clearest case:
`try_polymorph` reads `combatant.cc_target` and **hard-rejects** the kill target,
recording in the trace

> `cc_target equals kill target — would break on damage`

That guard is *correct* — Polymorph has `break_on_damage_threshold: 0.0`, so
sheeping the unit your own team is attacking wastes the cast immediately. But
notice what it assumes: **that we are attacking our kill target.** The rule
encodes a stance implicitly, and is right under `Press` and wrong outside it.

Under `Recover` the team is deliberately *not* dealing damage. The break-on-damage
objection dissolves, and polymorphing the enemy's damage dealer — our own kill
target — becomes one of the strongest plays available: it removes a damage source
for the full duration and buys exactly the time recovery needs.

So the correct rule is derived from the layers rather than fixed:

| Layer input | CC target |
|---|---|
| `Press` | enemy healer / off-target — create the outnumbering window (today's behaviour) |
| `Recover` | the enemy applying pressure, kill target included — buy time |
| Obligation: ally is kill target and in danger | that ally's pursuer, regardless of stance |

The break-on-damage guard does not disappear; it becomes conditional on *whether
this team intends to damage that target soon*, which is precisely a stance
question. That reframing — a hard-coded guard turning out to be a stance
assumption in disguise — is the sort of thing to look for elsewhere in the class
AIs while doing this work.

**Accepted suboptimality (decided, so it is not refiled later):** stance can flip
while a Polymorph is already in flight, and we will let the cast land rather than
build cast-cancelling machinery (only interrupts exist today). Note the cost is
slightly worse than a wasted GCD — a self-broken Polymorph still applies
**diminishing returns**, so the next one on that target is shorter. Judged a rare
enough edge case to accept.

Related mechanics that likely hide the same assumption: Fear (breaks on damage,
same shape), Freezing Trap (already off-target by design), and the Warlock's
healer-lockout Fear + Spell Lock pairing, which is a *fixed* offensive plan that
would be wrong in `Recover` for the same reason.

### The missing primitive: hold ground

`MovementGoal` needs a way to express "stay here, this is a good place" that is
distinct from "arrived at a `Point`". Today, arriving at a point and having no
directive are indistinguishable downstream, so a unit that should be *waiting*
looks identical to one that has nothing to do and gets re-tasked by the next
posture evaluation.

## What this does NOT solve

- **Arena scale.** A 139yd arena means melee-vs-melee opens with a ~130yd walk
  (observed in a live client run: gates open, then ~18s of approach). Team plans
  relocate the fight; they do not shorten the opener. Worth revisiting the bowl
  radius independently.
- **Faithfulness vs function.** If team plans make 40/80 spacing work, the
  spec geometry is vindicated. If they do not, the choice between visual fidelity
  and functioning cover is still live.

## Migration plan

1. **Do not recalibrate the 16 failing PillaredArena probes yet.** They would be
   tuned against reactive-only behaviour and need redoing. Task 7 waits behind
   this work.
2. Land `TeamPlan` with `anchor: None` for every comp — a pure no-op — and assert
   byte-identical BasicArena and PillaredArena results. This is the guard rail
   for everything after.
3. Enable the melee/healer pillar-camp opener on obstacle maps only. Measure
   whether combat relocates off centre and whether `cover_pull` starts firing
   with 40/80 spacing intact. **This is the experiment that decides whether the
   map is fine.**
4. Port positioning to the focal-rooted team solve, retiring `cover_pull` /
   `cover_seek` / `medic_chase` as separate mechanisms. The solve must support both
   convergent (`StackAnchor`) and divergent (pincer) team shapes from the start,
   even though counter-formation *selection* is deferred — retrofitting divergence
   later would mean redoing the solve.
5. Move kill-target selection onto the plan, held constant. Behaviourally close to
   today's configured priority, so this is a small step deliberately taken before
   anything depends on a *called* target.
6. Add the crude lethality model (2s linear damage extrapolation) and the
   obligation layer on top of it. Peels become possible here — which is the
   prerequisite for `Withdraw(Recover)` being enterable at all.
7. Add `Withdraw` with both reason codes, its dampening and mana-trajectory entry
   gates, and its exit conditions. Regenerate all balance baselines — this step
   changes behaviour on every map and is the point of no return for the existing
   CSVs.
8. Enemy-plan inference from observables, and counter-formations on top of it.
   Last because everything before it is playable without reading the opponent, and
   because its value depends on the enemy having plans worth reading.

The ordering principle: each step is measurable on its own, and the two steps with
irreversible balance consequences (7, and to a lesser extent 4) come after the
cheap experiment in step 3 has already told us whether the map geometry survives.

## Deferred

Everything else in this document is decided. These are genuinely open, and each is
scoped so it can land after the core layer without reshaping it:

- **Mid-match kill-target switching.** The plan owns the target and holds it
  constant in v1. Switching is strategically better and is what unlocks the full
  value of prediction-based arbitration (see the cap noted above), but it needs a
  commitment/hysteresis rule so the team does not thrash between targets.
- **Representing enemy cooldowns as an objective.** Open-field kiting to exhaust a
  melee's gap-closers is a real plan, but "drain that cooldown" is neither damage
  nor position and has no representation. Also unclear how much of an enemy's
  cooldown state a team may legitimately be assumed to track, given the
  observables-only rule.
- **Choosing between counter-formations.** Pile-on versus pincer against a camped
  team. The pincer requires the positioning solve to support a deliberately split
  team, so the *capability* is in scope now even though the *selection rule* is not.

## Resolved during review (2026-07-25/26)

Recorded so the reasoning is not re-litigated: plan storage (resource with a fixed
two-element array), enemy-plan observability (inference from observables only,
enforced by making the plan unreachable), hard-CC handling (removability decides
whether the plan re-solves), plan ownership of the kill target, duty arbitration by
prediction, the focal-rooted team positioning solve, `Recover` abort, `Bait`/`Recover`
unification, `Recover` exit conditions, and letting an in-flight Polymorph land
across a stance flip.
