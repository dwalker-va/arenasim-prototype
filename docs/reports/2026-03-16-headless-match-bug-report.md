# Headless Match Bug Report — 2026-03-16

**Matches run:** 24 (12 x 2v2, 12 x 3v3)
**New bugs found:** 7 distinct categories
**Known issues confirmed:** KI-1 (x8), KI-4 (x1), KI-5 (x1), KI-6 (x2), KI-7 (x10)
**Agents used:** 6 parallel runners

---

## New Bugs

### BUG-1: Team Composition Corruption — Classes Swapped Between Teams or Missing (P0)

**Frequency:** 3/24 matches (m17, m18, m19)
**Severity:** P0 (critical)

In 3v3 matches, combatants end up on the wrong team or fail to spawn entirely. The team initialization system appears to corrupt slot assignments when building larger teams.

**Examples:**
- **m17** (seed 6017): Config `team1: [Warrior, Rogue, Mage], team2: [Warlock, Priest, Paladin]`. Log shows Rogue on Team 2 and Priest on Team 1 — slot 2 classes swapped between teams.
- **m18** (seed 6018): Config specifies 3v3 but only 4 of 6 combatants spawn. Team 1 Mage and Team 2 Warlock are completely absent. Match runs as 2v2.
- **m19** (seed 6019): Config `team1: [Warrior, Warrior, Paladin]`. Log shows Team 1 = Warrior, Rogue, Priest — a Rogue appeared despite not being in the config at all.

**Root cause hypothesis:** The team spawning system has a bug in slot assignment for 3v3 configs. Possibly an indexing error when iterating team members, or a shuffle/sort that crosses team boundaries. The bug appears non-deterministic (m13-m16 and m20-m24 had correct compositions).

**Reproduction:**
```json
{"team1":["Priest","Paladin","Mage"],"team2":["Warrior","Rogue","Warlock"],"random_seed":6018}
```

---

### BUG-2: HoJ Stun Redirected to Felhunter Pet Instead of Warlock (P0)

**Frequency:** 4/24 matches (m04, m09, m16, and likely others with Warlock+Paladin matchups)
**Severity:** P0 (critical)

When Hammer of Justice targets a Warlock, the stun's DR tracking (and likely the actual CC effect) is applied to the Felhunter pet entity instead of the Warlock. The Warlock continues acting freely during what should be a 6s stun.

**Examples:**
- **m04** (seed 6004): `[16.37s] CC stuns Team 1 Warlock (6.0s)` → `[16.39s] Hammer of Justice on Team 1 Felhunter (6.0s, DR: 100%)` → Warlock casts Curse of Agony at 18.36s during "stun"
- **m09** (seed 6009): Same pattern — HoJ log says Warlock, DR entry says Felhunter
- **m16** (seed 6016): Same pattern — Warlock uses Wand Shot at 16.97s and Curse of Agony at 18.37s during "stun"

**Root cause hypothesis:** When iterating team members to find the Warlock entity, the Felhunter pet entity is selected instead (or in addition to) the Warlock. The pet/owner entity confusion also manifests in buff targeting (BUG-3).

**Reproduction:**
```json
{"team1":["Warlock","Priest"],"team2":["Mage","Paladin"],"random_seed":6004}
```

---

### BUG-3: Buffs Target Felhunter Pet Instead of Warlock (P2)

**Frequency:** 1/24 matches explicitly observed (m04), likely more
**Severity:** P2 (medium)

Power Word: Fortitude cast on "Team 1 Warlock" applies the buff to the Felhunter instead.

**Examples:**
- **m04** (seed 6004): `[0.00s] CAST Priest casts PW:Fort on Team 1 Warlock` → `[0.00s] BUFF Team 1 Felhunter gains PW:Fort (+30 max HP)`. Priest then re-casts at 1.49s to actually buff the Warlock.

**Root cause hypothesis:** Same pet/owner entity confusion as BUG-2. The Felhunter is encountered before the Warlock in entity queries.

---

### BUG-4: CC Bypass — Paladin Casts Divine Shield While Polymorphed (P1)

**Frequency:** 3/24 matches (m09, m14, m17)
**Severity:** P1 (high)

Paladins cast Divine Shield while under active Polymorph with no damage having broken the CC. In WoW Classic, Polymorph is an incapacitate that prevents all actions — Divine Shield should not be castable while polymorphed.

**Examples:**
- **m09** (seed 6009): Poly at 17.65s (10s duration) → Divine Shield at 20.80s (3.15s into poly, no damage in between)
- **m17** (seed 6017): Poly at 17.83s → Divine Shield at 21.70s (3.87s into poly)
- **m14** (seed 6014): Poly at 19.30s → Divine Shield at 20.99s (1.69s into poly)

