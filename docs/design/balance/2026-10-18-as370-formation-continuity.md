# AS-370: the FREE formation point turns instead of flipping

**Tier:** DIRECTIONAL (two paired sweeps), plus the formation-goal jump
measure AS-206 introduced. **Success condition, stated up front:** the
formation goal's frame-to-frame jumps fall in every cell of AS-206's five-comp
table and rise in none; no balance movement worth the name; `Legacy`
byte-identical. Measured 2026-10-18, `main` at `5e13a22a` against this branch.

## The shape

| Question | Answer | Why |
|---|---|---|
| What jumps? | The raw point's heading. The point stands `formation_offset` (8yd) off the engaged-ally centroid along "behind the line", away from the enemy nearest the centroid. That direction turns half way round whenever the enemy crosses the centroid — a melee at its teammate's feet does so constantly — and the point jumped to the far side with it, up to 16yd in a frame. | The measure below attributes each jump to its cause: on `main` the crossing is the largest single cause in the 3v3, and under `Legacy` it is three quarters of all jumps. |
| The fix | A turn-rate limit: the heading turns from the last one toward the new direction by at most `rate` radians a second of sim time (`formation::turn_limited`). A heading within reach takes the new direction exactly. | Of the card's three options it is the one that bounds the point's speed whatever the input. A blend over several enemies is still discontinuous where the blend cancels, and hysteresis or a commit window still jumps, only later. |
| What rate? | The healer's walking speed over `formation_offset` (5/8 rad/s for a 5yd/s healer): the point circles the centroid no faster than the healer can follow it round. A half turn takes about 5s. | Derived from the data, so no RON knob. The centroid's own movement passes through unchanged. |
| Per what? | Per second of sim time between evaluations, not per evaluation. | A healer that was casting (not evaluated) finds the point turned by what the cast lasted, which it could not have walked anyway. |
| A flickering direction? | Turns the heading by at most one frame's turn either way, so it holds. | A melee jittering across its teammate's position must not walk the healer back and forth (the AS-249 bob). `healers_do_not_bob_in_and_out` is `Legacy` and unchanged. |
| The stale walk | The free tick also re-commits when the healer's live walk aims more than `formation_shift_threshold` (3yd) from the point it would issue now (`formation::walk_is_stale`). | It re-committed only when the point had moved past the threshold since its last commit, and not while the healer stood within the dead zone of the new point, so a healer standing at a moving point could keep walking toward one up to 6yd away. Asking the walk itself covers both ways it went stale. |
| Changes of repair exit | Left a step. | The two exits stand either side of a teammate's shadow (or two teammates' shadows behind one pillar), so any continuous path between them crosses ground out of that teammate's sight, or detours round the pillar's near side. Easing the goal across blind ground was ruled out on AS-206. See *Changes of exit* below. |
| Profile | `TeamPlan` only (`formation::continuous`), as AS-206's repair. | The AI-profile rule. Ungated, the fix strands five `Legacy` probes; see *`Legacy`* below. |

Memory: `HealerPosture::formation` (`FormationMemory`) holds the heading with
AS-206's kept exit, and `forget()` clears both wherever the kept exit was
cleared before — leaving FREE, a medic chase, a dispel hold.

## The probes

`tests/formation_continuity.rs` (default run):

- **`a_teammate_walking_past_an_enemy_swings_the_point_round`** (staged, tier 3).
  Warrior+Druid vs Mage+Priest on BasicArena, both enemies Hammer-of-Justice'd;
  the Warrior walks to its kill target straight past the Mage, so the enemy
  nearest the Druid's centroid crosses it about a second in. Seeds 1-3. Under
  both profiles the crossing happens, the Druid stays FREE, and the
  unconstrained point read off the frames jumps over 10yd. Under `TeamPlan` the
  walk's goal never steps more than a re-commit (4yd) and its bearing from the
  Warrior turns past 90 degrees. The pair, `Legacy`, steps over 10yd. (The
  Druid because it has no wand: a Priest's point is clamped into wand range of
  a kill target 40yd off, and the clamp, not the heading, places it.)
- **`scenes::an_enemy_crossing_the_centroid_turns_the_point`** (one tick, each
  formation machine): the enemy steps across the healer's Mage in 0.1s. Under
  `TeamPlan` the point moves no more than a 0.1s turn allows; the same ticks
  with the memory forgotten, and `Legacy`, move it over 10yd.
- **`scenes::a_stale_walk_is_reissued_at_the_formation_point`**: a healer
  standing at its point, its walk aimed 5yd off, gets a new walk at the point
  under `TeamPlan`; a walk 1yd off is left alone, and so is `Legacy`'s.
- `kept_exit_reset::leaving_free_forgets_the_kept_exit` now holds the heading
  to FREE too.
- In-module (`formation.rs`): a slow target is taken exactly; a flipped target
  is swung round at the rate (never more than the rate's step, the full turn
  after pi/rate seconds); a flickering target holds the heading; the turn is
  per second; a fresh memory takes the target, a zero target keeps the
  heading.

