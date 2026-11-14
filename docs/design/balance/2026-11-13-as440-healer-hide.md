# AS-440: the healer that hides and the partner it exposes

**Card:** AS-440. **Status: RULED (user, 2026-11-14): the cover rule is
built (§4, `team_solve::SolveWorld::cover_waived`); the stance stays as it
is (§5, option 4), with the eight comps of §2's starred rows on the
milestone sweep's watch list.** §2-§4 were measured with a prototype
measurement switch (`team{1,2}_hide_rule`, removed when the rule shipped;
its banks are the committed CSVs, played by the binary at cfbf3a54) and the
Pillar play's existing withhold. The shipped rule's switch is
`team{1,2}_handoff_rule: false`.
**Tier: DIRECTIONAL** (`sweep-tiers.md`). 2v2, the 30 x 30 play-bank cells,
PillaredArena and TwinPillars, both seats, head-to-head: the option runs on one
healer + DPS side, paired with the same cell and seed with both sides shipped.
The diagnosis (§1) was measured on 5300b998. Everything else was measured on
main 0ca55319 at 12 seeds in three blocks (801-804 / 811-814 / 821-824), and
confirmed on fresh blocks 831-834 and 841-844. An effect is starred (*) when
its 95% seed-clustered bootstrap interval (resampling seeds) excludes zero
**and** every block agrees in sign.

## 1. Diagnosis: what hides the healer

`tests/healer_hide_exposure.rs` (ignored, a measurement) plays every cell in
which a healer + DPS team meets a team with a ranged DPS, both teams on
uniform `TeamPlan` with the trace on. That is 4,176 matches and 5,040 sides
(2,016 healer + melee, 3,024 healer + ranged). For each enemy `target_focus`
move off a side's healer onto its partner, the harness:

- finds the enemy DPS that was on the healer;
- walks back to the frame that DPS last saw the healer;
- attributes whose step broke the sight, from positions 0.5s either side;
- reads what held the healer's legs on that frame, from its posture machine
  (`ObservedHealerState`, new on the frame observer).

Rates of events are per side (one per qualifying team per match). Shares of
time are over the seconds both units were alive with an enemy ranged DPS
alive.

| | healer + melee | healer + ranged |
|---|---|---|
| sides with a focus move off the healer | 65.5% (0.71 moves/side) | 34.7% (0.38/side) |
| healer sees its partner in heal range at the move | 42% | 86% |
| hide began in the opener (never seen, or lost in the first 10s) | 73% | 66% |
| DPS blind to the healer before the move, median | 7.9s | 4.1s |
| focus then held on the partner, mean | 18.5s | 15.1s |

What put the healer out of sight, healer + melee, 1,434 moves:

| cause | share |
|---|---|
| never in that DPS's sight since the gates | 38.9% |
| hugging a pillar under the team's pillar stance (FREE) | 17.2% |
| plain FREE formation point | 10.0% |
| PRESSURED team-solve cover (`C_OCCLUDED`) | 9.4% |
| PRESSURED, walking out of CC reach | 6.0% |
| PRESSURED while silenced or locked out | 5.8% |
| FREE reach repair (AS-362) | 5.5% |
| ESCAPE, CC'd, walks, dip | 7.2% |

## 2. The 2v2 pillar stance: where dropping it helps

The Pillar play was withheld from the running side (`NoPillar`; 28,800
sides). Pooled, the side gains **+0.8*** [+0.5, +1.2] (blocks +0.5 / +0.6 /
+1.3). Round 1's +1.3 was 3 seeds on the pre-merge main.

**The pooled effect hides large effects of both signs, by side comp.** 15
comps can take the stance; Paladin comps never do (the Paladin never hugs),
and show 0 flips in 7,200 sides.

| side comp | effect | 95% interval | blocks | fresh block 831-834 |
|---|---|---|---|---|
| Warrior + Druid | **+6.3*** | +4.9, +7.8 | +5.4 / +5.2 / +8.3 | **+5.2*** |
| Warlock + Shaman | **+5.4*** | +3.5, +7.1 | +5.4 / +4.6 / +6.2 | **+2.1*** |
| Warlock + Priest | **+4.9*** | +2.5, +7.2 | +3.9 / +6.5 / +4.5 | **+4.0*** |
| Rogue + Shaman | **+4.2*** | +2.3, +5.9 | +4.8 / +5.2 / +2.6 | +1.2 |
| Rogue + Priest | **+4.0*** | +1.9, +6.2 | +1.7 / +2.3 / +8.1 | **+3.8*** |
| Rogue + Druid | **+2.4*** | +0.6, +4.2 | +5.3 / +0.2 / +1.7 | −0.6 |
| Hunter + Priest | +2.1 | +0.3, +4.0 | +2.1 / +4.7 / −0.5 | |
| Mage + Druid | +1.9 | +0.2, +3.8 | +2.5 / −1.1 / +4.4 | |
| Warrior + Shaman | +0.7 | −2.0, +3.3 | | |
| Hunter + Shaman | −0.3 | −2.0, +1.3 | | |
| Mage + Priest | −0.9 | −3.9, +2.6 | | |
| Mage + Shaman | **−2.5*** | −4.9, −0.5 | −3.2 / −2.2 / −2.2 | |
| Warrior + Priest | **−2.8*** | −4.8, −1.2 | −3.9 / −2.6 / −2.0 | |
| Warlock + Druid | **−3.1*** | −4.8, −1.2 | −1.8 / −5.0 / −2.5 | |
| Hunter + Druid | **−6.1*** | −7.8, −4.4 | −6.8 / −5.6 / −5.8 | |

