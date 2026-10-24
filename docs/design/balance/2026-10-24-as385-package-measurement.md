# AS-385 package measurement: sight yield + pillar stance + scream peel walk

**Tier: DIRECTIONAL** (`sweep-tiers.md`). The package is judged on win rate as a
whole (design D6), with per-piece mechanism metrics beside it. Nothing here may
be cited as a class's standing.

- **Package:** `card/AS-385-package` — AS-386 (sight yield, b7ebcc79), AS-387
  (pillar stance, 5ecee322) and AS-388 (scream peel walk, 5b18cdcd) merged on
  `main` @ 43a8ee54, with the yield's rule 3 wired to the stance. The measured
  binary is 6d3d4056 (later commits touch tests and docs only).
- **Main:** 43a8ee54.
- **A+B preliminary:** the branch before AS-388 (2bd60490; binary c44b1ad2).

## Banks

AS-373's head-to-head cells: every 2v2 team with a healer against every 2v2
team (no double healers), plus the six one-healer 3v3 teams against each other
in both orders. They run on Nagrand (PillaredArena) and TwinPillars, with the
camp comp included, at **fresh seeds 501-504**. That is 1,260 cells and 5,040
matches per run. Generator:
`sweeps/2026-10-24-as385-package-gen.py`; scorer (paired, McNemar on flips):
`sweeps/2026-10-24-as385-package-score.py`.

