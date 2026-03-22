---
title: "fix: CC and damage applied to dead targets"
type: fix
status: completed
date: 2026-03-22
---

# Fix CC/Damage Applied to Dead Targets (BUG-6)

## Overview

Frost Nova AoE roots dead combatants and melee auto-attacks briefly hit dead targets before retargeting.

## Root Causes

1. **Frost Nova** (mage.rs:303-310): Target collection loop doesn't check `info.is_alive` — dead combatants in range get rooted and damaged.
2. **Auto-attack** (auto_attack.rs:110-143): Attack queuing doesn't check if target is alive before queuing the swing.

## Fix

### Part A: Frost Nova target filtering (mage.rs)
Add `&& info.is_alive` to the Frost Nova target collection loop at line 304.

### Part B: Auto-attack target validation (auto_attack.rs)
Add `is_alive()` check before queuing auto-attack at ~line 110.

## Acceptance Criteria

- [x] Frost Nova does not root or damage dead combatants
- [x] Auto-attacks do not hit dead targets
- [x] No regressions in normal combat

## Verification

```bash
echo '{"team1":["Mage","Mage","Priest"],"team2":["Warrior","Warlock","Paladin"],"random_seed":6021,"map":"PillaredArena"}' > /tmp/bug6.json
cargo run --release -- --headless /tmp/bug6.json
```

## Sources

- Bug report: `docs/reports/2026-03-16-headless-match-bug-report.md` (BUG-6)
