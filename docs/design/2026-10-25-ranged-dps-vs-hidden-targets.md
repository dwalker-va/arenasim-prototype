# Ranged DPS against hidden targets (AS-389, with AS-392): design

Status: **BUILT (AS-389), as §6.1 describes; the user's rulings are §7. The
endgame pincer is BUILT as the `Hunt` play (AS-394, §6.2 and §9).**
The diagnosis (§1-§5) is on `main` @ `03f77e9e` (AS-385 merged: the sight
yield, AS-386, and the pillar stance, AS-387); the build's measurement is §8.
`TeamPlan` only throughout; `Legacy` is byte-identical.

## 1. The answer, short

1. **Most of the "blindness" is not a fight.** 77% of a ranged DPS's lining
   time (blind to its own target within reach, free to act) falls in the
   **lone-healer endgame**: the enemy is down to its healer, we outnumber it,
   and it hides behind a pillar (the `TakeCover` play). The outnumbering side
   wins 97% of those endgames. They fill 44% of all 2v2 match time.
2. **In the contested part, the cause is target choice, not chase timing.**
   A ranged DPS keeps a target it cannot see because `acquire_targets`
   re-targets only when the target becomes invalid, and sight is not part of
   validity. A prototype that switches a ranged DPS off a target hidden for
   0.5s onto an enemy in reach and in sight wins **+4.3pt head-to-head
   (z +14.7)**, +7pt for every ranged DPS class. A faster seek-chase *loses*
   (-1.0, z -4.2); unmasking the seek pass, and walking to a sighted vantage
   point, do nothing.
3. **AS-392 is not the ranged DPS's own mask.** Unmasking the ENGAGE
   seek-LoS pass moves 23 matches in 10,080. In a paired 2x2 of the package
   (own x enemy, 8 seeds), the TwinPillars Mage gains +2.2 (z +2.8) from its
   own team's package and loses -2.5 (z -2.8) to the enemy's; net -0.4 (ns).
   The -3.3 that AS-385 reported against pre-package `main` sits in the step
   that survives switching both pieces off (-3.2, z -2.47): design D3, which
   retired AS-373's lone-healer hug. Under the target switch, the TwinPillars
   Mage gains +8.2 in a uniform world.
4. **Built:** the **target switch as a team focus situation** (§6.1),
   +4.4pt head-to-head (z +25.5) over three seed blocks (§8). AS-392 is merged
   into this card and closed. The endgame pincer (§6.2) is AS-394's `Hunt`
   play (§9): endgames about 19% shorter (about 11s) where the team can
   staff both stations, about 2.5s over all endgames; wins and draws
   unchanged.

## 2. Instrument and banks

