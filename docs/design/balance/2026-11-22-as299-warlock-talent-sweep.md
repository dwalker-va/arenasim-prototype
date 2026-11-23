# AS-299: Warlock reference builds and node arms

Tier: **directional**. Success condition (design §4, "Sweeps measure nodes and
reference builds"): **no node dominates** — none moves the Warlock by more than
the run's resolution floor beyond a stat rank of the same price.

**Verdict: the condition fails on one node, clearly, and touches it on a
second.** Soul Link beats a Demonic Embrace rank by +6.0pt (pooled over two
seed blocks, z=15.8), against an allowance of +1.4pt for the second stat rank
its capstone price buys: +4.6pt over the line, with a 0.7pt floor. Bane's
first rank beats the stat rank by +0.9pt pooled, 0.2pt over its 0.7pt floor
(z=2.6) — significant on its own, not after allowing for the 13 node
comparisons, and not significant in the second block alone. Six nodes sit
significantly *below* a stat rank, one of them (Demonic Sacrifice) by 21pt.
Nothing here is retuned: tuning is AS-441's batch.

Measured at `main` be76925a plus this card's schema change (which leaves
untalented and map-written builds byte-identical; see *Byte-identity*).

## Method

**The reference builds** (`assets/config/talents.ron`, `reference_builds`),
each about 99 points of item budget:

| name | shape | ranks | price |
|---|---|---|---|
| Affliction | 6/0/2 | Improved Drain Life 2, Nightfall 2, Siphon Life 1, **Shadow Mastery** 1; Bane 2 | 99 |
| Demonology | 2/6/0 | Improved Drain Life 2; Demonic Embrace 1, Fel Stamina 2, Unholy Power 1, Master Demonologist 1, **Soul Link** 1 | 99.7 |
| Destruction | 0/2/6 | Fel Stamina 2; Improved Shadow Bolt 2, Bane 2, Improved Immolate 1, **Conflagrate** 1 | 99 |

Each reference's last point is its capstone. Its **stem** is the reference
without it (7 points). A **node arm** is a stem with its eighth point on the
node; its **comparison arm** is the same stem with that point on **Demonic
Embrace** (+3% maximum health, the Warlock's one computed `StatPercent`
node: 11.67 a rank at the reference combatant, against the 11 every other
non-capstone rank declares). A node arm and its comparison differ by exactly
one point, so their paired difference is the node's worth over a stat rank of
about its price. A capstone is priced at two ranks, so it may beat one stat
rank by what a second buys on the same stem (`stem + DE` against `stem`).

| node | its arm | comparison | notes |
|---|---|---|---|
| Amplify Curse, Curse of Exhaustion | Affliction stem + node | Affliction stem + DE | Exhaustion arm chooses Curse of Exhaustion on both enemies |
| Siphon Life | Affliction stem + DE | the same with Siphon Life's point on DE too | Siphon Life is Shadow Mastery's prerequisite, so it is measured by removal |
| Soul Link (capstone), Demonic Sacrifice, Nightfall, Improved Shadow Bolt, Bane | Demonology stem + node | Demonology stem + DE | Soul Link's arm is the reference; the Sacrifice arm chooses Sacrifice; each tier-1/2 rank is the node's first |
| Conflagrate (capstone), Shadowburn, Aftermath, Master Demonologist | Destruction stem + node | Destruction stem + DE | Conflagrate's arm is the reference |

Every arm is one directional 2v2 sweep: every cell a Warlock can reach plus 8
control cells (`gen_sweep.py --full 2 --exclude-double-healer --affects
Warlock --control-cells 8`, 424 cells), on **BasicArena, PillaredArena and
TwinPillars**, 4 seeds a cell a map — 5,088 matches an arm, under
**`TeamPlan`** on both sides (what players get). Every Warlock, on either
side, plays the arm's build, so the arms differ only in their builds and pair
at identical (label, seed). Scored for the side fielding the Warlock, over the
4,224 matches where one side does (`paired_sweep.py --headline affected`); the
768 Warlock mirrors are excluded from the score. 2v2 only: the directional
tier does not call for 3v3.

