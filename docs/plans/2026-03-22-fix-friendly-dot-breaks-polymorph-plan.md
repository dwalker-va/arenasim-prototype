---
title: "fix: AI avoids breaking own team's Polymorph with DoTs and direct damage"
type: fix
status: completed
date: 2026-03-22
---

# Fix Friendly DoTs/Damage Breaking Own Team's Polymorph (BUG-5)

## Overview

Teammates break their own Mage's Polymorph by applying DoTs or direct damage to the poly'd target. This is an AI decision problem — the break-on-damage system correctly follows WoW Classic behavior. The fix is to make the AI avoid damaging targets that have friendly breakable CC.

## Problem Statement

From the bug report:
- **m02**: Team 2 Mage Frostbolts own team's polymorphed target directly
- **m15**: Team 2 Warlock's Curse of Agony tick breaks Team 2 Mage's Polymorph
- **m16**: Team 1 Warrior's Rend tick breaks Team 1 Mage's Polymorph
- **m19**: Team 2 Warlock's Curse of Agony tick breaks Team 2 Mage's Polymorph

Two scenarios:
1. **New DoT/damage on poly'd target**: AI applies Corruption, CoA, Rend, or Frostbolt to a target already polymorphed by a teammate
2. **Poly on DoT'd target**: Mage polymorphs a target that already has teammate DoTs ticking — the DoT ticks immediately break the Poly

## Proposed Solution

### Part A: Add helper method to CombatContext

Add `has_friendly_breakable_cc()` to `class_ai/mod.rs` that checks if a target has a Polymorph from a same-team caster:

```rust
/// Check if target has a breakable CC (Polymorph) from a friendly caster.
/// Used to prevent AI from breaking own team's CC with damage/DoTs.
pub fn has_friendly_breakable_cc(&self, target: Entity) -> bool {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.active_auras
        .get(&target)
        .map(|auras| {
            auras.iter().any(|a| {
                a.effect_type == AuraType::Polymorph
                    && a.caster.and_then(|c| self.combatants.get(&c).map(|info| info.team))
                        == Some(my_team)
            })
        })
        .unwrap_or(false)
}
```

### Part B: Prevent new damage on poly'd targets

Add `!ctx.has_friendly_breakable_cc(target)` checks before:

| Location | File | What to check |
|---|---|---|
| `try_corruption()` | `warlock.rs:~214` | Skip if target has friendly Poly |
| `try_cast_curse()` | `warlock.rs:~631` | Skip if target has friendly Poly |
| `try_rend()` | `warrior.rs:~273` | Skip if target has friendly Poly |
| Frostbolt target selection | `mage.rs:~464` | Skip if kill_target has friendly Poly |

### Part C: Prevent Poly on DoT'd targets

Add a check in Mage's `try_polymorph()` to skip if the cc_target already has DoTs from teammates:

```rust
/// Check if target has DoTs from a friendly caster that would break Polymorph.
pub fn has_friendly_dots_on_target(&self, target: Entity) -> bool {
    let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
    self.active_auras
        .get(&target)
        .map(|auras| {
            auras.iter().any(|a| {
                a.effect_type == AuraType::DamageOverTime
                    && a.caster.and_then(|c| self.combatants.get(&c).map(|info| info.team))
                        == Some(my_team)
            })
        })
        .unwrap_or(false)
}
```

Add this check in `try_polymorph()` in `mage.rs` before casting — if cc_target has friendly DoTs, skip the Poly.

## Acceptance Criteria

- [x] Warlock does not apply Corruption/CoA to a target polymorphed by own team's Mage
- [x] Warrior does not apply Rend to a target polymorphed by own team's Mage
- [x] Mage does not Frostbolt a target polymorphed by own team's Mage
- [x] Mage does not Polymorph a target with active friendly DoTs
- [x] All classes still apply DoTs/damage to targets polymorphed by the ENEMY team (correct behavior)
- [x] No regressions in 2v2 matches (no Mage+DoT class matchups affected)

## Verification

```bash
# BUG-5 repro configs from bug report
echo '{"team1":["Warrior","Mage","Paladin"],"team2":["Rogue","Warlock","Priest"],"random_seed":6016,"team1_kill_target":1,"team2_kill_target":1}' > /tmp/bug5.json
cargo run --release -- --headless /tmp/bug5.json

# Warlock+Mage team — Warlock should not DoT poly'd target
echo '{"team1":["Mage","Warlock","Priest"],"team2":["Warrior","Rogue","Paladin"],"random_seed":6015}' > /tmp/bug5b.json
cargo run --release -- --headless /tmp/bug5b.json
```

## Files to Modify

1. `src/states/play_match/class_ai/mod.rs` — `has_friendly_breakable_cc()`, `has_friendly_dots_on_target()`
2. `src/states/play_match/class_ai/warlock.rs` — Corruption + Curse checks
3. `src/states/play_match/class_ai/warrior.rs` — Rend check
4. `src/states/play_match/class_ai/mage.rs` — Frostbolt + Polymorph checks

## Sources

- Bug report: `docs/reports/2026-03-16-headless-match-bug-report.md` (BUG-5)
- Aura caster field: `components/auras.rs:101` (`caster: Option<Entity>`)
- Polymorph break logic: `auras.rs:516-550` (break_on_damage_threshold: 0.0)