**Harness:** `tests/team_sight_cohesion.rs` (AS-385's), extended with a
`ranged` row per Mage, Hunter and Warlock: its lining seconds, split by what
held it there (§3.2), plus overlapping tallies. Per frame of lining it records:

- whether another living non-pet enemy stands in reach (36yd) and sight;
- the DPS's own sight yield and team stance, and the target team's stance
  (from the trace's `situation_change` events);
- the kiter's seek-chase bucket, replicated from `tick_kite_occlusion`
  (fill 1/s while occluded inside preferred range, drain 0.5/s, reset on a
  target change), and whether a `Point` chase directive is live;
- the nearest standable spot that sees the target (rings of 2-16yd, 16
  headings), and whether that spot also keeps the DPS's healer in sight;
- whether the target is the enemy's last living non-pet unit.

**Banks:** AS-385's package cells (`2026-10-24-as385-package-gen.py`): every
2v2 healer team against every 2v2 team, plus the six one-healer 3v3 teams,
on Nagrand (PillaredArena) and TwinPillars, 1,260 cells.

| run | seeds | arms | used for |
|---|---|---|---|
| mechanism | 501-502 | `pkg-h2h off`, `t1`, `t2`, `uniform` (harness, trace on) | the split (§3), lining per factorial cell |
| factorial | 501-508 | the same four (`--batch`) | AS-392 (§4), 20,160 sides per cell |
| prototypes | 501-504 | each prototype on one team, both seats (`--batch`) | the directional estimates (§5) |

**Reproduce:** `docs/design/balance/sweeps/2026-10-25-as389-*`:

- the reducers: `-reduce.py` for the split, `-endgame.py` for the endgame,
  `-factorial.py` for the 2x2;
- the rows and CSVs;
- `-prototypes.patch`, the prototype code. It is env-gated: `AS389_P` names
  the prototypes and `AS389_TEAMS` the teams that run them. With `AS389_P`
  empty, the patched binary's CSV is byte-identical to `main`'s over the
  5,040-match bank.

The `main` binary also reproduces AS-385's committed `pkg-ab-uniform` CSV at
seeds 501-504 with 0 flips. Win rates are scored with AS-385's
`2026-10-24-as385-package-score.py seat`.

**Tier: DIRECTIONAL** (`sweep-tiers.md`). Nothing here is a class's standing.

## 3. Measurements

### 3.1 Where the lining time is

Ranged DPS lining (uniform `TeamPlan`, package on, 3,600 DPS-matches):

| map | class | lining (share of post-contact life) | of which endgame | target is a healer |
|---|---|---|---|---|
| Nagrand | Mage | 25.1% | 93% | 98% |
| Nagrand | Hunter | 23.4% | 85% | 96% |
| Nagrand | Warlock | 24.6% | 78% | 97% |
| TwinPillars | Mage | 26.9% | 80% | 88% |
| TwinPillars | Hunter | 21.0% | 57% | 89% |
| TwinPillars | Warlock | 20.9% | 61% | 93% |
| both | all | 23.7% | **77%** | 94% |

**The lone-healer endgame** is one team reduced to its healer while the other
still has two or more units. In the 2v2 cells:

| | `Legacy` (AS-385 rows) | `TeamPlan`, package off | `TeamPlan`, package on |
|---|---|---|---|
| matches that reach it | 65% | 64% | 64% |
| median / mean length | 24.8s / 37.6s | 31.9s / 47.1s | 34.2s / 50.4s |
| share of all match time | 41% | 43% | 44% |
| outnumbering side wins / healer wins / draw | 96.6 / 2.3 / 1.1% | 96.9 / 1.4 / 1.7% | 97.0 / 1.4 / 1.6% |

By healer, the mean endgame is longest for the Paladin (67s) and Shaman
(56s), and shortest for the Priest (34s).

- `Legacy`'s lone healer is just as hard to finish, but it is not hidden:
  `Legacy` lining is 0.4-1.4%.
- `TeamPlan`'s `TakeCover` hides the lone healer and adds about 7-9s to the
  median endgame. It changes how often the endgame is won by under a point.

So the 19-27% that AS-385 §2.2 called self-CC is mostly a won endgame running
long, not a contested fight lost.

### 3.2 Why the DPS stays blind: the cause split

Exclusive partition of lining time, first rule that holds:

| cause | test | share (all / endgame) |
|---|---|---|
| pressed | an enemy melee or pet within 10yd: kiting or tanking | 17.2 / 7.5 |
| **(i) switchable** | another enemy in reach *and* in sight | **6.8 / 0.0** |
| **(iii) yield-blocked** | the DPS yields to its healer, and no spot within 8yd sees both | 5.6 / 5.0 |
| **(ii) bucket filling** | kiter, target inside preferred range, bucket below 3.5 | 9.9 / 8.7 |
| beyond preferred range | kiter, pursuit (not the bucket) is moving it | 6.6 / 5.4 |
| **(ii) chase armed, still blind** | kiter, the bucket has armed the `Point` chase | 34.1 / 32.3 |
| pursuing | Warlock: pursuit walks for sight at once, no bucket | 19.7 / 17.9 |

**(iv) The target hides under the enemy's stance or yield.** The enemy holds a
stance during 4.9% of lining time (TwinPillars Mage 8.8%). The rest is the
enemy healer's own cover solve (AS-362, AS-381) and `TakeCover`.

**The counterfactual split.** These are the paired factorial arms, the
package switched on per team. Lining shares are for ranged DPS, both maps,
2 seeds:

| own package | enemy package | Mage | Hunter | Warlock |
|---|---|---|---|---|
| off | off | 23.4% | 20.3% | 19.9% |
| on | off | 24.6% | 21.7% | 21.3% |
| off | on | 25.4% | 21.1% | 21.2% |
| on | on | 26.0% | 22.2% | 22.7% |

- **(iii)**, the DPS's own yield and stance pull: about +1.2-1.4pt of lining.
- **(iv)**, the enemy's yield and stance: about +0.8-2.0pt.
- **The rest, about 20-23pt, is there with both switched off.** It is the
  enemy healer's cover and `TakeCover`, which AS-385 did not add.
- Neither piece is where the win rate moves: the Mage's own package raises its
  lining and also its win rate (§4).

**(ii) is geometry, not timing.** Two facts:

- At the moment of lining, the nearest spot that sees the target is within
  2yd for 53% of the time, and within 8yd for 88%.
- Yet the armed chase (34%) and the Warlock's pursuit (20%) stay blind.

A unit hugging a pillar of radius `r` is visible from distance `D` only within
an angle of about `acos(r/D) + acos(r/r_h)` of the viewer's bearing, measured
round the pillar's centre, where `r_h` is the hugger's distance from the
centre:

- For a 6yd pillar and a hugger at 6.5yd: about 101° from 30yd, and 76° from
  10yd.
- **Closing on the hugger shrinks the arc it must stay out of.** The hugger
  also turns round the pillar about `D / r_h` times faster in angle at the
  same run speed.
- A lone chaser therefore cannot out-angle a pillar hugger. The chase walks
  the kiter onto the hugger and stays blind: the diagnostic trace of
  `Mage+Priest v Warlock+Priest` shows 30s of exactly that.
- **Two viewers about 160° or more apart round the pillar leave it nowhere to
  hide.**

That is why a faster chase loses (§5) and why the endgame's answer is a
pincer, not a better chase.

## 4. AS-392: the TwinPillars Mage

The paired 2x2 at 8 seeds, 2,400 sides per map:

| slice | own effect (enemy off / on) | enemy effect (own off / on) | both vs neither |
|---|---|---|---|
| TwinPillars Mage | +2.2 (z +2.78) / +2.1 (z +2.72) | -2.5 (z -2.78) / -2.5 (z -2.83) | -0.4 (z -0.37) |
| Nagrand Mage | -1.4 (z -2.08) / -0.3 (ns) | -2.2 (z -2.61) / -1.1 (ns) | -2.5 (z -2.65) |
| all classes | +0.6 (z +2.39) | -0.6 (z -2.37) | 0.0 |

The three suspects in the card:

- **Its own yield mask:** unmasking the ENGAGE seek-LoS pass (prototype
  `unmask`) moves 23 sides in 10,080 (-0.0, z -1.04). The AS-386 tester's
  0.7pt estimate does not reproduce. Its own package is a *gain* on
  TwinPillars.
- **The stance pull:** inside "own effect"; positive on TwinPillars.
- **Targets hidden by the enemy's yield and stance:** -2.5 on TwinPillars.
  - With the enemy package on, the enemy team stands all-hidden 19.3% of the
    time (14.6% off), and its healer 46.7% (40.4%).
  - The healer's hard-CC'd time against a Mage falls from 4.6% to 4.1%.
  - That is the stance doing its job: `AvoidCc` keys on a ready Polymorph.

**Where AS-385's -3.3 went.** Against pre-package `main` (AS-385's
`pkg-main-uniform`), the "neither piece" arm already costs the TwinPillars Mage
-3.2 (z -2.47, seeds 501-504). That arm differs from pre-package `main` in
what AS-387 changed unconditionally: D3 moved AS-373's per-healer hug under
the stance, so with Pillar withheld no healer hugs at all.

The step from "neither" to the full package is -0.1 on TwinPillars. The
Mage's measured loss is therefore D3's retirement of the lone hug, not the
yield or the stance. This is attribution by arm, not a mechanism read. The
mechanism behind D3's cost to the Mage is not measured here.

**Under the target switch (§5.1), uniform world:** the TwinPillars Mage gains
+8.2 (z +5.84) and the Nagrand Mage +4.5. That is more than the -3.3.

## 5. Candidates and their directional estimates

Head-to-head at seeds 501-504: the prototype on one team, against unmodified
`main` on the other, in both seats (10,080 sides). Lining is from the uniform
harness run at seeds 501-502.

| candidate | prototype | win, head-to-head | ranged classes | lining (uniform) | verdict |
|---|---|---|---|---|---|
| **target switch** | a ranged DPS whose target has been out of its sight 0.5s switches to the nearest enemy in reach (36yd) and in sight (`switch`) | **+4.3 (z +14.69)** | Hunter +7.5, Mage +7.1, Warlock +6.9 | 23.7 → 22.7%; switchable 6.8 → 0.4% of it | **build** |
| the same, 2.0s delay | `switchd2` | **+4.6 (z +15.88)** | Hunter +9.2, Mage +6.9, Warlock +6.6 | — | the delay is not sensitive |
| faster seek-chase | timeout 3.5 → 1.0s (`chase1`) | -1.0 (z -4.24) | Mage -2.3, TwinPillars Mage -4.6 | — | reject: closing on a hugger shrinks the sight arc (§3.2) |
| seek pass ignores the yield mask | `unmask` | -0.0 (23 flips) | — | — | reject: the mask is not binding |
| walk to the nearest sighted spot, keeping the healer in sight | `vantage`, Mage, Hunter and Warlock | +0.3 (ns) | Mage +3.1, but its draws rise from 23 to 156 | 23.7 → 18.7% | reject as built: blind time falls, wins do not move, endgames stall |
| the same, the DPS leading (no healer sight) | `vfree`, the "push" bound | +0.0 (ns) | Mage +2.6 (draws 172), Warlock -1.5 | 18.8% | reject: dropping the yield buys no wins |
| switch + vantage | `vswitch` | +4.4 (z +12.47) | as switch | — | the vantage adds nothing |
| switch + pincer | `swpincer` | +4.5 (z +13.94) | as switch | — | additive: the pincer adds nothing to wins |
| **endgame pincer** | in the lone-healer endgame, our healer takes the far side of the hider's pillar from our DPS (`pincer`) | -0.1 (ns) | Druid -1.9 (z -4.73), Priest +1.4 | 23.7 → 21.0% | optional: mean endgame 50.4 → 43.4s, wins unchanged |

**What the switch does in a uniform world** (both teams switch, 10,080
sides):

| class | change |
|---|---|
| Hunter | +5.9 |
| Mage | +6.4 |
| Warlock | +3.9 |
| Warrior | +2.3 |
| Rogue | **-10.0** (z -11.1) |
| healers | -0.8 to -4.0 |

The DPS spread compresses from 39-58% to 45-52%. The Rogue was the
strongest DPS: when a ranged DPS's target hides, the unit it can see is
usually the Rogue on top of it. That is the right target and a balance shift
the user should see (D2).

**Why the switch is worth so much more than its 6.8% of lining.** It moves the
kill order, not just the blind seconds. The ranged DPS's blind time falls only
1-5pt (Warlock 25.8 → 20.9%, Hunter 26.9 → 21.9%). But the enemy DPS that its
healer leaves exposed while it hides now takes the damage. Today
`acquire_targets` keeps the nearest enemy at acquisition until it dies, goes
stealthed or turns immune. Sight is not part of validity, so a ranged DPS
tunnels a healer it cannot see. Melee already has a swap (bucket A,
`swap_hysteresis`); ranged has none.

## 6. Proposal

### 6.1 Built: a team focus that leaves a hidden target

**Where it lives.** The `target_focus` team situation
(`class_ai::target_focus`, classified in `situation.rs` with the rest of the
team's situation, from the shared snapshot). The team decides; target
acquisition (`combat_ai::acquire_targets`) acts on it through
`target_focus::retarget`, reading the previous frame's classification.

**The situation.** `TeamPlan` only, on a map with obstacles. Per ranged DPS
(Mage, Hunter, Warlock) the team keeps how long its target has been out of its
sight (`TargetSight`, the cast's own sight test). When one has been blind for
`FOCUS_HIDDEN_SECS` and another living, visible, non-immune, non-pet enemy
stands within `FOCUS_REACH` of it and in its sight, the focus moves to the
nearest such enemy: `Focus { subject, from, since }`.

- `FOCUS_HIDDEN_SECS` = 1.5s. The prototype measured 0.5s and 2.0s within
  half a point of each other (§5); 1.5s, one global cooldown, lets a target
  that steps out between two casts come back without a switch (24% of the
  blind time is in episodes shorter than 1.5s, §3.1).
- `FOCUS_REACH` = 35yd, Auto Shot's range: the shortest of the three ranged
  classes' main attacks.

**Anti-strobe.**

- **Dwell:** a focus holds `FOCUS_DWELL` (2s) before it can move again.
- **The return's hysteresis:** there is no separate return rule. The focus
  goes back to its old target by the rule that left it: the focus itself out
  of sight for `FOCUS_HIDDEN_SECS`, and the old target in reach and sight. A
  target that juked out once is not returned to the moment it steps back.
- A focus ends only when its subject can no longer be taken (dead, stealthed,
  immune).

**Who takes it.** A ranged DPS takes the focus in acquisition when the focus
stands in its reach and sight, or when its own target is out of its sight
too; a second ranged DPS shooting a target it can see, with the focus out of
its reach, keeps its own. **Melee DPS do not follow it:** their target-blind
time is under 1% of their life (AS-385 §2.2: Warrior 0.9%, Rogue 0.5%), they
have their own swap (bucket A), and a team focus across the arena would cost
them swing time. The Hunter's pet follows its Hunter's target, as always.

**Interactions.**

- A DPS on the team's kill window subject (the Burst play's) never leaves it:
  its own blindness moves no focus, and it takes none another DPS moved.
- The player's kill call (`team{n}_kill_target`) wins over the focus.
- The Hunter's trap retarget runs after it, so it still moves the Hunter off a
  healer its own trap holds.

**Trace and switch.** Each onset (or move) and end is a `situation_change`
named `target_focus` (`subject` the focus, `from` the target it left);
`ability_decision`s carry it as `situation.target_focus`.
`team{1,2}_target_focus: false` is the measurement switch.

**Tests.** Scenes (`tests/target_focus_scenes.rs`), each a pair: hidden versus
in sight; an alternative in reach and sight versus none (behind cover, out of
reach, immune); the nearest of two; `Legacy`, withheld and melee; the kill
window, for the DPS that moves the focus and for a second one on the kill
window's subject; the dwell; the return's hysteresis; a focus that dies,
stealths or turns immune; taking the focus
(ranged versus melee, a second ranged DPS by reach or by its own blindness,
an immune focus). A staged TwinPillars match (`tests/target_focus_staged.rs`)
plays the switch with the focus on, withheld, and under a kill call. Every
condition was mutation-proved (§8).

### 6.2 The endgame pincer ("Hunt"; built by AS-394, §9)

`TakeCover`'s counterpart, a play for the team that outnumbers a lone hidden
healer:

- the team spreads round the hider's pillar, about 160° or more apart, so it
  has nowhere to hide;
- our healer, with nothing to heal, takes the far side;
- in 3v3 it can be the second DPS instead.

The crude prototype shortens the mean 2v2 endgame by 14% (50.4 → 43.4s) and
the median by 3s. It moves no wins, because the endgame is already won 97% of
the time, and it costs the Druid side 1.9pt in its prototype form.

Its value is match length and how the endgame reads, not win rate. It also
gives `TakeCover` an answer, so that play's measured cost (draws) cannot
grow.

### 6.3 Not proposed

- **A faster or more aggressive seek-chase:** it measures negative, and §3.2's
  geometry says why.
- **Exempting the seek pass from the yield mask:** nothing to gain.
- **A vantage walk:** it cuts blind time by a fifth and buys no wins. A team
  "push" that drops the yield (`vfree`) also buys nothing. The stance's
  denial is real, and the answer to it is the switch: shoot what you can see.
- **Revisiting D3 here.** Whether the lone-healer hug should come back,
  outside the stance, is AS-385's question (D4 below).

## 7. Rulings (user, 2026-10-24)

- **D1.** Build the target switch as a `TeamPlan` team focus situation (§6.1).
- **D2.** Accept the balance shift (the Rogue down, the ranged DPS up), with
  no Rogue exception; the 0.8 milestone sweep re-baselines.
- **D3.** AS-392 is merged into this card and closed: its own mask measures
  zero, its stance pull positive, and the enemy's stance (-2.5) is answered by
  the switch (§8: the TwinPillars Mage +6.9 in a uniform world).
- **D4.** No measurement card for the lone-hug retirement (AS-385 D3).
- **D5.** The endgame pincer is AS-394, low priority.

## 8. The build, measured

**Tier: DIRECTIONAL**, replicated. Banks: `2026-10-25-as389-focus-gen.py`
(AS-385's package cells, both teams `TeamPlan`, the AS-385 package on) at
fresh seeds in three blocks, 601-604, 605-608 and 609-612: 5,040 matches per
arm per block, four arms (`off`, `t1`, `t2`, `uni`), 60,480 matches. Scorer:
`2026-10-25-as389-focus-score.py`, paired at identical seeds, McNemar on the
flips per block and pooled. A slice counts as an effect only when its sign
agrees in all three blocks and its pooled |z| is at least 2. Block 601-604 ran
on a build that differs from the shipped one only by formatting and tests;
its `uni` arm reproduces the shipped binary's byte for byte.

**Head-to-head** (the side running the focus, against the same side without
it; the other team without it; both seats; 30,240 sides pooled):

| slice | 601-604 | 605-608 | 609-612 | pooled |
|---|---|---|---|---|
| all | +4.3 | +4.4 | +4.4 | **+4.4 (z +25.5)** |
| Nagrand / TwinPillars | +2.4 / +6.1 | +2.6 / +6.3 | +2.7 / +6.1 | +2.6 (z +13.1) / +6.2 (z +21.9) |
| Hunter | +9.0 | +10.0 | +8.8 | +9.3 (z +19.1) |
| Mage | +6.9 | +8.0 | +7.5 | +7.5 (z +17.8) |
| Warlock | +5.4 | +4.6 | +6.2 | +5.4 (z +12.0) |
| Warrior | +4.6 | +4.3 | +4.6 | +4.5 (z +14.9) |
| Rogue | +0.1 | +0.6 | +0.8 | +0.5 (z +2.0), at the bar |
| TwinPillars Mage | +9.8 | +11.2 | +11.7 | **+10.9 (z +15.2)** |
| 3v3 | -0.4 | +1.9 | +3.5 | +1.7 (z +1.5), not an effect |

Every healer class gains (Druid +1.3, Paladin +3.1, Priest +3.5, Shaman
+3.1, all z ≥ 4).

**Uniform** (both teams run it, against neither; every side):

| class | 601-604 | 605-608 | 609-612 | pooled |
|---|---|---|---|---|
| Hunter | +7.5 | +8.3 | +7.2 | +7.7 (z +14.6) |
| Mage | +6.2 | +6.2 | +5.9 | +6.1 (z +12.3) |
| Warlock | +3.5 | +2.8 | +4.5 | +3.6 (z +7.3) |
| Warrior | +1.5 | +1.1 | +1.4 | +1.3 (z +3.0) |
| **Rogue** | -10.6 | -10.1 | -10.2 | **-10.3 (z -19.3)** |
| TwinPillars Mage | +6.8 | +6.8 | +7.2 | +6.9 (z +8.5) |
| Druid / Priest / Shaman | | | | -2.0 / -3.5 / -2.9 (each z ≤ -4.3) |
| Paladin | | | | -0.8 (not an effect) |

The Rogue's loss is the ruled balance shift (D2): when a ranged DPS's target
hides, the enemy it can see is usually the Rogue on it.

**Blind to its target** (the AS-385 harness, `tests/team_sight_cohesion.rs`,
seeds 601-602, 2,520 matches per arm, ranged DPS, share of post-contact
life):

| class | focus off | focus on |
|---|---|---|
| all ranged | 26.4% | 23.1% |
| Hunter | 24.8% | 20.3% |
| Mage | 28.0% | 27.2% |
| Warlock | 26.2% | 21.7% |
| TwinPillars Mage | 27.8% | 25.9% |

The share of lining spent with another enemy in reach and sight (the
`switchable` cause, §3.2) falls from 7.4% to 1.1%. The Mage moves least: its
blind time is the lone-healer endgame (§3.1), where there is nobody else to
shoot.

**Identity.**

- `Legacy`: all 1,260 cells of the bank at seed 601, both teams `Legacy`, as
  full `--headless` match logs on `main` and on the build: `diff -r` clean
  over 31MB. Non-vacuous: 2,872 eliminations, 8,708 crit lines, 1,011
  distinct durations; winners team 1 671, team 2 576, 13 draws.
- `TeamPlan` with the focus withheld from both teams reproduces `main`'s
  `TeamPlan` CSV byte for byte over the 5,040 matches of 601-604.

**`camp_sweep`** (seeds 1-12, n=12): `TeamPlan` 11/12 → 10/12, heal to the
Warrior 323 → 267, occlusion after contact 20.8 → 22.1s (not down). One
discordant match; the Warlock now leaves the hidden Priest for the Warrior.
At n=12 this is not evidence either way.

**Mutation proof.** Each of 22 mutants failed a named test (the last three
added in review: the held focus judged by alive-and-not-immune, or by
alive-and-not-stealthed, instead of acquisition's validity; and `retarget`
ignoring the kill window): the hidden
threshold, the dwell, the carried clock, the alternative's reach, its sight,
its immunity, the nearest choice, the kill window, melee following,
retarget's two clauses and its validity check, a dead focus held, the
`TeamPlan` gate (both ways), the withholding switch, the trace event, the kill
call, and the acquisition wiring.

**The kill-window fix, re-measured** (review round 1 found `retarget` pulling
a second ranged DPS off the kill window's subject; it now keeps it). On `main`
@ `6eb17472` (AS-388 merged), block 609-612 was played twice, by the fixed
build and by the same build without the fix:

- the fix changes 1 match of 5,040 in the head-to-head `t1` arm, 1 in the
  `uni` arm, and none in `t2` or `off`;
- head-to-head +4.6 (z +15.5) both ways, Hunter +9.3, Mage +7.2, Warlock +7.3,
  TwinPillars Mage +11.1;
- uniform: Rogue -10.6, Hunter +7.6, Mage +5.7, Warlock +5.2, Druid -3.2,
  Priest -2.5, Shaman -2.9; Paladin -1.1 and Warrior +0.8, not effects in this
  block.

The three-block tables above are on `03f77e9e`; this block agrees with them
in every effect's sign and size. Identity re-checked on the merged build:
`Legacy` full logs `diff -r` clean (the same 2,872 eliminations, 8,708 crit
lines, 671 / 576 / 13), and `TeamPlan` with the focus withheld reproduces
`main`'s CSV byte for byte over 609-612. `camp_sweep` on the merged build:
`TeamPlan` 12/12, heal to the Warrior 306.

**Rows and CSVs:** `sweeps/2026-10-25-as389-final-*` (`03f77e9e`) and
`sweeps/2026-10-25-as389-r2-*` (`6eb17472`, block 609-612).

## 9. AS-394: the `Hunt` play, built and measured

### 9.1 The shape

`class_ai::hunt`, a `TeamPlan` play; the module docs and CLAUDE.md (*Plays*)
carry the rules. In short:

- **The situation** `hunt`: the enemy's only living non-pet unit is a healer
  within `HUNT_ENTER` (6yd) of a pillar's skin, held to `HUNT_LEAVE` (10yd);
  the team has two or more units; at onset the healer is hidden from one of
  them. The axis is the healer's bearing round the pillar at onset, and
  carries while it stays at that pillar.
- **The stations**: two points `HUNT_STANDOFF` (14yd) off the skin, at the
  axis ±90°. **180°, not 160°**: from 14yd off a Nagrand pillar, two viewers
  160° apart leave a sliver of the far side in neither's sight, and 180°
  leaves none (`two_stations_leave_a_hugger_nowhere_to_hide` sweeps every
  pillar of both maps, every axis in 15° steps, and every hugging bearing in
  2° steps at the skin and at the cover ring). A station the wall or another
  pillar takes is pulled in, then turned toward the axis.
- **Who**: two units or none (one viewer is §3.2's lone chaser). Ranged DPS
  first; the healer takes the second station when only one ranged DPS does.
  Melee take none: a melee on the healer already sees it. The two are the
  first that qualify in Entity order, not the nearest: a 3v3 with two ranged
  DPS stations the lower-numbered pair.
- **The healer's sight of its team (AS-385 D2)**. A healer on the far station
  cannot see the DPS on the near one. D2 orders sight of an OWED duty before
  everything else, so the healer stations only while no teammate is dying
  (below `urgency_hp_threshold`, where the medic chase and the team solve
  already walk it back into sight), and once one is, not again until all are
  back to `SIGHT_YIELD_HP`. A healthy teammate is owed nothing.
- **Holding** (`hold = !arrived || covered`, per unit): a unit holds its
  station until it first reaches it; after that, only while one of the two
  stationed units sees the healer. An arrived unit therefore gives its station
  up for its ordinary walk for sight as soon as nobody sees the healer, even
  while its partner is still walking to the other station, and takes it back
  once one of them sees it again. In the seed-723 replay the Mage, on its
  station, gives it up at 64.9s while our Priest is feared short of its own.
  A bad station cannot park the team blind.
- **The play** is MOVEMENT only (a `Point` directive from the kiter's ENGAGE,
  the Warlock's dispatch and the healers' `take_cover_tick`); Burst still
  leads the chain. It stands down while the healer is locked, so the Chain
  play extends the lock.
- **`PLAY_ORDER`: Peel, TakeCover, Hunt, Pillar, Chain, Burst.** After Peel
  (a teammate in danger first); beside TakeCover, its counterpart (the two
  never meet: one needs two units, the other one); before Pillar, because a
  team hunting the last enemy does not hide from that enemy's crowd control.

### 9.2 The exceptions, and how they were chosen

Chosen on two selection blocks, then measured on two fresh ones (§9.3).

| block | build | endgame mean | draws | finding |
|---|---|---|---|---|
| 701-702 (2,520) | every healer stations | 47.8 → 48.3 (+0.4, ns) | 23 → 71 | a stationed Paladin: Mage+Paladin +64.4s, Warlock+Paladin +14.4s; a stationed Druid: Warlock+Druid +22.1s, draws 11 → 30. Priest comps -13 to -18s |
| 711-714 (5,040) | Paladin, Druid excepted | 48.6 → 46.3 (-2.4, t -6.0) | 49 → 67 | all 23 new draws had a stationed Shaman (Mage+Shaman +5.6s, Hunter+Shaman +6.7s) |

The common cause: a healer with no attack that costs no mana. On the far
station it takes its sight off its partner, and once that partner is out of
mana or out of sight nobody hurts the lone healer. The Priest has its wand.
So `HUNT_EXCEPTIONS` names the Paladin, the Druid and the Shaman, and the
healer that stations is a Priest.

Rows: `sweeps/2026-10-25-as394-rows-pilot-*-701-702` and `-rows-*-711-714`,
from the two selection builds (this card's history; the `off` arms do not
depend on the build).

### 9.3 Measured

**Tier: DIRECTIONAL**, replicated over two fresh seed blocks, 721-724 and
725-728. Banks: `sweeps/2026-10-25-as394-gen.py`, AS-385's package cells (all
2v2 healer teams against every 2v2 team, the six one-healer 3v3 teams; Nagrand
and TwinPillars; both teams `TeamPlan`, the AS-385 package and the AS-389
focus on): 5,040 matches per arm per block, four arms (`off`, `t1`, `t2`,
`uni`). Endgames read per frame by `tests/hunt_endgame_sweep.rs`; scored by
`sweeps/2026-10-25-as394-score.py` (endgames, paired by seed) and AS-385's
`package-score.py` (wins). Rows: `sweeps/2026-10-25-as394-rows-*`.

**Endgame length** (the lone-healer endgame, onset to match end; paired: the
same matches reach it in both arms):

| | 721-724 | 725-728 |
|---|---|---|
| uniform, mean | 47.2 → 44.9s (-2.3, t -11.7) | 48.9 → 46.3s (-2.6, t -11.3) |
| uniform, median | 31.3 → 30.1s | 32.0 → 30.9s |
| head-to-head, seat 1 hunts | -2.6s (t -7.6) | -2.6s (t -7.8) |
| head-to-head, seat 2 hunts | -2.1s (t -9.1) | -2.6s (t -8.3) |
| Nagrand / TwinPillars (uniform) | -2.5 / -2.1s | -2.8 / -2.4s |
| **a team that can staff two stations**, uniform (744 / 762 endgames) | **57.3 → 46.5s (-10.8, t -12.7)** | **58.8 → 46.9s (-11.8, t -12.1)** |
| ... its median | 45.6 → 36.2s | 49.7 → 38.2s |
| ... head-to-head, seat 1 / seat 2 hunts | -14.9s (t -8.4) / -8.6s (t -9.7) | -14.1s (t -8.6) / -10.7s (t -8.8) |
| a team that cannot | 0.0s | 0.0s |
| any team with a Priest / with a Mage | -8.9 / -4.9s | -9.1 / -6.0s |

A team can staff two stations when it fields two of the Mage, Hunter, Warlock
and Priest (the classes `HUNT_EXCEPTIONS` leaves). About 21-22% of endgames
are hunted by such a team, and those shorten by about 19% (-10.8s of 57.3s,
-11.8s of 58.8s): more than the prototype's 14% where the play can run. The
other 78-79% (a melee and a healer, an excepted healer, two melee) are
untouched, byte for byte, so the same effect spread over every endgame is the
-2.3 to -2.6s above.

**Mechanism**: the share of endgame frames the healer is in a hunter's sight
rises 0.77 → 0.80 (staffed teams 0.66 → 0.78-0.79), and the share with two
hunters 120° or more apart round its pillar 0.09 → 0.14 (staffed 0.03-0.04 →
0.26-0.27).

**Win rate** (must not regress; McNemar on the flips):

| | 721-724 | 725-728 |
|---|---|---|
| head-to-head, both seats (10,080 sides) | +0.0 (5 / 1 flips, z +1.63) | +0.1 (9 / 1, z +2.53) |
| uniform | +0.0 (6 / 6) | +0.0 (10 / 6) |

No class or map slice moves by more than 0.6pt in either block (the largest, Nagrand teams with no healer, +0.6 at 725-728, is a gain).

**Draws**: head-to-head 104 → 104 and 130 → 126 matches; draws inside the
endgame 29 → 29 and 48 → 44.

**With the target focus** (AS-389). In the endgame there is one enemy, so the
focus has nobody to move to; it matters to the hunt only through which
endgames happen (the focus changes the kill order: 3,244 endgames without it,
3,506 with, block 721-724). Paired 2x2 at 721-724:

| | focus off | focus on |
|---|---|---|
| hunt's effect on the endgame | -2.0s (t -9.9) | -2.3s (t -11.7) |
| ... on staffed teams | -11.4s (t -10.6) | -10.8s (t -12.7) |

The two are additive; no rule couples them.

**Identity.**

- `Legacy`: all 1,260 cells at seed 721, both teams `Legacy`, as full
  `--headless` match logs from `main` (`ad30df3e`) and the build: `diff -r`
  clean over 31MB. Non-vacuous: 2,832 eliminations, 1,009 distinct durations;
  winners team 1 677, team 2 563, 20 draws.
- `TeamPlan` with the Hunt withheld from both teams reproduces `main`'s
  `TeamPlan` `--batch` CSV over the 5,040 matches of 721-724: 0 differ
  (winners 2,570 / 2,418 / 52 draws, 3,311 distinct durations).

**`camp_sweep`** (seeds 1-12): identical to `main` (`TeamPlan` 12/12, heal to
the Warrior 306). Its Warrior+Priest cannot staff two stations, and its
Warlock+Priest never meets a lone Priest.

### 9.4 Tests

- **Scenes** (`tests/hunt_scenes.rs`), each a pair: hidden versus in sight;
  at the pillar, the leave radius, a new pillar; a lone healer versus a second
  enemy, a lone non-healer, a team of one; `Legacy`, withheld, before the
  gates; who stations (two ranged DPS, a melee, the shipped exceptions and an
  override); the healer's owed sight and its release, and its own health;
  the axis and onset carrying; a station kept; one station left; holding and
  giving up; each executor (Mage, Priest, Warlock) against the play withheld;
  a stale walk renewed; the healer's hand-back; the lock stand-down.
- **Unit tests** (`class_ai::hunt`): the coverage sweep above with its 160°
  control; a station pulled in, turned toward the axis, and required to see
  the face it guards; the trace's transitions. `plays::tests` pins the order.
- **A staged endgame** (`tests/hunt_staged.rs`): a lone Priest on the far
  face of a Nagrand pillar from a Mage and a Priest. With the play the two
  stand 150° or more apart round it within 8s, and the lone Priest is in a
  hunter's sight 69% of the following frames against 22% with it withheld.
- **Mutation proof**: 42 mutants, one per condition, each killed by a named
  test. One mutant survived first (the "two units" guard before assignment):
  it duplicated the "two stations or none" guard after it, and was removed.

