# Team-Level Positioning AI (`TeamPlan`)

Status: **PARTLY IMPLEMENTED** — steps 2, 3 and half of 4 have landed.
Date: 2026-07-25, revised 2026-07-26 after review, amended 2026-08-04 from
implementation.

Open questions from the first draft are resolved and folded into the body below;
see *Resolved during review* at the end for the list, and *Deferred* for the three
items deliberately left open. Implementation order is in *Migration plan*.

**Read the amendments before building anything.** The design's SHAPE has held up
— a team-level plan, a focal-rooted solve, constraint sets per role — but several
specifics were wrong or incomplete in ways only measurement exposed, and one goal
turned out to be unachievable as stated. The amendments are marked inline:

| Where | What changed |
|---|---|
| *Corrections from implementation* | Three constraint bullets were under-specified; assume the untested three are too. |
| *The framing does not fit a kiter* | Constraint satisfaction is structurally wrong for a kiter, and step 4 needs a decision before it can close. |
| Migration step 5 | Its premise about today's behaviour is wrong; it is focus fire, not a small delta. |
| Migration step 6 | As ordered, it is guaranteed to be under-powered. Decide mid-match target switching first. |
| *How to measure a step* | Uniform-profile A/B cannot answer "is this better". Every measurement before 2026-08-04 made that mistake. |

What has actually landed, with numbers, on `Warrior+Priest` vs `Warlock+Priest`
on Nagrand: occlusion bought per match went from `Legacy`'s **0.0s to 28.1s**,
which is the robust result — a per-frame measure over thousands of samples. The
win-rate effect is small and not resolved at the sample sizes used here (+3pt at
n=36); see the correction below before quoting any percentage. `Legacy` is
byte-identical throughout; everything is gated on `AiProfiles`.

**DEFINITIVE MEASUREMENT 2026-08-06, n=100 per cell** (600 matches via the
parallel batch runner; CSV committed at
`design-docs/balance/2026-08-06-team-solve-headtohead-n100.csv`). Head-to-head,
the named side gets the healer solve + kiter leash, the other side runs Legacy:

| Side given the solve | Baseline | With solve | Effect | z |
|---|---|---|---|---|
| `Warlock+Priest` (vs Warrior+Priest) | 23% | **59%** | **+36pt** | **5.2** |
| `Hunter+Priest` (vs Rogue+Priest) | 21% | **35%** | **+14pt** | **2.2** |
| `Warrior+Priest` (vs Warlock+Priest) | 77% | 87% | +10pt | 1.8 |
| `Rogue+Priest` (vs Hunter+Priest) | 79% | 73% | -6pt | -1.0 |

Three of four sides gain, one decisively: the biggest winner is the side whose
baseline was being crushed (`Warlock+Priest` at 23% — its Priest getting real
cover play against a Warrior train is worth +36 points). The Hunter comp — once
reported as -17pt at n=12 — is a significant GAIN with the leash in. The one
negative (`Rogue+Priest`, -6pt) is within noise even at n=100.

The n=12 and n=36 numbers previously recorded here (+8pt/-17pt/+3pt/+11pt)
were all sample noise around these values and are superseded. The per-frame
mechanism evidence (occlusion 0.0s -> 28.1s, blocked share, blackout length)
stands unchanged, and is what correctly indicated the direction all along.

**That the effect is NOT uniform across comps still holds** — and the n=12
detour through the Hunter comp led to a real bug (the kiter healer leash). The
history below is kept for the method lessons; its magnitudes are superseded by
the table above:

| Comp given the solve | Gain |
|---|---|
| `Warrior+Priest` (vs Warlock, and vs Mage) | +8pt |
| `Warlock+Priest` | +8pt |
| `Rogue+Priest` | +17pt |
| `Mage+Priest` | +0pt — baseline is 12/12, so this comp measures nothing |
| `Hunter+Priest` | -17pt -> **+0pt** after the kiter healer-leash fix, below |

Every comp gains except `Hunter+Priest`, which lost badly until the cause was
found. It was NOT the solve failing to cover a mobile ally, which was the
first hypothesis. **A kiter had no awareness of its own healer at all**:
`formation_pull` is 0 for both kiters and nothing in `dps_postures.rs` ever
referenced `heal_range`, so `flee` (unbounded distance-maximisation) would take a
Hunter clean out of its own Priest's range, leaving it unhealable AND — past its
own shot range — unable to answer.

