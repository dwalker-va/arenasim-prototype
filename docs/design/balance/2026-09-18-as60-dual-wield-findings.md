# AS-60 dual wield — what enabling it costs

Measured 2026-09-18 on `card/AS-60-dual-wield`. Four raw per-match CSVs sit
beside this file; every number below is recomputable from them with
`scripts/agg_sweep.py`.

## Design of the measurement

**Not against the canonical baselines.** AS-96 records those as stale (AS-86
moved Paladin ~+2.4pt; AS-87 and AS-97 landed after). So both arms of each
comparison are **the same binary at the same seeds**, differing only by a
`team1_equipment` off-hand override. Nothing here is compared to a recorded
baseline, which sidesteps the staleness rather than working around it.

Tier: **directional**. 25 cells (every distinct opposing pair, double-healer
excluded) × n=100 × 2 arms = 2500 matches per arm, 300s cap, `--seed-base 0`
so match *i* of a cell uses seed *i* in both arms.

**Non-vacuity: all 10,000 matches ended in a KILL. Zero timeouts.** The Rogue
control alone spans 1329 distinct durations.

## Result

Rogue+Priest, off hand empty (shipped) vs a second Serpent Fang Dagger:

| arm | overall | CSV |
|---|---|---|
| empty off hand | 59.9% [57.9–61.8] | `...-rogue-control-n100.csv` |
| second dagger | **70.7% [68.9–72.5]** | `...-rogue-offhand-n100.csv` |

**+10.8pt**, non-overlapping intervals, 10 of 25 cells moved beyond noise —
as counted by `agg_sweep.py --compare`, the tool the reproduction section
below tells you to run. The contested cells moved hardest: Rogue mirror 52→82, vs Rogue+Shaman 15→50, vs
Mage+Priest 14→44, vs Warrior+Rogue 12→42.

**+10.8pt is the ONE-SIDED figure — read it as the mechanic's worth, not as
the cost of shipping it.** The override arms team 1 only, which is what makes
this a measurement of the off-hand swing rather than of a rules change: in the
seven cells where the enemy also fields a Rogue, a dual-wielding Rogue faces a
single-wielding one, and the mirror's 52→82 is that asymmetry. Arm the socket
in `loadouts.ron` instead and every Rogue gets it, so those cells largely wash
and the shipped quantity is a different — smaller — one. AS-122 measured it:
`2026-09-18-as122-rogue-offhand-findings.md`.

The off-hand item is a second copy of the Rogue's own default main hand, so
this is the conservative figure: it isolates the mechanic from an item
upgrade. A tier-1 off-hand lands higher.

Warrior+Priest, two-handed Arcanite Reaper vs two Frostbite Blades:

| arm | overall | CSV |
|---|---|---|
| Arcanite Reaper | 45.8% [43.9–47.8] | `...-warrior-control-n100.csv` |
| two Frostbite Blades | 47.6% [45.6–49.5] | `...-warrior-offhand-n100.csv` |

**+1.8pt, intervals overlapping, not one cell moved beyond noise.**

**The Warrior arm is not a clean same-tier swap, and the difference is the
point.** Arcanite Reaper is ilvl 60 carrying AP +4; two Frostbite Blades are
ilvl 58 each, carrying AP +6 and crit +0.02 between them — about +11% white
DPS after the miss penalty. That windfall IS the two-hander-draws-one-socket's-budget
asymmetry AS-115 describes, so including it is what makes the arm answer the
question. Read the +1.8 as "trade the 2H for two 1H, budget asymmetry and
all", not as "the off-hand swing alone is worth +1.8".

## What it means

Dual wield priced as a **trade** is worth roughly nothing. Priced as a **free
slot** it is worth +10.8pt. The Rogue's off-hand socket was empty when this was
measured, so there was no opportunity cost to price the damage against — that,
not the budget asymmetry, is what drives the number.

**That socket has since been priced and filled.** AS-122 armed the Rogue's
default off hand with the second dagger and measured the shipped change, which
is a different quantity from the +10.8pt above for the reason given with it.
See `2026-09-18-as122-rogue-offhand-findings.md`.

## Windfury procs on the main hand only

Windfury Totem's bonus swing rolls on the main-hand swing and has no twin in
the off-hand branch. This is a modelling simplification, but not a departure
from Classic's *behaviour*: the totem applies a temporary weapon ENCHANT, and a
Rogue's off hand carries a poison in that slot, which forces the buff onto the
main hand. That was also the efficient play — Windfury was proc-per-minute, so
a fixed budget of procs was worth more spent on the weapon with the higher top
end than sometimes rolled on the smaller one, and the poison still earned its
slot. Main-hand-only is the realistic outcome of the enchant-slot interaction,
so the sim models the outcome and skips the slot. Fuller version in
`docs/design/wow-mechanics.md`.

## Reproducing

From a release build of this branch:

```bash
cargo build --release

# Rogue arms
scripts/gen_sweep.py --t1 'Rogue+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer > /tmp/rogue_control.jsonl
scripts/gen_sweep.py --t1 'Rogue+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer \
  --extra '{"team1_equipment":[{"OffHand":"SerpentFangDagger"},{}]}' \
  > /tmp/rogue_offhand.jsonl

# Warrior arms
scripts/gen_sweep.py --t1 'Warrior+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer > /tmp/warrior_control.jsonl
scripts/gen_sweep.py --t1 'Warrior+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer \
  --extra '{"team1_equipment":[{"MainHand":"FrostbiteBlade","OffHand":"FrostbiteBlade"},{}]}' \
  > /tmp/warrior_offhand.jsonl

for arm in rogue_control rogue_offhand warrior_control warrior_offhand; do
  ./target/release/arenasim --batch /tmp/$arm.jsonl --out /tmp/$arm.csv --jobs 12
done

scripts/agg_sweep.py /tmp/rogue_offhand.csv --compare /tmp/rogue_control.csv
scripts/agg_sweep.py /tmp/warrior_offhand.csv --compare /tmp/warrior_control.csv
```

`gen_sweep.py` already takes its teams, seed base and overrides as arguments,
so there is no bespoke script and no hardcoded path to go stale. The committed
CSVs were produced by exactly these commands.
