# Headless Match Bug Report — 2026-02-20

**Matches run:** 20 (6 x 1v1, 8 x 2v2, 6 x 3v3)
**Total bugs found:** ~35 across 20 matches
**Critical bugs:** 3 distinct categories
**Agents used:** 4 parallel runners

---

## Critical Bugs

### BUG-1: Actions while stunned / CC bypass (P0)

**Frequency:** 6 matches (M2, M3, M8, M13, M15, M17, M18)
**Severity:** Critical — breaks core CC combat mechanic

Stunned combatants can begin or continue casting abilities. The stun aura applies on the same frame as an ability cast, and the cast is not interrupted or prevented.

**Examples:**
- M15: Warrior uses Pummel **3 seconds** into a 6s HoJ stun
- M17: Warlock casts Corruption 0.41s after being stunned by HoJ
- M18: Priest begins Flash Heal on the **same tick** as HoJ stun, completes full cast while stunned
- M3: Warlock's Immolate finished despite Hammer of Justice stun
- M3: Paladin cast 4 Flash of Lights + Holy Shock while feared

**Root cause hypothesis:** Two possible issues:
1. **System ordering:** CC aura application runs after ability decision system in the same frame, so a combatant can start a cast on the same tick they get stunned
2. **Missing incapacitation check:** The casting continuation system (`combat_core.rs`) may not check for active stun/fear/poly auras before advancing cast progress each frame

**Reproduction:** Run any match with Paladin (HoJ) against a caster. Seed 303 (M17) is the clearest example.

---

### BUG-2: 3v3 config silently drops combatants (P0)

**Frequency:** 1 match (M18), but 100% severity when it occurs
**Severity:** Critical — match plays with wrong number of combatants

Match 18 config: `team1: ["Rogue","Mage","Priest"], team2: ["Warrior","Warlock","Paladin"]`
Actual match: 2v2 — Team 1 Mage, Team 2 Warrior, and Team 2 Warlock never spawned.

**Root cause hypothesis:** Possible spawn slot limit or entity collision in headless runner. Only 2 combatants per team were created despite 3 being specified.

**Note:** All other 3v3 matches (M15, M16, M17, M19, M20) spawned correctly with 6 combatants. This may be seed-specific or a race condition in spawn ordering.

**Reproduction:** `{"team1":["Rogue","Mage","Priest"],"team2":["Warrior","Warlock","Paladin"],"seed":304}`

---

### BUG-3: Double kill — combatant dies twice (P1)

**Frequency:** 1 match (M8)
**Severity:** High — corrupts match state

Match 8: Warlock dies at 30.18s and again at 34.68s. The dead Warlock continues receiving damage and is killed a second time. This means the `is_alive()` check is not preventing damage application to dead combatants, or the combatant's alive state was not properly set on first death.

**Reproduction:** `{"team1":["Rogue","Priest"],"team2":["Warlock","Paladin"],"seed":202}`

---

## High-Priority Bugs

### BUG-4: Duplicate group buff application (P2)

**Frequency:** 6 matches (M8, M9, M12, M14, M15, M16, M17)
**Severity:** Medium — buffs stack incorrectly, affects balance

Both Devotion Aura and Battle Shout are applied **twice** to the same ally at match start (0.00s). This means:
- Devotion Aura: 20% damage reduction instead of 10%
- Battle Shout: +40 attack power instead of +20

**Root cause hypothesis:** The pre-match buff phase applies group buffs to all allies, but the caster is also included as an ally target, and the buff may be applied once by the "apply to all allies" logic and again by a separate "apply to self" path. Or the buff system iterates over allies twice.

---

### BUG-5: CC applied to dead combatants (P2)

**Frequency:** 10+ matches — nearly universal
**Severity:** Low-medium — wastes DR counters, cosmetic confusion

When a projectile (Frostbolt) kills a target, the CC component of the spell still applies to the dead target. The CC itself has no gameplay effect (target is dead), but it:
- Advances DR counters on the dead target (irrelevant unless resurrection exists)
- Creates confusing combat log entries

**Root cause:** `apply_pending_auras()` does not check `is_alive()` before applying CC auras.

---

### BUG-6: Dead combatants' DoTs continue ticking (P2)

**Frequency:** 2 matches (M7, M8, M10)
**Severity:** Low-medium — DoTs tick on dead casters' targets, or dead casters' DoTs persist

Rend ticks continued after the Warrior who applied them died. In WoW, DoTs persist after caster death (this is correct), but the targets receiving damage from a dead caster's DoTs should be logged clearly.

**Verdict:** Likely working as intended for DoTs. May need investigation for other lingering effects.

---

### BUG-7: Friendly fire breaks own team's Polymorph (P2)

**Frequency:** 1 match (M20)
**Severity:** Medium — AI wastes its own CC

Match 20: Team 2 Mage polymorphs Team 1 Paladin. 0.51s later, Team 2 Felhunter wand-shots the polymorphed target, breaking the Polymorph immediately. The pet AI does not check if its auto-attack target is polymorphed by a friendly.

**Root cause:** Pet auto-attack target selection doesn't account for friendly CC on the target. The Felhunter should avoid attacking targets that have friendly CC (Polymorph, Fear) active.

---

### BUG-8: Dead combatant's spell completes after death (P2)

**Frequency:** 3 matches (M13, M17, M18)
**Severity:** Medium — dead casters deal damage posthumously

A Mage begins casting Frostbolt and dies during the cast. The Frostbolt still completes and deals damage after the Mage is dead. In M17, the cast completed faster than the stated cast time (began at 25.93s, should finish at ~28.43s with 2.5s cast, but hit at 26.69s).