Traced on `solve_hunter_s8`: the first divergence between the Legacy and TeamPlan
runs is the enemy Rogue landing a Kidney Shot on the Priest at 27.62s, which in
the TeamPlan run never happens because the Priest evaded it. The Rogue redirected
onto the Hunter and got 33 connections instead of 7. **The solve did its job and
the team lost because of it** — the healer stopped soaking pressure its Hunter
could not survive.

Fixed by a `healer_leash` scorer term: zero inside heal range, ramping outside,
so it never opposes `flee` where fleeing is correct — the same shape
`corner_penalty` uses to bound `flee` away from corners. That alone moved the
comp from -17pt to +0pt.

The general lesson is worth more than the fix: **a comp with broken internal
spacing is knife-edge on healer positioning, so it will swing wildly under any
change to it and read as a verdict on that change.** Check the comp's own
fundamentals before concluding a positioning change is at fault.

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

- `OccupyCover` — occluded from enemy casters; within `heal_range` of the anchor
  ally; **and able to SEE that ally** (see below).
- `ScreenPartner` — has LoS to the partner; lacks LoS to the enemy kill target.
- `PressTarget` — in ability range of the kill target; has LoS to it.
- `HoldRange` — outside enemy threat range; retains LoS to the kill target;
  **and within our own ability range of it** (see below).
- `StackAnchor` — same side of the anchor as the rest of the team; occluded from
  enemy casters; within `heal_range` of the healer. The only intent where
  teammates deliberately *converge* instead of distributing.

#### Corrections from implementation (2026-08-04)

**These bullets were under-specified, and each gap was invisible until it was
measured.** Assume the three intents that still have no consumer
(`ScreenPartner`, `PressTarget`, `StackAnchor`) carry the same debt.

- **`OccupyCover` must require SIGHT of the ally.** The bullet listed only
  occlusion and heal range, which is satisfiable from a spot the healer cannot
  heal from — precisely the pathology step 4 exists to remove. The prose further
  down this section already implies it ("LoS to my ally, no LoS to their caster is
  one position query"); the bullet did not.
- **`OccupyCover`'s sight requirement must be CONDITIONED ON CASTABILITY.** A
  sightline is worth nothing during a school lockout, so holding one then buys no
  healing and costs real exposure. Sight is required only while the healer can
  actually land a heal; while it cannot, distance from casters is required
  instead. The two are mutually exclusive by construction, so they never compete
  for weight — which is why an unconditional standoff constraint measured worse on
  every axis and this does not. This is temporal, not a tradeoff, and no static
  weighting expresses it.
- **`HoldRange` needs an OUTER leash.** "Outside enemy threat range" is a floor
  with no ceiling — a half-space, not a ring — and is best satisfied by walking to
  the far wall and never casting again. The floor and ceiling together are the
  tuned `range_band` this intent replaces.

#### The framing does not fit a kiter, and that is structural

**Measured: `HoldRange` on the Mage/Hunter ENGAGE/KITE machine cost roughly 17
percentage points and was reverted.** The reason is not tuning.

A constraint set says "satisfy these conditions". That is genuinely a kiter's job
only in part: the tuned scorer carries `flee` as distance **maximisation** rather
than a threshold, precisely so a chased ranged DPS outruns an un-impaired chaser
at ALL ranges. A ring constraint plus a nearest-satisfying tie-break stops fleeing
the instant it is nominally satisfied, and the chaser closes again. A
distance-maximising tie-break was tried as a fix and measured worse still.

The scorer also carries kite entry/sustain hysteresis, the out-of-mana wand
fallback, and the seek-chase leaky bucket — none of which is a position
constraint.

So step 4 had two possible honest endings, and one had to be chosen:

1. **Ranged DPS stays on the scorer permanently.** The solve becomes a
   healer-and-melee mechanism. `los_seek` and `range_band` are then never retired.
2. **Intents gain an optional continuous objective** alongside their constraints,
   so "satisfy this, and among satisfying positions maximise that" is expressible.
   This is a real extension to the solve, not a tweak.

**DECIDED 2026-08-06: ending (1). Ranged DPS stays on the scorer, and step 4 is
CLOSED.** Three grounds. The measurement: `HoldRange` on the kiter machine cost
~17 points and a distance-maximising tie-break made it worse — the gap is
structural, and closing it via (2) would mean the solve re-acquiring four
mechanisms the scorer already has and has already tuned (distance-maximising
`flee`, the range band, kite hysteresis, the OOM wand fallback) for no measured
gain. The architecture: `argmax_interest` is ALREADY hard-constraints-plus-
continuous-objective, so ending (2) converges the solve onto the scorer's shape
rather than inventing a better one — different roles legitimately want different
machinery. The dip analysis: even a kiter's committed CC plays (a hypothetical
Mage nova-dip) would be postures BESIDE a solve, not constraint sets inside one,
so unification buys nothing there either.

