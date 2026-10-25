# AS-398: the 0.8 play rulings applied

**Card:** AS-398. **Tier: DIRECTIONAL** (`sweep-tiers.md`): the cells the
change reaches, three seed blocks. It applies the user's rulings on the 0.8
milestone sweep (`2026-10-25-0.8-milestone-sweep.md`, AS-395) and measures
only what they change.

**The change.** Peel, Hunt, Chain and Burst graduate: permanent under
`TeamPlan`, no longer withholdable (`PlayId::is_gated`); `Legacy` is frozen.
TakeCover and its `last_stand` situation are deleted. Peel gains two context
gates (Paladin@Hunter, Hunter@Priest, from the 0.8 sweep §3.1) and two 3v3
FORMAT gates (Mage@3v3, Druid@3v3, §1 below). Pillar stays gated.

## 1. Peel in 3v3: the Mage and the Druid

The 0.8 sweep found Peel costing 3v3 teams with a Mage (−1.5pt) and with a
Druid (−1.1pt), every block negative (§3.1 there). Those are TEAM slices: any
member's peel could be the cost. Here each class's own peel is switched off
in 3v3 and the team measured against the same team with it, so the delta is
that class's peel.

Design: the 0.8 sweep's 600-cell `sample3`, all three maps, both teams
`TeamPlan`; PN (team 1 runs Peel, team 2 has it withheld) and NP, scored from
the running side and paired with the shipped arm at the same cell and seed.
The arm adds every peel of the class (`Mage/<every class>`) to the running
side's exceptions; in a 3v3-only bank that is exactly "the class does not
peel in 3v3". Run on main at `9050d193` (`--batch`, 300s cap). Selection:
the 0.8 sweep's 3v3 seeds 801 / 811 / 821 (where the team effect was found).
Confirmation: fresh seeds 871-872 / 873-874 / 875-876, the rows whose running
team holds the class.

| gate | selection (gated − shipped) | per block | flips for / against peeling | confirmation | per block | flips | pooled | AS-290 bar |
|---|---|---|---|---|---|---|---|---|
| **Mage@3v3** | +1.0 (teams with a Mage) | +0.8 / +1.4 / +1.0 | 31 / 70 (z=−3.9) | +0.6 | +0.8 / +0.2 / +0.7 | 98 / 141 (z=−2.8) | 129 / 211, z=−4.4 | **met: add** |
| **Druid@3v3** | +1.0 (teams with a Druid) | +1.6 / +1.1 / +0.3 | 92 / 131 (z=−2.6) | +1.3 | +0.9 / +1.6 / +1.3 | 186 / 286 (z=−4.6) | 278 / 417, z=−5.3 | **met: add** |

Every block of both runs agrees in sign. The only negative map slice is the
Mage's Nagrand in confirmation, −0.1 (29 / 32, ns).

A format gate is a new key shape (`ExceptionKey::in_format`, config spelling
`"Mage@3v3"`): the peeler's class on a team of three, in any context. It is
re-checked like any gate, by lifting the table (`"team1_exceptions":
{"Peel": []}`). What remains for 3v3 — the per-context selection, healer
allies included — is AS-327's.

Recompute (from `docs/design/balance/sweeps/`, `S=2026-10-25-0.8-score.py`):

```bash
$S pair "peel3-Mage-PN:peel3---PN:1,peel3-Mage-NP:peel3---NP:2" 2026-10-25-as398-peel3-sel.csv.gz
$S pair "peel3-Druid-PN:peel3---PN:1,peel3-Druid-NP:peel3---NP:2" 2026-10-25-as398-peel3-sel.csv.gz
# the same two on 2026-10-25-as398-peel3-conf.csv.gz
```

Generator: `2026-10-25-as398-peel3-gen.py <arm> <seeds> <repo>` (`ONLY=` the
classes whose running rows to keep).
