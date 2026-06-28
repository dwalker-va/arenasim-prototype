//! Class-Specific AI Modules
//!
//! This module contains the AI decision logic for each character class.
//! Each class has a standalone `decide_<class>_action()` function that is
//! called from `combat_ai.rs` based on the combatant's `CharacterClass`.
//!
//! ## Architecture
//!
//! The combat AI works in two phases:
//! 1. **Context Building**: `CombatContext` collects all game state needed for decisions
//! 2. **Decision Making**: `combat_ai.rs` dispatches to the appropriate class module's
//!    `decide_<class>_action()` function, which directly executes abilities
//!
//! Shared helpers like `CombatContext`, `CombatantInfo`, and healer utilities
//! live in this module and are used by all class AI files.

pub mod mage;
pub mod dps_postures;
pub mod priest;
pub mod warrior;
pub mod rogue;
pub mod warlock;
pub mod paladin;
pub mod hunter;
pub mod shaman;
pub mod hunter_dip;
pub mod pet_ai;
pub mod cast_guard;
pub mod combat_snapshot;
pub(crate) mod healer_postures;
pub(crate) mod paladin_postures;

use bevy::prelude::*;
use std::collections::BTreeMap;

use crate::combat::log::CombatLog;
use super::match_config::CharacterClass;
use super::abilities::AbilityType;
use super::ability_config::AbilityDefinitions;
use super::components::{Aura, ActiveAuras, Combatant, AuraType, DispelPending, PetType, DRCategory, DRTracker};
use super::constants::GCD;
use super::{is_spell_school_locked, is_silenced};
use super::utils::log_ability_use;

/// Per-frame snapshot of a single combatant, used for AI decision making.
#[derive(Clone, Copy, Debug)]
pub struct CombatantInfo {
    pub entity: Entity,
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub current_health: f32,
    pub max_health: f32,
    pub current_mana: f32,
    pub max_mana: f32,
    /// Per-frame snapshot from Transform.
    pub position: Vec3,
    /// Estimated planar velocity (XZ, units/sec): the facing heading (from the
    /// Transform rotation, which `move_to_target` points along travel) scaled by
    /// `base_movement_speed`. `Vec3::ZERO` when the combatant is casting or
    /// channeling (planted) — so a consumer can lead a moving target and drop
    /// directly on a stationary one. Used by the Hunter to lead Freezing Trap
    /// into a kiting target's path. An estimate: a non-casting but idle target
    /// carries a stale heading, but trap targets (healers/casters) are normally
    /// either casting or kiting.
    pub velocity: Vec3,
    pub is_alive: bool,
    pub stealthed: bool,
    pub target: Option<Entity>,
    pub is_pet: bool,
    /// The ability this combatant is currently casting or channeling, if any.
    /// `Some` iff the entity has a live `CastingState`/`ChannelingState` this
    /// frame. Consumers map it to a `SpellSchool` (via `AbilityDefinitions`) to
    /// reason about interruptibility — e.g. the Rogue's Kidney Shot chain firing
    /// on a cast whose school is NOT covered by an active lockout.
    pub casting_ability: Option<AbilityType>,
    pub pet_type: Option<PetType>,
    /// Owner→pet reverse lookup. For pet-owning combatants (Hunter, Warlock)
    /// this is `Some(pet_entity)`. For pets themselves and non-owners, `None`.
    /// Populated by `CombatSnapshot::build` and `pet_ai_system`'s local build.
    pub pet: Option<Entity>,
}

/// Deferred instant melee attack (Mortal Strike, Ambush, Sinister Strike, etc.)
#[derive(Clone, Copy)]
pub struct QueuedInstantAttack {
    pub attacker: Entity,
    pub target: Entity,
    pub damage: f32,
    pub attacker_team: u8,
    pub attacker_class: CharacterClass,
    pub ability: AbilityType,
    pub is_crit: bool,
}

/// Deferred AoE damage (Frost Nova).
#[derive(Clone, Copy)]
pub struct QueuedAoeDamage {
    pub caster: Entity,
    pub target: Entity,
    pub damage: f32,
    pub caster_team: u8,
    pub caster_class: CharacterClass,
    pub target_pos: Vec3,
    pub is_crit: bool,
}

