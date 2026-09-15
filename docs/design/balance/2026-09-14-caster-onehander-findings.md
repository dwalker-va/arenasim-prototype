# Caster one-handers — filling the main-hand socket (AS-87)

Every one-handed weapon a Mage or a Warlock could hold carried attack power and
nothing else. Both train dagger, one-handed sword, staff and wand — no mace —
and the only main-hand items with a caster stat on them were two one-handed
maces and two two-handed staves. So their only main hand worth equipping was a
staff, which costs them the off-hand. The dagger-plus-tome loadout was
unrepresentable, and the staff-versus-one-hander choice was fake: one option
carried a stat and the other did not.

The Priest was a different problem with the same symptom. It may use one-handed
maces, so `MaceOfTheRedeemer` was always legal for it — its main hand was empty
for no reason at all.

AS-87 adds three one-handers that carry spell power, and gives all three caster
classes one by default.

**This card fills a CONTENT HOLE. The deliverable is a fillable slot with a real
choice in it, not a movement in win rate.** That matters for how the numbers
below should be read, and the section on the sweep says so plainly rather than
leaving it implied.

## What shipped

| item | tier | type | ilvl | stats | budget |
|---|---|---|---|---|---|
| Witchblade | base | 1H dagger | 58 | sp 5, mana 6 | 13.5 / 24.5 (55%) |
| Claw of Chromaggus | 1 | 1H dagger | 73 | sp 7, mana 8, mp5 0.5 | 21.0 / 30.8 (68%) |
| Azuresong Mageblade | 1 | 1H sword | 73 | sp 8, hp 4, crit 1% | 19.0 / 30.8 (62%) |

All three are real WoW Classic items, looked up through the Wowhead Classic MCP
by id (13964, 19347, 17103), each with its own era-faithful icon.

**Mage, Warlock and Priest default to Witchblade**, so the measured change is
exactly **+5 spell power and +6 max mana** on each. Nothing else moves: the
dagger's 7-11 swing is INERT for all three, because `apply_equipment` takes
attack damage from the MainHand only when `class.is_melee()`, and none of them
is melee — they swing from the Ranged socket. The tier-1 pair is pool-only; no
default loadout equips it.

One honest limitation: the card asked for daggers AND one-handed swords, and
both kinds are present, but the sword arrives only at tier 1. At the base tier
the caster choice is dagger-or-staff.

## Why they are spent under budget

A one-hander at full budget plus a 19-point off-hand would leave the staves
permanently dominated, because `slot_budget_multiplier` gives a two-hander the
same 0.5625 as a one-hander. Both staves already sit at 97-98% of their own
budget and cannot be raised to meet it.

The card asked whether a staff should out-stat a one-hander plus off-hand "by a
little". Under the current formula it cannot, and forcing it would mean cutting
the one-hander to about 5 budget points — which re-creates the dead slot this
card exists to remove. So Witchblade spends 55% instead:

    staff path      CrescentStaff                    sp 10, mana  9 = 24.0 points
    one-hand path   Witchblade + TomeOfKnowledge     sp 11, mana 16 = 32.5 points

That halves the gap a full-budget one-hander would have opened (43.5 points)
rather than closing it. **The real fix is the budget formula** — a two-hander
occupies two sockets and should draw two sockets' budget — and that is a
separate card, not this one.

## The guard

`every_socket_offers_a_useful_item` asserts that for every class and every
socket, an equippable item exists carrying a stat that class's own kit scales
with. The scaling stat is derived from `abilities.ron` rather than hardcoded, so
a class whose kit changes character changes what counts without anyone updating
a table.

It is the companion to AS-86's `every_class_can_fill_every_socket` and a
strictly stronger claim: "a legal item exists" and "a legal item worth
equipping exists" are different properties. A TWO-HANDER cannot be the item that
proves a main hand usefully fillable, which is what makes this catch the bug
rather than pass on the staff — `enforce_two_hand_conflicts` strips the
off-hand, so "fillable only by a two-hander" means the socket is a trade, not a
slot.

