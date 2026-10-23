# Healer pillar modes: hug the pillar or play in the open (AS-373): design

Status: **PROPOSAL, awaiting sign-off.** Nothing behaviour-changing is built.
The measurements in §2 are on `main` @ `ca41a9f1` (AS-362 merged). §10 lists the
decisions the user must make.

## 1. The model

The user's framing (2026-10-18), restated as the two positions a healer takes
relative to a pillar:

1. **Hug.** The healer stays loosely strapped to a pillar and follows its
   teammate round it. Because it stands close to the pillar, following the
   teammate to the other side is a short walk.
2. **Open.** The healer stands away from the pillar and follows its teammate
   while keeping sight. That brings it nearer the enemy and into the open,
   where crowd control (CC) can reach it. That exposure is the open mode's
   cost, and it is exactly what AS-362's `CcField` measures.

There is a third party to the problem. **It is also the DPS's job not to die
behind a pillar**, so the design has to say what a DPS owes (§6).

What prompted the card: AS-370 left the FREE spot's change of repair exit (A
to B round a pillar) as a one-frame jump, about 6 per 100 3v3 matches. Easing
the spot across the blind ground cost the Warrior (AS-206: the Warrior died
first in 36-45 of 60 matches, against 30 of 60 without it). The user declined
a standalone near-side route in favour of this model.

## 2. Measurements on current main

**The bank.** Two maps, PillaredArena (Nagrand, four octagonal pillars,
footprint disc radius 6yd) and TwinPillars (two cylinders, radius 2.5yd, in a
small octagon). Per map: every 2v2 team with a healer against every 2v2 team
(no double healers), seeds 1-2, plus six one-healer 3v3 teams against each
other in both orders, seeds 1-3. Both teams run the same profile. That is 2,580
matches and 4,360 healer-matches per profile.

**The harness** is `tests/healer_pillar_modes.rs` (ignored; a measurement). It
reads every frame through the read-only observer, and reads the healer's
posture from the decision trace. Inputs, rows and the reducer are under
`docs/design/balance/sweeps/2026-10-19-as373-*`:

```bash
docs/design/balance/sweeps/2026-10-19-as373-gen.py TeamPlan 2 3 > bank.jsonl
HPM_IN=bank.jsonl HPM_OUT=rows.jsonl HPM_TMP=/tmp/hpm HPM_JOBS=4 \
  cargo test --release --test healer_pillar_modes -- --ignored --nocapture
docs/design/balance/sweeps/2026-10-19-as373-reduce.py [--hug 4] rows*.jsonl[.gz]
```

**Definitions.**

- **Skin distance** is the healer's distance from the nearest pillar's
  footprint disc.