Revisit ONLY if step 8's counter-formations produce a concrete team-level
requirement for ranged DPS that the per-unit scorer cannot express (a pincer
assigning kiters to sides is the plausible candidate). That is a new requirement
with its own measurement, not a reopening of this decision.

Consequences of closing: the retirement clause is amended — `cover_pull` /
`medic_chase` are retired UNDER `TEAMPLAN` (measured; `cover_seek` was deleted
outright as dead in both profiles), while `Legacy` keeps them until `Legacy`
itself is retired, and the kiter's `los_seek` / `range_band` are permanent. The
step-4 deliverable stands as: the healer solve, validated at n=100.

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

## The geometry is a constant, not a parameter

**Nagrand's dimensions are the specification. The AI is the thing under
development.** The 40yd/80yd pillar spacing, the ~120yd bowl and the 10yd starting
rooms come from the reference arena; they are not knobs to be tuned for the
convenience of whatever the AI can currently handle.

This is a correction to an earlier framing in this document, which treated
spacing as adjustable and asked whether it was "viable". That question is
malformed. The layout is a given, and every measurement below asks *what the AI
needs in order to play it*, never *what the map should become*.

Three consequences that follow directly, and that an earlier draft got backwards:

- **A negative result indicts the AI, not the map.** If a pillar-camp opener fails
  to produce cover play, the conclusion is that the planner is not yet good
  enough — not that the pillars should move inward.
- **Coverage is an AI target.** "Only one pillar sees use" and "double-ranged
  mirrors never approach cover" are gaps in the comp→plan table, which is the
  entire lever once geometry is fixed. They are not evidence for reshaping the
  arena.
- **The long opener is a property to model, not a defect to tune away.** A 139yd
  arena means melee-vs-melee spends ~18s closing (observed in a live client run).
  Real teams use that approach to set up; the AI walking it in a straight line is
  the thing that is wrong, not the distance.

The one legitimate reason these numbers could still move is **improving the
fidelity estimate** — they were derived from a reference screenshot, not measured
from the game, so better source data may refine them. That is categorically
different from adjusting them because the AI finds them inconvenient.

## What this does NOT solve

- **Matchups with no camper.** The comp→plan table only camps for melee/healer.
  Double-ranged mirrors have no reason to approach a pillar and will still fight
  in the open. Closing that is further AI work, not a geometry question.

## Migration plan

1. **Do not recalibrate the 16 failing PillaredArena probes yet.** They would be
   tuned against reactive-only behaviour and need redoing. Task 7 waits behind
   this work.

   **RESOLVED 2026-08-06.** Archaeology first: the "16 failing probes" were
   re-pointed at `TwinPillars` when Nagrand landed (`e673490`) and still pass
   there — they are KEPT as regression armor for the reactive machinery, which
   is valid because `Legacy` has been byte-identical since their capture. The
   real debt was that Nagrand had no fixed-seed armor for the behaviours that
   exist only under `TeamPlan`. Added as `movement_probes::nagrand_teamplan`,
   calibrated from the shipped measurement trail: an occlusion FLOOR (measured
   22-24s/seed vs `Legacy`'s ~0; floor 10s), a heal-line CEILING (the inverse
   of the `pillar_self_block` pathology pins; healthy 11-17%, ceiling 35%), the
   kiter-leash bound (aggregate over discriminating seeds — leashed ~18s beyond
   heal range, unleashed ~119s, bound 60s; aggregate because the leash is a
   soft weight and single seeds are noisy), and the flat-field anti-statue
   check on BasicArena (1.25-2.04 u/s under melee pressure vs the ~0.65 statue
   band; floor 1.0). Each carries a non-vacuity guard and points at
   `scan_nagrand_teamplan` for re-pinning.