Twenty arms — 0 points, the three references, their three stems, the three
stem + DE comparisons, and ten node arms (a capstone's arm is its reference) —
and a second seed block (seeds 4-7) of the four arms behind the two
comparisons the first block put over the line and the stat rank Soul Link's
allowance reads. 122,112 matches.

## Results

In every table "before" is the comparison arm and "after" the arm under test,
both as the Warlock side's win rate; flips are matches whose winner moved (+
toward the Warlock side, − away); the floor is the smallest delta the pair's
own discordance could resolve.

### Each node against a stat rank in its place

| node | before | after | delta (pt) | flips | z | floor (pt) | verdict |
|---|---|---|---|---|---|---|---|
| **Soul Link** (capstone; allowance +2.0 in block 1, +1.4 pooled, which the verdict uses) | 48.9% | 54.8% | **+5.9** | 520 (+382/−133) | 10.93 | 1.1 | **dominates** |
| **Bane** (rank 1) | 48.9% | 49.9% | **+1.1** | 460 (+250/−205) | 2.06 | 1.0 | **at the line** |
| Shadow Mastery (capstone, a Patch, for scale; allowance +1.4) | 47.2% | 48.5% | +1.3 | 391 (+221/−166) | 2.74 | 0.9 | within |
| Nightfall (rank 1) | 48.9% | 48.9% | +0.0 | 829 (+412/−411) | 0.00 | 1.4 | within |
| Amplify Curse | 47.2% | 46.9% | −0.3 | 459 (+222/−235) | 0.56 | 1.0 | within |
| Master Demonologist | 50.3% | 49.7% | −0.6 | 196 (+84/−110) | 1.79 | 0.7 | within |
| Shadowburn | 50.3% | 49.6% | −0.6 | 308 (+140/−167) | 1.48 | 0.8 | within |
| Improved Shadow Bolt (rank 1) | 48.9% | 46.6% | −2.2 | 314 (+107/−202) | 5.35 | 0.8 | below |
| Aftermath (rank 1) | 50.3% | 47.9% | −2.4 | 554 (+223/−324) | 4.28 | 1.1 | below |
| Conflagrate (capstone, allowance +2.0) | 50.3% | 46.5% | −3.8 | 688 (+261/−422) | 6.12 | 1.2 | below |
| Siphon Life | 52.5% | 47.2% | −5.3 | 618 (+193/−416) | 9.00 | 1.2 | below |
| Curse of Exhaustion (chosen) | 47.2% | 41.2% | −5.9 | 656 (+200/−451) | 9.80 | 1.2 | below |
| Demonic Sacrifice (Sacrifice chosen) | 48.9% | 27.6% | −21.3 | 1460 (+275/−1175) | 23.61 | 1.8 | below |

Thirteen comparisons: at a Bonferroni threshold (z ≥ 2.89) Soul Link and the
six "below" rows stand; Bane does not.

### The replication (seeds 4-7)

| comparison | block | before | after | delta (pt) | flips | z | floor (pt) |
|---|---|---|---|---|---|---|---|
| Soul Link vs DE | seeds 0-3 | 48.9% | 54.8% | +5.9 | 520 (+382/−133) | 10.93 | 1.1 |
| | seeds 4-7 | 48.3% | 54.4% | +6.0 | 499 (+375/−120) | 11.42 | 1.1 |
| | **pooled** | 48.6% | 54.6% | **+6.0** | 1019 (+757/−253) | 15.83 | 0.7 |
| Bane (rank 1) vs DE | seeds 0-3 | 48.9% | 49.9% | +1.1 | 460 (+250/−205) | 2.06 | 1.0 |
| | seeds 4-7 | 48.3% | 49.1% | +0.8 | 431 (+230/−197) | 1.55 | 1.0 |
| | **pooled** | 48.6% | 49.5% | **+0.9** | 891 (+480/−402) | 2.59 | 0.7 |
| DE rank on the Demonology stem | seeds 0-3 | 46.8% | 48.9% | +2.0 | 181 (+132/−46) | 6.37 | 0.6 |
| | seeds 4-7 | 47.5% | 48.3% | +0.9 | 145 (+88/−52) | 2.96 | 0.6 |
| | **pooled** | 47.2% | 48.6% | **+1.4** | 326 (+220/−98) | 6.79 | 0.4 |