n = 1,440 sides per comp (12 seeds); 480 on the fresh block.

- **Concentration.** The net gain is +234 wins: +459 from the comps that
  gain and −226 from those that lose. The top three comps carry 103% of it.
  Ten comps replicate in all three blocks: six gain, four lose.
- **Shape and map.** Healer + melee gains **+1.9***. Healer + ranged gains
  +0.1, which is +Warlock and −Hunter/Mage cancelling. Nagrand **+1.0***,
  TwinPillars **+0.6***.
- **Enemy comp.** The enemies that bring no crowd control the stance
  defends against are a second built-in null: 9 comps, 0 flips. The largest
  gains are against Mage + Paladin (+8.4*), Warrior + Warlock (+6.7*) and
  Mage + Shaman (+6.1*). The largest losses are against Mage + Rogue (−4.6*)
  and Priest + Warlock (−4.2*).
- **Fresh block.** The Pillar play was withheld only for the six gaining
  comps, on new seeds. Those sides gained **+2.6pt** against the 12-seed
  estimate of +4.5, which is the shrinkage a selection on the same data
  predicts. Four of the six replicate: Warrior + Druid, Warlock + Priest,
  Rogue + Priest and Warlock + Shaman. Pooled over all 9,600 sides, the
  targeted withhold is worth **+0.8*** with no losing cells, the same as the
  blanket withhold's +0.8 with its four losing comps.

## 3. Why dropping the stance moves a match

**The healer's hug is the active part; the DPS's half is not.** The stance
has two halves. The healer hugs the pillar (AS-373). The DPS yields its
healer's sight to the stance side (AS-386's rule 3) and, for a kiter, pulls
toward that side. Each half was turned off alone, on the 15,120 sides the
stance can reach (12 seeds):

| arm | effect | per-comp pattern |
|---|---|---|
| no stance (both halves off) | **+1.5*** | as §2 |
| stance on, the healer does not hug (`StancePartnerHalf`) | **+1.0*** | reproduces §2 comp for comp: Warlock + Shaman +7.4*, Hunter + Druid −9.0* |
| stance on, the DPS's half off (`StanceHealerHalf`) | +0.1 | flat; the largest comp is +3.0 |

Dropping the hug carries two thirds of the effect, and its sign in every comp.
The DPS's half alone is worth nothing. The remaining +0.4 is the two halves
together.

**The hug is an opener move.** The stance and no-hug arms are identical,
frame by frame, until the healer's first hug (4,375 of 4,375 matches). That
hug comes 1.8-3.1s after the gates, before contact. In the 20s after it, a
healer that does not hug:

- stands off the pillar (within 4yd of it 4.2s against 9.8s, ranged-partner
  comps that gain);
- is seen by enemy casters a little more (10.1s against 8.6s);
- takes the same hard crowd control (0.80s against 0.83s).

Its partner's health 20s later is unchanged (49% against 47%). The difference
builds later in the match.

**What the hug changes is how the partner is kept alive, and its sign
depends on the comp.** Across the 15 comps, the 12-seed effect correlates with
these per-comp changes:

| what changes when the healer does not hug | correlation with the effect | gaining comps | losing comps |
|---|---|---|---|
| partner damage dealt (log) | +0.73 | +2 to +11% | −3 to −5% |
| partner alive | +0.68 | +0.8 to +3.8s | −3.5 to +0.4s |
| healing on the partner, per match | +0.67 | +0.06 to +0.17 of a bar | −0.24 to +0.03 |
| partner died first | −0.62 | −9 to +1pt | 0 to +7pt |
| healer's hard crowd control | −0.10 | −0.7 to −0.2s | −0.5 to +0.2s |
| healer hidden from the enemy's ranged DPS | +0.24 | | |

From 3 seeds per comp (360 sides each).

So the hug buys the healer nothing measurable in crowd control or exposure in
2v2. Its cost or benefit lands on the partner. In the gaining comps, a healer
not tied to a pillar ring keeps more healing on its partner, which lives
longer and deals more damage. In the losing comps, the reverse holds. Which
healer-side behaviour sets the sign per comp is not identified (Gaps).

