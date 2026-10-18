# AS-295: the lane Freezing Trap lands on the enemy's route round a pillar

Tier: **directional**. Success condition: **recover the TwinPillars cost of
the sight rule (AS-281: -4.5pt on the Hunter's side) without breaking it**,
scored from the Hunter's side; BasicArena byte-identical; PillaredArena read
for direction.

## The change

The lane Freezing Trap (`hunter::fallback_trap`) lands at the midpoint toward
the enemy healer, else the kill target. Since AS-281 a throw whose landing the
Hunter cannot see is held. On TwinPillars that midpoint is behind a central
pillar about half the time the lane trap is considered.

Now, when the lane is hidden, the landing comes off an enemy's route instead
(`class_ai/hunter_lane.rs`):

- **The route.** A visible enemy pursuing its target round a pillar walks the
  route its own movement steers (`map_geometry::steer_toward_goal`, the
  tangent arc), stopping at its preferred range. It is predicted by stepping
  that steering forward every 0.05s, resolved against the obstacles as
  `move_to_target` resolves it. An enemy has a route only when the straight
  line to its target is blocked, it is free to walk (not casting, rooted or
  held), and its heading agrees with the route's first step (cosine 0.866).
- **The landing.** It is the first point on the route that the Hunter can see,
  within throw range, which the enemy reaches no sooner than the trap arms
  there and no later than the spring window after. On a route round a pillar,
  that is where the route leaves the pillar's shadow, or a little further on.
- **The victim.** The springer is predicted along each enemy's route, or
  straight along its heading when it has none. Of the candidate landings, the
  one sprung soonest is thrown, and the decision is made on its springer, as
  every lane throw is.

A seen lane is thrown exactly as before. With no route, the hidden lane stands
and the placement still holds it for sight. The sight rule is untouched: every
throw still asks it at the placement.

Also on this card:

- `fallback_trap` holds for an immune predicted springer itself, traced
  `TargetImmune`, so the decision and the sting's reservation of the victim
  agree with the placement. It was already never thrown, so only the trace
  changes.
- The pressure trap's hold names the gate that refused it. A healer inside
  range whose lead (up to three trigger radii along its heading) carries the
  landing out of range is now `healer trap held: the healer's lead lands out
  of throw range`. AS-281's "out of throw range" for a healer 20yd off was
  this case: a scene unit not casting is extrapolated along its facing.
- The opportunistic off-target drop is deleted. It needed its healer within
  8yd, and any such enemy sends the Hunter into its dead-zone or closing
  block first, so it could never fire. While there is an off-target, the
  rotation still holds the trap for the dip.

## Arms

- **before**: `2368e2b` (main).
- **after**: this branch.
- **instrument**: the branch plus `sweeps/2026-10-17-as295-instrument.patch`,
  which only writes to stderr. `AS295_MODE` selects `ship` (the branch),
  `off` (main's lane), or `unseen` (main's lane with the lane exempt from the
  sight rule: the lane as it was before AS-281). `off` reproduces main's CSV
  byte for byte and `ship` reproduces the branch's, on TwinPillars and
  PillaredArena.

Batch: `gen_sweep.py --full 2 --exclude-double-healer --affects Hunter
--control-cells 8 --n 10 --extra '{"map":"<map>"}'`, 4,240 matches per map,
`Legacy`. Inputs are `sweeps/2026-10-17-as295-<map>.jsonl.gz`. CSVs are
`sweeps/2026-10-17-as295-{before,after}-<map>.csv.gz`, with
`-variant-unseen-TwinPillars` for the pre-AS-281 lane.

## Result

Scored from the Hunter's side with `sweeps/2026-10-12-as281-hunter-side.py`,
over every match with a Hunter on exactly one side (n=3,520 per map). Every
`paired_sweep.py` control is 80/80 identical.

| map | Hunter side, before -> after | flips (+/-) | z |
|---|---|---|---|
| BasicArena | 45.3% -> 45.3% | 0 / 0 | — |
| TwinPillars | 38.5% -> 40.0% (**+1.5pt**) | +110 / -58 | **+4.0** |
| PillaredArena | 40.6% -> 40.6% | 0 / 0 | — |
| TwinPillars, the unseen lane (pre-AS-281) | 38.5% -> 42.0% (+3.6pt) | +260 / -135 | +6.3 |

- **TwinPillars recovers about two fifths of the cost.** Against main, the
  route lane is worth +1.5pt (z=+4.0). The unseen lane, measured on the same
  seeds, is worth +3.6pt over main, so the route lane recovers about 40% of
  what the sight rule took from the lane trap. In `paired_sweep.py`'s slices,
  CLEAN is significant (+1.8pt, z=3.56) and ALL is not resolvable (+0.6pt,
  floor 0.8pt): 1 significant slice of 3.
- **BasicArena is byte-identical.** The two CSVs are the same file. 4,216 of
  4,240 matches ended by elimination, with 1,870 distinct durations. Nothing
  here can be asked without an obstacle.
- **PillaredArena is inert on this sweep.** Three matches moved in duration
  and none flipped. Nagrand's lane is hidden on 1% of the ticks it is
  considered, and the route lane threw 3 traps in 4,240 matches.

This is a direction, not a standing.

## How often a seen point on the route exists, and what the throws catch

Counts from `sweeps/2026-10-17-as295-lane-counts.py`, output in
`sweeps/2026-10-17-as295-lane-counts.txt`. Ticks are Hunter decision ticks
with Freezing Trap off cooldown and the lane trap considered (60 a second);
throws are throws.

- **A seen route point exists on 31% of hidden-lane ticks.** On TwinPillars,
  under main's behaviour, the lane is hidden on 745,940 of 1,491,501
  considered ticks (50%). On 232,022 of those (31.1%), some enemy has a route
  with a seen landing.

TwinPillars, fallback lane throws only:

| arm | path | throws | sprung | on the predicted springer | median secs to spring |
|---|---|---:|---:|---:|---:|
| main | seen lane | 612 | 81.4% | 41.3% | 2.65 |
| after | seen lane | 386 | 84.5% | 25.6% | 2.98 |
| after | **route** | **1,236** | **90.9%** | **83.7%** | 4.78 |
| unseen lane | seen lane | 267 | 87.6% | 40.8% | 3.83 |
| unseen lane | **hidden lane** | **1,861** | **96.5%** | **48.8%** | 4.05 |

- **The route throw catches who it is aimed at.** 83.7% of route throws spring
  on the enemy they were decided on, against 48.8% for the old hidden lane.
  The old one caught something more often (96.5% against 90.9%), but half the
  time it was not the enemy it was decided on.
- **What is left is the stealthed Rogue.** The springers, by class
  (instrument output, not committed):

  | path | Felhunter | Spider | Rogue | Warrior | other |
  |---|---:|---:|---:|---:|---:|
  | route | 636 | 408 | 0 | 70 | 10 |
  | old hidden lane | 597 | 380 | 631 | 181 | 6 |

  The route lane takes the pets and the melee that the hidden lane took. It
  cannot take a Rogue walking the lane in stealth: an unseen enemy has no
  route the Hunter can see. That gap of 631 Rogue catches is most of what the
  route lane does not recover. A trap thrown blind for an unseen enemy is the
  stealth branch's job and still needs a seen landing.
- **Many route throws are the opener.** The median of 4.78s from throw to
  spring is almost constant: enemy pets and melee leave their gates round the
  central pillars on the same paths every match.

## Tests

- `hunter_lane` unit tests cover the pure picks. The steered track rounds the
  pillar without entering it. The landing is the first seen point reached
  after arming. The shadow moves it along the route. It is refused out of
  range, beyond the spring window, or with no seen point.
- `tests/hunter_route_lane_scenes.rs` pairs each rule with its control. A
  hidden lane lands on the route, in sight. A walker heading elsewhere, or a
  rooted one, has no route. A seen lane is thrown as always, even when a
  route exists. The springer is predicted along the route, not the heading.
  An immune walker is held as `TargetImmune`.
- `tests/class_ai_decisions.rs` covers the pressure trap's lead-out-of-range
  note.