Run against main's data it fires on exactly `Mage/MainHand` and
`Warlock/MainHand`. It does not fire on Priest, matching the card's reading that
Priest was a loadout omission rather than a content hole. Two justified
exemptions are recorded with their reasons: the Rogue and Hunter off-hands,
where the off-hand is a second weapon (dual wield, AS-60) and a held-in-off-hand
frill is a caster stat stick by definition.

## The paired sweep

Three paired runs, on the three successive bases this card was rebased across
while other cards merged under it. Every run is BasicArena / Legacy, 300s cap,
both binaries at identical seeds, with a CONTROL arm filtered to cells where
NEITHER side fields a Mage, Warlock or Priest.

| run | base | size | why |
|---|---|---|---|
| 1 | `3c61185` | 3,870/binary | before AS-54 merged |
| 2 | `323bd93` | 3,870/binary | after AS-54 (Frost Armor as one debuff) |
| 3 | `17bf5b1` | 1,770/binary | after AS-97 (Shaman weapon damage) + AS-53 |

Run 3 is deliberately asymmetric: the CONTROL is at FULL strength because it is
the card's actual obligation, and the delta arms are small because two prior
runs had already answered that question and a third full-size pass would only
have raced the merge queue again.

### The control — the load-bearing result

| base | no buffed class either side | a buffed class present |
|---|---|---|
| `3c61185` | **720 identical, 0 differ** | 1,861 of 3,150 differ |
| `323bd93` | **720 identical, 0 differ** | 1,838 of 3,150 differ |
| `17bf5b1` | **720 identical, 0 differ** | 620 of 1,050 differ |

Non-vacuity: every run ended 100% of its matches in a kill (none by timeout),
with 2,170 / 2,158 / 1,189 distinct durations.

**This is what the card owes and it replicates on all three bases.** Cells the
change cannot touch are bit-identical in winner, end reason and duration; cells
it can touch move. A uniformly identical result would have meant the change
never reached the sim; a control cell moving would have meant it reached further
than intended. Neither happened, three times.

### Is the control wired up? (instrument non-vacuity)

A control that reads "720 identical, 0 differ" three times running is, from the
outside, indistinguishable from a control that is not connected to anything. The
split in the table above is the first answer — the same comparator, over the
same file, reports 1,861 / 1,838 / 620 DIFFERING rows among the buffed cells, so
it cannot be a comparator that always says "identical".

But those are different ROWS. The sharper question is whether the instrument
detects a change in the CONTROL rows specifically. It does, and the three
`before` runs prove it at no extra cost: they are unmodified main at three
successive commits, and two of those commits differ in ways with OPPOSITE
predictions for an arm that contains no Mage, Warlock or Priest by construction.

| between | the intervening change | prediction | measured |
|---|---|---|---|
| base1 -> base2 | AS-54, Frost Armor chill as one debuff — a MAGE ability | identical | **720 / 0** |
| base2 -> base3 | AS-97, the Shaman's main-hand weapon damage going live | differs | **482 / 238** |

And it does better than detect — it LOCALISES:

    rows WITH a Shaman    : 238 differ, 122 identical
    rows WITHOUT a Shaman :   0 differ, 360 identical

Every control row that moved contains a Shaman, and no row without one moved.
That is the same property AS-87's own control claims, demonstrated on the same
720 rows by a change this card did not make. The three clean controls are three
measurements, not three no-ops.

**One limit, stated rather than left implied.** The comparison is on
`(winner, end_reason, duration_secs)` at the CSV's 0.01s resolution, not on a
whole-log hash. A match whose internals moved without moving any of those three
reads as identical here — which is why 122 Shaman rows are identical above
despite AS-97 reaching them. The claim the control supports is "no observable
outcome difference at this resolution", which is the claim the card needs;
`scripts/behaviour_baseline.sh` is the whole-log instrument if a stronger one is
ever wanted.

### The delta — directional, and small