- **Hug** means within 4yd of the skin. On Nagrand that is a standing ring of
  radius about 8yd (6 + `MOVER_RADIUS` + `COVER_STANDOFF`) with 2yd of slack.
  The radius is justified in two ways:
  - **By what a side switch costs.** Measured below, a quarter turn round a
    pillar costs about `(r + skin) × π/2` of walking, so the cost grows
    linearly with distance. Within 4yd the switch costs at most about 1.5
    times the skin-tight minimum on Nagrand.
  - **By robustness.** Every conclusion below holds at 2yd and at 6yd as well
    (the reducer's `--hug`).
- **Contact** is the first health lost in the match. Everything below is
  measured after contact unless it says otherwise.
- **Anchor** is the most injured living non-pet teammate. A teammate is
  **dying** below 50% (`urgency_hp_threshold`).
- **Blind** means a pillar blocks the eye-height line between the healer and
  the teammate.

### 2.1 Where healers stand

Share of alive time after contact, by skin distance:

| profile | map | 0-2yd | 2-4 | 4-6 | 6-10 | 10-15 | 15-25 | 25+ |
|---|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2.7% | 2.2% | 2.8% | 6.8% | 16.4% | 30.1% | 39.0% |
| Legacy | TwinPillars | 12.6 | 11.7 | 9.9 | 15.6 | 17.8 | 30.1 | 2.3 |
| TeamPlan | Nagrand | 24.4 | 7.7 | 8.4 | 11.3 | 13.4 | 20.9 | 13.9 |
| TeamPlan | TwinPillars | 22.9 | 17.9 | 13.6 | 22.1 | 9.7 | 11.8 | 1.9 |

Share of post-contact time spent hugging, by posture (Nagrand):

| profile | FREE | PRESSURED |
|---|---|---|
| Legacy | 1.8% of all time (3% of FREE time) | 2.7% (7% of PRESSURED) |
| TeamPlan | 12.2% (27% of FREE time) | 18.6% (38% of PRESSURED) |

By healer class, the share of post-contact time spent hugging on Nagrand:
`Legacy` 4.4-5.0% for every class; `TeamPlan` Priest 26.5%, Paladin 42.1%,
Shaman 33.4%, Druid 24.4%.

**`Legacy` plays the open almost exclusively** on Nagrand (5% hug). **`TeamPlan`
already hugs a third of the time**: the PRESSURED `OccupyCover` solve takes
cover on a pillar's skin, and the opening camp (`Stance::Hold`) stands at 2-4yd
before contact. TwinPillars is small enough that everyone stands near a pillar
much of the time, so its open/hug split is less a choice.

### 2.2 What each mode costs and buys today

Shares of the time spent in each mode, after contact:

| profile | map | fmt | mode | share of time | hard CC'd | in CC reach | blind to anchor | blind to a dying teammate (of dying time) |
|---|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | hug | 4.8% | 4.1% | 41.9% | 7.3% | 11.3% |
| Legacy | Nagrand | 2v2 | open | 95.2% | 6.5% | 41.5% | 0.8% | 0.9% |
| Legacy | Nagrand | 3v3 | hug | 4.6% | 5.5% | 53.3% | 8.5% | 9.1% |
| Legacy | Nagrand | 3v3 | open | 95.4% | 12.6% | 55.3% | 1.0% | 2.4% |
| TeamPlan | Nagrand | 2v2 | hug | 32.8% | 3.0% | 22.9% | **18.6%** | **21.7%** |
| TeamPlan | Nagrand | 2v2 | open | 67.2% | 4.0% | 27.1% | 3.7% | 1.4% |
| TeamPlan | Nagrand | 3v3 | hug | 20.6% | 2.9% | 32.9% | **16.8%** | **32.2%** |
| TeamPlan | Nagrand | 3v3 | open | 79.4% | 5.5% | 30.3% | 2.9% | 3.1% |
| TeamPlan | TwinPillars | 2v2 | hug | 40.8% | 3.2% | 24.8% | 12.7% | 16.1% |
| TeamPlan | TwinPillars | 2v2 | open | 59.2% | 4.4% | 32.3% | 6.5% | 9.1% |

"In CC reach" counts range and sight only, with readiness ignored. The `Legacy`
TwinPillars rows read like `TeamPlan`'s, and are in the reducer output.

The same split by posture, `TeamPlan` Nagrand, both formats:

| posture | mode | share of time | hard CC'd | blind to anchor | blind to a dying teammate |
|---|---|---|---|---|---|
| FREE | hug | 12.2% | 3.4% | 16.7% | 25.1% |
| FREE | open | 33.1% | 2.3% | 2.4% | 1.1% |
| PRESSURED | hug | 18.6% | **2.8%** | 17.8% | 22.2% |
| PRESSURED | open | 29.8% | **5.9%** | 5.0% | 2.6% |

**This is the central finding: today's hug is a hide, not a follow.** A
hugging `TeamPlan` healer is blind to its anchor 4-7 times as often as an open
one, and to a dying teammate 8-23 times as often. What hugging buys is CC
protection under pressure: in PRESSURED it halves the time spent hard-CC'd
(2.8% against 5.9%). In FREE it buys nothing (3.4% against 2.3%).

Against enemy 2v2 comps with no caster CC (no Mage, Warlock or Druid), hugging
buys almost nothing even under `TeamPlan`: hard-CC'd 3.2% in hug against 3.9%
in the open, and in CC reach 15.7% against 17.7%. Meanwhile it is blind to the
anchor 25.5% of the time against 3.2%. A pillar is a defence against a
sight-needing cast, and against nothing else.

### 2.3 Hard-CC landings by mode

Onsets per minute alive in each mode: `TeamPlan` Nagrand hug 0.29 against open
0.32; `Legacy` Nagrand 0.53 against 0.63. Per landing the modes are close.
§2.2's PRESSURED split is where the difference shows.

### 2.4 What a side switch costs: quarter turns round one pillar

Each time the healer's bearing about one pillar turned through 90 degrees
while it stayed within 15yd of that pillar's skin:

| profile | map | mean skin distance | quarter turns per healer-match | median path | median time | path ÷ (r + skin)·π/2 |
|---|---|---|---|---|---|---|
| TeamPlan | Nagrand | 0-2yd | 2.54 | 10.1yd | 2.0s | 1.03 |
| TeamPlan | Nagrand | 2-4 | 0.66 | 22.1 | 10.3 | 1.53 |
| TeamPlan | Nagrand | 4-6 | 0.68 | 21.9 | 6.3 | 1.19 |
| TeamPlan | Nagrand | 6-10 | 0.44 | 23.9 | 7.4 | 1.16 |
| Legacy | Nagrand | 0-2 | 0.06 | 11.3 | 2.4 | 1.04 |
| Legacy | Nagrand | 6-10 | 0.42 | 25.6 | 5.0 | 1.16 |
| TeamPlan | TwinPillars | 0-2 | 1.14 | 6.1 | 1.9 | 1.10 |
| TeamPlan | TwinPillars | 6-10 | 0.72 | 19.5 | 6.9 | 1.24 |

**The user's premise holds as geometry.** A quarter turn at the skin is 10yd
and 2s on Nagrand. At 6-10yd out it is 24yd and 5-7s. Paths run 1.0-1.5 times
the arc, so a switch costs radius × angle almost exactly.

### 2.5 Blind episodes, and who broke sight

An episode is an unbroken spell in which a pillar hides a living teammate from
its living healer. Attribution compares positions 0.5s apart:

- **healer:** the healer's own step broke sight;
- **mate:** the teammate's step broke it;
- **both:** either step alone would have broken it;
- **joint:** neither step alone breaks it.

| profile | map | fmt | episodes ≥1s per healer-match | ≥3s | median length | broke: healer / mate / both / joint |
|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | 0.12 | 0.08 | 3.8s | 24% / 45% / 0% / 31% |
| Legacy | Nagrand | 3v3 | 0.15 | 0.11 | 4.5 | 26 / 41 / 0 / 33 |
| TeamPlan | Nagrand | 2v2 | **1.08** | 0.67 | 3.7 | 13 / **81** / 1 / 6 |
| TeamPlan | Nagrand | 3v3 | 0.89 | 0.52 | 3.3 | 19 / 61 / 1 / 19 |
| Legacy | TwinPillars | 2v2 | 0.93 | 0.35 | 2.3 | 26 / 38 / 3 / 33 |
| TeamPlan | TwinPillars | 2v2 | 1.59 | 0.62 | 2.4 | 25 / 60 / 2 / 13 |

Episodes ending in regained sight with the healer free to act, by its mode
when sight was lost (`TeamPlan` Nagrand):

| mode | episodes | median length | median healer walk | median teammate walk |
|---|---|---|---|---|
| hug | 1,093 | 3.0s | 10.2yd | 8.8yd |
| open | 961 | 4.0 | 13.4 | 16.3 |

So under `TeamPlan` on Nagrand, sight is lost about once per healer-match, and
in four cases out of five **the teammate walked out of sight** of a healer
hugging the pillar: the DPS rounds the pillar, and the healer stays. Once that
happens, the hugging healer does regain sight sooner and on a shorter walk than
an open one (3.0s and 10yd, against 4.0s and 13yd). The short switch is real;
the healer simply makes it too late.

### 2.6 DPS deaths at a pillar

**Definition.** A **pillar death** is a teammate death, with its healer alive,
in which:

- the teammate had been out of its healer's sight, a pillar between them, for
  at least the last 2s before it died; and
- the healer was not hard-CC'd for most of the last 5s.

**sight-only** adds a third condition: the teammate was within heal range of
the healer for most of the last 5s, so sight, not distance, was what failed.

| profile | map | fmt | deaths | pillar deaths | sight-only | per 100 healer-sides | final sight broken by: healer / mate / both / joint | healer hugging at death |
|---|---|---|---|---|---|---|---|---|
| Legacy | Nagrand | 2v2 | 886 | 3 (0.3%) | 0 | 0.1 | — | 0% |
| Legacy | Nagrand | 3v3 | 180 | 1 (0.6%) | 1 | 0.6 | — | 0% |
| TeamPlan | Nagrand | 2v2 | 858 | 18 (2.1%) | 17 | 0.9 | 39% / 33% / 0% / 28% | 78% |
| TeamPlan | Nagrand | 3v3 | 191 | 8 (4.2%) | 8 | 4.4 | 38 / 25 / 13 / 25 | 88% |
| Legacy | TwinPillars | 2v2 | 869 | 34 (3.9%) | 31 | 1.7 | 41 / 38 / 3 / 18 | 50% |
| TeamPlan | TwinPillars | 2v2 | 885 | 57 (6.4%) | 54 | 2.9 | 56 / 28 / 0 / 16 | 63% |
| TeamPlan | TwinPillars | 3v3 | 188 | 10 (5.3%) | 9 | 5.6 | 40 / 40 / 0 / 20 | 50% |

By the dead teammate's class (all four arms): Warrior 5.8% of its deaths,
Rogue 4.2%, Warlock 2.7%, Mage 2.3%, Hunter 2.2%. The melee that chase round
pillars die there most.

A looser reading counts deaths after which the teammate was hidden for at
least half of the last 5s (healer not CC'd). That is 4.9% of `TeamPlan`
Nagrand deaths against 1.3% of `Legacy`'s, and 15.6% on TwinPillars against
9.3%.

**Reading it.** Pillar deaths are a small share of deaths, and on Nagrand the
counts are small (18 and 8). Two things are nonetheless consistent across
every arm:

- **`TeamPlan`'s hug makes them.** It has about 7 times `Legacy`'s rate on Nagrand,
  and 78-88% of those deaths come with the healer hugging.
- **The fault is shared.** Healer and teammate break the final sight about
  equally often. The DPS half is real (§6).

### 2.7 What the measurements say

1. Hugging is worth CC protection only under pressure and only against
   sight-needing CC. Elsewhere it costs sight and buys nothing.
2. Today's `TeamPlan` hug hides behind the pillar instead of following round
   it. A mode that hugs *and* keeps the anchor in sight is the change the data
   asks for. A mode that merely hugs more would make things worse.
3. Open play under `TeamPlan` (with AS-206, AS-370 and AS-362) is already good
   at sight: blind 2-4% of the time, and 1-3% of a teammate's dying time.
4. The teammate breaks sight more often than the healer does (60-81% of
   `TeamPlan` episodes). A healer-only design can shorten the gap, but cannot
   remove it.

## 3. What picks the mode, and what keeps it from strobing (Q1)

A per-healer `PillarMode` on `HealerPosture`: `Open`, or
`Hug { pillar, angle }`. It is re-evaluated each posture tick, and reads only
what the healer perceives.

**Hug is chosen when all three hold:**

1. **The fight is at a pillar.** The anchor teammate stands within
   `HUG_ENTER` (12yd) of some pillar's skin. That pillar becomes the
   candidate.
2. **The pillar defends against something.** The healer's `CcField` (AS-362)
   holds at least one threat with `needs_sight` (a Fear, Polymorph, Cyclone,
   Hammer of Justice or Death Coil the healer is not immune or DR-immune to).
   Readiness uses a wider horizon than the positioning horizon (proposed 5s,
   `cc_mode_horizon`), because the mode is a stance, not a step.
   - With only area or chaser CC (Scream, Nova, the melee stuns) the pillar
     buys nothing (§2.2), so the healer plays open.
   - This is AS-362's threat set read for a second purpose, not a second
     threat set.
3. **The pillar's ring has ground that sees the anchor and is in heal range.**
   This is the feasible arc of §4. An empty arc means hugging that pillar
   cannot keep the teammate, so the healer stays open.

**Signals considered and rejected as keys:** the healer's own distance to a
pillar is the mode's *output*, not its key. "Whether the enemy has casters" is
subsumed by item 2, which is stricter: a caster whose CC is on DR or cooldown
for longer than the horizon does not count.

**Anti-strobe:**

- **Schmitt pairs on every continuous key.** Enter hug at 12yd from the skin;
  leave only beyond `HUG_LEAVE` (20yd) for a commit window. Item 2 must be
  false for `HUG_RELEASE` (3s) before it releases hug.
- **A minimum dwell** in each mode, `MODE_DWELL` (2s, as `COVER_HOLD_SECS`).
  Only the overrides below cut it short.
- **A sticky pillar.** The healer changes pillar only when the anchor stands
  `PILLAR_SWITCH` (8yd) nearer another pillar's skin.
- **Overrides keep their rank.** The dispel walk, the medic chase, ESCAPE, DIP,
  TakeCover and the Druid's Travel Form run as today and suspend the mode;
  they do not reset it.
- **Traced.** A `movement_decision` with trigger `PillarModeEnter` or
  `PillarModeLeave` names the pillar and the key that changed, so a strobe
  shows as a count.

## 4. What hug does (Q2)

**The ring.** A circle round the hugged pillar's centre at
`ρ = r_disc + MOVER_RADIUS + COVER_STANDOFF` (8yd on Nagrand, 4.5yd on
TwinPillars). "Loosely strapped" means a band `[ρ, ρ + HUG_SLACK]`, with
`HUG_SLACK` proposed at 3yd. The goal is a ring angle θ, not a free point.

**The feasible arc F.** The ring angles in sight of the anchor and within
`heal_range − HEAL_REACH_MARGIN` of it.

- For one teammate, F is **one connected arc**. A circle round the pillar
  meets the anchor's shadow in one arc, and the rest of the circle sees the
  anchor. This connectedness is what lets §8 remove the A→B jump.
- In a 3v3 the arc that sees both teammates is preferred while it is
  non-empty. Each teammate beyond the first is worth `TEAMMATE_YARDS`, as in
  AS-206. Otherwise F is the anchor's arc.

**The target angle θ\*** is chosen within F by `OccupyCover`'s own order,
restricted to the ring:

1. sight of the anchor, with a **sight margin**: θ\* stays at least
   `HUG_SIGHT_MARGIN` (proposed 3yd of arc) inside F's edge on the side the
   anchor is moving toward. The healer sits where the teammate is going, not
   where it was. This is what answers §2.5's "the teammate walked out of
   sight": the healer pre-empts the switch instead of reacting to it;
2. out of `CcField` reach;
3. occluded from enemy casters (the hide);
4. nearest the current θ (inertia).

**The motion.** θ turns toward θ\* at most `ω = v / ρ` radians a second,
AS-370's rule moved onto the ring: the goal circles no faster than the healer
can walk it. It turns only through F. If the shorter way round crosses the
anchor's shadow, it goes the other way, which is the near side and sighted by
construction. That near-side walk is the one the declined standalone card
wanted, here as a single ring angle rather than waypoints. Executor tangent
steering already rounds the skin cleanly. A walk gone stale is re-issued
(`walk_is_stale`, AS-370).

**Composition with what exists:**

| piece | in hug |
|---|---|
| FREE formation point (raw, AS-370 heading, AS-206 sight repair, AS-362 reach repair) | Replaced by the ring point while hugging. The sight repair has nothing to repair (F is sighted); the reach repair's job is θ\*'s rank 2. |
| AS-370 `turn_limited` | Becomes the ring-angle limit (above). |
| AS-206 kept exit | Not consulted in hug; forgotten on entering hug, as on leaving FREE. |
| `TeamPlan` PRESSURED `OccupyCover` solve | Runs as today, with three additions while the mode is hug: the ring-arc points of F (sampled, anchored to the pillar, so not a lattice round the mover); a soft leash to the ring band; and the sight margin as a scored term between `C_SIGHT` and `C_CC_REACH`. Depth samples (`SHADOW_DEPTHS`) stay: the solve's own history shows that sampling only the ring was worse against a caster using the same pillar (32.3yd from it, 7.2s hard CC). |
| Medic chase, dispel walk, TakeCover, ESCAPE, DIP | Outrank hug, unchanged. |
| Paladin | Not reached in v1 (§9, D5). Its FREE is melee pursuit, and AS-362 reaches it only in PRESSURED. |
| Opening camp (`Stance::Hold`) | Unchanged. It is already a pre-contact hug. The mode takes over at the camp's release. |

## 5. What open does (Q3)

**Open is today's FREE (`TeamPlan`: AS-370's heading, AS-206's sight repair,
AS-362's reach repair), with one change.** The sight constraint moves from the
POINT to the HEADING.

The raw point stands `formation_offset` out from the engaged-ally centroid
along a heading that AS-370 already turns continuously. The change:

- Headings whose point would stand in a near teammate's shadow are forbidden,
  with the same shadow wedge and near-obstacle limit AS-206 uses.
- The turn-limited heading may not enter the forbidden interval. A target
  inside it holds the heading at the boundary on the side it is already on,
  which is AS-206's kept exit, now in sight by construction with no part-way
  fade.
- A target that leaves the interval on the far side is reached by turning
  **the other way round the centroid**: through the open, past the enemy side
  of the teammate, in sight at every step, at the walking rate.

That walk is the open mode's answer as the user stated it: follow the DPS in
sight, nearer the enemy. Its cost is the exposure, which AS-362's reach repair,
still running after it, keeps out of ready discs where it can. That repair
never trades sight (sight no worse than the sight-repaired point is part of its
feasibility).

What is not changed: the reach repair, the stale-walk re-issue, the medic
chase's rank, and `Legacy`.

## 6. The DPS's share (Q5)

**What the data puts on the DPS.** The teammate breaks sight in 60-81% of
`TeamPlan` blind episodes (§2.5) and in about a third of pillar deaths
(§2.6). Warriors and Rogues die at pillars most.

**What a DPS owes: while it is in danger, its own movement does not break its
healer's sight.** "In danger" means `ally_in_danger` names it, or it is below
`urgency_hp_threshold`. That is the healer's anchor rule seen from the other
side, gated the same way:

- **Kiters (Mage, Hunter; scorer).** The `TeamPlan` kiter already has a range
  `healer_leash`. Add a sight mask: while the kiter is in danger, candidate
  steps that break sight of its own healer are masked, unless every step does.
  That is the existing fallback ladder's shape.
- **Melee pursuit (Warrior, Rogue, pets excluded).** While in danger, a pursuit
  that would round a pillar out of its healer's sight takes the tangent side
  that keeps sight. `steer_toward_goal` already scores sides; the change adds a
  sight preference. If neither side keeps sight, it holds at the pillar's edge
  on its healer's side for at most a commit window.
- **Not owed:** fighting where the healer can follow is the healer's job (§4);
  the DPS owes only not walking away from it while it is the one dying.

**Recommendation: a sibling card, sequenced after this one.** The two halves
touch different machines (the DPS scorer and melee pursuit, against the healer
postures and the solve). They also have their own probes (`movement_probes`
for kiters) and their own measurement: the same bank's "broke by mate" share
and pillar deaths, measured head-to-head with the DPS rule withheld per team.
Measured after this card, the DPS rule is judged against the healer the user
wants rather than against today's hide.

## 7. Profiles (Q4)

**Recommendation: `TeamPlan` only. `Legacy` stays byte-identical.**

- The AI-profile rule: every new AI behaviour lands gated.
- What hug reads is `TeamPlan`'s. `CcField` is built only under `TeamPlan`
  (AS-362's ruling), and PRESSURED's composition is with the `OccupyCover`
  solve.
- The problem is `TeamPlan`'s. `Legacy` hugs 5% of the time on Nagrand, its
  pillar deaths are 0.1-0.6 per 100 healer-sides, and its open play is already
  sighted (blind to the anchor under 1%). The hide-not-follow defect and the
  A→B jump are both `TeamPlan`'s.
- Lifting `Legacy` would also require AS-362's `Legacy` arm, which the user
  declined on 2026-10-18.

## 8. Does it resolve AS-370's A→B jump? (Q7)

**Yes, in both modes, by construction rather than by smoothing.**

- **Hug.** No exit choice exists. The goal is a ring angle inside one
  connected sighted arc, turned at a bounded rate. A switch of side is a walk
  round the near side, in sight throughout.
- **Open.** The exit pair is replaced by the forbidden-heading interval. The
  heading holds on its side. It crosses to the other only by turning round
  the sighted side of the centroid, at the walking rate, never through the
  shadow.
- **What is left** is a change of feasibility: a forbidden interval that grows
  over the heading, or a 3v3 whose allowed arcs split so that the healer's arc
  vanishes. These are the AS-370 "forced" cases. The interval's edge moves
  with the geometry, so the heading is pushed rather than flipped. When an
  allowed arc truly vanishes, the fallback is anchor-only sight (the other
  teammate's shadow stops being forbidden), so the remaining step is limited
  to a 3v3 losing sight of its anchor.
- **The probe** is AS-370's own: `measure_formation_goal_jumps` changes of exit
  over 5yd go to zero, and AS-370's three chosen-exit cases (3v3, seeds 20, 36
  and 49) are staged so the goal never steps more than a re-commit (4yd).

## 9. Measurement plan for the build (Q6)

**Tests, written before tuning:**

- **Scenes, each paired with its control (one condition removed):**
  - a pillar near the anchor and a Warlock with Fear ready gives hug (control:
    the Warlock silenced gives open);
  - the same with only a Rogue gives open;
  - an empty feasible arc gives open;
  - the dwell and the Schmitt edges hold the mode across one tick at each
    threshold.
- **Staged matches (no pinned seeds):**
  - **The follow.** A Warrior chases its kill target round a Nagrand pillar
    while a Warlock stands off with Fear ready. The hugging healer never loses
    sight for more than a GCD, and walks the near side. The pair, with the
    mode withheld, reproduces today's blind spell.
  - **The open traverse.** The healer's target heading crosses a teammate's
    shadow, and the goal goes round the sighted side and never into the
    wedge. The pair, with the AS-206 exits, makes the step.
  - **The three AS-370 chosen-exit situations**, staged as above.
- **Mutation-checked:** no sight margin, no only-through-F rule, no dwell, and
  the open heading allowed through the interval. Each must fail a named test.

**Mechanism metrics.** This card's bank and harness, before and after, per map,
`TeamPlan`. Success conditions, stated now:

| metric (§2) | today, Nagrand 2v2 / 3v3 | target |
|---|---|---|
| blind to anchor while hugging | 18.6% / 16.8% | at most the open mode's (3.7% / 2.9%) |
| blind to a dying teammate while hugging | 21.7% / 32.2% | at most 2x open's |
| hard-CC'd under PRESSURED, hug against open | 2.8% / 5.9% | the gap kept (hug at most 3.5%) |
| blind episodes of 1s or more per healer-match | 1.08 / 0.89 | halved |
| pillar deaths per 100 healer-sides | 0.9 / 4.4 | down; read with the counts, not as a rate alone |
| formation-goal changes of exit over 5yd (AS-370 measure, 3v3) | 6 / 100 matches | 0 |
| heading reversals per minute (AS-249, per healer × map cell) | AS-362 §10.2 | no cell up |
| mode changes per minute, and the dwell distribution | new | dwell at least `MODE_DWELL` by construction; report the count |

Plus `camp_sweep` (occlusion and heal line), and AS-362's CC-exposure bank (no
healer × map cell gains hard CC).

**Head-to-head.** A measurement-only per-team switch,
`team{1,2}_pillar_modes: false`, in the `withheld_plays` style. Both teams run
`TeamPlan`, one with the modes and one without, **in both seat assignments**,
on Nagrand and TwinPillars, over every 2v2 healer cell plus the six 3v3 teams.
Scored from the modes side with paired McNemar, per the sweep-tiers doc.

- DIRECTIONAL tier.
- Success condition: no significant loss overall or in any healer class, and
  the mechanism targets above met.
- A `TeamPlan`-against-`Legacy` head-to-head before and after, as AS-362 ran,
  gives the comparison against the baseline the milestone review reads.

## 10. Decisions for the user

**D1. The model as specified: Hug as a ring angle that follows the anchor
inside its sighted arc, Open as today's FREE with the sight constraint moved
from the point to the heading.** *Recommended: yes.* The measurement says
today's hug hides rather than follows, so sight of the anchor (with a margin
on the side it moves toward) is the hug's first rule, ahead of cover and CC.

**D2. The mode key.** *Recommended:*

- hug when the anchor fights within 12yd of a pillar **and** the enemy fields
  a sight-needing CC the healer is not immune to, ready within 5s, **and** the
  pillar's ring has sighted ground in heal range;
- otherwise open;
- with Schmitt pairs, a 2s dwell and a sticky pillar.

The alternative is a key on the pillar alone (hug whenever the fight is at a
pillar). Against melee-only comps the data says that buys nothing and costs
sight.

**D3. Profiles.** *Recommended: `TeamPlan` only. `Legacy` stays
byte-identical.* `Legacy`'s open play is already sighted, and its hug and
pillar deaths are rare.

**D4. Scope of the first build.** *Recommended: FREE and the `TeamPlan`
PRESSURED solve together.* Most of today's hug blindness is PRESSURED's (946
of 1,266 hug episodes of 1s or more on Nagrand). A FREE-only build would leave
the bulk of the defect. The alternative is FREE first, then PRESSURED as a
follow-up card if FREE measures positive.

**D5. The Paladin.** *Recommended: not in v1.* Its FREE is melee pursuit by
design, so it would take hug only in PRESSURED. That follows AS-362's ruling,
and can be revisited after the measurement.

**D6. The DPS share.** *Recommended: a sibling card, after this one.* While it
is in danger, a DPS's own step does not break its healer's sight: a sight mask
on the `TeamPlan` kiter's healer leash, and a sight-keeping side choice in
melee pursuit. `TeamPlan` only, measured head-to-head with the rule withheld
per team.

**D7. The open mode's heading constraint also replaces AS-206's exit pair**
(and so AS-370's step). *Recommended: yes.* It is the open half of resolving
the A→B jump. The alternative is to keep the exits and resolve the jump only
when hug is chosen, which leaves it in melee-only comps.
