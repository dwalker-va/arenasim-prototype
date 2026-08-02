# Behaviour baselines

Recorded output of `scripts/behaviour_baseline.sh` — a **determinism reference**,
not a balance measurement.

## What these are for

The claim they support is *"nothing changed"*, not *"these are the win rates"*.
Balance data lives in `design-docs/balance/` and is expected to move; these files
are expected to stay frozen, and a change to one is an event that needs a reason.

The `TeamPlan` migration (`design-docs/team-level-positioning-ai.md`) opens with a
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
| `legacy_behaviour_2026-08-02_backlash_ids.txt` | 2026-08-02 | **Current.** After the `[BACKLASH]` log-id fix. Verified reproducible: two independent runs agreed on all 27 cells. |
| `legacy_behaviour_2026-08-01_fixed_timestep.txt` | 2026-08-01 | After moving the simulation to `FixedUpdate`. Verified reproducible when captured. |
| `legacy_behaviour_2026-07-31.txt` | 2026-07-31, `main` @ `4e71746` | Pre-fixed-timestep. Also verified reproducible when captured. |

### What changed between them, and what did not

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

**This is a real Legacy behaviour change, not a printing artifact.** It is
confined to the `pet_comp` rows (`Hunter,Shaman` vs `Rogue,Priest`) on the two
obstacle maps, which is a specific enough signature to chase. Do NOT cite these
baselines as evidence that the `TeamPlan` work is a no-op without first accounting
for it; the 07-31 file is the record needed to tell the two apart.

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