impl CombatantInfo {
    /// Health as a percentage (0.0 to 1.0)
    pub fn health_pct(&self) -> f32 {
        if self.max_health > 0.0 {
            self.current_health / self.max_health
        } else {
            0.0
        }
    }

    /// Mana as a percentage (0.0 to 1.0)
    pub fn mana_pct(&self) -> f32 {
        if self.max_mana > 0.0 {
            self.current_mana / self.max_mana
        } else {
            0.0
        }
    }

    /// Distance to another position
    pub fn distance_to(&self, other_pos: Vec3) -> f32 {
        self.position.distance(other_pos)
    }
}

/// Shared context for AI decision making.
///
/// This struct provides a read-only view of the game state that AI modules
/// can use to make decisions without directly accessing ECS queries.
///
/// The `combatants` map contains ALL entities including pets.
/// Use `alive_enemies()` / `alive_allies()` for primary-combatant-only queries.
/// When iterating `combatants` directly, filter with `!info.is_pet`
/// unless the ability should affect pets (e.g., AoE damage, auto-attacks).
pub struct CombatContext<'a> {
    /// Map of entity to combatant info (per-frame snapshot).
    /// `BTreeMap` is used (not `HashMap`) so iteration order is deterministic
    /// across runs — required for seeded replays. See `CombatSnapshot` docs.
    pub combatants: &'a BTreeMap<Entity, CombatantInfo>,
    /// Map of entity to their active auras
    pub active_auras: &'a BTreeMap<Entity, Vec<Aura>>,
    /// Map of entity to their DR tracker (for immunity queries)
    pub dr_trackers: &'a BTreeMap<Entity, DRTracker>,
    /// Map of entity to their per-ability cooldowns (per-frame snapshot).
    /// Hunter AI reads this when dispatching pet abilities — it needs to know
    /// the pet's cooldown state without holding a mutable handle to pet
    /// `Combatant`. `BTreeMap` (nested) for determinism.
    pub ability_cooldowns: &'a BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
    /// The combatant making the decision
    pub self_entity: Entity,
}

impl<'a> CombatContext<'a> {
    /// Get info about self
    pub fn self_info(&self) -> Option<&CombatantInfo> {
        self.combatants.get(&self.self_entity)
    }

    /// Get info about target (if any)
    pub fn target_info(&self) -> Option<&CombatantInfo> {
        self.self_info()
            .and_then(|info| info.target)
            .and_then(|target| self.combatants.get(&target))
    }

    /// Get auras on self
    pub fn self_auras(&self) -> Option<&Vec<Aura>> {
        self.active_auras.get(&self.self_entity)
    }

    /// Get auras on target
    pub fn target_auras(&self) -> Option<&Vec<Aura>> {
        self.target_info()
            .and_then(|info| self.active_auras.get(&info.entity))
    }

