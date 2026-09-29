# Behaviour baselines

Recorded output of `scripts/behaviour_baseline.sh` — a **determinism reference**,
not a balance measurement.

## What these are for

The claim they support is *"nothing changed"*, not *"these are the win rates"*.
Balance data lives in `docs/design/balance/` and is expected to move; these files
are expected to stay frozen, and a change to one is an event that needs a reason.

The `TeamPlan` migration (`docs/design/team-level-positioning-ai.md`) opens with a
step that must be a **provable no-op**, and the test suite is the wrong instrument
for proving that. The 97 movement probes assert *bounded* properties — "occlusion
≥ 0.5s", "converges in < 200 steps" — so a real behaviour change that stays inside
every bound passes all of them. These files close that gap.

## How to use them

```bash
# Verify nothing changed
scripts/behaviour_baseline.sh | diff tests/baselines/legacy_behaviour_2026-08-01_fixed_timestep.txt -

# Capture a TeamPlan-profile baseline for a paired A/B
AI_PROFILE=TeamPlan scripts/behaviour_baseline.sh > /tmp/teamplan.txt
```

An empty diff means **byte-identical simulation** — not merely the same winner.
The `log_sha256` column digests the whole match log, which for a seeded run has no
wall-clock content, so agreement means every damage roll, movement decision and
ability choice matched. A non-empty diff names the exact cell that moved.

## When to regenerate

Only when behaviour changed **and that change was intended**. Regenerating to make
a diff go away destroys the only evidence that a "no-op" step was one.

Record why in the commit message, and add a new dated file rather than editing an
existing one — the old baseline is the record of what behaviour used to be.

## Files

Newest first. **Diff against the newest.** Older files are kept as the record of
what behaviour used to be — that is the point of dating them rather than
overwriting, and it is what makes a claim like "only timestamps moved" checkable
by someone who was not there.

| File | Captured | Notes |
|---|---|---|
| `legacy_behaviour_2026-09-28_weapon_speed.txt` | 2026-09-28 | **Current.** After every weapon took its swing speed from a named Classic item (card AS-167). Captured against a FRESH run of `main` @ `17cb9f0`, which reproduced the two-hander-budget file exactly. |
| `legacy_behaviour_2026-09-28_two_hander_budget.txt` | 2026-09-28 | After the Warrior's Arcanite Reaper was re-priced at its displaced pair (card AS-115). Captured against a FRESH run of `main` @ `4c4689c`, which reproduced the 09-27 file exactly. |
| `legacy_behaviour_2026-09-27_immolate_rng.txt` | 2026-09-27 | After Immolate's apply burst stopped drawing from `game_rng` (card AS-154). Captured against a FRESH run of `main` @ `a2f483a` — see the note under the table. |
| `legacy_behaviour_2026-09-13_frost_armor_chill.txt` | 2026-09-13 | After the Frost Armor chill became one compound debuff (card AS-54). Captured against a FRESH run of `main` @ `3c61185` rather than against the file below — see the note under the table. |
| `legacy_behaviour_2026-08-02_backlash_ids.txt` | 2026-08-02 | After the `[BACKLASH]` log-id fix. Verified reproducible: two independent runs agreed on all 27 cells. |
| `legacy_behaviour_2026-08-01_fixed_timestep.txt` | 2026-08-01 | After moving the simulation to `FixedUpdate`. Verified reproducible when captured. |
| `legacy_behaviour_2026-07-31.txt` | 2026-07-31, `main` @ `4e71746` | Pre-fixed-timestep. Also verified reproducible when captured. |

### 2026-09-28 — weapon speed from item data (AS-167)