**Root cause hypothesis:** Divine Shield is likely flagged as "usable while CC'd" (like a PvP trinket), but it should only be usable while stunned, not while polymorphed/incapacitated. The CC-type check may be too permissive.

**Note:** In WoW Classic, Divine Shield CAN be used while stunned (e.g., during HoJ) but NOT while polymorphed. The system needs to distinguish between stun (allows bubble) and incapacitate/poly (does not).

**Reproduction:**
```json
{"team1":["Mage","Warlock"],"team2":["Priest","Paladin"],"random_seed":6009}
```

---

### BUG-5: Friendly Fire Breaking Own Team's CC (P1)

**Frequency:** 4/24 matches (m02, m15, m16, m19)
**Severity:** P1 (high)

A team's own damage (usually DoT ticks) breaks their own Polymorph on the same target. This is both an AI problem (applying DoTs to polymorphed targets) and potentially a combat system issue (same-team damage shouldn't break same-team CC).

**Examples:**
- **m02** (seed 6002): Team 2 Mage Frostbolts own team's polymorphed target directly
- **m15** (seed 6015): Team 2 Warlock's Curse of Agony tick breaks Team 2 Mage's Polymorph
- **m16** (seed 6016): Team 1 Warrior's Rend tick breaks Team 1 Mage's Polymorph
- **m19** (seed 6019): Team 2 Warlock's Curse of Agony tick breaks Team 2 Mage's Polymorph

**Root cause hypothesis:** Two issues: (1) AI doesn't check for friendly CC before applying DoTs/damage to a target; (2) The Polymorph break-on-damage system doesn't distinguish between enemy and friendly damage sources.

**Reproduction:**
```json
{"team1":["Warrior","Mage","Paladin"],"team2":["Rogue","Warlock","Priest"],"random_seed":6016,"team1_kill_target":1,"team2_kill_target":1}
```

---

### BUG-6: CC/Damage Applied to Dead Targets (P2)

**Frequency:** 4/24 matches (m08, m14, m21, m22)
**Severity:** P2 (medium)

Frost Nova AoE roots and/or melee auto-attacks continue targeting dead combatants.

**Examples:**
- **m08** (seed 6008): Frost Nova roots dead Warrior at 23.47s (died 20.92s)
- **m14** (seed 6014): Frost Nova roots dead Warrior at 25.54s (died 22.48s)
- **m21** (seed 6021): Frost Nova roots dead Warlock twice at 24.02s and 24.14s (died 22.98s)
- **m22** (seed 6022): Warrior auto-attacks dead Warlock at 18.17s (died 18.03s)

**Root cause hypothesis:** AoE target filtering and melee target-switching don't check the `is_alive` state. Melee attackers also have a brief window where they keep swinging at a dead target before retargeting.

**Reproduction:**
```json
{"team1":["Mage","Mage","Priest"],"team2":["Warrior","Warlock","Paladin"],"random_seed":6021,"map":"PillaredArena"}
```

---

### BUG-7: Frost Nova CC Applied Through Divine Shield (P1)

**Frequency:** 1/24 matches (m02)
**Severity:** P1 (high)

Frost Nova's root CC is applied to a Paladin with active Divine Shield. Damage is correctly blocked to 0, but the CC effect still lands.

**Examples:**
- **m02** (seed 6002): Divine Shield at 27.04s → Frost Nova root at 28.32s (damage = 0 but root applied for 6.0s)

**Root cause hypothesis:** The CC immunity check is separate from the damage immunity check. Divine Shield blocks damage but the CC application path doesn't verify immunity status.

**Reproduction:**
```json
{"team1":["Mage","Priest"],"team2":["Warlock","Paladin"],"random_seed":6002}
```

---

## Known Issues Confirmed