**Mutation-checked** — each mutant fails the named test(s):

| Mutation | Fails |
|---|---|
| The Priest's `compute_formation_point` skips the turn | `an_enemy_crossing_the_centroid_turns_the_point` |
| The caster-healer one skips it | that scene and the staged probe |
| The Priest's free tick ignores a stale walk | `a_stale_walk_is_reissued_at_the_formation_point` |
| The caster-healer one ignores it | the same |
| `forget()` keeps the heading | `leaving_free_forgets_the_kept_exit`, `an_enemy_crossing_the_centroid_turns_the_point` |

## Formation-goal jumps against `main`

`measure_formation_goal_jumps` (`#[ignore]`d, `FORMATION_JUMPS_SEEDS=50`): seeds
0-49, both team orders (100 matches a comp), 120s, `TeamPlan`, every formation
healer's point goal on FREE formation frames. The measure now attributes each
jump over 5yd (`jump_cause`: a teammate died or gained/lost a target; the
healer's own target changed; the enemy nearest the centroid turned past 90
degrees about it; otherwise `other`) and counts the frames on which the goal,
and the healer, had no sight of the most injured teammate.
`FORMATION_JUMPS_PROFILE` picks the profile. main → branch:

| Comp | > 5yd | > 10yd | > 25yd | returns | 0.5s spans over 10yd |
|---|---|---|---|---|---|
| Warrior+Priest v Mage+Rogue | 1 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 8 → 0 |
| Warrior+Mage+Priest v Rogue+Warlock+Druid | 106 → 16 | 71 → 10 | 4 → 0 | 13 → 0 | 104 → 14 |
| Mage+Druid v Hunter+Paladin | 42 → 0 | 18 → 0 | 0 → 0 | 6 → 0 | 30 → 0 |
| Warlock+Priest v Rogue+Druid | 31 → 1 | 11 → 0 | 0 → 0 | 0 → 0 | 39 → 0 |
| Hunter+Shaman v Warrior+Paladin | 13 → 8 | 11 → 7 | 3 → 3 | 0 → 0 | 11 → 7 |

No cell is worse than `main`. Jumps over 5yd by cause, main → branch:

| Comp | main | branch |
|---|---|---|
| Warrior+Priest v Mage+Rogue | engaged 1 | — |
| Warrior+Mage+Priest v Rogue+Warlock+Druid | crossing 47, engaged 26, other 26, target 7 | engaged 10, other 6 |
| Mage+Druid v Hunter+Paladin | crossing 1, engaged 4, other 37 | — |
| Warlock+Priest v Rogue+Druid | crossing 8, other 22, target 1 | target 1 |
| Hunter+Shaman v Warrior+Paladin | engaged 12, other 1 | engaged 8 |

The classifier reads the crossing only over the three frames before a jump,
so a flip re-committed later reads `other`; every `crossing` is gone, and so
is all but six of `other`. What remains is `engaged`: a teammate gaining or
losing a target, or dying, which moves the centroid and the Shaman's
wand-range clamp. In the Shaman's comp those are contact at 50-54s, much as on
`main` (seeds 7, 13, 28, 39, 44 and 49 make the same contact jumps on both, to
within 0.1yd). They are decisions, not jitter, and outside this card.

**Sight.** Frames (of the counted pairs) on which the anchor was out of sight,
main → branch, goal / healer:

| Comp | goal | healer |
|---|---|---|
| Warrior+Priest v Mage+Rogue (34k pairs) | 2 → 1,405 | 3,959 → 4,537 |
| Warrior+Mage+Priest v Rogue+Warlock+Druid (255k) | 452 → 494 | 4,561 → 4,406 |
| Mage+Druid v Hunter+Paladin (134k) | 0 → 0 | 0 → 0 |
| Warlock+Priest v Rogue+Druid (178k) | 12 → 32 | 5,054 → 4,598 |
| Hunter+Shaman v Warrior+Paladin (222k) | 18 → 18 | 26 → 26 |

The healer itself is out of sight less often in two comps and more in one. The
goal's rise in the 2v2 camp comp is one 0.2s spell a match at the camp release
(18.1-18.3s on every seed — the opening is near-deterministic): the walk's
point, which the healer has not re-committed because it has moved under 3yd,
while the Warrior steps round the camp pillar.

## Changes of exit

Counted at the repair itself (every evaluation, not only the frames the goal
measure reads), over the same bank: changes of kept exit over 5yd, main →
branch, are 5 → 6 in the 3v3 (4 → 4 over 10yd) and 1 → 1 in Warlock+Priest v
Rogue+Druid; none elsewhere. The branch's seven:

- three are forced: the kept exit stopped being a candidate (the point left its
  shadow, or the exit moved more than `KEEP_RADIUS` in a frame) — 6.5, 8.8 and
  12.2yd;
