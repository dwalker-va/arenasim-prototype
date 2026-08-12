# Watch set — why the CC model wins on one map and not the other

**Reclassified 2026-08-09 against the current build.** An earlier version of this
set was classified before several changes landed AND before a bug fix that
matters: `--replay` never carried `cc_policy`, so the graphical client silently
ran **every** match as `Identity` and both halves of a pair played identically.
That is fixed — the launch banner now prints
`cc CcPolicies { team1: Priced, ... }`, which is worth glancing at to confirm.

## The open question

`cc_policy: Priced` is a solid win on BasicArena (+9pt, z=2.13) and flat on
PillaredArena (-1pt). **Seven hypotheses have been measured and refuted**: cover,
displacement, delivery rate, OOM tail, post-first-kill window, tempo, and mana
cost. The live theory is that the match is decided by a **DoT/dispel exchange**
the model does not represent, and the CC choice is a perturbation on top of it.

The aggregate "flat" hides the shape: over 30 PillaredArena seeds the change
diverges on 6 — **3 better, 3 worse.** It is not inert there; it helps and hurts
about equally.

## Running

```bash
cargo run --release -- --replay match_configs/pillared-diagnosis/pillared-seed16-regression-nearmiss-identity.json
cargo run --release -- --replay match_configs/pillared-diagnosis/pillared-seed16-regression-nearmiss-priced.json
```

Same comp, map and seed in each pair; only `cc_policy` differs. Matches are
deterministic, so what you watch is exactly what was measured.

## The set

Team 1 is `Warlock + Priest` (ours) versus `Priest + Rogue`. The **enemy Priest
is the called kill target** — a player input, and the condition under which the
CC change fires at all.

Context: across 32 audited matches **we win if and only if the enemy Rogue
dies**. The called healer dies almost every match; the game turns on the
off-target.

| file prefix | what happens | why it is here |
|---|---|---|
| `pillared-seed16-regression-nearmiss` | Identity wins; Priced loses with the Rogue on **7 HP** | As close as a loss gets. Whatever Priced spends differently costs exactly one more Rogue tick. |
| `pillared-seed6-improvement` | Rogue **239 → dead** | The mirror: the same policy, clearly better. |
| `pillared-seed1-regression` | Rogue **dead → 62 HP** | A second regression, to see whether it fails the same way as seed 16. |
| `basic-seed22-contrast-improvement` | Rogue **372 → dead** | **The control.** BasicArena is where the change works; watch what "working" looks like for contrast. |

## What to watch for

1. **The DoT/dispel exchange.** Measured: 9.5 DoT applications per match, **46%
   dispelled, mean lifetime 2.1s**. Corruption applied at 27.73s was gone at
   27.75s. Does the Warlock look like it is losing this trade, and does Priced
   change how often it re-applies versus Fears?
2. **Does the Fear stop a dispel?** Unstable Affliction punishes a dispel for 138
   damage plus a silence — but only if they actually dispel. A Fear can suppress
   the punish. Does that look like it is happening, and does it matter?
3. **The off-target.** The Rogue spends these matches on our Priest. The Warlock
   now peels for the team (Death Coil at the highest-denial enemy in 30yd, not
   just one attacking itself). Does it peel in time?
4. **Compare against the BasicArena control.** The most useful single question:
   what does the Warlock do differently on the map where this works?