## 4. The cover rule

While the enemy team runs a target focus and one of its units that follows
one could take one of the healer's teammates
(`target_focus::could_turn_on_teammate`: targetable, within `FOCUS_REACH`,
in its sight; never on a map with no obstacles), the PRESSURED solve gives no
credit for cover from any enemy caster and samples no shadows. The shadow
samples carry much of the rule: with no credit, a shadow sample ties every
other spot on cost, and the nearest-spot tie-break walks the healer into it
(3 of 6 diverging matches went back to main's output with the samples kept).
CC-reach avoidance, sight of the partner, a dying teammate's reach and an
owed dispel are unchanged. Against a team without a focus (`Legacy`, or
`team{n}_target_focus: false`) the rule does not fire.

The figures in this section's first three bullets were measured with the
prototype's definition (a Mage, Hunter or Warlock; 2D distance; no
targetability test). The shipped rule asks the focus machinery itself. On the
fresh block 851-854 the two definitions measure the same (+0.1 and +0.1).

- **Effect:** **+1.0*** [+0.8, +1.3] at 12 seeds (blocks +0.8 / +0.8 /
  +1.5). On fresh blocks: +0.8* (841-844), and +0.1 in 831-834 (the sides
  outside the six comps, where it ran alone).
- **Localisation: broad.**
  - Healer + melee +1.2*, healer + ranged +0.9*; both maps +1.0*.
  - Paladin +1.8* (replicated at +1.7* on 841-844); Druid +1.2*, Shaman
    +0.9*, Priest +0.2.
  - The top five comps carry 62% of the net: +328 gross gain, −32 gross
    loss. No comp is significantly negative; Mage + Druid −0.8 and
    Warrior + Priest −1.3 have intervals that exclude zero, but their blocks
    disagree.
  - Built-in null: enemies with no ranged DPS show 0 flips (8,640 sides),
    because the rule never fires against them.
- **Mechanism: the enemy's fire comes back to the healer.** At seed 801,
  against an enemy with a ranged DPS:
  - the healer is hidden from them less (52.8 to 49.5% of the time, healer
    + melee);
  - they shoot the partner less (47.5 to 45.7%) and the healer more (17.1 to
    19.6%; healer + ranged 58.9 to 56.6% and 28.7 to 31.6%);
  - damage to the partner falls by 1.5%;
  - the healer's time below 10% mana falls by 0.6-0.9s;
  - the healer's hard crowd control is unchanged (4.11 to 4.10s).

  The healer, which heals itself, absorbs the fire the focus would have
  moved onto its partner.
- On the six gaining comps, adding the cover rule to the targeted stance
  withhold did not add to it on the fresh block (+2.0 against +2.6).
- **The shipped rule on main f366815d**, head-to-head (the side running it
  against the same side with `team{n}_handoff_rule: false`, both seats,
  paired with both off):
  - fresh block 851-854: +0.1 [−0.1, +0.4] (9,600 sides; the prototype's
    definition on the same block: +0.1 [−0.2, +0.5]);
  - block 841-844 again: **+0.7*** [+0.5, +0.9], against +0.8 with the
    prototype on 0ca55319.

  - fresh block 861-864 (main f41e9401, with the enemy-focus gate):
    **+0.6*** [+0.4, +0.8].
- **The shipped rule's effect: +0.5pt [+0.3, +0.7]**, seed-clustered over
  its 12 seeds (841-844, 851-854, 861-864; 28,800 sides; blocks +0.7 / +0.1 /
  +0.6). That is about half to two thirds of the prototype's +1.0 over its
  own first 12 seeds. 851-854 is low under both definitions, so that block's
  reading comes from its seeds, not from how the rule is defined. The two
  earlier blocks ran on main f366815d and the third on f41e9401; the
  enemy-focus gate changes nothing in these banks, where both teams run a
  focus.
- **Determinism.** Full match logs from main f366815d and the rule's head
  are byte-identical:
  - 120 BasicArena `TeamPlan` and 60 BasicArena `Legacy` matches;
  - 120 `Legacy` matches on the pillar maps;
  - 120 `TeamPlan` pillar-map matches with the rule switched off for both
    teams.

  The 300 matches outside the switched-off set saw 856 deaths and 1,717 crits.
  With the rule on, 68 of 120 `TeamPlan` pillar-map matches diverge. In 54
  of them, the first unit whose traced position differs is a healer, 51 of
  them PRESSURED. In the other 14, the first difference is a reordered event:
  a sight-yield change, a healer movement decision, or a decision whose target
  moved.
  Re-checked at main f41e9401 with the enemy-focus gate: 60 BasicArena
  `TeamPlan`, 30 BasicArena `Legacy`, 60 pillar-map `Legacy`, 60 pillar-map
  `TeamPlan` with the rule off, and 60 pillar-map `TeamPlan` against a
  `Legacy` team (where the gate keeps the rule off) are byte-identical to main
  (603 deaths, 1,133 crits outside the switched-off set); 33 of 60
  `TeamPlan`-against-`TeamPlan` matches diverge.

