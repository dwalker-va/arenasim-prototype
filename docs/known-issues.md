# Known Issues

Bugs and behaviors that have been identified and are either **accepted**, **intentional**, or **already tracked for fix**. The `/bug-hunt` skill cross-references this file to avoid re-reporting known problems.

Each entry has a status:
- **accepted** — Known limitation, not worth fixing now
- **intentional** — Working as designed, may look like a bug
- **tracked** — Bug is real and tracked for fix (don't re-report)
- **fixed** — Was a bug, has been fixed (kept here to prevent false re-reports if symptoms look similar)

---

## Intentional Behaviors

### KI-1: DoTs continue ticking after caster death
**Status:** intentional
**Category:** damage-after-death
**Description:** When a combatant dies, their active DoTs (Rend, Corruption, Curse of Agony, etc.) continue dealing damage to targets. This matches WoW Classic behavior where DoTs persist independently of the caster's alive state.
**Bug hunt note:** Do NOT flag `[DMG]` from DoT ticks where the original caster is dead. Only flag new ability casts or direct damage from dead combatants.

### KI-2: Projectiles landing after caster death
**Status:** intentional
**Category:** damage-after-death
**Description:** If a spell (e.g., Frostbolt, Shadow Bolt) was already in flight as a projectile when the caster dies, it will still hit the target and deal damage. This matches WoW Classic behavior. Only spells still being *cast* (not yet released) should be cancelled on death.
**Bug hunt note:** Check timestamps carefully. A projectile that was launched before death and lands after is fine. A new cast *starting* after death is a bug.

### KI-3: Resource label always says "Mana" in match reports
**Status:** accepted
**Category:** cosmetic
**Description:** The match report header shows "Mana: X/Y" for all classes, including Warriors (Rage) and Rogues (Energy). This is a cosmetic issue in the log formatter only — the actual resource systems work correctly in combat.
**Bug hunt note:** Do NOT flag this. It's a known cosmetic issue in `combat/log.rs`.

### KI-4: Extended wand/auto-attack phases when OOM
**Status:** accepted
**Category:** balance
**Description:** When both teams run out of mana, matches can devolve into 15-30 seconds of pure auto-attacking. This is a balance concern (mana pools vs match duration) but not a code bug.
**Bug hunt note:** Do NOT flag OOM auto-attack phases as bugs. Note them as observations if they last >30s.

### KI-5: AI goes idle during enemy Divine Shield
**Status:** accepted
**Category:** ai-behavior
**Description:** When an enemy Paladin activates Divine Shield, opposing combatants may stop attacking for the full 12s duration instead of switching targets, healing, or repositioning. This is an AI priority gap, not a combat system bug.
**Bug hunt note:** Note as an observation, not a bug.

### KI-6: Warrior dealt 0 damage (chain CC'd)
**Status:** accepted
**Category:** balance
**Description:** Warriors can be chain-CC'd from match start to death without ever reaching melee range. While technically correct (the Warrior was always CC'd or out of range), this is a balance concern about gap-closing tools, not a code bug.
**Bug hunt note:** Only flag 0 damage as a bug if the Warrior was NOT CC'd — check for extended CC chains first. If the Warrior was CC'd the whole match, note as observation.

### KI-8: Paladin casts Divine Shield while Polymorphed or CC'd
**Status:** intentional
**Category:** cc-immunity
**Description:** Divine Shield can be activated while under any form of crowd control (stun, polymorph, fear, root, etc.) — the only exception is a spell lockout on the Paladin's Holy school. This matches WoW Classic behavior where Divine Shield is usable through all CC types as long as the Holy school is not locked out.
**Bug hunt note:** Do NOT flag Paladin using Divine Shield during any CC as a bug. Only flag it if the Paladin's Holy school is locked out (SpellLockout aura on Holy) and they still cast it.

---

## Tracked Bugs (already known, don't re-report)

### KI-7: Duplicate group buff application at match start
**Status:** tracked
**Category:** duplicate-buff
**Description:** Battle Shout and Devotion Aura can be applied twice to the same ally at 0.00s during the pre-match buff phase, effectively doubling their effect. This is caused by the buff being applied once to "all allies" and once to "self" as a separate path.
**Bug hunt note:** If you see double buff application at 0.00s, note it as "KI-7 confirmed" but do not write it up as a new bug.

---

## Fixed Bugs (kept for reference)

<!-- Add entries here as bugs are fixed, to prevent the bug hunt from re-reporting
     symptoms that look similar but are actually resolved. Format:

### KI-N: [title]
**Status:** fixed
**Fixed in:** [commit hash or date]
**Category:** [category tag]
**Description:** [what the bug was]
**Bug hunt note:** [what residual symptoms might look like]
-->