- four are chosen: a better exit, by the band, the 3yd a second teammate in
  sight counts, or both — 12.0, 18.0, 18.9 and 19.8yd. In three of them the
  straight walk between the exits loses sight of the anchor; the kept exit was
  already 11-13yd from the raw point, where the repair takes it only part of
  the way.

Neither of the card's two remedies applies to those three without crossing
blind ground. Holding the kept exit until a sighted switch keeps it in the
fade zone, where the goal slides toward the raw point — into the shadow — and
the switch comes anyway once it passes `REPAIR_LIMIT`. An intermediate sighted
point exists only round the pillar's near side, a multi-waypoint walk for
three events per 100 3v3 matches. So the step stays a step.

**The juke probe** (`juke_bounded_over_the_seed_bank`; its Warrior+Shaman side
is `TeamPlan`, so the Shaman's formation point changes before the Warrior
dies) still passes, and its guard is unchanged. Which seeds dance moves with
the change: 30 dances on `main`, 27 on the branch; the most fizzle-length
windows a minute any dance holds 13.5 → 14.1 (bound 16), the mean 6.6 → 5.8.

## Balance

**Directional paired sweeps**, 2v2, `TeamPlan` both sides,
`--affects Priest,Druid,Shaman`, 683 cells x 10 seeds = 6,830 matches a map
(inputs and both CSVs: `sweeps/2026-10-18-as370-teamplan-*`):

```
PillaredArena
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  49.8% -> 50.2%  +0.4pt  flips 159 (+91/-66) z=1.92 ns
CLEAN           n=2250  48.3% -> 48.6%  +0.3pt  flips 49 (+28/-21)  z=0.86 ns
AGAINST         n=2250  51.8% -> 52.3%  +0.4pt  flips 41 (+25/-15)  z=1.42 ns
MIRRORED        n=2250  49.3% -> 49.7%  +0.4pt  flips 69 (+38/-30)  z=0.85 ns
NON-VACUITY: 6774/6830 ended by elimination; 1711 moved in winner or duration

BasicArena
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
ALL reachable   n=6750  49.8% -> 49.7%  -0.1pt  flips 148 (+69/-79) z=0.74 ns
CLEAN           n=2250  44.4% -> 44.2%  -0.2pt  flips 45 (+20/-25)  z=0.60 ns
AGAINST         n=2250  55.1% -> 55.1%  -0.0pt  flips 47 (+23/-24)  z=0.00 ns
MIRRORED        n=2250  49.9% -> 49.7%  -0.2pt  flips 56 (+26/-30)  z=0.40 ns
NON-VACUITY: 6788/6830 ended by elimination; 1096 moved in winner or duration
```

Consistent with no movement on either map; each run resolves 0.4pt.

**Attribution.** For a random sample of moved matches the two binaries were
re-run with the trace on and the first differing event read. Nagrand, 20: ten
first differ at a formation healer's `FormationShift`; the other ten at an
ability decision whose only differing fields are positions and distances (the
healer's, or of a unit beside it) — movement that the trace does not record
as a decision (a walk kept alive by a refresh is not traced). BasicArena, 12:
eleven at a formation healer's `FormationShift`, one at a position. None first
differs anywhere else.

**`camp_sweep`** (Warrior+Priest vs Warlock+Priest, seeds 1-12): `Legacy`
identical. `TeamPlan`: four seeds' heal or occlusion move, same winners; heal
line occluded 26% → 25% (after contact 40% → 37%), heal to the Warrior 170 →
181, the Priest hard-CC'd 6.7s → 5.7s, denied pre / post contact 15.1s /
12.4s → 15.1s / 12.1s.

## `Legacy`

`Legacy` on both maps (the same 2 x 6,830 configs with `"ai_profile":"Legacy"`):
CSV identical, 13,545 of 13,660 matches ended by elimination. The gate returns
the unconstrained direction and never asks for a stale walk, so this is
expected, not only observed; the `Legacy` jump measure is identical to `main`
line for line.

What the gate leaves: `Legacy`'s 3v3 makes 465 formation-goal jumps over 5yd
per 100 matches (346 of them crossings, 145 returns). Ungated, an earlier build
of this change took that to 2, and left five `Legacy` probes stale, each
seed-pinned or a seed bank whose situation the change moved:
`casting_mana_juke_probe` (juked lone-Shaman 2v1s 5 → 2 of 90, floor 4),
`frame_rate_determinism::results_and_report_do_not_depend_on_the_display_rate`
(its TwinPillars match no longer fears or roots anyone),
`hunter_trap_trace::{a_lane_trap_springs_on_the_enemy_it_was_decided_on,
the_hunters_own_shot_does_not_break_its_trap}` (pinned seeds),
`movement_probes::u10_press::press_comp_resolves_before_cap` (one kill past
dampening onset, needs two) and
`victory_choreography::a_ring_slot_inside_a_pillar_moves_in_a_real_match`.
Each would need migrating rather than re-pinning, in areas this card does not
own. Lifting the gate is the one-line change in `formation::continuous` plus
those migrations.
