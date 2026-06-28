---
title: "Shaman trace recipes: jq + combat-log queries for totems, Purge, Wind Shear, postures"
tags:
  - observability
  - shaman
  - class-ai
  - decision-trace
  - jsonl
  - totems
  - purge
  - wind-shear
category: implementation-patterns
module: states/play_match/class_ai/shaman
---

# Shaman trace recipes

Copy-paste `jq` recipes over the AI decision trace (`match_logs/*_trace.jsonl`)
and the human-readable combat log (`match_logs/match_*.txt`) for diagnosing
Shaman behavior. Generate a trace with `--trace-mode on`:

```bash
cargo run --release -- --headless /tmp/test.json --trace-mode on
# Trace at match_logs/match_<timestamp>_trace.jsonl; combat log at match_logs/match_<timestamp>.txt
T=match_logs/match_*_trace.jsonl
LOG=match_logs/match_*.txt
```

Style matches the Hunter/Priest recipes in `CLAUDE.md`. For the full trace
schema and the variant-to-predicate map see
[`ai-decision-trace.md`](./ai-decision-trace.md).

## Ability rejection histogram

Every ability the Shaman AI considered and rejected, grouped by typed reason
(out of range, on cooldown, insufficient mana, no valid target, etc.):

```bash
jq -r 'select(.actor.class == "Shaman")
       | .candidates[] | select(.status == "rejected")
       | .reason | if type == "object" then keys[0] else . end' $T \
  | sort | uniq -c
```

Why didn't the Shaman cast a specific ability (e.g. Lightning Bolt)? Show its
rejections by reason:

```bash
jq -c 'select(.actor.class == "Shaman")
       | .candidates[] | select(.ability == "LightningBolt" and .status == "rejected")
       | .reason' $T | sort | uniq -c
```

## Totem placement (ability_decision ending in "Totem")

Totem drops are real `ability_decision` events — `try_totem` calls
`builder.choose`, so each drop is an `action_taken` with an ability name ending
in `Totem` (`AirTotem` / `WaterTotem` / `EarthTotem` / `FireTotem`). The drop
*position* is not carried on the event (the totem spawns at the Shaman's feet —
verify placement with the `shaman_totems` probes in `tests/movement_probes.rs`):

```bash
jq -c 'select(.kind == "ability_decision" and .actor.class == "Shaman"
              and .outcome.type == "action_taken"
              and (.outcome.ability | endswith("Totem")))
       | {t: .sim_time, ability: .outcome.ability}' $T
```

Totem drops are also logged with a `[TOTEM]` note (drop + per-ally pulse):

```bash
grep "\[TOTEM\]" $LOG | head
# [  1.50s] [BUFF] [TOTEM] Team 1 Shaman drops Healing Stream Totem
# [  1.52s] [BUFF] [TOTEM] Healing Stream Totem buffs Team 1 Warrior
```

## Purge target strips

The cast decision is an `action_taken` event for `Purge` (carrying the enemy
`target_id`); the actual buff removed lands in the combat log with a `[PURGE]`
note (the strip is resolved by `process_dispels`, one frame later):

```bash
# Purge casts and their targets (trace)
jq -c 'select(.kind == "ability_decision" and .actor.class == "Shaman"
              and .outcome.type == "action_taken" and .outcome.ability == "Purge")
       | {t: .sim_time, target_id: .outcome.target_id}' $T

# What each Purge actually stripped (combat log) — buff name + victim
grep "\[PURGE\]" $LOG
# [ 13.93s] [BUFF] [PURGE] Power Word: Shield removed from Team 2 Priest
# [ 16.97s] [BUFF] [PURGE] Ice Barrier removed from Team 2 Mage
```

Purge rejections (e.g. no enemy carries a purgeable buff → `NoValidTarget`):

```bash
jq -c 'select(.actor.class == "Shaman")
       | .candidates[] | select(.ability == "Purge" and .status == "rejected")
       | .reason | if type == "object" then keys[0] else . end' $T | sort | uniq -c
```

## Wind Shear interrupts (combat-log only — NO trace event)

Wind Shear does NOT emit an `ability_decision` event. Interrupts run through a
separate pipeline (`check_interrupts` → `InterruptPending` →
`process_interrupts`), not the per-tick `decide_abilities` builder, so there is
no `action_taken` for `WindShear` in the trace. Diagnose interrupts from the
combat log instead — `process_interrupts` writes a school-lockout line:

```bash
grep "Shaman interrupts" $LOG
# Team 1 Shaman interrupts Team 2 Mage's Frostbolt - Frost school locked for 4.0s

# Count interrupts landed this match
grep -c "Shaman interrupts" $LOG
```

A silent Wind Shear (the Shaman never interrupting) is usually a range or
target-not-casting issue — there is nothing in the trace to query, so confirm
the enemy actually hard-cast an interruptible spell within Wind Shear range.

## Posture transitions (movement_decision FREE/PRESSURED/ESCAPE)

The Shaman is on the shared healer posture machine. `movement_decision` events
fire on posture transitions and committed-direction changes (never per-tick).
`posture` is lowercase (`free` / `pressured` / `escape`); `previous_posture` is
present only on real transitions (re-commits like `FormationShift` /
`CommitExpired` omit it):

```bash
jq -c 'select(.kind == "movement_decision" and .actor.class == "Shaman")
       | {t: .sim_time, from: .previous_posture, to: .posture, trigger}' $T
```

Trigger histogram (triggers are bare-string unit variants):

```bash
jq -r 'select(.kind == "movement_decision" and .actor.class == "Shaman") | .trigger' $T \
  | sort | uniq -c
```

PRESSURED windows (when was the Shaman under melee/proximity threat?):

```bash
jq -c 'select(.kind == "movement_decision" and .actor.class == "Shaman"
              and (.trigger == "PressuredEnter" or .trigger == "PressuredExit"))
       | {t: .sim_time, trigger}' $T
```

Scorer term breakdown — which weighted terms drove a chosen direction
(`scorer_terms` is a `{name: value}` map, present only when the scorer ran;
re-commits / Point goals omit it):

```bash
jq -c 'select(.kind == "movement_decision" and .actor.class == "Shaman"
              and .scorer_terms != null)
       | {t: .sim_time, posture, dir: .chosen_direction, terms: .scorer_terms}' $T
```

## Tolerating truncated traces

A match killed mid-flush (SIGKILL / OOM) leaves a partial last line. Read
defensively:

```bash
head -n -1 $T | jq ...           # skip the partial line
jq -c '. // empty' $T 2>/dev/null # or let jq skip parse errors (jq 1.6+)
```