2. Land `TeamPlan` with `anchor: None` for every comp — a pure no-op — and assert
   byte-identical BasicArena and PillaredArena results. This is the guard rail
   for everything after.
3. Enable the melee/healer pillar-camp opener on obstacle maps only. Measure
   whether combat relocates off centre and whether occlusion appears, with the
   spec geometry intact. **This measures whether the AI can yet use the map** —
   the map is not on trial. "cover_pull fired" is too weak a criterion; use
   occlusion-seconds per match, and track how many distinct pillars see use.
4. Port positioning to the focal-rooted team solve, retiring `cover_pull` /
   `cover_seek` / `medic_chase` as separate mechanisms. The solve must support both
   convergent (`StackAnchor`) and divergent (pincer) team shapes from the start,
   even though counter-formation *selection* is deferred — retrofitting divergence
   later would mean redoing the solve.

   **CLOSED 2026-08-06.** The healer half shipped and is validated at n=100
   head-to-head (+36pt Warlock+Priest z=5.2, +14pt Hunter+Priest z=2.2, +10pt
   Warrior+Priest z=1.8, -6pt noise Rogue+Priest). Ranged DPS was ported,
   measured at ~-17 points, reverted, and the ending DECIDED: ranged DPS stays
   on the scorer permanently — see the decision block in "The framing does not
   fit a kiter" for the grounds and the single narrow reopening condition
   (step 8). Regression armor for the shipped behaviours lives in
   `movement_probes::nagrand_teamplan`.
5. Move kill-target selection onto the plan, held constant.

   **MEASURED AND REFUTED AS SPECIFIED (2026-08-06).** Four variants were built
   and measured head-to-head at n=100 per cell — pick at gates-open vs at first
   contact, priority nearest-to-melee vs healer-first — and EVERY static held
   call was catastrophic for at least one side of a matchup while helping its
   mirror:

   | Side with the call (solve-only baseline) | nearest@gates | nearest@contact | healer-first |
   |---|---|---|---|
   | `Warrior+Priest` (87%) | 91% | 87% | **18%** |
   | `Warlock+Priest` (59%) | 67% | 67% | **0%** |
   | `Hunter+Priest` (35%) | 35% | **100%** | 35% |
   | `Rogue+Priest` (73%) | **24%** | **25%** | 98% |

   Each side wants a DIFFERENT target called: the Rogue team wants the enemy
   healer (98%); the Hunter team wants the burst melee, once visible at contact
   (100%); the Warrior team must not chase the healer (18%); and the Warlock
   team forced onto the healer collapses to 0/100 — its class AI already
   handles the enemy healer separately (the shipped Fear+Spell Lock lockout
   logic), so the call double-books the healer and destroys its damage
   rotation. What per-unit acquisition does well is ADAPT — it converges on the
   killable target as the fight reveals it — and a held call cannot express
   that. **Mid-match switching is therefore a PREREQUISITE for the call, not a
   deferred enhancement**; until it exists, `plan.kill_target` is filled by the
   planner (healer-first at contact, tested) and consumed by NOTHING — the
   step-2 provable-no-op shape. The revert was verified to restore the
   committed pre-call numbers exactly (hunter_LT 73/100).

   A comp-conditional call table would fit these four observations, but with
   two matchups measured and the Warlock case showing class-AI double-handling,
   it would be pure overfit. Do not build it without out-of-sample comps.

   **CORRECTION (pre-dates the above): this is NOT "behaviourally close to
   today's configured priority".** That is true only when `teamN_kill_target` is set in the match
   config, and it usually is not — every sweep and baseline runs without it. The
   actual default in `acquire_targets` is that **each unit independently picks its
   own nearest visible non-pet enemy**. Introducing a team-wide call is therefore
   the introduction of FOCUS FIRE, which is normally a large effect, not a small
   delta. Expect and measure it as such.

   Two further notes for whoever builds it. "Held constant" must mean *constant
   while the target lives* — `update_team_plans` deliberately clears the field on
   replan, because a death is the commonest replan trigger and the dead unit is
   the likeliest stale target. And to isolate the variable, pick the target with a
   deliberately boring rule (the enemy our melee would have chosen anyway) so the
   experiment measures COORDINATION; changing the priority at the same time
   measures coordination and priority together and attributes neither.