Team 1 win rate. **Runs 1 and 2 are n=1,050 per arm; run 3 is n=350 and is
DIRECTIONAL ONLY — it may not be cited as any class's standing.**

| arm | slice | base 3c61185 | base 323bd93 | base 17bf5b1 |
|---|---|---|---|---|
| Mage | all | +1.4 ns | +1.8 (z=2.30) | +1.4 ns |
| | CLEAN | +0.0 ns | +0.0 ns | -0.8 ns |
| | MIRRORED | +2.2 (z=2.00) | +2.8 (z=2.81) | +2.7 ns |
| Warlock | all | +0.3 ns | +1.2 ns | +3.1 ns |
| | CLEAN | +2.1 ns | +2.1 ns | +3.2 ns |
| Priest | all | -0.2 ns | -0.2 ns | +0.6 ns |
| | CLEAN | +1.3 ns | +2.1 ns | +5.6 (z=2.00) |

**The effect is about +1 to +2 points and does not resolve from zero on any of
three bases. That is the ideal outcome, not a failed measurement.** This card
fills a CONTENT HOLE — two classes had no one-handed weapon carrying a caster
stat, so the slot was unusable and the staff-versus-one-hander choice was fake.
The deliverable is a fillable slot with a real choice in it. An unresolvable
+1pt means the hole is closed WITHOUT distorting balance. A clean +3pt would
have been the worrying result, because it would mean the item was doing
something the card did not intend.

**A prediction that missed, recorded because it is useful.** Before measuring,
reasoning from AS-86's relics (+5 spell power and +3 mana worth +2.4pt to a
Paladin comp at z=4.86), this card predicted +2 to +3pt in the clean slice.
Measured clean slices across three bases: +0.0, +0.0, -0.8 (Mage); +2.1, +2.1,
+3.2 (Warlock); +1.3, +2.1, +5.6 (Priest). The prediction was HIGH for the Mage
and roughly right for the other two. Whoever sizes the next sweep should weight
that an equivalent stat step on a different class and socket did not reproduce
the relics' magnitude.

**On the cells that read as significant.** Three appear across the three runs,
and they are NOT the same cell: Mage MIRRORED (z=2.00, then z=2.81), and Priest
CLEAN on run 3 (z=2.00, n=350). They are named here rather than quietly dropped.

The Mage MIRRORED pair is the interesting one, and the reasoning went both ways.
One cell at z=2.00 out of twelve slices is what multiple comparisons produce,
which was the first read. The SAME cell returning larger on a second base is
harder to dismiss on those grounds. What keeps it from being a finding is
different: runs 1 and 2 share seeds and comps and differ only in the base, so
they are CORRELATED, not independent replications — agreement between them is
much weaker evidence than two independent samples. And on run 3 that cell is
+2.7pt but NOT significant, while a different cell entirely (Priest CLEAN) is.
A significant cell that wanders between runs is the signature of multiple
comparisons, not of an effect.

The honest statement: the mirrored slice moved the same direction on all three
runs at a size the design cannot resolve, while the CLEAN slice that isolates
the buff from the mirror is +0.0pt on both full-size runs. Noted, unresolved,
and not a claim about where any class stands.

**An open question for whoever reads several of these.** A significant team-1
gain in the MIRRORED slice — the slice where the effect is supposed to wash —
has now appeared on more than one card this cycle: AS-86's relics measured
mirrored +1.9pt at z=2.01, and this card's Mage arm measured z=2.00 then z=2.81.
Two other cards in the same cycle (AS-54's Frost Armor, AS-97's Shaman weapon
damage) showed nothing in that slice. Two of four is not a pattern worth betting
on, and nothing here chases it. It is recorded because if it IS systematic, it
would be a fact about the MEASUREMENT rather than about any class, and it would
quietly bias every mirrored slice this project reports. If it shows up on a
third card, it deserves to become one.

## The medic-chase scan — the clearest evidence the buff works