    /// Check if self has a specific aura type
    pub fn has_aura(&self, aura_type: AuraType) -> bool {
        self.self_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if target has a specific aura type
    pub fn target_has_aura(&self, aura_type: AuraType) -> bool {
        self.target_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if self is incapacitated (stunned, feared, or polymorphed).
    /// NOTE: The canonical CC type list lives in `utils::is_incapacitated`.
    /// CombatContext can't delegate because it stores auras as `&[Aura]`, not `&ActiveAuras`.
    pub fn is_incapacitated(&self) -> bool {
        self.has_aura(AuraType::Stun) || self.has_aura(AuraType::Fear) || self.has_aura(AuraType::Polymorph) || self.has_aura(AuraType::Incapacitate)
    }

    /// Check if an entity is currently CC'd (Stun, Fear, Root, or Polymorph).
    /// Useful for preventing CC overlap on targets.
    pub fn is_ccd(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    matches!(
                        a.effect_type,
                        AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph | AuraType::Incapacitate
                    )
                })
            })
            .unwrap_or(false)
    }

    /// The enemy healer — first alive non-pet Priest/Paladin in deterministic
    /// entity order (BTreeMap), if any. Shared by bucket-A burst-during-CC and
    /// the Hunter's freezing-trap targeting (which both want "the healer to
    /// shut down"), replacing per-class `find_enemy_healer` copies.
    pub fn enemy_healer(&self) -> Option<Entity> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .iter()
            .find(|(_, info)| {
                info.team != my_team && info.is_alive && !info.is_pet && info.class.is_healer()
            })
            .map(|(entity, _)| *entity)
    }

    /// True when a living enemy healer exists AND is currently unable to cast
    /// a heal — the bucket-A burst window. This is the CAST-PREVENTING CC
    /// subset (Stun / Fear / Polymorph / Incapacitate), NOT [`is_ccd`]: a
    /// rooted healer still heals freely, so Root must not open a burst window.
    pub fn enemy_healer_is_cced(&self) -> bool {
        let Some(healer) = self.enemy_healer() else {
            return false;
        };
        self.active_auras.get(&healer).map_or(false, |auras| {
            auras.iter().any(|a| {
                matches!(
                    a.effect_type,
                    AuraType::Stun
                        | AuraType::Fear
                        | AuraType::Polymorph
                        | AuraType::Incapacitate
                )
            })
        })
    }

    /// Get all alive enemies (excluding pets)
    pub fn alive_enemies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get all alive allies (including self, excluding pets)
    pub fn alive_allies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team == my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get lowest health ally
    pub fn lowest_health_ally(&self) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Find the lowest-health ally below a given HP percentage threshold, within range, excluding pets.
    pub fn lowest_health_ally_below(
        &self,
        max_hp_pct: f32,
        max_range: f32,
        my_pos: Vec3,
    ) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .filter(|info| {
                !info.is_pet
                    && info.health_pct() < max_hp_pct
                    && my_pos.distance(info.position) <= max_range
            })
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Returns true if all allies are above the given HP threshold.
    pub fn is_team_healthy(&self, threshold: f32, my_pos: Vec3) -> bool {
        self.lowest_health_ally_below(threshold, f32::MAX, my_pos).is_none()
    }

    /// Check if `entity` currently has a specific aura type.
    fn entity_has_aura(&self, entity: Entity, aura_type: AuraType) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Visibility check mirroring the `can_see` closure in `combat_ai.rs`:
    /// a stealthed enemy is invisible unless the observer has Shadow Sight,
    /// or the enemy itself holds Shadow Sight (picking up the buff reveals
    /// the holder).
    fn visible_to(&self, observer: Entity, enemy: &CombatantInfo) -> bool {
        !enemy.stealthed
            || self.entity_has_aura(observer, AuraType::ShadowSight)
            || self.entity_has_aura(enemy.entity, AuraType::ShadowSight)
    }

    // ------------------------------------------------------------------
    // Threat predicates (healer postures — R6/R7 trigger and window inputs)
    // ------------------------------------------------------------------

    /// Visible enemies whose current target is `me`. Enemy pets count as
    /// threats (`is_pet` entities are included, unlike `alive_enemies()`).
    /// Stealth-filtered: a stealthed enemy is NOT a threat unless shadow
    /// sight applies — healers never pre-dodge invisible Rogues.
    ///
    /// Iterates the `BTreeMap` snapshot, so the returned order is
    /// deterministic (ascending `Entity`).
    pub fn enemies_targeting(&self, me: Entity) -> Vec<&CombatantInfo> {
        let Some(my_team) = self.combatants.get(&me).map(|i| i.team) else {
            return Vec::new();
        };
        self.combatants
            .values()
            .filter(|c| {
                c.team != my_team
                    && c.is_alive
                    && c.target == Some(me)
                    && self.visible_to(me, c)
            })
            .collect()
    }

    /// Visible alive enemies (pets included) within `radius` of `pos` —
    /// the proximity half of the PRESSURED threat set (an enemy in your face
    /// is a threat even when it currently targets someone else). Same stealth
    /// filtering as `enemies_targeting`; same deterministic BTree order.
    pub fn visible_enemies_within(&self, me: Entity, pos: Vec3, radius: f32) -> Vec<&CombatantInfo> {
        let Some(my_team) = self.combatants.get(&me).map(|i| i.team) else {
            return Vec::new();
        };
        self.combatants
            .values()
            .filter(|c| {
                c.team != my_team
                    && c.is_alive
                    && pos.distance(c.position) <= radius
                    && self.visible_to(me, c)
            })
            .collect()
    }

    /// The nearest alive visible enemy (including pets) currently targeting
    /// `me`. Ties resolve to the lowest `Entity` (BTreeMap iteration order)
    /// for determinism.
    pub fn primary_attacker(&self, me: Entity) -> Option<&CombatantInfo> {
        let my_pos = self.combatants.get(&me)?.position;
        self.enemies_targeting(me)
            .into_iter()
            .min_by(|a, b| {
                my_pos
                    .distance(a.position)
                    .partial_cmp(&my_pos.distance(b.position))
                    .unwrap()
            })
    }

    /// Remaining movement-impairment window on `attacker`: the longest
    /// remaining Root/Stun/Incapacitate duration, or `None` if the attacker
    /// is free to move. Fear is deliberately excluded — a feared attacker
    /// wanders away on its own, so it is not a reliable escape window.
    pub fn attacker_escape_window(&self, attacker: Entity) -> Option<f32> {
        self.active_auras.get(&attacker).and_then(|auras| {
            auras
                .iter()
                .filter(|a| {
                    matches!(
                        a.effect_type,
                        AuraType::Root | AuraType::Stun | AuraType::Incapacitate
                    )
                })
                .map(|a| a.duration)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
        })
    }

    /// Product of `MovementSpeedSlow` magnitudes currently on `entity`
    /// (`1.0` = unslowed; `0.5` = moving at half speed). Mirrors the
    /// executor's slow handling in `move_to_target`, so the ESCAPE window
    /// math (R7) predicts the same effective speed the directive will
    /// actually move at.
    pub fn movement_slow_multiplier(&self, entity: Entity) -> f32 {
        self.active_auras
            .get(&entity)
            .map(|auras| {
                auras
                    .iter()
                    .filter(|a| a.effect_type == AuraType::MovementSpeedSlow)
                    .map(|a| a.magnitude)
                    .product()
            })
            .unwrap_or(1.0)
    }

    /// Derived closing intent — no velocity history (keeps the hot path free
    /// of mutable state). `threat` is closing on `me` when its kill target is
    /// `me` AND its pursuit movement would reduce the distance this frame,
    /// i.e. it currently sits beyond its preferred range to me (pursuit in
    /// `move_to_target` walks toward targets outside `preferred_range` and
    /// holds position inside it). A stationary caster already in range
    /// targeting me is NOT closing.
    pub fn is_closing(&self, threat: Entity, me: Entity) -> bool {
        let Some(threat_info) = self.combatants.get(&threat) else {
            return false;
        };
        let Some(my_info) = self.combatants.get(&me) else {
            return false;
        };
        if !threat_info.is_alive || threat_info.target != Some(me) {
            return false;
        }
        let preferred = match threat_info.pet_type {
            Some(pet_type) => pet_type.preferred_range(),
            None => threat_info.class.preferred_range(),
        };
        threat_info.distance_to(my_info.position) > preferred
    }

    /// Check if target has a break-on-any-damage CC from a friendly caster.
    /// Uses threshold-based detection: any aura with `break_on_damage_threshold == 0.0`
    /// (breaks on ANY damage) from a same-team caster is protected.
    /// Used to prevent AI from breaking own team's CC with damage/DoTs.
    pub fn has_friendly_breakable_cc(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.break_on_damage_threshold == 0.0
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
    }

    /// Check if target has DoTs from a friendly caster that would break Polymorph/Freezing Trap.
    pub fn has_friendly_dots_on_target(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageOverTime
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
    }

    /// Check if an entity has damage immunity (Divine Shield).
    pub fn entity_is_immune(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity))
            .unwrap_or(false)
    }

    /// Check if an entity is DR-immune to a specific CC category.
    /// AI uses this to avoid wasting CC abilities into immunity.
    pub fn is_dr_immune(&self, entity: Entity, category: DRCategory) -> bool {
        self.dr_trackers
            .get(&entity)
            .map(|tracker| tracker.is_immune(category))
            .unwrap_or(false)
    }

    /// Start a decision-trace `ability_decision` builder for the current
    /// actor. Returns None only when the snapshot doesn't contain self
    /// (defensive — shouldn't happen in normal dispatch).
    ///
    /// Replaces the actor_view + target_view + builder boilerplate that
    /// every `decide_<class>_action` had to assemble by hand.
    pub fn start_ability_decision<'t>(
        &self,
        decision_trace: &'t mut crate::states::play_match::decision_trace::DecisionTrace,
        target: Option<Entity>,
        my_pos: Vec3,
    ) -> Option<crate::states::play_match::decision_trace::DecisionEventBuilder<'t>> {
        use crate::states::play_match::decision_trace::{ActorView, TargetView};
        let actor_view = ActorView::from_info(self.self_info()?);
        let target_view = target
            .and_then(|t| self.combatants.get(&t))
            .map(|info| TargetView::from_info(info, my_pos));
        Some(decision_trace.start_ability_decision(actor_view, target_view))
    }
}

