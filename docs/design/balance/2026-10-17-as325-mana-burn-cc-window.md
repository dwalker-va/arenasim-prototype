# AS-325: the Priest's Mana Burn CC window, deleted

**Card:** AS-325. **Tier:** DIRECTIONAL. **Result:** the rule is deleted. It
was live, but it was worth nothing measurable to the Priest.

## The rule

Mana Burn sits above Flash Heal in the Priest's chain, gated on team health.
Outside a lock the floor was 0.90, which is Flash Heal's own trigger, so the burn
only took surplus global cooldowns. While the enemy healer was locked
(`situation.enemy_healer_locked`) the floor dropped to 0.70. A team between 70%
and 90% then burned instead of healing.

Now the floor is 0.90 whether the healer is locked or not. A burn deals no
damage, so it never breaks crowd control, and the mana it burns stays burned
after the lock ends. A lock therefore adds nothing for a burn to spend. Without
that, an until-aware play keyed on `fits_window` has no deadline to respect.

## It was not dead

The card's premise came from AS-270, which found that the rule changed 0 of
40,873 decisions. That does not hold on current main
(`5ca3e72d`). The seed bank below (`sweeps/2026-10-17-as325-bank.py`) ran
2,560 matches:

- Priest + DPS against every healer + DPS, both sides, BasicArena and
  PillaredArena, `Legacy` and `TeamPlan`, seeds 1-3;
- plus four 3v3 comps at 10 seeds each.

| | count |
|---|---|
| matches whose log differs | **254** of 2,560 |
| of those, first divergence is a main-side Mana Burn | 254 |
| Priest decisions taken under a lock (main) | 178,307 |
| Mana Burn chosen under a lock (main) | 924 |
| Flash Heal chosen under a lock (main) | 1,714 |
| deaths across the bank (non-vacuity) | 6,889 |

Each of the 254 diverging matches diverges first where the main build begins a
Mana Burn and this branch begins a heal. That is the change the deletion
predicts, attributed directly rather than by elimination.

The locks behind those burns are mostly:

- Fear, from a Warlock partner;
- Polymorph, from a Mage partner;
- Psychic Scream, from the Priest itself;
- a few Kidney Shots and Freezing Traps.

128 of the 254 are Priest mirrors.

| winner changed, deletion vs main | count |
|---|---|
| single-Priest matches: the Priest's side gains | 9 |
| single-Priest matches: the Priest's side loses | 10 |
| Priest mirrors: winner flipped | 16 |

## The sweep

These are paired directional sweeps, main against this branch, at identical
seeds. Each covers every 2v2 cell a Priest change can reach, plus 8 control
cells, at n=10: 2,830 matches per arm. The win rate is scored from the
Priest's side: in the CLEAN slice the Priest is on team 1 only.

| arm | slice | before | after | flips | z |
|---|---|---|---|---|---|
| `Legacy`, BasicArena | CLEAN (Priest is team 1) | 48.8% | 48.8% | 0 | 0.00 |
| | AGAINST (Priest is team 2) | 50.6% | 50.6% | 0 | 0.00 |
| | MIRRORED | 50.4% | 51.6% | 3 (+3/-0) | 1.15 |
| `TeamPlan`, PillaredArena | CLEAN | 45.4% | 45.4% | 0 | 0.00 |
| | AGAINST | 41.8% | 41.8% | 0 | 0.00 |
| | MIRRORED | 41.2% | 40.8% | 3 (+0/-1) | 0.00 |

In both arms, all 80 control matches are identical. In each arm, every
reachable slice reads "consistent with no movement":

- the `Legacy` arm resolves 0.2pt across all reachable cells;
- the `TeamPlan` arm resolves 0.1pt.

Inputs and results are in `sweeps/`, as `2026-10-17-as325-*`.

## What pins it

`tests/situation_scenes.rs` holds the replacement pair:

- a team at 80% is healed, locked or not;
- a topped team burns, locked or not.

The first test fails if the lock lowers the floor again. The second fails if
the burn becomes unreachable.
