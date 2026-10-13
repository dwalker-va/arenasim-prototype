# AS-249: healer positioning — findings

Card AS-249, from the 0.7 review (FB-1 c1, c2). Four items: keep heal range on a
focused teammate, stop the range-edge bob, take a pillar when left alone against
two, and the Warrior's step back in the Purge replay. Each is attributed from the
per-frame observer and the decision trace of the two replays the user watched:

- **c1** — Warrior+Druid v Hunter+Shaman, PillaredArena, seed 3 (`Legacy`).
- **c2** — Mage+Shaman v Warrior+Druid, BasicArena, seed 1 (`Legacy`).

Both replays are `Legacy`: it is the default profile, so the user watched it.

## 1. No range-edge bobbing (the cause of c1's box-out too)

**Cause.** A caster targeting a healer pressures it (`situation::pressured`)
only while *closing*: farther than its own preferred range (28yd for the
Shaman, Druid, Priest and Warlock) and inside `threat_intent_radius` (30yd).
That band is 2yd wide. PRESSURED entered and left on that one test:

- In c1 the Druid's trace shows `PressuredEnter` → `PressuredExit` every
  2.5-3.0s from 10.9s to 24.9s (combat clock), 6 cycles while its Warrior
  lived. Each entry took it west (`chosen_direction` [-1, 0]) for
  `pressured_hold` (1.5s); each exit handed it to the FREE formation point by
  its Warrior, 50-90yd east, past the Shaman, which walked it back into the
  band. The Druid oscillated between x = -21 and -31 while its Warrior
  charged the Hunter to x = 66: 43 → 96yd from it, out of heal range for the
  last 14s of the Warrior's life. That is c1's "boxed out by the shaman".
- In c2 both healers do it to each other (each targets the other), and after
  the Warrior dies the Druid's FREE pursuit to its own preferred range walks it
  back into the Shaman's band: the "bobbing, especially once their partner is
  dead".

Measured as heading reversals (a 0.5s step turning back within ~45° of its last
heading), per minute of healer life:

| | `main` | branch |
|---|---|---|
| c1 Druid | 15.7 | 2.2 |
| c2 Druid | 15.9 | 0.0 |
| c2 Shaman | 14.3 | 2.3 |
| 48-match `Legacy` bank (`tests/healer_positioning.rs`) | 14.3 | 3.3 |

**Fix (`Legacy`).** A hysteresis gap on the closing branch only: a FREE healer
enters PRESSURED only when a closing threat is `danger_edge_margin` (1yd)
inside both edges of the band (`AllyStanding::pressure_onset`); it stays
PRESSURED on the plain test. For a 28yd caster the onset band is empty, so a
caster closing to cast range no longer flips the posture; a Hunter (25yd)
still onsets between 26 and 29yd. Proximity, melee and pets onset unchanged.

**`TeamPlan` keeps the plain onset.** Its PRESSURED is the team solve's
`OccupyCover`, which holds the healer in cover within reach of its ally rather
than backing off and letting go. With the gap under `TeamPlan` too, the
Warrior+Priest v Warlock+Priest Nagrand camp lost its heal line on a median 30%
of frames (seeds 1-12, `scan_nagrand_teamplan`) against 16% on `main`; without
it, 14%.