**Note:** In WoW, spells already in flight (projectiles) do land after caster death, but casts in progress are interrupted by death. The system should cancel any `CastingState` when the caster dies.

---

## Low-Priority / Cosmetic Issues

### BUG-9: Resource label always says "Mana" (P3)

**Frequency:** Every match with Warrior or Rogue
**Severity:** Cosmetic only

Warriors show "Mana: X/100" (should be "Rage"), Rogues show "Mana: X/100" (should be "Energy"). The match report header uses a generic "Mana" label regardless of class resource type.

---

### BUG-10: Fear landed through Divine Shield (P2)

**Frequency:** 2 matches (M3, M9)
**Severity:** High if confirmed — Divine Shield should grant full immunity

Match 3: Fear allegedly landed on a Paladin through Divine Shield. Match 9: Polymorph allegedly landed through Divine Shield. Needs verification — may be a timing issue where the CC lands on the same frame Divine Shield expires or before it fully applies.

---

### BUG-11: Warrior dealt 0 damage entire match (P3)

**Frequency:** 2 matches (M9, M16)
**Severity:** AI/balance concern, not a code bug

Warrior was chain-CC'd from match start to death without ever reaching melee range or using an ability. While technically "correct" (Warrior was always CC'd or out of range), this represents a balance concern where Warrior has no tools to close the gap against CC-heavy comps.

---

### BUG-12: Missing death log entries (P3)

**Frequency:** 1 match (M11)
**Severity:** Low — logging gap

Match 11: Rogue reached 0 HP but no `[DEATH]` entry was logged. The Rogue simply stops appearing in the combat log.

---

## Observations (Not Bugs)

### OBS-1: AI goes completely idle during Divine Shield (~12s)

**Frequency:** Every match with Paladin (M13, M14, M17, M18)

When enemy Paladin pops Divine Shield, all opposing combatants stop acting entirely for the full 12s duration. They should continue buffing, healing, or repositioning instead.

### OBS-2: Paladin wastes Divine Shield without healing

**Frequency:** M13, M14

Paladin uses Divine Shield at low HP but only auto-attacks during the immunity window instead of healing. AI should prioritize self-healing during bubble.

### OBS-3: Extended wand-only phases (OOM)

**Frequency:** M11, M14

Matches devolve into 15-22 seconds of pure auto-attacking when both sides go OOM. Balance concern — mana pools may be too small for match duration.

---

## Summary by Priority

| Priority | Count | Description |
|----------|-------|-------------|
| P0 | 2 | Stun bypass, missing combatants in 3v3 |
| P1 | 1 | Double kill on dead combatant |
| P2 | 6 | Duplicate buffs, CC on dead, friendly Poly break, dead caster spells, Fear through bubble |
| P3 | 3 | Resource labels, 0-damage Warrior, missing death log |

## Recommended Fix Order

1. **BUG-1 (Stun bypass)** — Most impactful. Check incapacitation state before allowing cast start AND cast progress each frame.
2. **BUG-2 (3v3 spawn)** — Investigate seed 304 specifically. May be a one-off spawn race.
3. **BUG-3 (Double kill)** — Add `is_alive()` guard before damage application.
4. **BUG-4 (Duplicate buffs)** — Deduplicate group buff application at match start.
5. **BUG-5 (CC on dead)** — Add `is_alive()` check in `apply_pending_auras()`.
6. **BUG-7 (Friendly Poly break)** — Add friendly-CC check to pet auto-attack targeting.
7. **BUG-8 (Dead caster spell)** — Cancel `CastingState` on death.

---

## Match Results Summary

| # | Format | Teams | Winner | Duration | Bugs |
|---|--------|-------|--------|----------|------|
| 1 | 1v1 | Warrior vs Mage | Team 2 | — | 1 |
| 2 | 1v1 | Rogue vs Priest | — | — | 2 |
| 3 | 1v1 | Warlock vs Paladin | — | — | 4 |
| 4 | 1v1 | Mage vs Priest | — | — | 1 |
| 5 | 1v1 | Warrior vs Rogue | — | — | 0 |
| 6 | 1v1 | Warlock vs Mage | Team 2 | 23.65s | 2 |
| 7 | 2v2 | War+Pri vs Mage+Pri | Team 2 | 55.60s | 2 |
| 8 | 2v2 | Rog+Pri vs Lock+Pal | Team 1 | 38.64s | 5 |
| 9 | 2v2 | War+Pal vs Mage+Lock | Team 2 | 35.71s | 5 |
| 10 | 2v2 | Rog+Mage vs War+Pal | Team 1 | 41.65s | 1 |
| 11 | 2v2 | Lock+Pri vs Rog+Pal | Team 1 | 38.20s | 1 |
| 12 | 2v2 | Mage+Pri vs War+Lock | Team 1 | 33.97s | 2 |
| 13 | 2v2 | Rog+Pri vs Mage+Pal | Team 1 | 60.86s | 2 |
| 14 | 2v2 | War+Pri vs Lock+Pal | Team 1 | 60.23s | 1 |
| 15 | 3v3 | WMP vs RWPal | Team 1 | 33.78s | 3 |
| 16 | 3v3 | WRP vs MWPal | Team 2 | 35.05s | 3 |
| 17 | 3v3 | WWP vs RMPal | Team 1 | 49.10s | 4 |
| 18 | 3v3 | RMP vs WWPal | Team 1 | 60.86s | 3 |
| 19 | 3v3 | WMPal vs RWP | Team 1 | 34.86s | 1 |
| 20 | 3v3 | WRPal vs MWP | Team 2 | 36.68s | 2 |

**DR system behavior:** Correct across all matches. Escalation 100%→50%→25%→Immune verified. Timer resets confirmed. Categories independent. No DR-specific bugs found.
