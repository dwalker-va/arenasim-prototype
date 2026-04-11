---
date: 2026-04-05
topic: class-strategic-options
status: implemented
---

# Class Strategic Option Layers

## Problem Frame

Rogue and Warlock each have a configurable strategic choice (opener selection, curse distribution) that changes how they play in a given match. The other four classes — Warrior, Mage, Priest, and Paladin — follow a fixed priority every game regardless of matchup. This limits team composition depth and counter-play.

Three classes have natural WoW Classic "choose one from a set" mechanics that fit as strategic layers without encroaching on future talent tree design space. Priest is intentionally excluded — no clean thematic fit exists yet.

## Requirements

**Warrior — Shout Choice**
- R1. Warrior gains a shout preference: Battle Shout, Demoralizing Shout, or Commanding Shout
- R2. Battle Shout remains as-is (attack power buff for allies)
- R3. Demoralizing Shout reduces enemy attack power (debuff applied to nearby enemies)
- R4. Commanding Shout grants max health increase to allies (team HP buff)
- R5. The AI casts the chosen shout during pre-match; only one shout is active at a time

**Mage — Armor Choice**
- R6. Mage gains an armor preference: Frost Armor, Mage Armor, or Molten Armor
- R7. Frost Armor slows melee attackers who strike the Mage (defensive, anti-melee)
- R8. Mage Armor increases mana regeneration or provides magic resistance (sustain, anti-caster)
- R9. Molten Armor grants crit chance or reflects damage to melee attackers (offensive)
- R10. The armor is self-cast during pre-match countdown (costs a GCD, like Ice Barrier)
- R11. Only one armor can be active at a time

**Paladin — Aura Choice**
- R12. Paladin gains an aura preference: Devotion Aura, Shadow Resistance Aura, or Concentration Aura
- R13. Devotion Aura remains as-is (flat damage reduction for allies)
- R14. Shadow Resistance Aura reduces shadow damage taken by allies (counter Warlock/Priest)
- R15. Concentration Aura reduces spell pushback for allied casters
- R16. Aura is a pre-match passive — always active, no cast time or GCD cost
- R17. Only one aura can be active at a time

**Configuration**
- R18. Each strategic choice is configured at match setup time (like Rogue opener and Warlock curses)
- R19. Each class has a sensible default: Battle Shout for Warrior, Frost Armor for Mage, Devotion Aura for Paladin
- R20. Headless match config supports specifying the choice per combatant

## Success Criteria

- Each of the three classes has a configurable choice that changes observable match behavior
- The choice is visible in match config and combat logs
- Default behavior (no explicit choice) matches current behavior for backwards compatibility

## Scope Boundaries

- Priest is excluded — no natural "choose one" mechanic fits without feeling forced
- These are tactical knobs within existing class identity, not spec-level changes
- No new damage spells or healing spells — only buff/debuff/passive utility choices
- Talent trees are a separate future system; these choices should coexist with talents later
- Exact numeric values for new abilities (magnitude of slows, resistance amounts, etc.) are deferred to planning, where Wowhead data can inform them

## Key Decisions

- **Tactical knobs, not specs**: Strategic layers should feel like choosing which utility to bring, not choosing a specialization. This preserves design space for future talent trees.
- **Priest excluded**: No clean thematic fit exists in Classic WoW for a "choose one from a set" mechanic. Will revisit when new abilities or talents create a natural choice point.
- **Fire Resistance Aura cut**: No current class deals meaningful fire damage to justify it yet.
- **Mage Armor is a self-cast buff**: Cast during pre-match countdown (costs a GCD) for thematic consistency with WoW, rather than being a free passive like Paladin Aura.
- **Paladin Aura is a passive**: Auras in WoW are always-on and don't cost a cast — keep that feel.

## Outstanding Questions

### Deferred to Planning
- [Affects R3][Needs research] What should Demoralizing Shout's attack power reduction magnitude be? Look up WoW Classic values.
- [Affects R4][Needs research] What should Commanding Shout's max health increase be?
- [Affects R7, R8, R9][Needs research] What are appropriate magnitudes for each Mage Armor effect? Look up WoW Classic Frost Armor slow, Mage Armor regen, Molten Armor crit values.
- [Affects R14][Needs research] What shadow resistance value makes a meaningful but not overpowered difference against Warlock/Priest damage?
- [Affects R15][Technical] How should "spell pushback reduction" interact with the current casting system? Does the sim model pushback?
- [Affects R9][Technical] Should Molten Armor grant crit chance, reflect damage, or both? Classic WoW did both but balance may differ here.

## Next Steps

→ `/ce:plan` for structured implementation planning
