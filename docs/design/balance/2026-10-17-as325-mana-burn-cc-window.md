# AS-325: the Priest's Mana Burn CC window, deleted

**Card:** AS-325. **Tier:** DIRECTIONAL. **Result:** the rule is deleted. It
was live under both AI profiles, and `Legacy` output moves, BasicArena
included. It was worth nothing measurable to the Priest.

## The rule

Mana Burn sits above Flash Heal in the Priest's chain, gated on team health.
Outside a lock its floor was 0.90, which is Flash Heal's own trigger, so the
burn took only surplus global cooldowns. While the enemy healer was locked
(`situation.enemy_healer_locked`), the floor dropped to 0.70. A team between
70% and 90% then burned instead of healing.

Now the floor is 0.90 whether the healer is locked or not. A burn deals no
damage, so it never breaks crowd control, and the mana it burns stays burned
after the lock ends. A lock therefore adds nothing for a burn to spend. With no
such gain, an until-aware play keyed on `fits_window` has no deadline to
respect.

## It was not dead, and `Legacy` moves

The card's premise came from AS-270, which found the rule changed 0 of 40,873
decisions. That does not hold on current main (`5ca3e72d`).

**The rule ran under `Legacy` as well as `TeamPlan`, so deleting it changes
`Legacy` match logs, on BasicArena as well as on PillaredArena.** This change
is not a `Legacy` byte-identity change.

The seed bank is `sweeps/2026-10-17-as325-bank.py`, which prints every figure
in this section. It ran 2,560 matches:

- Priest + DPS against every healer + DPS, with the Priest's team on each side,
  on BasicArena and PillaredArena, under `Legacy` and `TeamPlan`, at seeds 1-3;
- four 3v3 comps at 10 seeds each.

| | count |
|---|---|
| matches whose log differs | **254** of 2,560 |
| `Legacy`, BasicArena | 72 |
| `Legacy`, PillaredArena | 67 |
| `TeamPlan`, BasicArena | 85 |
| `TeamPlan`, PillaredArena | 30 |
| first differing event is main beginning a Mana Burn | 254 of 254 |
| Priest decisions under a lock (main) | 178,307 |
| Mana Burn chosen under a lock (main) | 924 |
| Flash Heal chosen under a lock (main) | 1,714 |
| deaths across the bank (non-vacuity) | 6,889 |

The deletion predicts what the logs show. In every diverging match, the first
event that differs is a Mana Burn that main begins while the enemy healer is
locked, and that this branch replaces with a heal. That attributes each
divergence directly, rather than by elimination.

| winner, deletion vs main | count |
|---|---|
| single-Priest match: winner unchanged | 107 |
| single-Priest match: the Priest's side gains | 9 |
| single-Priest match: the Priest's side loses | 10 |
| Priest mirror: winner unchanged | 112 |
| Priest mirror: winner changed | 16 |

## The sweep

These are paired directional sweeps, main against this branch, at identical
seeds. Each covers every 2v2 cell a Priest change can reach, plus 8 control
cells, at n=10: 2,830 matches per arm.

Each figure below is the win rate of the side that fields the Priest. In the
CLEAN slice that is team 1; in the AGAINST slice it is team 2. A mirror has a
Priest on both sides, so the MIRRORED slice reports team 1's rate.
`scripts/paired_sweep.py` prints team 1's rate in every slice. Its AGAINST
figures (50.6% and 41.8%) are therefore the NON-Priest side's rate. They are
not the complement of the figures here, because a draw is neither side's win.

| arm | slice | before | after | winners changed | duration-only changes |
|---|---|---|---|---|---|
| `Legacy`, BasicArena | CLEAN | 48.8% | 48.8% | 0 | 10 |
| | AGAINST | 49.1% | 49.1% | 0 | 7 |
| | MIRRORED (team 1) | 50.4% | 51.6% | 3 (+3/-0, z=1.15) | 17 |
| `TeamPlan`, PillaredArena | CLEAN | 45.4% | 45.4% | 0 | 6 |
| | AGAINST | 46.4% | 46.4% | 0 | 3 |
| | MIRRORED (team 1) | 41.2% | 40.8% | 3 (+0/-1, z=0.00) | 7 |

In both arms all 80 control matches are identical. `paired_sweep.py` finds
every reachable slice consistent with no movement. Across all reachable cells
the run resolves 0.2pt for `Legacy` and 0.1pt for `TeamPlan`. The CLEAN and
AGAINST slices have no winner changes, so that floor comes from the mirrors.

**Why the bank's 19 single-Priest winner changes have no counterpart here.**
The sweep does not replay the bank's matches.

- The bank always seats the Priest in slot 1 of its team.
- `gen_sweep.py` orders each team by its class list, which puts a Priest
  partnered with a Warrior, Mage or Rogue in slot 2.
- Slot order changes the match.
- Each sweep arm also runs one profile on one map, where the bank crosses both.

In the single-Priest slices, the sweep's deletion shows only as duration
changes with the same winner: 17 under `Legacy` and 9 under `TeamPlan`.

Inputs and results are in `sweeps/`, as `2026-10-17-as325-*`.

## What pins it

`tests/situation_scenes.rs` holds the replacement pair:

- a team at 80% is healed, locked or not;
- a topped team burns, locked or not.

The first test fails if the lock lowers the floor again. The second fails if
the burn becomes unreachable.