6. Add the crude lethality model (2s linear damage extrapolation) and the
   obligation layer on top of it. Peels become possible here — which is the
   prerequisite for `Withdraw(Recover)` being enterable at all.

   **MID-MATCH SWITCHING IS NO LONGER OPTIONAL HERE.** Step 5's measurement
   settled what the earlier warning hedged: a held call is not merely
   under-powered, it is net-harmful for at least one side of every matchup
   tested (-48 to -69pt). The obligation layer therefore CANNOT build on a held
   call at all; switching (with a commitment/hysteresis rule so the team does
   not thrash) must land first, and the call's consumer comes back only then.
7. Add `Withdraw` with both reason codes, its dampening and mana-trajectory entry
   gates, and its exit conditions. Regenerate all balance baselines — this step
   changes behaviour on every map and is the point of no return for the existing
   CSVs.

   **This is a project, not a step.** It carries a new stance with two entry
   gates, depends on step 6's peels, and regenerates every recorded CSV. Give it
   its own plan and its own baseline strategy rather than sequencing it as one
   line item.
8. Enemy-plan inference from observables, and counter-formations on top of it.
   Last because everything before it is playable without reading the opponent, and
   because its value depends on the enemy having plans worth reading.

The ordering principle: each step is measurable on its own, and the two steps with
irreversible balance consequences (7, and to a lesser extent 4) come after the
cheap experiment in step 3 has already told us whether the map geometry survives.

### How to measure a step (added 2026-08-04)

**A uniform-profile A/B cannot answer "is the new AI better", and every
measurement taken before 2026-08-04 made that mistake.** Running the same seeds
with both teams on `Legacy` and then both teams on `TeamPlan` compares two
internally consistent worlds; a win-rate shift there means one comp benefits MORE
from the change than the other. That is a real signal, but it is not the question
usually being asked.

`AiProfiles` is per-team for this reason. Set the sides differently and the two
implementations play each other on one seed. Run BOTH assignments — comps are not
evenly matched, so a single assignment confounds the AI with the comp — and report
each side's GAIN against its own uniform baseline rather than raw head-to-head
counts. **This methodology is a tool: `scripts/headtohead_sweep.py`** (batch
runner, n=100 default, Wilson intervals, z-tests). For per-frame mechanism
metrics — occlusion, blocked share, the WHY behind a win-rate delta — use
`tests/camp_sweep.rs`.

Two traps found the hard way:

- **n=12 IS NOT ENOUGH FOR WIN RATE, and this cost the most.** Twelve paired
  seeds was treated as sufficient throughout this work; it is not. Re-measuring
  at n=36 moved the headline from +8pt to +3pt and the Hunter comp from -17pt to
  +11pt — i.e. both the flagship result and the flagship problem were largely
  sample noise. A binary outcome at a ~80% base rate needs HUNDREDS of matches to
  resolve a few points, which is why `scripts/hunter_2v2_matrix.sh` defaults to
  100 per cell. Pairing does not rescue it: once the AIs diverge the matches are
  chaotic in the seed, so the same seed under two profiles is closer to two
  independent draws than to a matched pair.
  **Prefer per-frame mechanism metrics** — occlusion-seconds, blocked share, heal
  delivered, time-to-death — which aggregate thousands of samples per match
  rather than one bit, and reserve win rate for a final confirmation at real
  scale.
- **Check the baseline is not saturated.** `Mage+Priest vs Warrior+Priest` is
  12/12 under Legacy, so it measures nothing and will report "+0pt" for any
  change whatsoever.
- **Pick a comp that actually contains the subsystem under test.** The pillar-camp
  sweep contains no unit on the ENGAGE/KITE machine at all, so it was structurally
  incapable of seeing the DPS half of the solve.

And one method note: **watching a replay found three defects that no metric
caught** — the original camp failure, the solve snapping between lattice points,
and both teams sharing a profile. A seed now reproduces byte-identically between
the client and headless, so what is watched is exactly what was scored. Use it.

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
