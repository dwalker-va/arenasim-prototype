---
date: 2026-04-05
topic: tier-1-items
---

# Tier 1 Equipment Items (Item Level 75)

## Problem Frame

All current equipment sits at item level 54-60 (Tier 0), providing no sense of progression or meaningful gearing decisions. Adding a higher tier of items creates a power progression layer and introduces stat specialization that makes upgrades feel interesting rather than purely numerical.

## Requirements

**Tier Structure**
- R1. Add a complete set of Tier 1 items at item level 75 (±3 for slot variation, matching Tier 0's pattern of slightly varying ilvls across slots)
- R2. Tier 1 items must be entirely new names and identities — not renamed/suffixed versions of Tier 0

**Stat Design**
- R3. Health and mana must remain present on every Tier 1 item where they appear at Tier 0, though their magnitude may be reduced to accommodate specialization (R4). These are PvP-essential stats — no pure glass-cannon builds.
- R4. Tier 1 items should lean into role specialization (e.g. a DPS plate helm emphasizes crit/AP more aggressively than Tier 0) while a few interesting pieces per set offer hybrid stat mixes
- R5. All items must pass the existing item level budget validation (within 5% tolerance)

**Set Coverage**
- R6. Mirror Tier 0's armor set structure: plate DPS (9 pieces, including a caster/hybrid head variant), plate holy (8 pieces), mail (8 pieces), leather (8 pieces), cloth (8 pieces)
- R7. Include Tier 1 versions of shared slots: cloaks (3), necklaces (3), rings (2 Ring1 + 2 Ring2), trinkets (2 Trinket1, matching Tier 0 — no Trinket2)
- R8. Include Tier 1 weapons matching Tier 0's roster: one per weapon_type (Axe, Sword, Dagger, Mace, Staff, Wand, Bow, Crossbow, OffhandFrill, Shield) — 10 weapons total

**Integration**
- R9. Add all new ItemId variants to the equipment enum
- R10. Do not modify default loadouts — Tier 0 remains the default. Temporary Tier 1 test loadouts may be created for headless simulation validation.

## Success Criteria

- All Tier 1 items pass `cargo test` budget validation
- For each slot, the Tier 1 item's total budget usage is at least 20% higher than its Tier 0 counterpart
- At least 2 items per armor set include a stat not present on their Tier 0 counterpart, or shift more than 30% of their budget toward a different primary stat
- Tier 1 vs Tier 0 headless simulation shows a noticeable power difference (using temporary test loadouts)
- Item names feel like a natural progression of the game's fantasy

**Expected item count:** ~61 items (41 armor pieces across 5 sets, 10 shared slot items, 10 weapons)

## Scope Boundaries

- No new armor type variants (e.g. shadow cloth vs healer cloth) — future work
- No loadout changes — Tier 0 remains the default equipment
- No tier selection UI or match config options — items just exist in the pool
- No item icons for Tier 1 — can be added later
- No Tier 2+ items — validate this tier first

## Key Decisions

- **Item level 75 for Tier 1**: +15 from Tier 0's ~60 baseline. Creates a noticeable power jump (~25-40% budget increase) without being overwhelming
- **All new item names**: Avoids feeling like reskins; each tier has its own identity
- **Same set structure as Tier 0**: Keeps scope manageable; additional role variants (shadow cloth, offensive leather, etc.) planned as future work across both tiers
- **No default loadout changes**: Decouples item creation from the progression/selection system, which hasn't been designed yet. Temporary test loadouts are in scope for validation only.

## Outstanding Questions

### Deferred to Planning
- [Affects R1][Needs research] What specific ilvl-per-slot mapping should Tier 1 use within the 72-78 range? Tier 0 uses 54-60 (wrists=54, waist=55, shoulders/feet=56, head/chest/legs=58-60). Planning should assign Tier 1 ilvls per slot following a similar pattern.
- [Affects R4][Needs research] What specific stat profiles make sense for each Tier 1 set? Planning should draft stat allocations within budget constraints
- [Affects R9][Technical] Does ItemConfig need a `tier` field so a future selection system can filter items by tier? Adding it now is trivial; adding it later requires touching every item.
- [Affects R4][Needs research] Verify that the budget formula produces interesting stat distributions at ilvl 75 — budgets should be large enough to differentiate items but not so large that every stat fits comfortably

## Next Steps

-> `/ce:plan` for structured implementation planning