Every weapon now swings at the exact speed of a named real Classic item, with
its per-swing damage scaled so its weapon DPS holds (two-handers gained
Classic's two-hander premium), and Warrior rage per swing scales with the
weapon's speed.

**All 27 cells moved, and the change predicts that.** Every comp here has a
weapon in a live socket on both sides — a wand, a bow, a two-hander or a
mace — so no cell is out of reach. Four flipped winner (`BasicArena
healer_v_healer 1`, `BasicArena ranged_v_melee 4`, `TwinPillars
ranged_v_melee 1`, `PillaredArena healer_v_healer 7`).

The attribution is positive, cell by cell: re-running all 27 on both binaries,
the FIRST line that differs in every log is a ranged auto-attack's per-shot
damage. A slower wand fires fewer, bigger shots at the same DPS, and the
Ashwood Bow's 2.4s shot is slightly smaller than its old 2.5s one:

- `healer_v_healer` on BasicArena and PillaredArena: the Priests' opening Wand
  Shots into each other's shields, 10 -> 13 absorbed;
- `healer_v_healer` on TwinPillars: the Warlock's Wand Shot into the Warrior's
  shield, 7 -> 9 absorbed;
- `ranged_v_melee`: a Priest's (BasicArena, PillaredArena) or the Mage's
  (TwinPillars) Wand Shot;
- `pet_comp`: the Hunter's Auto Shot, 32 -> 31 absorbed.

The balance side is a paired directional sweep, not these cells:
`docs/design/balance/2026-09-28-as167-weapon-speed.md`.

### 2026-09-28 — the Arcanite Reaper re-priced (AS-115)

Two-handers are now priced at what the one-hander + off-hand pair they displace
spends. The only one any default loadout wears is the Warrior's Arcanite Reaper,
which went from 4 attack power to 11 attack power + 3% crit. Its weapon damage
and speed did not change.

**17 of 27 cells moved, and every one of them has a Warrior in it.** The nine
`pet_comp` cells (`Hunter,Shaman` vs `Rogue,Priest`) are byte-identical, log SHA
included. That is a positive attribution: no Warrior means no Reaper, so there
is nothing for the change to reach.

The 18th Warrior cell, `TwinPillars healer_v_healer 4`, is byte-identical, and
its log says why. The Warrior dies at 27s. Its one Mortal Strike logs no damage,
and its other hits — auto-attacks, Heroic Strike, Rend — do not scale with
attack power. None of its crit rolls lands in the 3% window the new crit opens.

The balance side is a paired directional sweep, not these cells:
`docs/design/balance/2026-09-27-as115-two-hander-budget.md`.

### 2026-09-27 — Immolate's burst leaves the sim RNG (AS-154)

Immolate's apply-moment flame burst drew 57-85 values from `game_rng` on every
landing, in headless too. It is now a deterministic marker the graphical client
expands with its own RNG, so every roll after an Immolate landing is
re-randomised. That is the only change: no rule, number or AI decision moved.

**Eight of 27 cells moved, all `healer_v_healer`** — the only comp here with a
Warlock. The ninth, `TwinPillars healer_v_healer 4`, is byte-identical, and the
change predicts that too: both of its Immolate casts were interrupted or never
landed, so it never drew the burst. The 18 `ranged_v_melee` and `pet_comp`
cells are byte-identical, log SHA included.

**Measured against a fresh run of `main` @ `a2f483a`, not against the
2026-09-13 file.** `main` had drifted under that file on all 27 cells by then,
so diffing against it would have attributed other cards' changes to this one.

The balance side is a paired directional sweep, not these cells: 3,090 matches
at identical seeds, Warlock -0.6pt (z=0.95, resolves >=1.2pt), control 80/80
identical.

### 2026-09-13 — the Frost Armor chill (AS-54)

Frost Armor's proc used to hang two independent auras on a melee attacker. They
are now one compound debuff that lands, diminishes, and comes off as a unit.

**Nine of 27 cells moved, and they are exactly the nine `ranged_v_melee` cells**
— `Mage,Priest` vs `Warrior,Paladin`, the only comp in this matrix with a Mage
in it. Three of the nine flipped winner (`TwinPillars` 4 and 7, `PillaredArena`
4). The other 18 cells — `healer_v_healer` and `pet_comp`, across all three maps
— are byte-identical, log SHA included.

That is the attribution, and it is a POSITIVE one rather than an argument from
elimination: no Mage means no Frost Armor, no chill, and nothing for the change
to touch. The change predicts which cells move, and those are the cells that
moved.

**Measured against a fresh run of `main` @ `3c61185`, not against the
2026-08-02 file.** That file is known to be stale on the nine `pet_comp` cells
(`main` has drifted under it), so diffing against it would have mixed someone
else's drift into this change's evidence. Both arms of the comparison above
were captured on this machine, minutes apart, from the two commits.

The balance consequence is measured separately, over 5,150 paired matches:
`docs/design/balance/2026-09-13-frost-armor-one-debuff-findings.md`. Nine cells
of a determinism reference are not a balance result.

### What changed between the 07-31 and 08-01 files, and what did not

Moving combat systems from `Update` to `FixedUpdate` shifted every logged
timestamp by one tick (1/60 ≈ 0.02s), so all 27 log hashes changed.

**23 of 27 cells kept their winner AND duration. Four did not, and one of those
flipped the winner:**

| Cell | 07-31 | 08-01 |
|---|---|---|
| `TwinPillars pet_comp 1` | Team_2, 85.47s | **Team_1, 71.18s** |
| `TwinPillars pet_comp 4` | Team_2, 79.54s | Team_2, 80.02s |
| `TwinPillars pet_comp 7` | Team_2, 83.92s | Team_2, 69.98s |
| `PillaredArena pet_comp 4` | Team_1, 48.22s | Team_1, 48.23s |

Bisected: the divergence appears exactly at `3a16a46` (the `Update` →
`FixedUpdate` move) and is NOT the match-clock lag — it survives `a9861e3`, which
put `headless_track_time` back in phase with the sim. So the schedule move is not
a pure re-timing of headless: it changes the simulation, and a one-tick offset in
a deterministic sim is enough to cascade into a different winner.

**This is a real Legacy behaviour change, not a printing artifact — but it is
NOT a bug, and there is nothing here to chase.** Localised 2026-08-02 by diffing
the flipped cell's log against `fbe4250` with timestamps stripped:

- The first 94 log events are content-identical. It is not a cascade from some
  early mistake.
- Event 95 is a within-tick REORDER: `[CAST] Purge` and a Healing Stream Totem
  tick swap places, because the sim was re-phased by one tick and they landed on
  the same tick instead of consecutive ones.
- Event ~113 is the first consequence: the Spider's auto-attack reads
  `7 damage` before and `0 damage (7 absorbed)` after, i.e. the Purge stripped
  the enemy's Power Word: Shield a tick later and the absorb was still up.

Changing the stepping model from "one sim step per `app.update()`" to a fixed
accumulator re-phases the whole simulation by a tick. Every effect still happens;
some land on a different tick. In a deterministic sim that is enough to flip a
knife-edge interaction and cascade to a different winner. **The 07-31 outcomes are
superseded, not contradicted** — neither run is more correct.

The concentration in `pet_comp` is a Shaman artifact, not a pet or obstacle one:
that is the only comp here containing a Shaman, so it is the only one where
`Purge` races an enemy `Power Word: Shield` — a genuinely one-tick race. The other
two comps have no dispel and re-phased harmlessly.

Do NOT cite these baselines as evidence that the `TeamPlan` work is a no-op
without first accounting for this; the 07-31 file is the record needed to tell the
two apart.

To reproduce the flipped cell:

```bash
echo '{"team1":["Hunter","Shaman"],"team2":["Rogue","Priest"],"map":"TwinPillars",
       "max_duration_secs":300,"random_seed":1,"ai_profile":"Legacy"}' > /tmp/c.json
cargo run --release -- --headless /tmp/c.json
```

### Recorded: the `[BACKLASH]` id fix changed some digests (text only)

`effects/backlash.rs` was the last combat-log line still written in the retired
`Team {team} {class}` shape, and it named a dispelling PET as its owner. Fixing it
to the `#slot` ids changes the TEXT of any log containing a `[BACKLASH]` line, so
those cells' `log_sha256` move. Verified behaviour-neutral: all nine
`healer_v_healer` cells (the only comp here with a Warlock) keep their exact
winner and duration; only the 5 cells that actually emit a `[BACKLASH]` line
change hash.

Re-blessed as `legacy_behaviour_2026-08-02_backlash_ids.txt`. Confirmed on
capture: the five cells whose hash moved (`BasicArena healer_v_healer 4/7`,
`TwinPillars healer_v_healer 4/7`, `PillaredArena healer_v_healer 4`) keep their
exact winner AND duration, and the other 22 are byte-identical. Do not read those
five hash diffs as a behaviour change.

### Why a new file rather than an overwrite

Overwriting would have destroyed the only record of pre-change behaviour, leaving
no way to check the "timestamps only" claim. Re-blessing to make a diff disappear
is exactly the instinct this directory exists to resist; dating the new capture
lets a real fix land without discarding the evidence that it was safe.

### Caveat on `TwinPillars ranged_v_melee seed 4`

That cell runs 164s against siblings at 105s and 75s — close enough to the 300s
cap to be draw-sensitive. If a future diff fires on only that row, suspect a
timing-sensitive cell before suspecting a real regression, and check whether the
other 26 held.
