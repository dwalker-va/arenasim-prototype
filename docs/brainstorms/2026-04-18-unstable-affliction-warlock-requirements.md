---
date: 2026-04-18
topic: unstable-affliction-warlock
status: planned
---

# Unstable Affliction — New Warlock Ability

## Problem Frame

The 2026-04-12 bug-hunt report flagged Warlock as the clear worst-performing class: 20% win rate in 2v2 and 38% in 3v3, with the Warlock dying first in every 2v2 appearance and dealing <20 total damage in two matches. Root cause: Warlock damage is DoT-dependent, and enemy Priests/Paladins dispel DoTs the moment they land — leaving the class with no meaningful damage contribution against dispel-capable comps.

Unstable Affliction is a new Shadow DoT whose dispel punishes the dispeller with **both** a direct Shadow damage burst and a 5-second blanket silence. It taxes the enemy twice for using the dispel-DoT counterplay that currently neuters the class: once in HP (immediate), once in action economy (5s silence). Healers have to choose between eating DoT pressure or paying HP + a cast lockout — and because dispels pick a random magic debuff, the healer can't cleanly surgically remove Corruption while avoiding UA.

## Requirements

**Ability behavior**
- R1. Add `UnstableAffliction` as a new Warlock Shadow-school DoT ability applied to a single enemy target. UA is a standard DoT: `break_on_damage_threshold = -1.0` (never breaks on damage), matching Corruption.
- R2. UA coexists with Corruption and all curses — a target can have both Corruption and UA stacked simultaneously
- R3. UA follows the existing dispel model: it is magic-dispellable, and dispels pick a random magic debuff (unchanged behavior); the dispeller does **not** get to target UA specifically

**On-dispel backlash**
- R4. When UA is removed by a dispel whose dispeller is on an **opposing team** to UA's caster (queried via the removed aura's existing `caster: Option<Entity>` field), the dispeller receives **both** (a) a `Silence` aura for 5 seconds and (b) a direct Shadow-school damage burst (see R4a). Applies to Dispel Magic, Cleanse, and Devour Magic.
- R4a. Backlash damage is calculated at UA cast time using the Warlock's spell power and stored on the aura, so the amount does not change if the Warlock's stats shift or the Warlock dies before the dispel. Damage profile: Shadow-school direct damage (respects armor/resistance and Divine Shield immunity), base and coefficient deferred to planning-tuning; starting point for simulation is `base: 40` + `spell_power_coefficient: 0.3`. Damage applies **before** the Silence aura so a silenced-and-lethal dispeller dies correctly.
- R5. Silence prevents the affected combatant from using any ability where the caster's resource type is Mana AND the ability's `mana_cost > 0`. (Does not gate based on spell school.)
- R6. Silence does **not** prevent rage-cost, energy-cost, zero-mana-cost abilities (e.g. Divine Shield if configured zero-cost), auto-attacks, or abilities consumed purely by cooldown. If a caster's resource type is not Mana, Silence does not affect them at all.
- R7. Silence triggers **only** on a qualifying enemy dispel. It does not trigger on natural expiration or on any other removal path. (Code audit confirms no "remove DoTs on caster death" behavior exists today.)
- R8. UA backlash refresh rule: if UA is dispelled while the dispeller is already Silenced, the Silence duration resets to 5 seconds from the new dispel event. Silence participates in the existing diminishing returns system with its own DR category `DRCategory::Silence` — the second Silence within the DR window lasts 2.5s (50%), the third lasts 0s (immune), reset per the existing `DR_RESET_TIMER`.
- R12. Silence is magic-dispellable. An ally can dispel a Silence off a silenced teammate via standard Dispel Magic / Cleanse, but silencing a dispeller creates a ~5s window where the enemy team's dispel rotation is disrupted.

