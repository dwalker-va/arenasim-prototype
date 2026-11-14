# AS-129: healers dispel pets

**Tier: DIRECTIONAL** (`sweep-tiers.md`). Measured at `main` @ `325265cc` against
the AS-129 branch. Success condition: the change moves only matches with a team
that holds both a dispelling healer and a pet, and the direction is reported for
the Warlock cells, Hunter v Rogue+healer and the healers' own outcomes.

## The change

The Priest's Dispel Magic and the Paladin's Cleanse now take their team's pets
as targets (user ruling on AS-126). One rule, `class_ai::ally_dispel_candidate`,
read by every dispel chooser:

| Reader | Pets now |
|---|---|
| The rotation's scan, `scan_ally_dispel` (Priest, Paladin; urgent, root and routine bars; the Priest's Shadow Word: Pain hold) | included, below a non-pet teammate at equal priority |
| `owed_dispel`, so the `Legacy` dispel walk, the `TeamPlan` `DispelGoal` and the Medallion's `expected_dispeller` | included; a reached non-pet, then a reached pet, then the nearest unreached non-pet, then the nearest unreached pet |
| Devour Magic | unchanged: it already took pets and preferred a non-pet |
| `can_free_ally` (the Hunter's trap AI) | included, through `ally_removal`'s `reaches_pets` |
| Master's Call | unchanged: its own scans, no pets |

A pet is set aside while any living non-pet teammate is dying, the healer
included. A pet counts as melee for the root tier (`root_holds_back`): a root
that keeps it off its target is held back.

Nothing else was needed to free the pet. `pet_ai_system` skips an incapacitated
pet each frame from its live auras, so a pet whose trap is dispelled acts the
next frame (`tests/pet_dispel.rs`, `dispel_magic_frees_a_trapped_felhunter_in_a_match`).

**World measured.** AS-125 (trap placement) has merged, so these numbers
describe a Hunter that already traps on purpose. AS-67 (reaction latency) is
open, so the dispel's timing here is provisional on it.

## Blast radius

Every match without a healer+pet team on either side is byte-identical in winner
and duration: **6,030 of 6,030** in the 2v2 BasicArena arm, 80 of 80 on
Nagrand, 15 of 15 in the 3v3 arm. That includes matches that field a Priest,
Paladin, Hunter or Warlock apart, so the diff stays inside the one team shape
it targets.

## Non-vacuity

Full logs, both arms, every reachable 2v2 cell at seeds 0 and 1 (448 matches a
map per arm):

| | BasicArena/Legacy before | after | Nagrand/TeamPlan before | after |
|---|---|---|---|---|
| Freezing Trap sprung on a pet beside a Priest/Paladin | 92 | **1** | 9 | **2** |
| Freezing Trap sprung on a pet with no dispeller on its team | 32 | 32 | 33 | 33 |
| Freezing Trap sprung on a non-pet | 56 | 53 | 53 | 54 |
| Healer dispels cast on a pet (skipped before) | 0 | **48** | 0 | **76** |
| Unstable Affliction backlash on a dispeller | 232 | 249 | 228 | 243 |

The healers' dispels on pets took Entangling Roots (31 / 29), Unstable
Affliction (14 / 16), Fear, Death Coil, Polymorph, Psychic Scream and Frost
Armor, and once a Freezing Trap. The pet trap almost disappeared: the Hunter's
trap AI asks `can_free_ally`, so a pet beside a Priest or Paladin no longer
counts as a trap that sticks, and the Hunter holds the trap instead. That is
the dominant mechanism, and neither of the card's two predicted effects.

## Win rates

Scored from the side holding the healer+pet team, McNemar on the flips
(`sweeps/2026-11-13-as129-pet-dispel.py sides`).

| Slice | BasicArena 2v2, Legacy (n=10/cell) | Nagrand 2v2, TeamPlan (n=10/cell) | BasicArena 3v3, Legacy (n=5/cell) |
|---|---|---|---|
| Healer+pet side v a side without one | **+1.9pt** (z=+3.3) | -0.1 (ns) | -0.9 (ns) |
| Priest+pet | **+3.7** (z=+4.3) | +0.8 (ns) | -0.8 (ns) |
| Paladin+pet | -0.3 (ns) | -1.0 (z=-1.8, ns) | -0.5 (ns) |
| Healer+Hunter | **+3.2** (z=+4.4) | +0.3 (ns) | -0.5 (ns) |
| Healer+Warlock | +0.1 (ns) | -0.6 (ns) | -1.1 (ns) |
| **Healer+Warlock v a Hunter team** | +0.0 (54 flips, 27/27) | +0.6 (2 flips) | -1.2 (ns) |
| **Hunter v Rogue+healer** | 0 flips | 0 flips | +3.2 (z=+1.9, ns) |

`paired_sweep.py --affects Priest,Paladin,Hunter,Warlock`: 2v2 BasicArena +0.1pt
team1 over 8,190 reachable matches (156 flips, floor 0.3pt); Nagrand -0.0pt (75
flips, floor 0.8pt); 3v3 -2.3pt team1 (z=3.1), all of it in the MIRRORED slice,
where the tool flags it as noise unless corroborated. Nothing corroborates it:
it is a seat effect, which a side-scored slice cannot express, and the 3v3
healer+pet side against a team without one is -0.9 (ns).

**Hunter v Rogue+healer** moves only when the Hunter's own partner is a Priest
or Paladin (the Rogue team has no pet), and in 2v2 that cell did not flip once.

## Which effect dominates

The decomposition arm (`armh`) gives healers the pet dispel but leaves the
Hunter's trap AI on the old rule: `can_free_ally` patched to
`(!victim.is_pet || freer.is_pet) && r.scope.takes(aura)`. Same reachable 2v2
cells, same seeds:

| Healer+pet side | Dispel alone (before → armh) | Hunter re-aims (armh → after) | Both |
|---|---|---|---|
| BasicArena, all | -0.4 (ns) | **+2.3** (z=+4.1) | +1.9 |
| BasicArena, Priest+pet | **+2.3** (z=+2.8) | +1.3 (ns) | +3.7 |
| BasicArena, Paladin+pet | **-3.0** (z=-4.2) | **+2.7** (z=+3.7) | -0.3 |
| BasicArena, healer+Warlock v Hunter | -1.9 (ns) | +1.9 (ns) | 0.0 |
| Nagrand, all | -0.2 (ns) | +0.0 (1 flip) | -0.1 |
| Nagrand, Paladin+pet | -1.0 (ns) | 0.0 | -1.0 |

- **The healer's GCD cost is real and is the Paladin's.** On its own the pet
  dispel costs a Paladin+pet team 3.0pt on BasicArena: its Cleanse takes
  roots off the Felhunter and Spider, and pulls Unstable Affliction off the
  Felhunter into a backlash silence. The Priest gains 2.3pt from the same
  change.
- **The Hunter's lost benefit is the larger effect where it exists.** Once the
  Hunter stops trapping a pet a healer would free, healer+pet teams gain 2.3pt
  on BasicArena, cancelling the Paladin's cost. On Nagrand under `TeamPlan` the
  Hunter rarely trapped such a pet in the first place (9 traps in 448 matches),
  so there the change is the dispel alone, and it is null.
- **The Warlock cells cancel.** Healer+Warlock v a Hunter team: the dispel
  alone moves the Warlock side -1.9, the Hunter's re-aim moves it back +1.9, and
  the net is 27 flips each way. The Warlock matchup stays as hard for the
  Hunter as it was (the Hunter side wins 24.7% before and after on BasicArena,
  25.9-26.6% on Nagrand).

The pet root tier is almost inert: with it reverted (`root_holds_back` taking
pets down the ranged branch) 41 of 2,240 BasicArena matches and 17 of 2,240
Nagrand matches move, with 5 and 0 flips.

## Reproduce

Inputs are rebuilt by the commands in the script's docstring
(`sweeps/2026-11-13-as129-pet-dispel.py`); the 2v2 JSONL is
`gen_sweep.py`'s own deterministic output, and the Nagrand and 3v3 sets derive
from it. Each arm is one binary run over each set at `--jobs 4`.
