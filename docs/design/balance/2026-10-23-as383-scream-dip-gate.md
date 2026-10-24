# AS-383: the TeamPlan scream-dip gate

**Tier: DIRECTIONAL.** Reachable-cell head-to-head, scored from the `TeamPlan`
side, on fresh seeds. Not a class standing.

## The change

Under `TeamPlan`, the Priest's offensive Psychic Scream dip does not start in
either of two cases (`class_ai::priest::dip_hold`):

- the spot it would end on, scream range short of the dip target, is inside
  the disc of a ready enemy hard CC (`cc_reach::CcField`), excluding the dip
  target's own;
- the dip target already targets the Priest from within `threat_intent_radius`.

`Legacy` is unchanged.

**Why.** Diagnosis is on branch `card/AS-383-open-field-priest-cc`. AS-292
stopped the closing enemy healer from cutting the dip short with PRESSURED. The
dip then ran into the enemy DPS's Fear and Polymorph reach, which is where every
added landing came from. CcField read those threats correctly; the dip was the
one walk that never consulted it.

## Head-to-head

Data: `sweeps/2026-10-23-as383-gen.py 20 4 1000`, which is AS-292's cells at
seeds 1000-1019, with 4 Legacy-vs-Legacy control seeds per cell. That is 19,800
matches per arm. Before is `main` cd0cdf3d, after is this branch. CSVs:
`sweeps/2026-10-23-as383-h2h-{before,after}.csv.gz`. Scored with
`sweeps/2026-10-19-as292-h2h.py`.

| slice | n | before | after | delta | z |
|---|---|---|---|---|---|
| all | 18000 | 51.0% | 51.8% | +0.78pt | +4.55 |
| BasicArena | 9000 | 51.7% | 53.6% | +1.90pt | +7.23 |
| PillaredArena | 9000 | 50.4% | 50.0% | -0.34pt | -1.57 |
| TeamPlan Priest | 6000 | 48.8% | 51.2% | +2.33pt | +4.55 |
| TeamPlan Shaman, Druid | 12000 | | | 0 flips | |
| B Priest v Shaman | 1000 | 42.2% | 52.2% | +10.0pt | +6.68 |
| B Priest v Druid | 1000 | 52.9% | 56.9% | +4.0pt | +4.31 |
| B Priest v Priest | 1000 | 48.0% | 51.1% | +3.1pt | +1.96 |
| P Priest v Shaman | 1000 | 59.5% | 56.3% | -3.2pt | -2.63 |
| P Priest v Druid | 1000 | 44.1% | 42.9% | -1.2pt | -1.36 |
| P Priest v Priest | 1000 | 46.3% | 47.6% | +1.3pt | +1.02 |

## Legacy is byte-identical

- **Control rows.** All 1,800 Legacy-vs-Legacy rows are identical: 1,770 ended
  by a kill, with 1,217 distinct durations.
- **Rows with no TeamPlan Priest.** All 12,000 are identical. 4,000 of them have
  a Legacy Priest.
- **Full match logs.** 300 Legacy matches (DPS+Priest v DPS+{Shaman, Priest,
  Druid}, both maps, seeds 2000-2001) give byte-identical logs: 74,064 lines,
  119 team-1 Psychic Screams and 2,247 crit lines.

## Logscan

`2026-10-19-as292-logscan.py` cells (DPS+Priest TeamPlan v DPS+healer Legacy)
at seeds 1000-1007, 200 matches per arm. Values are per match, main → this
branch.

| enemy healer, map | Fear | Polymorph | Frost Shock | heal | win |
|---|---|---|---|---|---|
| Shaman, Basic | .20 → .06 | .23 → .01 | 1.22 → .80 | 258 → 370 | 42 → 51% |
| Shaman, Pillared | .12 → .08 | .09 → .01 | 1.56 → 1.07 | 328 → 371 | 60 → 55% |
| Druid, Basic | .12 → .10 | 0 → 0 | — | 521 → 524 | 49 → 55% |
| Druid, Pillared | .06 → .05 | .07 → .01 | — | 464 → 467 | 44 → 43% |

## camp_sweep

The data is Warrior+Priest v Warlock+Priest on Nagrand, seeds 1-12, with both
teams under `TeamPlan`.

| | main | this branch |
|---|---|---|
| wins | 12/12 | 9/12 |
| heal to the Warrior | 307 | 137 |
| Priest hard-CC'd | 6.0s | 8.1s |
| heal line occluded | 33% | 20% |
| Warlock denied the Priest, after contact | 12.3s | 8.8s |

- The matches split 2 for main and 1 for this branch, with 3 discordant pairs.
  That is below what this file treats as conclusive.
- The head-to-head points the same way for this comp on Nagrand: 39 of 40 wins
  become 34 of 40 across both seats.
- **Mechanism.** These figures are from AS-383-test's and AS-383-test-r2's
  traces of all 12 seeds.
  - On `main` all 72 camp dips end in PRESSURED, and none screamed (0
    `DipComplete`).
  - The opening dip of every match lasts 4.85s: `DipEnter` at 15.08s,
    PRESSURED at 19.93s. The 12 opening dips account for 58.2s of the 102s
    spent in DIP.
  - The other 60 mostly end within about 1.5s: 30 under 0.5s and 27 between
    0.5 and 1.5s. Three outliers run 1.62s, 2.62s and 8.78s. (Seed 4's third
    dip, for example, runs 23.42s to 23.95s.)
  - So on Nagrand the dip is a walk toward the enemy Priest that tips the
    healer into PRESSURED, where the team solve positions it.
  - The gate removes that path and leaves the Priest in FREE. Per-match
    posture time, counted from the gates, goes from FREE 14.4s / DIP 8.5s /
    PRESSURED 24.2s on `main` to FREE 31.1s / PRESSURED 23.5s on the branch.
  - Of the branch's 40 held dips, 26 are held because the target is already
    on the Priest and 14 because of the Fear.
  - The Warlock fears on `main` come from the DEFENSIVE Scream cast in
    PRESSURED: 2 casts across the 12 seeds, in seeds 4 and 11. The branch
    screams more, with 9 fear landings against 4.
- **Variant tried, not shipped.** I also exempted any CC source standing
  inside scream range of the arrival. That changed only the Fear clause, so
  the 26 dips held by the targeting rule stayed held, and it recovered
  nothing: 9/12, heal 120. The head-to-head was otherwise the same: +0.81pt
  overall, P -0.27pt.

## Heal-line probe migrated

`movement_probes::teamplan_healer_keeps_its_heal_line_on_nagrand` pinned seeds
7 / 2 / 12, and seed 2 lost every PRESSURED frame: the dip had been ending in
PRESSURED. The probe was migrated to a bank of seeds 1-24, not re-pinned.

- The pooled ceiling is 30%. This branch measures 19.2% over 4,815 frames. With
  `C_SIGHT` removed it measures 44.7%.
- Any seed with at least 300 frames is held to 50%. The `C_SIGHT` mutant's seeds
  6 and 12 read 74% and 80%.
- The bank must hold at least 3,000 frames.