// ============================================================================
// Shared Targeting Utilities
// ============================================================================

/// Bucket A target-swap chooser (pure). Given the kill target's current HP and
/// an iterator of eligible melee candidates `(entity, distance, current_health)`,
/// returns the SOFTEST (lowest current HP) candidate within `swap_range` whose
/// current HP is at least `hp_margin` (a fraction of the kill target's CURRENT
/// HP) below it — so a swap is only offered when it meaningfully shortens
/// time-to-kill, never for a trivial difference. Deterministic tie-break by
/// entity. Returns `None` when nothing qualifies.
///
/// The caller is responsible for passing only ELIGIBLE candidates (alive,
/// non-pet, visible, not immune) and excluding the current kill target itself.
/// Kept context-free so it unit-tests in isolation and composes with the raw
/// tuple lists in `acquire_targets`.
pub fn select_softer_melee_target<I>(
    kill_target_health: f32,
    candidates: I,
    swap_range: f32,
    hp_margin: f32,
) -> Option<Entity>
where
    I: IntoIterator<Item = (Entity, f32, f32)>,
{
    let threshold = kill_target_health * (1.0 - hp_margin);
    candidates
        .into_iter()
        .filter(|(_, distance, _)| *distance <= swap_range)
        .filter(|(_, _, health)| *health <= threshold)
        .min_by(|(ea, _, ha), (eb, _, hb)| {
            ha.partial_cmp(hb).unwrap().then(ea.cmp(eb))
        })
        .map(|(entity, _, _)| entity)
}