Soul Link replicates to a tenth of a point. Its pooled margin over the
two-rank allowance is 6.0 − 1.4 = **+4.6pt**, against a 0.7pt floor. Bane's
second block falls inside its floor; pooled it sits 0.2pt above it.

### One stat rank, and each reference against 0 points

| comparison | before | after | delta (pt) | flips | z | floor (pt) |
|---|---|---|---|---|---|---|
| Affliction stem + DE vs stem | 45.8% | 47.2% | +1.4 | 204 (+131/−71) | 4.15 | 0.7 |
| Demonology stem + DE vs stem | 46.8% | 48.9% | +2.0 | 181 (+132/−46) | 6.37 | 0.6 |
| Destruction stem + DE vs stem | 48.3% | 50.3% | +2.0 | 200 (+141/−57) | 5.90 | 0.7 |
| **Affliction 6/0/2** vs 0 points | 43.0% | 48.5% | **+5.5** | 932 (+578/−347) | 7.56 | 1.4 |
| **Demonology 2/6/0** vs 0 points | 43.0% | 54.8% | **+11.7** | 751 (+620/−124) | 18.15 | 1.3 |
| **Destruction 0/2/6** vs 0 points | 43.0% | 46.5% | **+3.5** | 707 (+424/−278) | 5.47 | 1.3 |

Three builds of the same price (99-100) are worth +3.5 to +11.7pt: price does
not predict power across the trees. Soul Link alone is 7.9pt of Demonology's
11.7 (the reference against its stem). Destruction is better without its
capstone (stem +1.8pt over the reference).

Each arm against its own reference is in the script's `table` output.

### Non-vacuity

**Control:** every arm's 96 control matches (no Warlock on either side; the
control fields all 8 other classes on both sides) are identical in winner and
duration to the 0-point arm's. **Decisive:** 5,058-5,082 of 5,088 matches end
by elimination in every arm.

**The nodes fire.** The batch CSV carries outcomes only, so a fixed slice of
60 Warlock matches per arm (64 Warlocks, both sides) was replayed with full
logs (`sample`):

| arm | Shadow Bolt hits | Immolate hits | node events |
|---|---|---|---|
| 0 points | 69 | 60 | — |
| Affliction | 73 | 55 | Siphon Life cast 78; Shadow Trance 28 procs, 22 spent |
| + Amplify Curse | 81 | 57 | Amplify Curse cast 54 |
| + Curse of Exhaustion | 69 | 56 | Curse of Exhaustion cast 124 |
| Demonology | 79 | 78 | Soul Link transfers 1,029; Master Demonologist on 128 units |
| + Demonic Sacrifice | 72 | 58 | 64 Felhunters sacrificed (Touch of Shadow) |
| + Nightfall | 80 | 65 | Shadow Trance 18 procs, 15 spent |
| + Improved Shadow Bolt | 68 | 71 | Shadow Vulnerability 5 procs |
| Destruction | 46 | 92 | Conflagrate cast 50; Shadow Vulnerability 9 |
| + Shadowburn | 75 | 66 | Shadowburn cast 21 |
| + Aftermath | 87 | 68 | Aftermath 6 procs |
| + Master Demonologist | 80 | 69 | Master Demonologist on 128 units |

Bane leaves no log line; it shows as the arm's 460 flips and its Shadow Bolt
count (80, against 74 for its comparison).

## What it says (findings for the user, not changes)

