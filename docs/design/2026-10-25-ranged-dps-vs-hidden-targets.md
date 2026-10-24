# Ranged DPS against hidden targets (AS-389, with AS-392): design

Status: **PROPOSED, awaiting the user's decisions (§7).** Nothing here changes
behaviour. Every measurement is on `main` @ `03f77e9e` (AS-385 merged: the
sight yield, AS-386, and the pillar stance, AS-387). `TeamPlan` only
throughout; `Legacy` is not touched by anything proposed.

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
4. **Recommendation:** build the **target switch as a team focus rule**
   (§5.1). Merge AS-392 into AS-389 (§6). Hold the endgame pincer (§5.2) as
   an optional second card: it shortens endgames by about 14% and moves no
   wins.

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

### 6.1 Build: a team focus that leaves a hidden target

**Where it lives.** A team situation, as the yield is: `focus`, classified in
`situation.rs` from the shared snapshot, traced as a `situation_change`, and
read by `acquire_targets` for the team's ranged DPS. The team decides; the
unit acts. It is not a per-unit predicate in the targeting loop.

**The rule.** `TeamPlan` only. A ranged DPS (Mage, Hunter, Warlock) leaves its
target when:

1. the target has been out of its sight for `FOCUS_HIDDEN_SECS`;
   - the prototype used 0.5s and 2.0s, which measure the same;
   - proposed: 1.5s, one global cooldown;
2. **and** another living, visible, non-immune, non-pet enemy stands in its
   reach and sight. It takes the nearest such enemy, and the team's other
   ranged DPS converge on the same one if they can reach it: one focus, not
   two.

It returns to the old target only by the same rule, when the new target hides
in turn. There is no sticky "home" target, which the prototype did not need.

**Anti-strobe.** The focus holds for at least `FOCUS_DWELL` (2s). A lone
viewer cannot flicker the rule, because the hidden clock resets on sight.

**Interactions:**

- Burst's `kill_window` outranks: a DPS does not leave a kill-window target.
- The Hunter's `trap_retarget` keeps priority as today.
- A configured `team{n}_kill_target` is a measurement override and wins.

**Tests.** A scene pair: a hidden healer and an exposed DPS in reach switches;
remove the exposed DPS and it does not. A staged match on TwinPillars shows
the switch and the return. `Legacy` stays byte-identical by construction,
because the situation is never classified there.

### 6.2 Optional second card: the endgame pincer ("Hunt")

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

## 7. Decisions for the user

**D1. Build the target switch as a team focus situation (§6.1)?**
*Recommended: yes.* It is the one lever that measured: +4.3pt head-to-head,
z +14.7, robust to its delay. The alternative is a per-unit rule in
`acquire_targets`. That is cheaper, and it is what the prototype measured,
but it is not a team decision.

**D2. Accept the balance shift?** In a uniform `TeamPlan` world the switch
moves the Rogue -10pt and the ranged DPS +4 to +6pt, compressing the DPS
spread. *Recommended: yes, and re-baseline at the 0.8 milestone sweep.* The
alternative is to gate the switch to non-Rogue targets. That would be a named
exception without a measurement behind it, which the plays rule declines.

**D3. Merge AS-392 into AS-389?** *Recommended: yes.* AS-392's three suspects
measure as follows:

- its own mask: 0;
- its stance pull: positive;
- the enemy's stance: -2.5. That is the stance working, and its answer is
  this card's switch, which lifts the TwinPillars Mage +8.2.

The residual of AS-385's -3.3 is D3's retirement of the lone hug, which is not
a ranged-DPS mechanism. Close AS-392 as diagnosed, with this doc as its
record.

**D4. The lone hug (AS-385 design D3).** On TwinPillars, switching both
package pieces off costs the Mage -3.2 (z -2.47) against pre-package `main`.
The only unconditional change in that arm is D3. *Recommended: a measurement
card, not a revert.* Run D3 alone, both ways round, before reopening it.
Decide whether to file it.

**D5. The endgame pincer (§6.2) as a second card?** *Recommended: file it
behind D1, at low priority.* It is for match length and readability, and it
moves no wins.
