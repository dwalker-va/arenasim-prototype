---
title: Two-Agent Bug Hunting Workflow for Combat Log Analysis
category: workflow-patterns
tags:
  - multi-agent
  - headless-testing
  - log-analysis
  - bug-detection
  - combat-system
  - automation
  - claude-code
module: combat/log.rs, headless/runner.rs
symptoms:
  - Dead units dealing damage after death
  - Duplicate log entries for abilities
  - Missing CC event logging
  - Inconsistent aura application tracking
  - AI making suboptimal decisions
root_cause: Manual log review cannot scale; automated detection requires structured workflow with confidence levels and intended-behavior documentation to distinguish bugs from features
severity: medium
created: 2026-01-31
---

# Two-Agent Bug Hunting Workflow

A reusable workflow pattern using Claude Code's Task tool to coordinate two specialized agents for systematic bug detection and fixing in combat simulations.

## Problem Statement

Manual testing of combat simulations is insufficient for several reasons:

1. **Combinatorial Complexity**: With 5 classes, multiple team compositions (1v1, 2v2, 3v3), and two arena maps, there are hundreds of possible match configurations
2. **Rare Edge Cases**: Some bugs only manifest under specific conditions (e.g., simultaneous attacks, CC overlaps, resource edge cases)
3. **Regression Risk**: Balance changes or new abilities can introduce subtle bugs
4. **Human Fatigue**: Reading through match logs manually is tedious and error-prone

The headless simulation mode provides the foundation for automated testing, but interpreting results requires domain knowledge about expected WoW mechanics.

## Solution Overview

A two-agent workflow using Claude Code's Task tool:

```
┌─────────────────┐      /tmp/combat-bugs.json      ┌─────────────────┐
│   Bug Hunter    │ ─────────────────────────────▶  │    Bug Fixer    │
│                 │                                  │                 │
│ - Run matches   │                                  │ - HIGH: Fix     │
│ - Analyze logs  │                                  │ - MEDIUM: Ask   │
│ - Categorize    │                                  │ - LOW: Report   │
└─────────────────┘                                  └─────────────────┘
```

### Agent 1: Bug Hunter

- Generates diverse match configurations
- Runs headless simulations
- Parses match logs for anomalies
- Outputs structured JSON findings
- Respects documented "intended behaviors" to avoid false positives

### Agent 2: Bug Fixer

- Reads Bug Hunter's JSON output
- Prioritizes by confidence level
- Investigates and fixes HIGH confidence issues
- Asks for user guidance on MEDIUM confidence
- Reports LOW confidence for manual review

## Implementation Details

### Bug Hunter Output Format

The Bug Hunter writes findings to `/tmp/combat-bugs.json`:

```json
{
  "matches_run": 6,
  "findings": [
    {
      "id": 1,
      "confidence": "high",
      "category": "damage",
      "summary": "Dead unit deals damage after death",
      "details": "In Rogue vs Rogue match, both used Sinister Strike at timestamp 112.72s. Team 2 Rogue dies first, but their attack still lands.",
      "file_hint": "src/states/play_match/combat_core.rs",
      "match_file": "match_logs/match_20260131_103000.txt"
    }
  ],
  "verified_working": ["Absorb shields", "Interrupts", "DoT ticking"]
}
```

### Confidence Levels

| Level | Meaning | Action |
|-------|---------|--------|
| **HIGH** | Clear violation (0 damage, negative resources, impossible states) | Fix automatically |
| **MEDIUM** | Potentially incorrect but might be intended | Ask user first |
| **LOW** | Suspicious patterns that warrant review | Report only |

### Category Types

| Category | Description |
|----------|-------------|
| `damage` | Damage calculations (wrong coefficients, 0 damage) |
| `healing` | Healing calculations (overhealing bugs, wrong targets) |
| `cc` | Crowd control (wrong durations, early breaks) |
| `resource` | Mana/Rage/Energy (negative values, wrong costs) |
| `ability` | Ability mechanics (wrong range, cooldowns) |
| `aura` | Buff/debuff systems (missing ticks, wrong magnitudes) |
| `ai` | Decision making (stuck loops, wrong targets) |