**AI integration**
- R9. Warlock AI treats UA as part of its standard opener, applying it alongside Corruption on its current kill target
- R10. UA does not break friendly CC. Implementation: `try_unstable_affliction` must call `ctx.has_friendly_breakable_cc(target)` before casting, mirroring the existing `try_corruption` pattern. The Apr 12 guard is NOT automatic for new abilities — it is a per-ability opt-in via explicit check.
- R11. Backlash fires strictly on the team-comparison check in R4. A Felhunter that dispels an enemy-cast UA off its own ally will be silenced (it is the enemy's UA, dispelled by an opposing-team combatant). Mirror match: if Team 1 Warlock applies UA to a Team 2 combatant and Team 2 dispels it, Team 2's dispeller is silenced — symmetric behavior.

**UI / Presentation**
- R13. UA DoT has a distinct on-target visual effect while active — readable at a glance as "this combatant has UA" and visually distinguishable from Corruption. Follows the spawn/update/cleanup 3-system pattern in `src/states/play_match/rendering/effects.rs`.
- R14. When UA is dispelled and triggers backlash, a visual effect plays on the dispeller at the moment of silence application. The effect should read as "something just hit you" (e.g. shadow burst, impact flash), clearly distinct from the existing `DispelBurst` effect used for ordinary dispels.
- R15. Silenced combatants display floating status text (e.g. "Silenced") over their head for the duration of the silence. Reuses the existing floating-text system (`FloatingCombatText` pattern).
- R16. The UA ability icon appears in all icon-consuming UIs:
    - View-combatant screen: listed in the Warlock's ability roster
    - Ability usage timeline: shown alongside Corruption / Shadow Bolt / etc. when UA is cast
    - On-target debuff display (if one exists): UA icon shown on the target with remaining duration
    - Loadout editor / class ability list
- R17. Silence status (whether as the dispeller backlash or any future Silence application) is visible in the view-combatant screen or equivalent debuff list.
- R18. Download authentic UA icon (`spell_shadow_unstableaffliction` or closest Classic-era match) via the Wowhead MCP and save to `assets/icons/abilities/`.

## Success Criteria

- Over a fresh bug-hunt run against the same seed set as `docs/reports/2026-04-12-bug-hunt-2v2-3v3.md` (seeds 6001–6031, same 2v2 and 3v3 brackets, same match matrix documented in that report), Warlock win rate rises from 20% to at least 35% in 2v2 and from 38% to at least 50% in 3v3. **Risk accepted:** the 2v2 target may not be reachable because UA does not address the "dies first" failure mode. If UA ships correctly and 2v2 win rate stays flat, that result is evidence that survivability is the next Warlock priority — not evidence that UA failed. The 3v3 target is the harder bar for the mechanic itself.
- In at least 3 of 24 matches, a Priest or Paladin is silenced by UA backlash during an observed kill window (combat-log inspection) and at least one heal cast is denied that would otherwise have completed. (The prior "at least one match" criterion was too weak — raising to 3/24 gives a directional signal.)
- Dispel pipeline still works correctly for all other dispellable auras (Corruption, Fear, Polymorph, etc.)
- Warlock team does not exhibit a "runaway silence chain" pattern, defined quantitatively as: **no single match where a given dispeller spends more than 40% of total match duration under Silence**. If this threshold is exceeded in any match, Silence duration is too long.

## Scope Boundaries

- UA does **not** address Warlock's "dies first" problem. That's a separate survivability issue and will need its own treatment (e.g. Death Coil, Shadow Ward, Healthstone) — tracked as out of scope here.
- UA is not a nuke, a channel, or an instant burst spell. It is a DoT with a dispel-punishment rider.
- Silence is a general primitive, not UA-specific. Once introduced it may be reused by future abilities (Warlock talents, Priest Silence, Mage Counterspell) but this brainstorm does not scope those.
- Existing dispel RNG behavior is **not** changing — dispellers still remove a random magic debuff. The strategic tension comes from the new consequence, not from changing dispel mechanics.
- Exact numeric balance (DoT damage per tick, DoT duration, cast time, mana cost) is deferred to planning, to be tuned via headless simulations.

## Key Decisions

- **Blanket silence, not per-school lockout.** User direction: UA silence should prevent any mana-costing ability regardless of school. This is simpler than `SpellSchoolLockout` and more punishing — a silenced Paladin can't Flash of Light *or* Hammer of Justice *or* Cleanse.
- **5-second silence duration.** Matches WoW TBC's UA. Long enough to decide a kill window; short enough that a healer returns to action before the next UA tick cycle.
- **Coexist with Corruption, not replace.** Replacing Corruption would be a smaller net buff and wouldn't fix the "<20 damage in a match" outcomes. Stacking both DoTs plus the dispel-gamble is the targeted fix.
- **Silence triggers only on enemy dispel.** Natural expiration or other removal paths don't count. Keeps the mechanic clean — backlash is strictly "you paid a GCD to dispel, you got silenced".
- **Introduce `Silence` as a reusable aura type.** Even though UA is the only consumer today, the primitive is useful for future Warlock talents, Priest's Silence, and Mage's Counterspell. Distinct from `SpellSchoolLockout` because the gating predicate is cost-based (any mana-cost ability), not school-based — the two primitives model different CC semantics.
- **Silence has its own DR category.** `DRCategory::Silence` is added to the existing diminishing-returns system. Second Silence within the DR window lasts 50%, third is immune, reset per `DR_RESET_TIMER`. Prevents runaway silence chains while keeping the first backlash full-strength.
- **Healer AI continues to dispel normally when UA is present.** The mechanic's tension comes from the *random-debuff* dispel RNG, not from an AI skill-check. If a target has Corruption + UA, the dispeller has ~50% chance per dispel to hit UA and eat the silence. This matches WoW's actual dispel dynamic and keeps the mechanic legible.

## Dependencies / Assumptions

- **`DispelPending` needs a new `dispeller: Entity` field.** Confirmed via code inspection (`src/states/play_match/components/combatant.rs` ~L603 and `src/states/play_match/effects/dispels.rs`): today's struct carries only `target`, `log_prefix`, `caster_class`, `heal_on_success`, and `aura_type_filter`. The dispeller's entity is not plumbed through. All three dispel call sites (`class_ai/mod.rs:402` Priest/Paladin, `class_ai/pet_ai.rs:364` Felhunter) must be updated to populate this field before `process_dispels` can apply Silence to the dispeller.
- **`Aura.caster: Option<Entity>` already exists and is populated.** Verified in `src/states/play_match/components/auras.rs:125` and `AuraPending::from_ability`. The team-comparison check in R4 can use `removed_aura.caster` directly — no new caster-identity field needed.
- **Friendly-CC-break guards are per-ability, not automatic.** `has_friendly_breakable_cc` in `class_ai/mod.rs:239` is explicitly called by ability-application functions (e.g. `try_corruption`). UA's `try_unstable_affliction` must include the same call; protection is not inherited by virtue of being a DoT.
- **Silence enforcement must gate all mana-deduction paths.** Mana is deducted in two places: (1) instant abilities deduct directly in `class_ai/*.rs` (and dispel itself at `class_ai/mod.rs:394`), (2) cast-time abilities deduct in `combat_core/casting.rs:189`. Both paths — and the dispel helper that bypasses `can_cast_config` — must be gated by a Silence check, or refactored to a single gate.
- **Visual systems require dual registration.** Per project memory: graphical-mode visual systems are registered in `src/states/mod.rs` only, NOT in `src/states/play_match/systems.rs` (which is headless-only). The UA DoT aura visual, backlash animation, and silence status text are graphical-only and register in `states/mod.rs`. Any core combat logic (aura application, silence gate, backlash trigger) must be registered in both places or logic will work in headless but be invisible (or vice versa) in the graphical client.
- **View-combatant UI reads ability lists from hardcoded class maps.** Per prior work (Apr 5 class strategic options), the view-combatant screen has hardcoded ability display names per class. Adding UA to the Warlock's visible ability list requires updating that map.
- **New `AuraType::Silence` requires plumbing in multiple systems.** Enumerated for planning: DR classification in `components/auras.rs::DRCategory::from_aura_type` (Silence **has its own DR category** — see Key Decisions; the earlier draft of this note said "freestanding" and is superseded), HUD color in `rendering/hud.rs`, icon mapping in `rendering/mod.rs` (the `get_aura_icon_key` match is exhaustive — adding `AuraType::Silence` without updating it breaks the build), Divine Shield clear-list in `effects/divine_shield.rs`, and dispellability in `is_magic_dispellable` (R12 resolves this: Silence IS magic-dispellable).
- The dispel pipeline exists and is well-tested across Priest Dispel Magic, Paladin Cleanse, and Felhunter Devour Magic. Adding a `dispeller` field does not require refactoring the core dispel flow.

## Outstanding Questions

### Resolve Before Planning
- (none — all review findings resolved 2026-04-18)

### Deferred to Planning
- [Affects R1] [Balance] What are UA's final numbers: cast time (instant vs 1.5s), mana cost, DoT duration, damage per tick? Tune via headless simulations against dispel-heavy comps.
- [Affects R4] [Technical] `process_dispels` is the hook location, immediately after `active_auras.auras.remove(idx_to_remove)`. Compare `removed_aura.caster`'s team to `dispeller`'s team; if opposing, queue a Silence `AuraPending` on the dispeller.
- [Affects R5] [Technical] Enforce Silence in `can_cast_config` (abilities.rs) as the primary gate, AND add a secondary gate at the dispel-helper mana deduction in `class_ai/mod.rs:394` which bypasses `can_cast_config`. Audit all instant-cast paths to confirm each goes through `can_cast_config`.
- [Affects R6] [Technical] Enumerate each class's ability roster and confirm which abilities remain usable while Silenced. Key audit targets: Paladin Divine Shield and Hammer of Justice, Priest Power Word: Shield and Dispel Magic, healer trinket-style instants if any exist.

### Review findings resolved (2026-04-18)
All findings from the document review have been resolved:
1. **Silence architecture** → new `AuraType::Silence` (distinct from `SpellSchoolLockout` because the predicate is cost-based, not school-based).
2. **2v2 target reachability** → keep 35%, accept the risk. If 2v2 stays flat after UA ships, that is evidence survivability is the next priority, not evidence UA failed.
3. **Healer AI behavior** → AI continues dispelling normally. The mechanic's tension is in the *random-debuff* RNG, not in an AI skill-check. Corruption + UA = ~50% chance per dispel to eat a silence.
4. **Missing alternatives** → added 2s dispel-immunity window, raised dispel mana cost, and Shadow Bolt boost to Alternatives Considered with rationales.
5. **R8 refresh rule** → reset to 5s, gated by a new `DRCategory::Silence` so chain dispels hit DR (50% → 25% → immune) and can't produce runaway silence chains.

## Alternatives Considered

- **Silence Holy school only** — narrower, simpler, but since no DPS class currently dispels, it's functionally identical to "silence all schools" today while being less reusable. Rejected.
- **Silence-only backlash (no damage)** — simpler, narrower. Rejected on 2026-04-18: the damage burst reinforces that dispelling UA is a *costly* choice rather than merely a strategic pause, which better serves the "punish the dispeller" theory. Starting damage is conservative (40 + 0.3 SP) so the risk of overshooting the Warlock buff is controllable via simulation-driven tuning.
- **Replace Corruption entirely** — smaller net buff; doesn't fix the <20-damage outcomes. Rejected.
- **Only punishes healers (Priest/Paladin)** by checking class — feels arbitrary and doesn't generalize. Rejected.
- **2-second dispel-immunity window on all DoTs after application** — directly addresses "DoTs dispelled the moment they land" by making instant dispel pointless. Rejected because (a) it's a blanket buff across every DoT-casting class, not just Warlock, and the bug-hunt data did not flag Shadow Priest / Rogue Rend / Hunter traps as underperforming; (b) it produces a passive-damage-buff feel rather than the counterplay drama UA creates.
- **Raise healer dispel mana cost or cooldown** — would make healers dispel less often, indirectly helping Warlock. Rejected because it weakens dispel against other dispel-dependent mechanics (Polymorph removal, Fear removal) and is a nerf to healers broadly rather than a Warlock buff.
- **Boost Shadow Bolt raw damage** — gives Warlock a non-DoT damage floor that dispels can't touch. Rejected because Shadow Bolt has a 2s cast and is routinely kicked/interrupted; buffing its damage doesn't help against the classes that make Warlock suffer most. UA's punishment mechanic is a more surgical fit for the observed problem.

## Next Steps

-> `/ce:plan` for structured implementation planning