| KI | Description | Matches Observed |
|----|-------------|-----------------|
| KI-1 | DoTs tick after caster death | m03, m04, m05, m08, m14, m15, m16, m21 |
| KI-4 | Extended OOM auto-attack phase | m01 |
| KI-5 | AI idle during Divine Shield | m24 |
| KI-6 | Warrior 0 damage (chain-CC'd) | m12, m21 |
| KI-7 | Duplicate buff at 0.00s | m02, m04, m06, m10, m13, m14, m15, m17, m19, m22 |

KI-7 (duplicate buff stacking) was the most commonly observed known issue, appearing in 10/24 matches — nearly every match involving Warrior (Battle Shout) or Paladin (Devotion Aura).

---

## Clean Matches

The following matches had no new bugs detected:

- m01 (2v2 Warrior+Priest vs Rogue+Paladin)
- m03 (2v2 Rogue+Priest vs Warrior+Paladin)
- m05 (2v2 Warrior+Mage vs Rogue+Warlock)
- m06 (2v2 Rogue+Mage vs Warrior+Warlock)
- m10 (2v2 Warrior+Warrior vs Priest+Paladin)
- m11 (2v2 Rogue+Paladin vs Mage+Warlock, Pillared)
- m13 (3v3 WMP vs RLP)
- m23 (3v3 Mirror WMP)
- m24 (3v3 Mirror RLP, Pillared)

---

## Match Results Summary

| # | Format | Teams | Winner | Duration | Bugs |
|---|--------|-------|--------|----------|------|
| m01 | 2v2 | War+Pri vs Rog+Pal | Team 1 | 62.51s | Clean |
| m02 | 2v2 | Mage+Pri vs Lock+Pal | Team 1 | 41.83s | BUG-5, BUG-7 |
| m03 | 2v2 | Rog+Pri vs War+Pal | Team 1 | 64.99s | Clean |
| m04 | 2v2 | Lock+Pri vs Mage+Pal | Team 2 | 43.99s | BUG-2, BUG-3 |
| m05 | 2v2 | War+Mage vs Rog+Lock | Team 1 | 29.24s | Clean |
| m06 | 2v2 | Rog+Mage vs War+Lock | Team 1 | 26.34s | Clean |
| m07 | 2v2 | Pri+Pal vs War+Rog | Team 2 | 46.32s | BUG-2* |
| m08 | 2v2 | War+Pal vs Mage+Pri (Pillared) | Team 2 | 44.35s | BUG-6 |
| m09 | 2v2 | Mage+Lock vs Pri+Pal | Team 1 | 45.79s | BUG-2, BUG-4 |
| m10 | 2v2 | War+War vs Pri+Pal | Team 1 | 29.56s | Clean |
| m11 | 2v2 | Rog+Pal vs Mage+Lock (Pillared) | Team 1 | 40.31s | Clean |
| m12 | 2v2 | War+Rog vs Mage+Lock | Team 2 | 26.97s | Clean |
| m13 | 3v3 | WMP vs RLP | Team 1 | 49.58s | Clean |
| m14 | 3v3 | WLP vs RMP | Team 2 | 40.09s | BUG-4, BUG-6 |
| m15 | 3v3 | WRP vs MLP | Team 1 | 66.88s | BUG-5 |
| m16 | 3v3 | WMPal vs RLP | Team 1 | 36.60s | BUG-2, BUG-5 |
| m17 | 3v3 | WRM vs LPrPal | Team 1 | 49.58s | BUG-1, BUG-4 |
| m18 | 3v3 | PrPalM vs WRL | Team 2 | 46.32s | BUG-1 |
| m19 | 3v3 | WWPal vs MLPr | Team 1 | 66.88s | BUG-1, BUG-5 |
| m20 | 3v3 | RRPr vs WMPal | Team 2 | 32.51s | Minor (dup Frost Nova) |
| m21 | 3v3 | MMPr vs WLPal (Pillared) | Team 1 | 43.64s | BUG-6 |
| m22 | 3v3 | LLPal vs WRPr | Team 2 | 41.74s | BUG-6 |
| m23 | 3v3 | Mirror WMP | Team 1 | 30.23s | Clean |
| m24 | 3v3 | Mirror RLP (Pillared) | Team 2 | 39.04s | Clean |

*m07 BUG-2 variant: Warrior uses Pummel during HoJ stun (CC bypass without pet involvement)

---

## Observations (Not Bugs)

1. **Paladin AI goes auto-attack-only when solo** (m07): After Priest dies, Paladin exclusively auto-attacks for 13s straight. No self-heals, no Hammer of Justice, no Judgement. Low mana may explain some of this, but zero-cost abilities were also unused. AI priority gap when healing target dies.

2. **Ambiguous logging with duplicate classes** (m10, m20, m22): When both slots share a class (e.g., Warrior+Warrior), the log shows "Team 1 Warrior" with no slot indicator, making it impossible to distinguish which combatant performed an action. This makes CC bypass analysis unreliable for mirror-class compositions.

3. **Rogue 0-damage in 3v3** (m20): Team 1 Rogue (slot 2) dealt 0 total damage. Not chain-CC'd — the Rogue simply never got an offensive opener off before dying. Used its one action window on Kidney Shot (CC, no damage) instead of a damage ability.

4. **Duplicate Frost Nova CC application** (m20): Frost Nova applied its root and damage twice to the same target at the same timestamp, suggesting AoE hit detection fires twice on the same combatant.