| arm | what is paired | runs |
|---|---|---|
| (a) TeamPlan vs Legacy | the TeamPlan side's result, main against package, both seats | `legacy-h2h t1/t2` on both binaries |
| (b) package vs main, both TeamPlan | every side's result, same file on both binaries | `uniform` |
| (b') design §7.1 head-to-head | a package team against a team with the yield off and Pillar withheld, on the package binary | `pkg-h2h off/t1/t2` |
| (c) variant | rule 1 leaves the healer's cast out (`team{1,2}_yield_healer_cast: false`) | `legacy-h2h --variant`, `uniform --variant` |

(b') cannot switch AS-388 off: the dip's retirement under `TeamPlan` has no
switch, so both sides there play the peel walk. Read it as yield + stance only.

```bash
G=docs/design/balance/sweeps/2026-10-24-as385-package-gen.py
$G legacy-h2h t1 501-504 > lh-t1.jsonl          # and t2; --variant for (c)
$G uniform - 501-504 > uni.jsonl                 # (b); --variant for (c)
$G pkg-h2h off 501-504 > ph-off.jsonl            # and t1, t2
target/release/arenasim --batch lh-t1.jsonl --out lh-t1.csv --jobs 5 --trace-mode off
S=docs/design/balance/sweeps/2026-10-24-as385-package-score.py
$S seat main-lh-t1.csv pkg-lh-t1.csv main-lh-t2.csv pkg-lh-t2.csv
$S uniform main-uni.csv pkg-uni.csv
```

The CSVs are committed as `sweeps/2026-10-24-as385-pkg-*.csv.gz`.

**Non-vacuity.** Every arm paired 10,080 sides with 0 unpaired and 0 errored
rows. Flip counts are in each table. The DPS-only sides in (a) are a built-in
control: they hold no healer, so neither the yield nor the stance runs for
them, and they show **0 flips in 1,600** matches on both binaries.

## (a) TeamPlan against Legacy, main and package

The TeamPlan side's level: **main 51.5%, package 51.9%** (A+B alone 52.1%).

| slice | n | main | package | delta | +/- flips | z |
|---|---|---|---|---|---|---|
| all | 10080 | 51.5 | 51.9 | +0.3 | 479/445 | +1.12 |
| Nagrand | 5040 | 50.8 | 51.2 | +0.4 | 227/205 | +1.06 |
| TwinPillars | 5040 | 52.3 | 52.5 | +0.2 | 252/240 | +0.54 |
| 2v2 | 9600 | 51.5 | 52.0 | +0.5 | 416/372 | +1.57 |
| 3v3 | 480 | 52.3 | 50.2 | -2.1 | 63/73 | -0.86 |
| healer Druid | 2160 | 51.1 | 52.4 | +1.2 | 120/93 | +1.85 |
| healer Paladin | 2080 | 58.9 | 59.3 | +0.4 | 62/54 | +0.74 |
| healer Priest | 2160 | 55.2 | 53.1 | **-2.1** | 161/207 | **-2.40** |
| healer Shaman | 2080 | 51.1 | 53.3 | **+2.2** | 136/91 | **+2.99** |
| DPS Hunter | 2400 | 36.5 | 37.0 | +0.5 | 80/67 | +1.07 |
| DPS Mage | 2400 | 53.3 | 54.1 | +0.8 | 116/97 | +1.30 |
| DPS Rogue | 2480 | 63.2 | 63.4 | +0.2 | 136/132 | +0.24 |
| DPS Warlock | 2400 | 45.4 | 45.2 | -0.2 | 62/66 | -0.35 |
| DPS Warrior | 2480 | 50.2 | 49.8 | -0.3 | 148/156 | -0.46 |
| TwinPillars Shaman | 1040 | 51.5 | 54.5 | +3.0 | 77/46 | +2.80 |
| Nagrand Priest | 1080 | 51.5 | 49.4 | -2.0 | 63/85 | -1.81 |
| TwinPillars Priest | 1080 | 59.0 | 56.8 | -2.2 | 98/122 | -1.62 |

**The Priest loss is AS-388's.** A+B alone measured Priest -0.9 (z -1.27). Its
other slices match the full package: Shaman +2.2, Druid +1.2, overall +0.6
(z +2.12).

## (b) Package against main, both TeamPlan

Both sides change, so the overall row is zero-sum (+0.0, 724/722 flips). Per
class, this is the class's standing moving in a TeamPlan-only world:

| slice | n | main | package | delta | +/- flips | z |
|---|---|---|---|---|---|---|
| healer Druid | 2160 | 51.2 | 51.7 | +0.5 | 174/163 | +0.60 |
| healer Paladin | 2080 | 60.3 | 60.0 | -0.3 | 146/152 | -0.35 |
| healer Priest | 2160 | 52.9 | 53.8 | +0.8 | 202/184 | +0.92 |
| healer Shaman | 2080 | 45.7 | 45.6 | -0.1 | 139/142 | -0.18 |
| DPS Hunter | 2400 | 37.5 | 38.3 | +0.8 | 161/142 | +1.09 |
| DPS Mage | 2400 | 47.6 | 45.5 | **-2.1** | 151/201 | **-2.67** |
| DPS Rogue | 2480 | 58.8 | 58.1 | -0.7 | 160/177 | -0.93 |
| DPS Warlock | 2400 | 44.8 | 43.9 | -0.9 | 132/153 | -1.24 |
| DPS Warrior | 2480 | 47.4 | 49.5 | **+2.1** | 244/191 | **+2.54** |
| TwinPillars Mage | 1200 | 50.8 | 46.2 | **-4.5** | 95/149 | **-3.46** |
| TwinPillars Shaman | 1040 | 46.0 | 44.7 | -1.2 | 79/92 | -0.99 |

**(b') Head-to-head, yield + stance against neither** (package binary): overall
+0.7 (z +1.95); Nagrand +1.4 (z +2.88); TwinPillars +0.0 (z +0.04); Priest
+1.8 (z +1.97); Shaman +1.5 (z +1.48); Paladin -0.4 (z -0.91); TwinPillars Mage
+2.1 (z +1.85).

## (c) The variant: the healer's cast left out of rule 1

The variant is near-identical to the package:

- **Against Legacy:** 38 flips in 10,080 sides, +0.1 (z +1.30).
- **Uniform:** 44 flips, 22 each way.
- **Mechanism metrics:** match the package's to within 0.5pt.

Removing the cast changes which rule names the yield. It barely changes where
anyone stands: a healer that casts is usually also pressured (rule 2), or its
DPS is hurt (rule 4).

## The bars

| bar | figure | verdict |
|---|---|---|
| AS-373 TwinPillars -0.8pt (z -2.37) | (a) +0.2 (z +0.54); (b') +0.0 | met: at or above zero |
| AS-373 Shaman -1.3pt (z -2.00) | (a) +2.2 (z +2.99); TwinPillars Shaman +3.0 | met |
| Mage lining on TwinPillars (AS-386 +2.4pt) | lining share 25.1% → 26.4% (+1.3pt); (b) TwinPillars Mage standing **-4.5pt (z -3.46)** | **not met**: lining is over the 1pt bar, and the uniform world costs the TwinPillars Mage |
| camp comp (`camp_sweep`, seeds 1-12) | TeamPlan 12/12, heal to Warrior 301, occlusion pre/post 15.0/12.1s (AS-383: 10/12, 157) | met: heal recovers toward 307 |
| Chain displacement | the stance now takes 8.8% of the Druid's and 3.1% of the Mage's chain-window decisions (AS-387 tester: 0) | **a finding**: below |
| no significant loss in any healer class (§7.1) | (a) Priest -2.1 (z -2.40) | **not met**: the loss is AS-388's |

## Mechanism metrics (§7)

Run with `tests/team_sight_cohesion.rs` on the uniform `TeamPlan` bank, seeds
501-502, 2,520 matches per arm. The main arm ran the same harness on
43a8ee54. Rows: `sweeps/2026-10-24-as385-pkg-rows-{main,package,variant}.jsonl.gz`.
Reducer: `sweeps/2026-10-24-as385-reduce.py`. Chain:
`sweeps/2026-10-24-as385-package-chain.py`.

| piece | metric | main | package | target |
|---|---|---|---|---|
| yield | DPS blind to its healer, Nagrand 2v2 / TwinPillars 2v2 | 12.3% / 10.9% | 12.1% / 9.9% | halved: **not met** |
| yield | same, 3v3 Nagrand / TwinPillars | 4.2% / 19.6% | 2.0% / 11.0% | — |
| yield | blind while dying, Nagrand 2v2 | 8.9% | 9.2% | ≤ 1.2%: **not met** |
| yield | episodes broken by the DPS's step, Nagrand / TwinPillars 2v2 | 78% / 55% | 79% / 53% | below 50%: **not met** |
| stance | healer hidden while a DPS is exposed, Nagrand / TwinPillars 2v2 | 10.9% / 23.6% | 10.9% / 22.9% | halved: **not met** |
| stance | same, 3v3 | 5.8% / 20.3% | 3.7% / 14.8% | — |
| stance | all hidden and mutually sighted, 2v2 | 27.2% / 26.0% | 29.4% / 28.0% | up: met |
| stance | pillar deaths (healer alive), Nagrand / TwinPillars | 23 / 54 | 28 / 37 | down overall: met (77 → 65); Nagrand +5 |
| peel walk | heal-line probe, goal blind (48 seeds, maps.ron geometry) | 10.2% [7.0, 13.0] | 20.8% [15.9, 26.9] | ceiling 40%; **doubled** |

**What the numbers say:**

- The yield moves 3v3 sight (TwinPillars 19.6% → 11.0%) and pillar deaths
  (TwinPillars 54 → 37). It barely moves 2v2 blindness: a healthy DPS still
  leads under rule 5, as AS-386 reported.
- The heal-line doubling is the peel walk's. A+B measured 10.9% over seeds
  1-24, and AS-388 reported 8.9% → 15.6% on the old geometry.

**Chain displacement.** These are the decisions, per class, taken inside a
Chain window (`situation.lock_ending`):

| class | main: under Chain | package: under Pillar | package: under Chain |
|---|---|---|---|
| Druid | 82.2% | 8.8% | 76.0% |
| Mage | 97.0% | 3.1% | 94.6% |
| Priest / Hunter / Shaman | — | 7.4% / 5.0% / 4.0% | — |

Pillar sits ahead of Chain in `PLAY_ORDER`, so a Druid or Mage that holds a
hand-off plays Pillar instead whenever its team holds a stance. Its hand-off is
then no longer at the head of its chain. These counts are decision ticks, not
hand-offs lost. Whether a Cyclone or Polymorph was actually withheld needs a
cast-level count (follow-up).

## Legacy byte-identity

All 1,260 cells of the bank at seed 501, both teams `Legacy`, ran as full
`--headless` match logs on main and on the package binary. That is
29.5MB of logs, `diff -r` clean. The bank is not vacuous:

- 3,577 deaths;
- 7,078 crit lines;
- 996 distinct durations;
- winners 692 / 552 / 16 draws.