1. **Soul Link dominates** (+4.6pt past its allowance, replicated). Twelve
   percent of the Warlock's damage taken, unpreventable, sends 16 transfers a
   Warlock a match to a Felhunter of about 210 health (45% of the reference
   Warlock's 388, `Combatant::new_pet`, raised 20% by Fel Stamina 2/2;
   `talents.ron`'s 204 is Fel Stamina 2/2 on an untalented Warlock's 377).
   Direction: a smaller share
   (the design priced 12% at about two ranks of effective health), or a
   tighter condition. With it, the Demonology reference is twice either
   other's worth over 0 points.
2. **Bane's first rank is at the line**, not convincingly over it. Its effect
   rides on Shadow Bolt, which the AI casts about once a Warlock a match.
   Watch it in AS-441 rather than act on it alone.
3. **The Shadow Bolt nodes have little to act on.** Shadow Bolt lands 1.1 times
   a Warlock a match, so Improved Shadow Bolt (0.08 procs a Warlock a match at
   rank 1), Aftermath (0.09) and Nightfall's payoff are small at any
   magnitude. Improved Shadow Bolt and Aftermath measure below a stat rank.
   This is an AI-priority question (where Shadow Bolt sits in the Warlock's
   chain) before it is a tuning one.
4. **Demonic Sacrifice is −21pt** against a stat rank: the Felhunter (Spell
   Lock, Devour Magic, its damage and body) is worth far more than +15%
   Shadow damage, and the stem's Fel Stamina, Unholy Power and Master
   Demonologist go with it. As a per-match choice it is never dominant, but
   it is never right either on this sweep.
5. **Siphon Life (−5.3pt), Curse of Exhaustion (−5.9pt, when chosen on every
   enemy) and Conflagrate (−3.8pt; the Destruction build is +1.8pt better
   without it)** measure below a stat rank. Conflagrate consumes the
   Immolate burn it rides on and costs the global cooldown Shadow Bolt would
   have had (Shadow Bolt hits 46 against 79 for its stem). Siphon Life costs
   a global cooldown for 40 damage over 30s. Directions: Conflagrate only
   when the burn left is worth less than the blast, or a bigger blast; Siphon
   Life's priority or magnitude.

Each is a direction for AS-441, not a value: a directional run may not be
cited as the Warlock's standing.

## Byte-identity

The schema change leaves untalented play, and builds written as maps, byte
for byte as on `main`: 2,040 seeded matches (900 2v2 `Legacy` BasicArena,
900 2v2 `TeamPlan` PillaredArena, 240 Warlock 2v2 `TeamPlan` with an
Affliction build written as a map) give identical CSVs from a `main`
be76925a binary and this branch's — 2,034 decisive, 1,399 distinct
durations — and four seeded `--headless` logs (2v2 and 3v3, both profiles,
three maps) are identical.

## Reproducing it

Every arm is a config on this branch's binary; no patch is needed. The
committed artefacts are the arms' JSONLs and CSVs.

```bash
cargo build --release
S=docs/design/balance/sweeps
# the inputs as run (or regenerate: $S/2026-11-22-as299-arms.py gen <dir>,
# and `gen <dir2> 4 demo demo_de demo_bane demo_stem` for the replication)
mkdir -p /tmp/as299 && tar -xzf $S/2026-11-22-as299-arms.jsonl.tar.gz -C /tmp/as299
for f in /tmp/as299/block1/*.jsonl /tmp/as299/block2/*.jsonl; do
  target/release/arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
done
# or take the CSVs as run:
tar -xzf $S/2026-11-22-as299-arms.csv.tar.gz -C /tmp/as299
$S/2026-11-22-as299-arms.py table /tmp/as299/block1
$S/2026-11-22-as299-arms.py replicate /tmp/as299/block1 /tmp/as299/block2
$S/2026-11-22-as299-arms.py sample /tmp/as299/block1 target/release/arenasim
# any one pair, with its control and slices:
scripts/paired_sweep.py /tmp/as299/block1/demo_de.csv /tmp/as299/block1/demo.csv \
  --affects Warlock --tier directional --expect nothing --headline affected
```

Every figure above recomputes from the committed CSVs except the non-vacuity
counts, which replay matches through the binary (about a minute).
