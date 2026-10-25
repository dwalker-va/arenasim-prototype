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

Generator: `2026-10-25-as398-peel3-gen.py <arm> <seeds>` (`ONLY=` the classes
whose running rows to keep).

## 2. What the build changes, attributed

Graduating a play changes nothing a match does: it only takes away the
withhold switch. So AS-398's build should play exactly as main does with
TakeCover withheld from both teams and Peel's table extended by the four new
gates, written in main's own controls. It does.

Bank (`2026-10-25-as398-h2h-gen.py`): 2v2, every team of two distinct classes
with at most one healer against every such team (30 × 30, both orders), three
maps, seeds 901-902 / 911-912 / 921-922; 3v3, the 0.8 sweep's `sample3`, three
maps, seeds 901 / 911 / 921. 21,600 matches an arm, both teams `TeamPlan`.

| arms | rows | differing | decisive | distinct durations |
|---|---|---|---|---|
| AS-398 `plain` against main `sim` | 21,600 | **0** | 21,449 | 5,816 |

Every change in the build is therefore one of the two ruled changes. Against
main's own `plain` arm, the rows each can reach:

| rows the change can reach | rows | changed | winner changed |
|---|---|---|---|
| 2v2, TakeCover only | 9,408 | 4,025 | 125 |
| 2v2, context gates | 2,088 | 862 | 66 |
| 2v2, BasicArena, no gate | 4,704 | 3 | 0 |
| 3v3, TakeCover only | 318 | 210 | 3 |
| 3v3, context gates | 396 | 246 | 16 |
| 3v3, format and context gates | 1,359 | 932 | 93 |
| 3v3, format gates | 3,168 | 2,197 | 266 |
| 3v3, BasicArena, no gate | 159 | 0 | 0 |

The three BasicArena rows with no gate are TakeCover too, positively
attributed: TakeCover's movement was inert without obstacles, but its place
in the walk was not. A last-stand healer's play was TakeCover, which held it
off the Chain play below it. In `Mage+Rogue` against `Warlock+Druid`, seed
911, main's trace has the Druid's play `take_cover` from the Warlock's death
on (133 trace events); AS-398's has it Cyclone the Mage as a chain hand-off
at the same moment, and the Druid lives 5.9s longer. The second row is the
same cell with the seats swapped. The third, `Mage+Shaman` against
`Warlock+Paladin`, seed 922, is a lone Paladin: from the same frame on, main
traces `take_cover` and AS-398 the Chain step holding the global cooldown for
a hand-off.

## 3. Head-to-head against main

One team runs AS-398's behaviour (main's `sim1` / `sim2` arms: the `sim`
controls on that team only), the other main's; each side is scored against
the same side in main's all-shipped `plain` twin, both seats (the 0.8 sweep's
method). Pooled 2v2 and 3v3, 43,200 sides:

| slice | n | main | AS-398 | effect | per block | +/− | z |
|---|---|---|---|---|---|---|---|
| **all** | 43,200 | 49.6 | 49.9 | **+0.4** | +0.4 / +0.4 / +0.3 | 353 / 186 | +7.2 |
| 2v2 competitive | 32,400 | 49.6 | 49.8 | +0.2 | +0.3 / +0.1 / +0.2 | 121 / 43 | +6.1 |
| — Basic / Nagrand / Twin | | | | +0.0 / +0.2 / +0.4 | | | |
| 3v3 competitive | 10,800 | 49.5 | 50.3 | +0.8 | +0.7 / +1.2 / +0.6 | 232 / 143 | +4.6 |
| — Basic / Nagrand / Twin | | | | +0.4 / +0.6 / +1.5 | | | |
| 2v2, teams with a Paladin | 5,400 | 63.9 | 65.0 | +1.1 | +1.7 / +0.3 / +1.2 | 84 / 25 | +5.7 |
| 3v3, teams with a Mage | 3,717 | 52.5 | 53.6 | +1.1 | +0.9 / +1.7 / +0.8 | 132 / 90 | +2.8 |
| 3v3, teams with a Druid | 3,978 | 42.9 | 44.3 | +1.4 | +0.9 / +2.4 / +0.9 | 154 / 98 | +3.5 |

Both seats agree (+0.4 each). No map × seat slice and no class slice is
significantly negative; the lowest, 3v3 teams with a Warrior, is −0.3 (ns).
It moves where the rulings predicted: the Paladin's team, which TakeCover
cost −1.7pt in 2v2 (0.8 sweep §3.6), and the Mage's and Druid's 3v3 teams.

## 4. Uniform, both teams on the build

The same bank, AS-398's `plain` against main's: the class standings move,
with both teams changed, by 2v2 competitive Paladin +1.0 (z=5.3, every
block) and Rogue +0.3; 3v3 competitive Druid +1.1, Paladin +0.9 and Warrior
−1.0 (each every block). Overall 2v2 and 3v3 competitive: +0.1 and +0.2, ns.

## 5. Legacy

`Legacy` is untouched: 144 matches (36 2v2 cells of every healer shape, 12 3v3
cells, each on all three maps) played through `--headless` by main and by
AS-398 give byte-identical full match logs, 144 / 144 (363 eliminations and
1,101 crit lines among them). `2026-10-25-as398-legacy-logs.py <main> <as398>
<dir>`.

## 6. What deleting TakeCover took with it

TakeCover's lone healer was the only AI that hid a lone healer at a pillar.
Three tests used it as their hider, and no AI produces their situation now:

- `movement_probes::chase_los`, the Mage's occlusion-timeout chase against a
  lone pillar hugger, is migrated to a staged match where the hugger holds by
  construction (`tests/lone_healer_chase_staged.rs`: a rooted Shaman on the
  pillar's far face; the Mage stays out of sight 9.7s, chases, and kills it).
- `tests/hunt_staged.rs` keeps its scene with the hider rooted. It pins the
  stations (the two split round the pillar in 5.3s; excepted, they never do)
  but no longer the sight gained, which needs a hider that moves away from
  its hunters.
- `movement_probes::juke_chase`, the leaky-bucket chase against a juking
  hider, cannot be staged and is retired; the bucket's arithmetic under a
  juke cadence stays pinned by
  `dps_postures::tests::juke_cadence_arms_at_shipped_decay_but_never_when_flicker_empties_the_bucket`.

The Hunt play still runs on any lone healer at a pillar (its onsets fire in
every seed of the staged match), but in AI-against-AI play nothing now hides
one there on purpose.

## Files

From `docs/design/balance/sweeps/`, with `A=2026-10-25-as398-score.py`:

| figures | command |
|---|---|
| §1 | `2026-10-25-0.8-score.py pair ...` (above), on `...-peel3-sel.csv.gz` and `...-peel3-conf.csv.gz` |
| §2 identity | `$A identity 2026-10-25-as398-main-sim.csv.gz 2026-10-25-as398-as398-plain.csv.gz` |
| §2 rows, §4 | `$A moved 2026-10-25-as398-main-plain.csv.gz 2026-10-25-as398-as398-plain.csv.gz` |
| §3 | `$A h2h 2026-10-25-as398-main-plain.csv.gz 2026-10-25-as398-main-sim1.csv.gz 2026-10-25-as398-main-sim2.csv.gz` |
