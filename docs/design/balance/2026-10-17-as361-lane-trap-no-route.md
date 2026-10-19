# AS-361: no lane trap, and no reserved sting, for a hidden lane with no route

Tier: **directional**. Success condition: **the sting's reservation and the
lane throw agree**, proven by a paired scene; TwinPillars read from the
Hunter's side; BasicArena byte-identical. Also measured: whether a lane trap
spent on an enemy pet beats holding it for a player.

## The change

`hunter::fallback_trap` decides the lane Freezing Trap, and the Serpent Sting
reserves the enemy it would catch. Since AS-295, a hidden lane lands on an
enemy's route round cover (`class_ai/hunter_lane.rs`). With no such route,
`fallback_trap` still returned the hidden lane and its predicted springer. The
placement always refuses a hidden landing, so the sting was held for a throw
that never came.

`fallback_trap` now returns `LosBlocked` for a hidden lane with no route. The
placement traces a hidden landing the same way. The sting's reservation sees
no throw, and the sting goes out.

`tests/hunter_route_lane_scenes.rs` pairs the cases. A Rogue pursuing the
Hunter round a pillar has a seen route: the sting is reserved and the trap
lands on the route. The same Rogue with no target has no route: the sting is
cast, and with the sting cooling the trap is held `LosBlocked`. Reverting the
fix fails the second arm. The sting is then reserved, the trap is refused, and
nothing is cast. The control that a walker heading away from a hidden lane is
held now reads `LosBlocked`. Its seen-lane twin still reads "no enemy is heading
into the lane".

## Arms

- **before**: `fd8ac5e1` (main, with AS-295).
- **after**: this branch.
- **probe**: the branch plus `sweeps/2026-10-17-as361-reservation-probe.patch`.
  This patch writes to stderr only. It evaluates the old and new `fallback_trap`
  at every sting decision and prints where the two reservations differ. Its CSV
  is byte-identical to the branch's.
- **pet hold**: the branch plus `sweeps/2026-10-17-as361-pet-hold.patch`.
  `AS361_PET_HOLD=route` holds a route-lane throw decided on a pet.
  `AS361_PET_HOLD=all` holds any lane throw decided on a pet. With `off`, its
  CSV is byte-identical to the branch's.

Batch: AS-295's inputs (`sweeps/2026-10-17-as295-<map>.jsonl.gz`), 4,240 2v2
matches per map, `Legacy`, `--trace-mode off`. Scored from the Hunter's side
with `sweeps/2026-10-12-as281-hunter-side.py` (n=3,520). The `paired_sweep.py`
controls are 80/80 identical.

## Result: inert on both maps

| map | Hunter side, before -> after | flips |
|---|---|---|
| BasicArena | 45.3% -> 45.3% | 0 / 0 |
| TwinPillars | 40.0% -> 40.0% | 0 / 0 |

- **BasicArena is byte-identical.** The two CSVs are the same file. 4,216 of
  4,240 matches ended by elimination, with 1,869 distinct durations.
- **TwinPillars is byte-identical too, and the probe shows why.** On 165,856
  sting decisions, the old `fallback_trap` returned a throw that the new one
  holds. 163,786 of them were decided on a pet. Only 142 had the sting's own
  target as the lane's victim, so only 142 changed the reservation. All 142 had the Hunter with no living non-pet
  teammate, and all 142 had the target **out of sight and out of range**. 31 of
  them had the trap ready. The sting's cast gate refuses those casts in either
  arm.
- **Why the defect cannot reach a 2v2 match while the team stands.** The
  reservation needs the lane's victim to be the sting's target. The sting's
  target is the Hunter's kill target. With a living non-pet teammate,
  `fallback_trap` already holds a trap on the kill target because "the team is
  attacking the enemy it would catch". With the Hunter alone, a lane hidden
  toward its one target almost always hides the target too.
- **Where it does bite.** It bites when the Hunter is alone and sees its target
  past a pillar edge, while the lane toward an enemy healer is hidden. That is
  the scene. The fix is a correctness fix. This sweep cannot price it.

## Finding: a trap on a pet beats holding it for a player

On TwinPillars, the branch's route-lane throws are decided almost entirely on
pets (pet-hold instrument, `off` arm):

| path | decided on a pet | on a player | unseen (stealth) |
|---|---:|---:|---:|
| route | 1,228 | 8 | — |
| seen lane | 97 | 59 | 230 |

Holding those throws for a player costs the Hunter's side:

| arm | Hunter side | delta vs branch | flips (+/-) | z |
|---|---|---|---|---|
| hold route throws on a pet | 38.5% | **-1.5pt** | +58 / -110 | **-4.0** |
| hold every lane throw on a pet | 38.4% | -1.6pt | +59 / -114 | -4.2 |

- **The pet catch is the route lane's value.** Holding route throws on a pet
  gives back AS-295's whole TwinPillars gain: 38.5% is the pre-AS-295 figure,
  with the mirror of its +110/-58 flips. A held trap does not wait for a
  player: only 8 route throws ever go to one.
- **Holding seen-lane pet throws as well adds nothing measurable** (-0.1pt,
  +13/-16, z=-0.6 against the route-only hold). With route throws held, the
  Hunter throws 330 seen-lane traps on pets instead of 97.
- This argues for **no change**. A Freezing Trap on a Felhunter or Spider takes
  its Spell Lock, Devour Magic or Web out of the fight, and that is worth more
  than a trap held for a player who rarely walks into it.

This is a direction, not a standing.
