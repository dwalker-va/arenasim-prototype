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
scripts/behaviour_baseline.sh | diff tests/baselines/legacy_behaviour_2026-07-31.txt -

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

| File | Captured | Notes |
|---|---|---|
| `legacy_behaviour_2026-07-31.txt` | 2026-07-31, `main` @ `4e71746` | `Legacy` profile. 3 maps × 3 comps × 3 seeds. Verified reproducible: two independent runs agreed on all 27 cells. Taken *before* any `TeamPlan` work, on a `main` that already includes PR #89's combat-log-id changes. |

### Caveat on `TwinPillars ranged_v_melee seed 4`

That cell runs 164s against siblings at 105s and 75s — close enough to the 300s
cap that a small perturbation could push it to a draw. If a future diff fires on
only that row, suspect a timing-sensitive cell before suspecting a real
regression, and check whether the other 26 held.