Seven movement probes broke on this change. The identical suite passes 107/107
on a dev build of main, so they are attributable to it rather than to drift.
Six were vacuity failures — a probe's own guard reporting that its pinned seed
no longer exercises its condition. The seventh needed distinguishing from a real
regression before re-pinning past it, so both binaries were scanned over 30
paired seeds on Warrior+Priest vs Warrior+Shaman — a ONE-SIDED buff, since the
Priest gains and the Shaman does not.

**The chase is not slower.** Its bound — longest contiguous occluded-distress
window <= 8s — is breached on no seed on either binary, and its maximum falls
5.40s -> 5.20s.

**What rose was the NUMBER of windows, not their length.** The fraction of match
spent occluded from a distressed ally roughly doubles, 0.029 -> 0.061. The
reason is the opposite of a defect:

| | before | after |
|---|---|---|
| seeds where an ally died before a heal landed | 5 | 1 |
| team-1 (buffed Priest) wins across the scan | 16/30 | 19/30 |

A stronger Priest keeps its Warrior ALIVE at low HP instead of letting it die,
so there are more rescues to perform. More rescues, not slower ones. This is a
per-frame mechanism measurement rather than a win-rate one, which is why it
carries more weight than the sweep's headline for the question "does the buff
work".

**On match length.** Three of thirty seeds (0, 2, 17) stop resolving and run to
the probe's 200s cap where they previously ended in 37-46s. That is worth
knowing, and it is now card AS-106. But it must not be read as "the change
lengthens matches": excluding those three, the paired duration delta is
**-0.98s, with 12 seeds longer and 14 shorter**. Matches did not get longer.
Three of them stopped ending.

One more thing the scan turned up, now card AS-105: the probe's 10s
time-to-heal ceiling is already breachable on main. Seed 12 breaches it at
13.27s on BOTH binaries — the identical value, that seed being untouched here.
That ceiling holds at the seeds it is pinned to, not universally.

## Combination-only probe failures — a general result

Rebasing onto AS-54 broke FOUR probes, and two of them are the interesting kind:

  u9_seek_reset  37 -> 9     AS-87's own re-pin fell to zero cast-start blocks
                             with AS-54 underneath
  los_probes     37/38 -> 9/26   both AS-87 fizzle seeds fell to zero fizzles
  juke_chase     11 -> 9     PASSED ON EACH BRANCH ALONE
  oom_wand       22 -> 33    PASSED ON EACH BRANCH ALONE

`juke_chase` and `oom_wand` were touched by neither branch. They broke because
both changes move Mage trajectories, and only the merged tree shows it. **No
amount of care on either branch in isolation would have caught them**, and
taking either side's file wholesale during the rebase would have shipped main
red.

The determinism pin makes the same point numerically: AS-54 alone recorded
47.982773s and AS-87 alone 58.44928s, and the merged value is 49.38275s.
Neither branch's figure survived, because the pin is a property of the two
together.

The lesson for a push-and-rebase workflow: on a conflict in a seed-pinned or
value-pinned test, **re-derive on the merged tree rather than picking a side**,
and confirm the base is green first — plain main at `323bd93` passes 107/107,
which is the step that turns "tests are failing" into "these four are mine".

## Reproducing this

Unlike AS-86, the sweep inputs are committed, and every line names its `map`
and `ai_profile` so a file fully determines its run — the batch CSV has no
column for either.

  `2026-09-14-as87_caster_onehanders_sweep.jsonl`  runs 1 and 2 (3,870 matches)
  `2026-09-14-as87_base3_sweep.jsonl`              run 3 (1,770 matches)

Results, one pair per base:

  `2026-09-14-as87_base1_3c61185_{before,after}.csv`
  `2026-09-14-as87_base2_323bd93_{before,after}.csv`
  `2026-09-14-as87_base3_17bf5b1_{before,after}.csv`

    arenasim --batch docs/design/balance/2026-09-14-as87_base3_sweep.jsonl \
      --out results.csv --jobs 8

Run it from the root of a tree holding the assets you mean to measure. **The sim
loads `assets/` relative to the process working directory**, so a run whose CWD
is not pinned to its own tree can silently read another one's `items.ron`.
