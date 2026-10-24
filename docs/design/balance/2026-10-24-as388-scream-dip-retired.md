# AS-388: the TeamPlan Priest's offensive scream dip, retired

**Tier: DIRECTIONAL** (`sweep-tiers.md`). Nothing here may be cited as a
class's standing.

**The change.** Under `TeamPlan` the Priest no longer runs its offensive
Psychic Scream dip. Its offensive Scream is the Peel step's alone, on an
attacker of the teammate in danger already inside the Scream. `Legacy` keeps
the dip, and the defensive Scream is unchanged (design D5, ruled 2026-10-24:
dip retired, no walk).

## Bank

The Priest-side cells of the AS-385 package bank
(`2026-10-24-as385-package-gen.py legacy-h2h`): the `TeamPlan` side holds a
Priest, the other side is `Legacy`, on Nagrand and TwinPillars, both seats,
seeds 501-512. 3,840 matches with the Priest team in seat 1 and 2,640 in
seat 2: 6,480 paired sides per arm. Paired at identical seeds, McNemar on the
flips; the base is `main` @ 03f77e9e (the AS-385 package).

```bash
G=docs/design/balance/sweeps/2026-10-24-as385-package-gen.py
$G legacy-h2h t1 501-512 | python3 -c 'import json,sys
[print(l,end="") for l in sys.stdin if "Priest" in json.loads(l)["team1"]]' > t1.jsonl
# t2: the same with t2 and team2
target/release/arenasim --batch t1.jsonl --out arm-t1.csv --jobs 5 --trace-mode off
S=docs/design/balance/sweeps/2026-10-24-as385-package-score.py
$S seat main-t1.csv arm-t1.csv main-t2.csv arm-t2.csv
```

CSVs: `sweeps/2026-10-24-as388-{main,retired}-t{1,2}.csv.gz`.

## Result: TeamPlan Priest against Legacy

| slice | n | main | retired | delta | +/- flips | z |
|---|---|---|---|---|---|---|
| all | 6480 | 54.4 | 54.9 | +0.6 | 317/281 | +1.47 |
| Nagrand | 3240 | 52.1 | 51.9 | -0.2 | 81/88 | -0.54 |
| TwinPillars | 3240 | 56.7 | 58.0 | +1.3 | 236/193 | +2.08 |
| with Hunter | 1440 | 39.1 | 41.2 | +2.1 | 71/41 | +2.83 |
| with Mage | 1440 | 65.3 | 64.9 | -0.3 | 82/87 | -0.38 |
| with Rogue | 1200 | 69.6 | 69.0 | -0.6 | 19/26 | -1.04 |
| with Warlock | 1440 | 47.9 | 48.9 | +1.0 | 105/91 | +1.00 |
| with Warrior | 1440 | 51.9 | 51.9 | +0.0 | 83/83 | +0.00 |

By seed block the overall delta is -0.8 (501-504), +0.6 (505-508) and +1.9
(509-512). The package measurement's Priest row read seeds 501-504 only; a
per-class delta at n=2,160 swings about this much between blocks.

## The walk that is not built

Each variant ran on the walk build with a measurement-only switch
(`sweeps/2026-10-24-as388-walk-arms.patch`, an `AS388_DIAG` environment
variable on `card/AS-388-scream-peel-walk` @ c9104596). Against the retired
dip with no walk, same bank:

| variant | delta | +/- flips | z | CSVs |
|---|---|---|---|---|
| walk at any attacker of the teammate in danger, `dip_budget` 6s | -0.51 | 26/59 | -3.58 | `walk` |
| melee and pet attackers only | -0.19 | 17/29 | -1.77 | `melee` |
| one global cooldown of travel (1.5s) | -0.20 | 16/29 | -1.94 | `short` |
| both | -0.15 | 15/25 | -1.58 | `melee_short` |
| the gate keeps the target's crowd control that lands beyond the Scream | -0.20 | 19/32 | -1.82 | `rangedcc` |
| the walk does not defer non-critical heals | -0.45 | 15/44 | -3.78 | `nodefer` |
| the gate exempts the crowd control of an enemy healer the Scream would catch | -0.62 | 26/66 | -4.17 | `exempt` |

Per frame, over seeds 501-504 of the same cells (1,200 matches), the walk ran
0.12 times a match for 1.5s. 86% of walks ended without a Scream. A walk at a
ranged attacker (mean 22yd) left the Priest hard crowd-controlled within the
walk or 3s after it in 65% of walks, against 6% at a melee attacker: the
attacker's own Polymorph or Fear, which the gate leaves out. The walk was
blind to its teammate on 0.8% of its frames, and the teammate in danger died
as often with it as without it (46.0% against 45.7% of danger windows).

## Mechanism

- **Camp** (`camp_sweep`, seeds 1-12): `TeamPlan` 12/12, heal to the Warrior
  306, occlusion before/after contact 15.0s/12.5s; `Legacy` 10/12.
- **Heal-line probe** (48 seeds): goal blind 20.7% of 13,300 PRESSURED frames,
  worst seed with 300 or more 30%; ceilings 30% pooled, 50% per seed. The
  package read 10.9% of 34,172. The blind frames fell; the denominator lost
  the PRESSURED frames the dip's path ended in.
- **Peel probe** (`peel_play_probe`): answered z 5.28, survival z -0.42.

## Legacy byte-identity

All 1,260 cells of the package bank at seed 501, both teams `Legacy`, as full
`--headless` match logs from `main` and from this change: `diff -r` is clean.
The bank is not vacuous: 3,577 deaths, 7,078 crit lines, 996 distinct
durations; winners team 1 692, team 2 552, 16 draws.