## 5. Options put to the user

Ruled: option 2 built; for the stance, option 4.

1. **Recommended: a Pillar-play exception table keyed by (healer, partner)
   for 2v2**, following Peel's pair gates. It names the four comps that
   replicated on the fresh block: Druid with a Warrior, Priest with a Warlock,
   Priest with a Rogue, Shaman with a Warlock. The stance stays everywhere
   else, including the four comps it helps (Hunter + Druid, Warlock + Druid,
   Warrior + Priest, Mage + Shaman). Each entry cites its measurement and is
   re-checked at the milestone sweep. On the fresh block the four comps
   gained +3.8pt between them, about +0.75pt pooled over every healer + DPS
   side.
2. **The cover rule**, a separate yes or no. It is broad, worth +0.8 to
   +1.0pt over four of five blocks, costs the healer no crowd control, and
   has no comp significantly negative. It is the more general mechanism, and
   it acts mid-fight where the stance acts at the opener. On the named comps
   it did not stack with option 1.
3. **A blanket 2v2 exception on the Pillar play.** +0.8pt pooled, the same
   as option 1, but it costs 2.5-6pt in four comps that replicate.
4. **No change.**

## Gaps

- The cover rule is measured in 2v2 only. It also runs in 3v3, where a healer
  has two teammates the rule can protect.

- DIRECTIONAL, 2v2 only. Neither change is measured in 3v3, where the stance
  measured +4.3 in AS-391's bank at 0.8.
- §3 names the active part (the hug) and where its effect lands (the
  partner's healing and survival). It does not identify the healer-side
  behaviour that sets the sign comp by comp: the per-comp mechanism metrics
  are 3 seeds (360 sides a comp), and none of the healer measures (CC,
  exposure, link, distance) separates the gaining comps from the losing ones.
- The fresh-block confirmation selects its comps from the 12-seed data. The
  named four are the ones that held on new seeds. Two that held at 12 seeds
  (Rogue + Shaman, Rogue + Druid) did not.
- Double-healer 2v2 teams are in no bank.

## Recompute

From `docs/design/balance/sweeps/` (`R=2026-11-13-as440-reduce.py`,
`G=2026-11-13-as440-gen.py`, files `2026-11-13-as440-…`):

| figures | command |
|---|---|
| §1 | `$R diag …-diag.rows.jsonl.gz` |
| §2 | `$R local …-r2-ctl.csv.gz …-r2-np.csv.gz`; fresh block `$R local …-r2-c4_ctl.csv.gz …-r2-c4_tgt.csv.gz` |
| §3 decomposition | `$R local …-r2-ctl.csv.gz <arm> --eligible`, arm `…-r2-np.csv.gz`, `…-r2-d_StancePartnerHalf.csv.gz`, `…-r2-d_StanceHealerHalf.csv.gz` |
| §3 event study | `$R event ctl.rows np.rows`: the harness over `$G ctl` / `$G h2h NoPillar` at seeds 801-802, 811-812, 821-822, restricted to nine comps: the five largest gaining comps of §2 (Rogue + Druid aside) and the four losing ones |
| §3 per-comp table | `$R partner ctl.rows np.rows local.txt`: the harness over `$G h2h-eligible NoPillar 801,811,821` and the matching control rows; damage dealt from `$R mechreport …-r2-mech-np.jsonl.gz` |
| §4 shipped, pooled | concatenate `…-r3-cover841-*`, `…-r3-cover851-*` and `…-r4-cover861-*` (ctl and arm, headers once each), then `$R local ctl.csv arm.csv` |
| §4 | `$R local …-r2-ctl.csv.gz …-r2-nc.csv.gz`; `…-r2-c5_ctl.csv.gz …-r2-c5_nc.csv.gz`; mechanism `$R mechreport …-r2-mech-nc.jsonl.gz`; shipped rule `$R local …-r3-cover851-ctl.csv.gz …-r3-cover851-arm.csv.gz` (`$G cover 851-854`), `…-r3-cover851-protodef.csv.gz`, `…-r3-cover841-{ctl,arm}.csv.gz` (`$G cover 841-844`) |

The banks: `$G ctl|h2h <rule>|h2h-eligible <rule>|targeted <seeds> <comps> [NoCover]`, played with
`--batch --jobs 4 --trace-mode off`; the harness banks with
`HHX_TRACE=0` (`HHX_KEEP_LOGS=<dir>` for the log join).