### Intended Behaviors Document

To prevent false positives, document intended behaviors in `design-docs/wow-mechanics.md`:

```markdown
## Intended Behaviors (Not Bugs)

| Scenario | Intended? | Reason |
|----------|-----------|--------|
| DoT damage continues after caster dies | Yes | Authentic WoW behavior |
| Projectile hits after caster dies | Yes | Already in flight |
| Simultaneous kills (both die) | Yes | Will be addressed by RNG/gear |
```

## Invocation

### Manual via Task Tool

```
Task(subagent_type="general-purpose", prompt="Bug Hunter - Run 6 diverse match
simulations, analyze logs for anomalies, output to /tmp/combat-bugs.json.
Read design-docs/wow-mechanics.md for intended behaviors to avoid false positives.")

# Wait for completion, then:

Task(subagent_type="general-purpose", prompt="Bug Fixer - Read /tmp/combat-bugs.json,
fix HIGH confidence issues, ask about MEDIUM, report LOW.")
```

### Recommended Match Configurations

```bash
# Mirror matches (same-frame timing bugs)
{"team1":["Warrior"],"team2":["Warrior"]}
{"team1":["Rogue"],"team2":["Rogue"]}

# Healer vs DPS (healing/CC interactions)
{"team1":["Priest"],"team2":["Rogue"]}

# 2v2 with healer (target priority, buff stacking)
{"team1":["Warrior","Priest"],"team2":["Mage","Warlock"]}

# Full 3v3 (maximum complexity)
{"team1":["Warrior","Mage","Priest"],"team2":["Rogue","Warlock","Priest"]}
```

## Example Session Results

### Bugs Found and Fixed

| Bug | Confidence | Fix |
|-----|------------|-----|
| Dead unit deals damage | HIGH | Added `died_this_frame` tracking in combat_core.rs |
| Dispel log predicts wrong aura | HIGH | Changed to generic "casts Dispel Magic" message |
| Duplicate Kick logging | HIGH | Removed redundant log call |
| Warlock AI fails vs kiting | MEDIUM (confirmed) | Added `is_being_kited()` detection |
| Fear CC not logged | HIGH | Added `log_crowd_control()` call |
| Duplicate Fortitude buffs | HIGH | Added `fortified_this_frame` tracking |
| Inconsistent CC logging | HIGH | Extended logging to all CC types |

### Verified NOT Bugs

| Behavior | Resolution |
|----------|------------|
| DoT damage from dead caster | Documented as intended (WoW behavior) |
| Projectile damage after caster death | Documented as intended |
| Simultaneous kills | Documented as intended (RNG will reduce) |

## Prevention & Best Practices

### When to Run Bug Hunt

| Trigger | Scope |
|---------|-------|
| After new ability/class | Full simulation with new content |
| After refactoring combat systems | Targeted hunt on affected systems |
| Weekly maintenance | Complete cycle on all classes |
| Before releases | 5+ matchup smoke test |

### Workflow Improvements Over Time

1. **Expand intended behaviors list** as edge cases are clarified
2. **Add regression tests** for each HIGH confidence bug fixed
3. **Automate match generation** to cover more combinations
4. **Track patterns** across sessions to identify systemic issues

### Critical Invariants to Test

| Invariant | Implementation |
|-----------|----------------|
| Dead units deal no damage | `died_this_frame` HashSet check |
| Buffs don't duplicate | Frame-level tracking (`fortified_this_frame`) |
| CC events are logged | Structured `log_crowd_control()` calls |
| Projectiles work headless | Transform component spawning |

## Cross-References

- `design-docs/wow-mechanics.md` - Intended behaviors list
- `design-docs/bevy-patterns.md` - Common pitfalls & solutions
- `design-docs/session-notes.md` - Development history with bug fixes
- `CLAUDE.md` - Headless simulation instructions

## The Compounding Philosophy

Each documented solution compounds team knowledge:

1. First time solving "dead unit damage" → Research (30 min)
2. Document the solution → This file (5 min)
3. Next similar issue → Quick lookup (2 min)

**Each unit of engineering work should make subsequent units easier—not harder.**