// ============================================================================
// Shared Healer Utilities
// ============================================================================

/// Calculate dispel priority for an aura type.
/// Higher values = more urgent to dispel.
/// Used by Priest (Dispel Magic) and Paladin (Cleanse).
pub fn dispel_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        AuraType::Polymorph => 100,       // Complete incapacitate
        AuraType::Fear => 90,              // Loss of control
        AuraType::Root => 80,              // Can't move
        AuraType::DamageOverTime => 50,    // Taking damage
        AuraType::MovementSpeedSlow => 20, // Minor (typically not worth dispelling)
        _ => 0,
    }
}

/// Calculate purge priority for a BENEFICIAL aura on an enemy.
/// Higher values = more valuable to strip with Purge.
///
/// Defensive buffs (shields/absorbs and incoming-damage reductions) outrank
/// offensive buffs (attack/spell power, crit), which outrank minor utility
/// buffs (mana regen, lockout reduction, resistances). Mirrors
/// [`dispel_priority`] but for the offensive (enemy-buff-strip) direction.
/// Only auras for which [`Aura::can_be_purged`] is true should be passed here.
/// Minimum [`purge_priority`] worth spending a GCD on: only high-value
/// defensives (Absorb / DamageTakenReduction / HoT-class sustain) clear this
/// bar, so Purge never wastes a cast stripping cheap re-buffs like Fortitude.
pub const PURGE_MIN_PRIORITY: i32 = 70;

pub fn purge_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        // Defensives — most valuable to remove (denies mitigation / sustain).
        AuraType::Absorb => 100,              // PW:Shield / damage absorb
        AuraType::DamageTakenReduction => 90, // flat incoming-damage cut
        AuraType::MaxHealthIncrease => 15,    // cheap re-buff (PW:Fortitude) — not worth a GCD to strip
        AuraType::HealingOverTime => 70,      // ongoing sustain (Healing Stream)
        // Offensive throughput buffs.
        AuraType::AttackPowerIncrease => 60,
        AuraType::SpellPowerIncrease => 60,
        AuraType::WindfuryBuff => 55,
        AuraType::CritChanceIncrease => 50,
        // Minor utility buffs.
        AuraType::MaxManaIncrease => 30,
        AuraType::ManaRegenIncrease => 25,
        AuraType::SpellResistanceBuff => 20,
        AuraType::FrostArmorBuff => 20,
        AuraType::LockoutDurationReduction => 15,
        _ => 0,
    }
}

