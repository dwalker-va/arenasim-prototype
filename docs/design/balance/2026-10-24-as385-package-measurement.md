# AS-385 package measurement: the sight yield and the pillar stance

**Tier: DIRECTIONAL** (`sweep-tiers.md`). The package is judged on win rate as
a whole (design D6), with per-piece mechanism metrics beside it. Nothing here
may be cited as a class's standing.

**What ships:** AS-386 (the sight yield) and AS-387 (the team pillar stance) as
one change on `main` @ 43a8ee54, with the yield's rule 3 reading the stance.

**What is held:** AS-388 (the Priest's scream peel walk). It is a separate
follow-up: the Priest's loss and the heal-line rise in the last section are
diagnosed there before it merges. Chain order against Pillar waits on a
hand-off count (AS-391).

## Banks

The cells are AS-373's head-to-head set, on Nagrand (PillaredArena) and
TwinPillars, camp comp included, at **fresh seeds 501-504**:

- every 2v2 team with a healer against every 2v2 team (no double healers);
- the six one-healer 3v3 teams against each other, in both orders.

That is 1,260 cells and 5,040 matches per run.

- Generator: `sweeps/2026-10-24-as385-package-gen.py`.
- Scorer: `sweeps/2026-10-24-as385-package-score.py`, paired at identical seeds, McNemar on flips.

| arm | what is paired | CSVs (`sweeps/2026-10-24-as385-pkg-*.csv.gz`) |
|---|---|---|
| (a) TeamPlan vs Legacy | the TeamPlan side's result, main against the package, both seats | `main-legacy-h2h-t{1,2}`, `ab-legacy-h2h-t{1,2}` |
| (b) package vs main, both TeamPlan | every side's result, same file on both binaries | `main-uniform`, `ab-uniform` |
| (b') design §7.1 head-to-head | a package team against a team with the yield off and Pillar withheld, one binary | `ab-h2h-{off,t1,t2}` |
| (c) variant | rule 1 leaves the healer's cast out (`team{1,2}_yield_healer_cast: false`) | `ab-variant-legacy-h2h-t{1,2}`, `ab-variant-uniform` |

```bash
G=docs/design/balance/sweeps/2026-10-24-as385-package-gen.py
$G legacy-h2h t1 501-504 > lh-t1.jsonl          # and t2; --variant for (c)
$G uniform - 501-504 > uni.jsonl                 # (b); --variant for (c)
$G pkg-h2h off 501-504 > ph-off.jsonl            # and t1, t2
target/release/arenasim --batch lh-t1.jsonl --out lh-t1.csv --jobs 5 --trace-mode off
S=docs/design/balance/sweeps/2026-10-24-as385-package-score.py
$S seat main-lh-t1.csv ab-lh-t1.csv main-lh-t2.csv ab-lh-t2.csv
$S uniform main-uni.csv ab-uni.csv
```

**Non-vacuity.**

- Every arm paired 10,080 sides, with 0 unpaired and 0 errored rows. Flip counts are in each table.
- The DPS-only sides in (a) are a built-in control: they have no healer, so neither the yield nor the stance runs for them. They show **0 flips in 1,600**.
- The shipped branch reproduces the measured binary byte for byte: arms (a) and (b), 15,120 matches, give identical CSVs.

## (a) TeamPlan against Legacy, main and package

The TeamPlan side's level: **main 51.5%, package 52.1%**.

| slice | n | main | package | delta | +/- flips | z |
|---|---|---|---|---|---|---|
| all | 10080 | 51.5 | 52.1 | **+0.6** | 432/372 | **+2.12** |
| Nagrand | 5040 | 50.8 | 51.5 | +0.7 | 217/184 | +1.65 |
| TwinPillars | 5040 | 52.3 | 52.8 | +0.5 | 215/188 | +1.34 |
| 2v2 | 9600 | 51.5 | 52.2 | +0.7 | 369/303 | +2.55 |
| 3v3 | 480 | 52.3 | 51.0 | -1.2 | 63/69 | -0.52 |
| healer Druid | 2160 | 51.1 | 52.4 | +1.2 | 120/93 | +1.85 |
| healer Paladin | 2080 | 58.9 | 59.3 | +0.4 | 62/54 | +0.74 |
| healer Priest | 2160 | 55.2 | 54.3 | -0.9 | 114/134 | -1.27 |
| healer Shaman | 2080 | 51.1 | 53.3 | **+2.2** | 136/91 | **+2.99** |
| DPS Hunter | 2400 | 36.5 | 37.1 | +0.6 | 75/61 | +1.20 |
| DPS Mage | 2400 | 53.3 | 54.4 | +1.1 | 99/72 | +2.06 |
| DPS Rogue | 2480 | 63.2 | 63.5 | +0.2 | 139/133 | +0.36 |
| DPS Warlock | 2400 | 45.4 | 45.2 | -0.1 | 38/41 | -0.34 |
| DPS Warrior | 2480 | 50.2 | 50.6 | +0.4 | 144/134 | +0.60 |
| TwinPillars Shaman | 1040 | 51.5 | 54.5 | +3.0 | 77/46 | +2.80 |
| TwinPillars Mage | 1200 | 57.2 | 58.5 | +1.2 | 58/43 | +1.49 |

## (b) Package against main, both TeamPlan

Both sides change, so the overall row is zero-sum (624/625 flips). Per class,
this is the class's standing moving in a TeamPlan-only world:

| slice | n | main | package | delta | +/- flips | z |
|---|---|---|---|---|---|---|
| healer Druid | 2160 | 51.2 | 52.7 | +1.6 | 173/139 | +1.92 |
| healer Paladin | 2080 | 60.3 | 60.0 | -0.3 | 131/137 | -0.37 |
| healer Priest | 2160 | 52.9 | 53.1 | +0.1 | 136/133 | +0.18 |
| healer Shaman | 2080 | 45.7 | 45.0 | -0.7 | 121/135 | -0.88 |
| DPS Hunter | 2400 | 37.5 | 39.3 | +1.8 | 171/128 | +2.49 |
| DPS Mage | 2400 | 47.6 | 45.9 | **-1.7** | 114/155 | **-2.50** |
| DPS Rogue | 2480 | 58.8 | 58.3 | -0.5 | 155/167 | -0.67 |
| DPS Warlock | 2400 | 44.8 | 43.7 | -1.1 | 114/140 | -1.63 |
| DPS Warrior | 2480 | 47.4 | 48.1 | +0.7 | 183/166 | +0.91 |
| TwinPillars Mage | 1200 | 50.8 | 47.4 | **-3.3** | 76/116 | **-2.89** |
| TwinPillars Shaman | 1040 | 46.0 | 44.0 | -1.9 | 68/88 | -1.60 |

**(b') Head-to-head, a package team against a team with neither piece:**

| slice | delta | z |
|---|---|---|
| overall | +0.7 | +1.99 |
| Nagrand | +1.0 | +2.12 |
| TwinPillars | +0.4 | +0.77 |
| Priest | +2.5 | +2.65 |
| Shaman | +1.1 | +1.09 |
| Paladin | -0.3 | -0.73 |
| TwinPillars Mage | +3.4 | +3.10 |

The Mage gains when its own team runs the package (b'). It loses ground only
when both teams run it (b). The enemy team's yield and stance are what cost the
TwinPillars Mage, not its own.

## (c) The variant: the healer's cast left out of rule 1

The variant is near-identical to the package:

- **Against Legacy:** 41 flips in 10,080 sides, +0.1 (z +1.09).
- **Uniform:** 32 flips, 16 each way.
- **Mechanism metrics:** within half a point of the package's.

A healer that is casting is nearly always also pressured (rule 2), or its DPS
is hurt (rule 4). So removing the cast changes which rule names the yield, not
where anyone stands. The shipped rule keeps the cast. The switch stays only
for measurement.

## The bars

| bar | figure | verdict |
|---|---|---|
| AS-373 TwinPillars -0.8pt (z -2.37) | (a) +0.5 (z +1.34); (b') +0.4 | met |
| AS-373 Shaman -1.3pt (z -2.00) | (a) +2.2 (z +2.99); TwinPillars Shaman +3.0 | met |
| no significant loss overall, per map or per healer (§7.1) | (a) all slices ns or positive; Priest -0.9 (z -1.27) | met |
| Mage lining on TwinPillars (AS-386 +2.4pt) | lining share 25.1% → 26.9% (+1.8pt); (b) TwinPillars Mage -3.3 (z -2.89); (b') +3.4 | **not met**; follow-up |
| camp comp (`camp_sweep`, seeds 1-12) | TeamPlan 11/12, heal to the Warrior 323, occlusion pre/post 15.1/20.8s (AS-383: 10/12, 157) | met |
| Chain displacement | the stance takes 8.8% of the Druid's and 2.8% of the Mage's chain-window decisions | open: AS-391 counts hand-offs |

## Mechanism metrics (§7)

Run with `tests/team_sight_cohesion.rs` on the uniform `TeamPlan` bank (seeds
501-502, 2,520 matches per arm); main ran the same harness at 43a8ee54.

- Rows: `sweeps/2026-10-24-as385-pkg-rows-{main,ab,ab-variant}.jsonl.gz`
- Reducer: `sweeps/2026-10-24-as385-reduce.py`
- Chain: `sweeps/2026-10-24-as385-package-chain.py`

| piece | metric | main | package | target |
|---|---|---|---|---|
| yield | DPS blind to its healer, 2v2 Nagrand / TwinPillars | 12.3% / 10.9% | 11.8% / 9.8% | halved: **not met** |
| yield | same, 3v3 | 4.2% / 19.6% | 2.6% / 12.3% | — |
| yield | blind while dying, Nagrand 2v2 | 8.9% | 7.8% | at most 1.2%: **not met** |
| yield | episodes broken by the DPS's step, 2v2 | 78% / 55% | 78% / 52% | below 50%: **not met** |
| stance | healer hidden while a DPS is exposed, 2v2 | 10.9% / 23.6% | 11.0% / 22.9% | halved: **not met** |
| stance | same, 3v3 | 5.8% / 20.3% | 4.7% / 16.0% | — |
| stance | all hidden and mutually sighted, 2v2 | 27.2% / 26.0% | 29.0% / 28.3% | up: met |
| stance | pillar deaths with the healer alive, Nagrand / TwinPillars | 23 / 54 | 25 / 39 | down overall: met (77 → 64) |
| camp | heal-line probe, goal blind (48 seeds, `maps.ron` geometry) | 10.2% | 10.9% | under the 30% ceiling |

**What the metrics show:**

- **3v3 sight and pillar deaths move.** On TwinPillars 3v3, blindness falls from 19.6% to 12.3%, and pillar deaths fall from 12 to 5.
- **2v2 blindness barely moves.** A healthy DPS still leads under rule 5. Making it follow is the lever if the halving target stands.

**Chain displacement.** Each class's decisions taken inside a Chain window
(`situation.lock_ending`):

| class | main: under Chain | package: under Pillar | package: under Chain |
|---|---|---|---|
| Druid | 82.2% | 8.8% | 74.8% |
| Mage | 97.0% | 2.8% | 94.6% |
| Priest / Hunter / Shaman | — | 7.4% / 5.9% / 4.4% | — |

Pillar sits ahead of Chain in `PLAY_ORDER`. So while its team holds a stance, a
Druid or Mage with a hand-off plays Pillar, and its hand-off is no longer at
the head of its chain. These are decision ticks, not hand-offs lost. AS-391
counts the casts before the order is decided.

## Legacy byte-identity

All 1,260 cells of the bank at seed 501 ran with both teams `Legacy`, as full
`--headless` match logs on main and on the shipped branch. `diff -r` is clean
over 29.5MB of logs. The bank is not vacuous:

- 3,577 deaths;
- 7,078 crit lines;
- 996 distinct durations;
- winners: team 1 692, team 2 552, 16 draws.

## The held piece: AS-388

AS-388 (the Priest's scream peel walk) was measured on the same banks, on top
of this package. It turns arm (a)'s Priest from -0.9 (ns) into **-2.1
(z -2.40)**. It also doubles the heal-line probe's goal-blind share, from
10.9% to 20.8% over the same 48 seeds. It merges after a diagnosis of both.
