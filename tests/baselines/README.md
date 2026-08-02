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
| `legacy_behaviour_2026-08-01_fixed_timestep.txt` | 2026-08-01 | **Current.** After moving the simulation to `FixedUpdate`. Verified reproducible: two independent runs agreed on all 27 cells. |
| `legacy_behaviour_2026-07-31.txt` | 2026-07-31, `main` @ `4e71746` | Pre-fixed-timestep. Also verified reproducible when captured. |

### What changed between them, and what did not

Moving combat systems from `Update` to `FixedUpdate` shifted every logged
timestamp by one tick (1/60 ≈ 0.02s), so all 27 log hashes changed.

**Winner and duration are identical in all 27 cells.** Event order is unchanged.
The evidence says the simulation is unaffected and only the printed time column
moved.

That is strong evidence, not proof. The residual one-tick offset is
**characterised but not explained** — the obvious cause (the match clock left in
`Update` while the sim moved to `FixedUpdate`) was fixed and the offset survived
it, so something else in the schedule carries it. If a future investigation shows
the shift was not benign, the 07-31 file is the record needed to tell.

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