**What it costs.** The bob was, by accident, a kite: it kept healers near the
edge of a closing caster's range. A `Legacy` healer now stands where its
formation puts it, in that range. Directional sweep, both maps, scored from
each healer's side (below): Priest -2.1pt on BasicArena (z -2.9), Shaman
+3.0pt there (z 3.8), Druid +2.6pt and Paladin -1.7pt on Nagrand; pooled over
the four healers +/-0.2pt, not resolvable. The Nagrand camp comp shows the
cost per frame: the `Legacy` Priest stands 33.5yd from the nearest caster
(36.7 on `main`) and spends 8.2s hard-CC'd (3.3s), over 12 seeds.
A deliberate caster standoff — keep out of a closing caster's reach without
strobing, heal range first — is the follow-up; two formation-point versions
tried here jittered (a radial push is ill-conditioned when the allies fight at
the caster's feet) and were not shipped.

## 2. Heal range on a focused teammate

**Cause.** A PRESSURED healer's scorer masks every step that leaves heal range
or sight of its anchor ally. Once the healer is already out of reach, every
step is masked, and the fallback ladder's first rung dropped the anchor: the
healer fled freely, each step carrying it further off. (c1's Druid was out of
reach for this reason during its PRESSURED entries; the bob is what put it
there.)

**Fix (`Legacy`).** Rung 0 of the ladder (`AnchorConstraint::close_when_out`):
for an anchor an enemy is targeting, keep the steps that end nearer it before
letting it go. It applies in PRESSURED only; ESCAPE windows keep the plain
ladder. Staged (`tests/healer_positioning.rs`): a Druid 46yd from its focused
Warrior with an enemy Warrior on it, between them, closes to 33.9yd in 6s;
with the rung off it drifts to 46.4yd.

**`TeamPlan`: no change.** `OccupyCover` already leashes heal range as a graded
constraint (`C_LEASH`) and makes reaching a dying teammate lexicographic
(`C_DYING`). In c1 under `TeamPlan` the Warrior survives and Warrior+Druid win.

## 3. Losing alone means take cover (`TeamPlan` play)

**Situation and play.** `last_stand` (`situation.rs`): a team's last living
non-pet unit while two or more living non-pet enemies it can see stand
against it. Its healer runs `Play::TakeCover`: it walks to
`team_solve::last_stand_cover` — the fewest enemy casters in sight, then
furthest out of the nearest closer's reach, then nearest — from the moment the
teammate dies, and holds the spot for 2s once there (unless an enemy is inside
`danger_radius`). Gated under `TeamPlan`, withheld per team like Peel and
Burst; inert with no obstacles: on BasicArena every one of 5,400 matches with
the play running matches its withheld twin in winner, duration and end reason
(the sweep CSV's columns), and the staged
`the_last_stand_is_inert_with_nothing_to_hide_behind` holds the full combat log
and every frame's positions identical.

**The hold.** Re-solving every 0.6s commit window let the healer step out of
every cast begun on it the moment it was seen. In the juke probe's bank
(Mage+Priest v Warrior+Shaman, TwinPillars) that held seed 57 to a draw at the
300s cap: the Mage never landed a Frostbolt in 255s. With the 2s hold (longer
than a Frostbolt) every match in the bank resolves.

**Measured** (three-arm head-to-head, 2v2, PillaredArena, 3 seeds × 900 cells,
both teams `TeamPlan`): the play moves no wins (scored from the covering side,
45.9% → 45.8%, 6 up / 9 down). It turns a few lost 1v2s into draws: 258 matches
without a winner in the paired baseline, 273 and 279 in the two arms that run
it (~+0.7pt), and matches run ~3s longer. Staged
(`tests/healer_positioning.rs`): the Druid left alone against Hunter+Shaman is
out of both enemies' sight on 67% of frames in the 15s after its Warrior dies,
first within 5s; 0% with the play withheld.

## 4. The Warrior's step back (c2)

**Attributed.** The trace's only Warrior `movement_decision` is `MeleeReset`
(11.58s combat): the Warrior tempo reset (`warrior::evaluate_warrior_reset`).
The Mage's Frost Nova rooted it 7.5-14.5s (combat); the root armed the reset,
and with Charge still on cooldown and the Mage out of melee once the root
broke, the Warrior walked toward its Druid (`MovementGoal::Point` at the
Druid's position) for the 3s window, then turned back. It is the reset working
as designed; the design is what reads as "unclear" — it walks a Warrior 25yd
toward its healer when its kill target is 9yd away. Not the bob: filed as a
follow-up rather than changed here.

## A Druid defect found on the way

A Druid rooted with a threat on it owes Travel Form its next global cooldown
(one emergency heal may go first). Mark of the Wild headed the rotation ahead
of the shift, so a Mark the Shaman purged cost the Druid a GCD rooted under its
chaser (Rogue+Druid v Hunter+Shaman, BasicArena, seed 1, which the onset gap
moved onto it; `druid_control_probes` caught it). The
Mark now waits behind an owed shift, as it already waited behind a dying
teammate's heals (`DruidTurn::take`), pinned by a staged test.

## Sweeps

All under `docs/design/balance/sweeps/` (inputs and outputs gzipped):

- **`Legacy` before/after, DIRECTIONAL.** `2026-10-13-as249-legacy-{basicarena,pillaredarena}.jsonl.gz`
  (`gen_sweep.py --full 2 --exclude-double-healer --affects Priest,Paladin,Shaman,Druid --control-cells 8 --n 10`,
  the second with `--extra '{"map":"PillaredArena"}'`), 8,080 matches per
  arm per map; `main` @ 51b5c046 vs the branch. `paired_sweep.py`: control
  80/80 identical on both maps; team-1 slices not resolvable (the change is
  symmetric). Scored per healer side by `2026-10-13-as249-healer-side.py`:

  | healer side | BasicArena | PillaredArena |
  |---|---|---|
  | Priest | -2.1 (z -2.87) | +0.6 (z 0.67) |
  | Paladin | -0.4 (z -0.58) | -1.7 (z -2.22) |
  | Shaman | +3.0 (z 3.77) | -0.6 (z -0.66) |
  | Druid | -0.8 (z -0.99) | +2.6 (z 3.08) |
  | pooled | -0.1 (z -0.18) | +0.2 (z 0.58) |

- **TakeCover three-arm.** `2026-10-13-as249-cover.jsonl.gz` from
  `2026-10-13-as249-gen.py cover 3`, analysed by `2026-10-13-as249-cover.py`.

Directional tier: these are directions, not class standings.

## Probes moved, and how

The onset gap changes most `Legacy` healer matches, so seed-pinned probes whose
situation it moved were migrated — to seed banks, staged matches or rates —
never re-pinned to a new seed:

| probe | moved to |
|---|---|
| `dispel_walk_edge` rule 1 | staged (Paladin Holy-locked, its Mage feared out of range) |
| `dispel_walk_edge` rule 3 | the existing staged `dispel_walk_dying_first.rs` |
| `paladin_dispel_walk` feared teammate | staged |
| `priest_dispel_walk` feared teammate | staged (its Mage feared out of range) |
| `hunter_one_trap` Frost Trap hold | staged |
| `hunter_trap_trace` aimed / dispeller / pressure traps | banks; the dispeller trap's spring as a 2/3 rate; the polearm swing in the kill-target case as happening, not per match (the scenario fell from ~3 in 4 seeds to a handful in 48) |
| `movement_probes` OOM wand | staged (Mage out of mana v a lone Shaman) |
| `movement_probes` critical heal in a window | staged |
| `movement_probes` Mage recovery / los_seek term | the existing occlusion bank |
| `movement_probes` press probe | the existing press bank |
| `movement_probes` chase / juke banks | the lone Shaman under `TeamPlan` (TakeCover hides it: the hiding the chase exists to end) |
| `movement_probes` self-block attribution | TakeCover withheld (the pathology is the 2v2 camp's) |
| `druid_control_probes` root break | a comp × seed bank; the emergency-heal floor 10 → 5 (the hand-picked list chose matches for their heals) |
| `frame_rate_determinism` crowd-control visuals | a five-seed bank, visuals pinned as its union |
| `victory_choreography` three winners / pillar slot | small banks; the Nagrand comp swapped to one whose three winners usually survive |
| `determinism_pin` 2v2 | re-recorded (intended), first diff named in place |