/// Shared dispel logic used by Priest (Dispel Magic) and Paladin (Cleanse).
///
/// Finds the ally with the highest priority dispellable debuff and casts
/// the specified dispel ability on them. The actual aura removed is randomly
/// selected in process_dispels (WoW Classic behavior).
///
/// The `min_priority` parameter controls which debuffs are considered:
/// - 90: Only urgent CC (Polymorph, Fear)
/// - 50: Include roots and DoTs
/// - 20: Include slows (not recommended)
///
/// Predicate failures emit typed reject events on the dispel ability;
/// success emits choose.
#[allow(clippy::too_many_arguments)]
pub fn try_dispel_ally(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    ability_type: AbilityType,
    log_prefix: &'static str,
    log_name: &str,
    caster_class: CharacterClass,
    trace: &mut crate::states::play_match::decision_trace::DecisionEventBuilder<'_>,
) -> bool {
    use crate::states::play_match::decision_trace::RejectionReason;

    let def = abilities.get_unchecked(&ability_type);

    // Check if spell school is locked out
    if is_spell_school_locked(def.spell_school, auras) {
        trace.reject(
            ability_type,
            RejectionReason::SilencedOrLocked { school: def.spell_school },
        );
        return false;
    }
    // Silence gate (UA backlash). The dispel helper bypasses can_cast_config and
    // deducts mana directly, so this check must live here — otherwise a silenced
    // healer would still successfully dispel.
    if is_silenced(combatant, auras) && def.mana_cost > 0.0 {
        trace.reject(
            ability_type,
            RejectionReason::SilencedOrLocked { school: def.spell_school },
        );
        return false;
    }

    if combatant.current_mana < def.mana_cost {
        trace.reject(
            ability_type,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    // Cleanse also removes poison/disease; Dispel Magic / Devour Magic do not.
    let removes_poison = ability_type == AbilityType::PaladinCleanse;

    // Find ally with highest priority dispellable debuff
    let mut best_candidate: Option<(Entity, i32)> = None;

    for (e, info) in ctx.combatants.iter() {
        // Must be alive ally, skip pets (Felhunter handles its own dispels)
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }

        // Check range
        if my_pos.distance(info.position) > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let Some(ally_auras) = ctx.active_auras.get(e) else {
            continue;
        };

        // Find highest priority dispellable debuff on this ally
        let mut highest_priority = -1;
        for aura in ally_auras {
            if !aura.can_be_dispelled() && !(removes_poison && aura.is_cleansable_poison()) {
                continue;
            }

            // Cleansable poisons (e.g. Crippling's 70% slow) are worth a
            // maintenance cleanse — rate them at 50 rather than the bare
            // MovementSpeedSlow's 20, so a healthy Paladin lifts them, but an
            // under-pressure Paladin (urgent-only, min_priority 90) still
            // prioritizes healing over the snare.
            let priority = if aura.is_cleansable_poison() {
                50
            } else {
                dispel_priority(aura.effect_type)
            };

            if priority > highest_priority {
                highest_priority = priority;
            }
        }

        if highest_priority < min_priority {
            continue;
        }

        match best_candidate {
            None => best_candidate = Some((*e, highest_priority)),
            Some((_, best_prio)) if highest_priority > best_prio => {
                best_candidate = Some((*e, highest_priority));
            }
            _ => {}
        }
    }

    let Some((dispel_target, _)) = best_candidate else {
        trace.reject(ability_type, RejectionReason::NoValidTarget);
        return false;
    };

    trace.choose(ability_type, Some(dispel_target), true);

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants.get(&dispel_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, log_name, target_tuple, "casts");

    // Spawn pending dispel
    commands.spawn(DispelPending {
        target: dispel_target,
        dispeller: entity,
        log_prefix,
        caster_class,
        heal_on_success: None,
        aura_type_filter: None,
        removes_poison,
    });

    info!(
        "Team {} {} casts {} on ally",
        combatant.team,
        combatant.class.name(),
        log_name
    );

    true
}

/// Offensive dispel: the Shaman's Purge. Structural mirror of
/// [`try_dispel_ally`], but scans ENEMIES (team != self) for a beneficial,
/// [`Aura::can_be_purged`] aura and strips the single highest-[`purge_priority`]
/// one.
///
/// Target selection: among enemies in Purge range carrying a purgeable buff,
/// pick the one whose best buff has the highest priority. Ties prefer the enemy
/// HEALER (deny its defensives first), then the lowest entity id (BTreeMap
/// iteration order — deterministic for seeded replay).
///
/// Gated by [`pre_cast_ok`] with `check_friendly_cc: false` (offensive — no
/// friendly-CC concern) and `check_target_immune: true` (respect Divine Shield;
/// range/mana/lockout/silence handled by the guard). Predicate failures emit
/// typed reject events; success emits choose and spawns a `DispelPending` whose
/// `aura_type_filter` is pinned to the single chosen (purgeable) buff type, so
/// `process_dispels` strips that beneficial aura from the enemy — a random pick
/// only if the enemy holds several auras of that same type (intentional).
#[allow(clippy::too_many_arguments)]
pub fn try_purge_enemy(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    trace: &mut crate::states::play_match::decision_trace::DecisionEventBuilder<'_>,
) -> bool {
    use self::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
    use crate::states::play_match::decision_trace::RejectionReason;

    let ability = AbilityType::Purge;
    let def = abilities.get_unchecked(&ability);

    let enemy_healer = ctx.enemy_healer();

    // Best enemy to purge: (entity, position, chosen aura, priority, is_healer).
    let mut best: Option<(Entity, Vec3, AuraType, i32, bool)> = None;

    for (e, info) in ctx.combatants.iter() {
        // Must be an alive enemy, skip pets.
        if info.team == combatant.team || !info.is_alive || info.is_pet {
            continue;
        }

        // Range gate (final mana/range is re-checked by pre_cast_ok on the winner).
        if my_pos.distance(info.position) > def.range {
            continue;
        }

        let Some(enemy_auras) = ctx.active_auras.get(e) else {
            continue;
        };

        // Highest-priority purgeable buff on this enemy. First aura at the max
        // priority wins (stable by aura-vec order — deterministic).
        let mut best_aura: Option<(AuraType, i32)> = None;
        for aura in enemy_auras {
            if !aura.can_be_purged() {
                continue;
            }
            let priority = purge_priority(aura.effect_type);
            match best_aura {
                None => best_aura = Some((aura.effect_type, priority)),
                Some((_, bp)) if priority > bp => best_aura = Some((aura.effect_type, priority)),
                _ => {}
            }
        }

        let Some((aura_type, priority)) = best_aura else {
            continue;
        };

        // Value floor: only spend a GCD purging high-value defensives
        // (Absorb / DamageTakenReduction / HoT-class sustain, priority >= 70).
        // Cheap re-buffs (Fortitude, attack/spell power) aren't worth the cast.
        if priority < PURGE_MIN_PRIORITY {
            continue;
        }

        let is_healer = enemy_healer == Some(*e);
        let better = match best {
            None => true,
            Some((_, _, _, best_prio, best_heal)) => {
                priority > best_prio || (priority == best_prio && is_healer && !best_heal)
            }
        };
        if better {
            best = Some((*e, info.position, aura_type, priority, is_healer));
        }
    }

    let Some((target_entity, target_pos, chosen_aura, _, _)) = best else {
        trace.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    // Universal pre-cast guard (lockout / silence / cooldown / mana / range /
    // target immunity). Offensive cast — no friendly-CC guard.
    let opts = PreCastOpts {
        check_friendly_cc: false,
        check_friendly_dots: false,
        check_target_immune: true,
        bypass_silence: false,
    };
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts) {
        trace.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts),
        );
        return false;
    }

    trace.choose(ability, Some(target_entity), true);

    // Execute.
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    if def.cooldown > 0.0 {
        combatant.ability_cooldowns.insert(ability, def.cooldown);
    }

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "casts");

    // Pin the filter to the chosen (highest-priority) buff type so process_dispels
    // targets that valuable buff rather than any purgeable aura. If the enemy
    // holds several auras of that type the strip is a random pick among them
    // (intentional — see process_dispels).
    commands.spawn(DispelPending {
        target: target_entity,
        dispeller: entity,
        log_prefix: "[PURGE]",
        caster_class: combatant.class,
        heal_on_success: None,
        aura_type_filter: Some(vec![chosen_aura]),
        removes_poison: false,
    });

    true
}
