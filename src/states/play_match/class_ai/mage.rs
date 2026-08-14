//! Mage AI Module
//!
//! Handles AI decision-making for the Mage class.
//!
//! ## Priority Order
//! 1. Ice Barrier (self-shield when no shield or HP < 80%)
//! 2. Mage Armor (self-buff based on preference: Frost Armor / Mage Armor / Molten Armor)
//! 3. Arcane Intellect (buff mana-using allies pre-combat)
//! 4. Frost Nova (defensive AoE when enemies in melee)
//! 5. Polymorph (CC non-kill target to create outnumbering situation)
//! 6. Frostbolt (main damage spell with kiting behavior)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use crate::combat::log::CombatLog;
use crate::states::match_config::MageArmor;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRIT_DAMAGE_MULTIPLIER, DEFENSIVE_HP_THRESHOLD, GCD, MELEE_RANGE, SAFE_KITING_DISTANCE,
};
use crate::states::play_match::combat_core::{calculate_cast_time, roll_crit, get_attack_power_bonus_from_slice, get_spell_power_bonus_from_slice, get_crit_chance_bonus_from_slice};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use, spawn_speech_bubble};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use super::CombatContext;
use crate::states::play_match::cc_value::{
    action_cost, displaces_target, expected_incoming, forgone_damage,
    interrupt_value, predict_t_eff, AttackerMix, CastValue, CostInputs, TEffInputs,
};

/// Whether the Mage's Polymorph uses the priced model.
///
/// **Currently ON, and under review.** It only takes effect under
/// `CcPolicy::Priced`; `Identity` is the default, so default behaviour is
/// unchanged either way.
///
/// ## The measurement error that produced the earlier "off"
///
/// Eight attempts were scored neutral-to-negative and the feature was parked.
/// Every one of them used `Mage+Priest vs Warlock+Priest` — which is the
/// Warlock's **designed** counter to the Mage. The Felhunter's Devour Magic
/// exists precisely to shred a Mage's crowd control, so that cell is the worst
/// case by construction, and a feature judged only there was judged unfairly.
///
/// Measured across seven matchups instead (n=300 each, BasicArena), and then
/// fixed twice. The column that matters is the last one:
///
/// | opponent | as-parked | + commitment | + denial discount |
/// |---|---|---|---|
/// | **Paladin+Warrior** | +41 | +70 | **+83** (z=+20.4) |
/// | Warrior+Priest | -3 | -3 | **+3** |
/// | Mage+Priest (mirror) | +1 | +1 | +1 |
/// | Paladin+Shaman | -1 | -1 | -1 |
/// | Hunter+Priest | -8 | -8 | **-6** |
/// | **Rogue+Priest** | -27 | -25 | **-13** |
/// | Warlock+Priest (the counter) | -19 | -19 | **-18** |
///
/// Net across the seven: **-16 -> +49**. Every cell improved or held.
///
/// ## The two defects, both backward-looking
///
/// **1. `forgone_damage` was charged at the VICTIM's trailing incoming rate**,
/// which is ~0 at exactly the moment a burn is about to begin — so the
/// "do not crowd-control what you are killing" penalty vanished precisely when
/// it mattered. It now uses COMMITMENT: the demonstrated output of our own units
/// pointed at that target. An attacker's output rate transfers across a target
/// switch; a victim's incoming rate does not.
///
/// The effect is visible as *restraint*. Against `Paladin+Warrior` the Mage went
/// from 26 Polymorphs to **14**, wasted Warrior sheeps from 11 to **4**, total
/// denial from 146.1s to **92.5s** — and won 29 points more. Less crowd control,
/// less denial, better result.
///
/// **2. Damage denial and healing denial were priced at par.** They are not the
/// same: healing denied is ERASED (our damage lands unhealed and converts into a
/// kill) while damage denied is merely DEFERRED until the crowd control ends.
/// See `DAMAGE_DENIAL_DISCOUNT`. This is what made the Mage sheep an enemy Rogue
/// 19 times in 20 matches for 132.7s of denial and lose by 27 points, in a
/// matchup the plain heuristic wins 90% of by never sheeping at all.
///
/// It also predicts its own blast radius, which is why the Warlock's canonical
/// cell is untouched at +9pt (z=+2.43): a healer's denial is almost all
/// `healing_capped`, which is not discounted.
///
/// ## What is still wrong
///
/// Three cells remain negative — `Rogue+Priest` (-13), `Warlock+Priest` (-18)
/// and `Hunter+Priest` (-6). The Warlock cell is the designed counter and is
/// expected to be hostile; the other two are not yet diagnosed.
///
/// See `match_configs/mage-matchups/` for both cases as watchable replays.
const MAGE_PRICED_POLYMORPH: bool = true;

/// RETIRED as a veto — kept only as documentation of a measured dead end.
///
/// Expected effective seconds below which a Polymorph was considered not worth
/// its cast.
///
/// Lower than the Warlock's Fear floor (1.5s) on purpose. Polymorph breaks on
/// ANY damage, so its realistic durations are shorter across the board, and a
/// floor calibrated for a 100-damage budget would reject nearly every sheep.
///
/// **Calibrated, and the floor is not the lever.** Raising it monotonically
/// worsens the result (n=300, `Mage+Priest` vs `Warlock+Priest`, BasicArena):
/// 1.0 -> -7pt, 2.0 -> -8pt, 3.0 -> -8pt, 5.0 -> -11pt. So the priced Mage is
/// not losing because it sheeps too eagerly — a more conservative floor only
/// makes it worse. Kept at 1.0 as the least-bad measured value; the cause of the
/// regression is NOT diagnosed. See the design doc.
#[allow(dead_code)]
const MIN_POLYMORPH_T_EFF: f32 = 1.0;

/// Pick the Polymorph target by expected value, or `None`.
///
/// The identity path this replaces asks two questions: "what is `cc_target`" and
/// "is it the kill target". The second is a hardcoded stance assumption — right
/// while we are damaging that unit, wrong otherwise — and the first defers the
/// whole decision to a heuristic scored elsewhere.
///
/// Priced, the question is what a sheep would actually be WORTH: how long it
/// would hold given that any damage breaks it, times what removing that unit
/// denies us, against what the cast costs. Polymorph's `break_on_damage` is
/// 0.0, so `T_eff` is dominated by whether anyone is hitting the target — which
/// is exactly the "do not sheep the unit we are killing" instinct, derived
/// rather than asserted, and correctly relaxed when nobody is actually hitting
/// them.
fn pick_polymorph_target(
    combatant: &Combatant,
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
) -> Option<Entity> {
    let def = abilities.get_unchecked(&AbilityType::Polymorph);
    let aura = def.applies_aura.as_ref()?;
    let mut best: Option<(Entity, f32)> = None;

    // Pets deliberately EXCLUDED here, and this is a measured decision rather
    // than the old blanket filter.
    //
    // Including them made the Felhunter matchup much worse: -10pt -> **-17pt
    // (z=-4.17)** at n=300, with the Mage spending 6 of 19 sheeps on the pet.
    // The valuation arithmetic shows why — nothing on their side removes a CC
    // from a pet, so `T_eff` on the Felhunter is a near-full 10s (value ~100-150)
    // while a sheep on the healer is devoured in 0.03s (value ~25). The model
    // therefore prefers the pet by a wide margin.
    //
    // That preference is wrong, and the reason is a gap in `D`: it prices
    // "damage this unit would deal us" and "healing it would deliver"
    // symmetrically per second, but denying a HEALER compounds into a kill while
    // denying a pet only slows their offense. Until `D` distinguishes those,
    // pets must stay out of this particular decision.
    //
    // `alive_enemies_including_pets` remains, and the Warlock's Death Coil peel
    // still uses it — peeling a pet off our healer is exactly what that ability
    // is for, and it is a defensive choice rather than a ranking against a
    // healer.
    for info in ctx.alive_enemies() {
        let target = info.entity;
        if ctx.entity_is_immune(target) || ctx.is_dr_immune(target, DRCategory::Incapacitates) {
            continue;
        }
        let already_ccd = ctx
            .active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    matches!(
                        a.effect_type,
                        AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                    )
                })
            })
            .unwrap_or(false);
        if already_ccd {
            continue;
        }

        // Absorb sits in front of the break budget: a shielded target holds a
        // break-on-any-damage CC until the shield is gone.
        let absorb: f32 = ctx
            .active_auras
            .get(&target)
            .map(|auras| {
                auras
                    .iter()
                    .filter(|a| a.effect_type == AuraType::Absorb)
                    .map(|a| a.magnitude)
                    .sum()
            })
            .unwrap_or(0.0);

        let mut mix = AttackerMix::default();
        for other in ctx.combatants.values() {
            if other.team == info.team || !other.is_alive || other.target != Some(target) {
                continue;
            }
            if other.class.is_melee() {
                mix.melee += 1;
            } else {
                mix.ranged += 1;
            }
        }
        let trailing = ctx.recent_damage.get(&target).copied().unwrap_or(0.0);

        // Who on their side can actually take this sheep off, counted by
        // CAPABILITY rather than role — the Shaman is a healer with no ally
        // dispel at all. See `free_ally_dispellers`.
        // `None` when the aura is not dispellable at all — Polymorph is, but
        // reading it off the aura keeps this honest if the ability changes.
        let free_dispellers = aura
            .aura_type
            .is_magic_dispellable()
            .then(|| ctx.free_ally_dispellers(target));

        let t_eff = predict_t_eff(&TEffInputs {
            applied_duration: aura.duration * ctx.dr_multiplier(target, DRCategory::Incapacitates),
            break_threshold: aura.break_on_damage,
            accumulated_damage: 0.0,
            incoming: expected_incoming(trailing, mix),
            displaces_target: displaces_target(AuraType::Polymorph),
            absorb_remaining: absorb,
            free_dispellers,
        })
        .t_eff;

        // `I` — interrupt value. THE fix for the priced Mage's regression.
        //
        // Diagnosed by reading a match: a Polymorph landed on the enemy healer
        // and was **devoured 0.03 seconds later** by the Warlock's Felhunter —
        // yet in that instant it still interrupted a Flash Heal. Its whole worth
        // was as a pseudo-interrupt. Priced on `D x T_eff` alone that sheep
        // scores ~0 (T_eff ~ 0.03s against a devourer) and the Mage declines it,
        // which is why raising `MIN_POLYMORPH_T_EFF` made the regression WORSE
        // rather than better: the problem was never over-eagerness.
        //
        // A CC that cancels a cast is worth the cast, however briefly it holds.
        let interrupt = ctx
            .combatants
            .get(&target)
            .and_then(|t| t.casting_ability.map(|a| (a, t.casting_remaining)))
            .map(|(cast_ability, remaining)| {
                let cd = abilities.get_unchecked(&cast_ability);
                let mut cv = CastValue::default();
                if cd.is_heal() {
                    // Cap at the deficit — denying an overheal buys nothing.
                    // Base only: an enemy's spell power is not on the AI's
                    // view of them, so the scaled component is unavailable and
                    // this under-states big heals rather than inventing a number.
                    let raw = (cd.healing_base_min + cd.healing_base_max) / 2.0;
                    let deficit = ctx
                        .combatants
                        .values()
                        .filter(|c| c.team == info.team && c.is_alive)
                        .map(|c| (c.max_health - c.current_health).max(0.0))
                        .fold(0.0f32, f32::max);
                    cv.healing_denied = raw.min(deficit.max(0.0));
                }
                if cd.is_damage() {
                    cv.damage_denied = (cd.damage_base_min + cd.damage_base_max) / 2.0;
                }
                interrupt_value(cv, remaining, def.cast_time)
            })
            .unwrap_or(0.0);

        // A landed CC costs its target AT LEAST one global, whatever its
        // duration. Diagnosed from a match where a Polymorph was devoured 0.03s
        // after landing and still cancelled a Flash Heal — and where the `I`
        // term could not see it, because at decision time the heal had not
        // started yet. Interrupt value cannot be read off a cast in flight when
        // the cast begins after the decision; what CAN be relied on is that
        // landing the CC at all denies a global and forces a re-cast.
        //
        // So the duration used for VALUE is floored at one GCD. `t_eff` itself
        // is untouched — this is a floor on what the denial is worth, not a
        // claim about how long the CC holds.
        let value_duration = t_eff.max(GCD);
        // Subtract what WE give up. Polymorph breaks on any damage, so our own
        // team stops attacking anything carrying it — sheeping the unit we are
        // killing takes it out of the kill. This derives the guard the identity
        // path hardcoded, and correctly permits sheeping a target nobody is
        // actually hitting (delivery 0 -> no penalty).
        //
        // The rate this is charged at is the fix for the `Rogue+Priest`
        // regression. It used to be the VICTIM's trailing incoming damage,
        // which is ~0 at exactly the moment a burn is about to begin — so the
        // penalty vanished precisely when it mattered, and the Mage would sheep
        // the unit it was about to kill. Same backward-looking defect step 0
        // found in the break term.
        //
        // Use commitment instead: the demonstrated output of our own units that
        // are POINTED at this target. An attacker's output rate transfers
        // across a target switch; a victim's incoming rate does not. Kept as a
        // `max` with the trailing figure so damage already in flight from
        // someone who has since re-targeted is not lost.
        let committed_rate: f32 = ctx
            .combatants
            .values()
            .filter(|c| c.team != info.team && c.is_alive && c.target == Some(target))
            .map(|c| ctx.recent_damage_dealt.get(&c.entity).copied().unwrap_or(0.0))
            .sum();
        let delivery = ctx
            .recent_damage
            .get(&target)
            .copied()
            .unwrap_or(0.0)
            .max(committed_rate);
        let forgone = forgone_damage(delivery, value_duration, aura.break_on_damage == 0.0);
        let value = interrupt + ctx.denial_rate_of(target, abilities) * value_duration - forgone;
        // What the sheep displaces: a Frostbolt. Priced through the same
        // expected-damage helper the Warlock uses for its DoTs so both classes
        // compare CC against their rotation on one scale.
        let fb = abilities.get_unchecked(&AbilityType::Frostbolt);
        let displaced = (fb.damage_base_min + fb.damage_base_max) / 2.0
            + fb.damage_coefficient * combatant.spell_power;
        let cost = action_cost(&CostInputs {
            mana_cost: def.mana_cost,
            current_mana: combatant.current_mana,
            displaced_value: displaced,
        });
        if value < cost {
            continue;
        }
        // Ties break on entity id so the choice stays deterministic at a seed.
        let better = match best {
            None => true,
            Some((be, bv)) => value > bv || (value == bv && target < be),
        };
        if better {
            best = Some((target, value));
        }
    }
    best.map(|(e, _)| e)
}


/// Mage AI: Decides and executes abilities for a Mage combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_mage_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event (emission gate).
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Ice Barrier (self-shield)
    if try_ice_barrier(commands, combat_log, abilities, entity, combatant, ctx, &mut builder) {
        builder.finish();
        return true;
    }

    // Priority 2: Mage Armor (self-buff based on preference)
    if try_mage_armor(commands, combat_log, abilities, entity, combatant, ctx, &mut builder) {
        builder.finish();
        return true;
    }

    // Priority 3: Arcane Intellect (buff mana-using allies)
    if try_arcane_intellect(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Frost Nova (defensive AoE)
    if try_frost_nova(
        commands,
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        frost_nova_damage,
        same_frame_cc_queue,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 5: Polymorph (CC non-kill target)
    if try_polymorph(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Frostbolt (main damage spell)
    if try_frostbolt(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
    false
}

/// Try to cast Ice Barrier on self.
/// Returns true if the ability was used.
fn try_ice_barrier(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ice_barrier = AbilityType::IceBarrier;
    let barrier_def = abilities.get_unchecked(&ice_barrier);

    // Check if already shielded
    let has_absorb_shield = ctx.active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::Absorb))
        .unwrap_or(false);

    if has_absorb_shield {
        builder.reject(ice_barrier, RejectionReason::AlreadyApplied);
        return false;
    }

    let is_full_hp = combatant.current_health >= combatant.max_health;
    let is_below_threshold =
        combatant.current_health < combatant.max_health * DEFENSIVE_HP_THRESHOLD;
    if !(is_full_hp || is_below_threshold) {
        builder.reject(
            ice_barrier,
            RejectionReason::PreconditionUnmet {
                note: "HP above defensive threshold and not full".into(),
            },
        );
        return false;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&ice_barrier) {
        builder.reject(ice_barrier, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }

    if combatant.current_mana < barrier_def.mana_cost {
        builder.reject(
            ice_barrier,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: barrier_def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ice_barrier, Some(entity), true);

    spawn_speech_bubble(commands, entity, "Ice Barrier");
    combatant.current_mana -= barrier_def.mana_cost;
    combatant.ability_cooldowns.insert(ice_barrier, barrier_def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, "Ice Barrier", None, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(entity, entity, barrier_def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} casts Ice Barrier",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast the chosen Mage Armor on self (Frost Armor, Mage Armor, or Molten Armor).
/// Returns true if the ability was used.
fn try_mage_armor(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let (ability, aura_check) = match combatant.mage_armor {
        MageArmor::FrostArmor => (AbilityType::FrostArmor, AuraType::FrostArmorBuff),
        MageArmor::MageArmor => (AbilityType::MageArmorSpell, AuraType::ManaRegenIncrease),
        MageArmor::MoltenArmor => (AbilityType::MoltenArmor, AuraType::CritChanceIncrease),
    };

    let already_buffed = ctx.active_auras
        .get(&entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == aura_check))
        .unwrap_or(false);

    if already_buffed {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost {
        builder.reject(
            ability,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    builder.choose(ability, Some(entity), true);

    spawn_speech_bubble(commands, entity, &def.name);
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, &def.name, None, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(entity, entity, def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} casts {}",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Arcane Intellect on an unbuffed mana-using ally.
/// Returns true if the ability was used.
fn try_arcane_intellect(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::ArcaneIntellect;
    let def = abilities.get_unchecked(&ability);

    let mut unbuffed_mana_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 {
            continue;
        }
        if !info.class.uses_mana() {
            continue;
        }
        let has_arcane_intellect = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxManaIncrease))
            .unwrap_or(false);
        if has_arcane_intellect {
            continue;
        }
        unbuffed_mana_ally = Some((*ally_entity, info.position));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_mana_ally else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((buff_target, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((buff_target, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(buff_target), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&buff_target).map(|info| info.log_id());
    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, "Arcane Intellect", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(buff_target, entity, def) {
        commands.spawn(aura_pending);
    }

    info!(
        "Team {} {} casts Arcane Intellect on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Frost Nova when enemies are in melee range.
/// Returns true if the ability was used.
fn try_frost_nova(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    frost_nova_damage: &mut Vec<super::QueuedAoeDamage>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let frost_nova = AbilityType::FrostNova;
    let nova_def = abilities.get_unchecked(&frost_nova);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(frost_nova, nova_def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            frost_nova,
            classify_pre_cast_failure(frost_nova, nova_def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    let enemies_in_melee_range = ctx.combatants.iter().any(|(_, info)| {
        info.team != combatant.team && info.is_alive && !info.is_pet
            && my_pos.distance(info.position) <= MELEE_RANGE
    });

    if !enemies_in_melee_range {
        builder.reject(
            frost_nova,
            RejectionReason::PreconditionUnmet {
                note: "no enemies in melee range".into(),
            },
        );
        return false;
    }

    builder.choose(frost_nova, None, true);

    spawn_speech_bubble(commands, entity, "Frost Nova");
    combatant.current_mana -= nova_def.mana_cost;
    combatant.ability_cooldowns.insert(frost_nova, nova_def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, "Frost Nova", None, "casts");

    // Carry each target's pet-aware combat-log id so the root-CC log below
    // attributes correctly when Frost Nova catches an enemy pet.
    let mut frost_nova_targets: Vec<(Entity, Vec3, crate::combat::log::CombatantId)> = Vec::new();
    for (enemy_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team && info.is_alive {
            let distance = my_pos.distance(info.position);
            if distance <= nova_def.range {
                frost_nova_targets.push((*enemy_entity, info.position, info.log_id()));
            }
        }
    }

    let self_auras = ctx.active_auras.get(&entity).map(|v| v.as_slice()).unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    // Frost Nova scales with SpellPower, so include any spell-power auras (e.g. an
    // ally Shaman's Flametongue totem) — matching the generic hardcast path.
    let sp_bonus = get_spell_power_bonus_from_slice(self_auras);
    let crit_bonus = get_crit_chance_bonus_from_slice(self_auras);
    for (target_entity, target_pos, target_id) in &frost_nova_targets {
        let mut damage = combatant.calculate_ability_damage_config(nova_def, game_rng, ap_bonus, sp_bonus);
        let is_crit = roll_crit(combatant.crit_chance + crit_bonus, game_rng);
        if is_crit { damage *= CRIT_DAMAGE_MULTIPLIER; }
        frost_nova_damage.push(super::QueuedAoeDamage {
            caster: entity,
            target: *target_entity,
            damage,
            caster_team: combatant.team,
            caster_slot: combatant.slot,
            caster_class: combatant.class,
            target_pos: *target_pos,
            is_crit,
        });

        if let Some(aura) = nova_def.applies_aura.as_ref() {
            if !ctx.entity_is_immune(*target_entity) {
                if let Some(aura_pending) = AuraPending::from_ability(*target_entity, entity, nova_def) {
                    same_frame_cc_queue.push((*target_entity, aura_pending.aura.clone()));
                    commands.spawn(aura_pending);
                }

                let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
                let message = format!(
                    "{}'s {} roots {} ({:.1}s)",
                    caster_id, nova_def.name, target_id, aura.duration
                );
                combat_log.log_crowd_control(
                    caster_id,
                    target_id.clone(),
                    "Root".to_string(),
                    aura.duration,
                    message,
                );
            }
        }
    }

    // Movement after Frost Nova is owned by the ENGAGE/KITE posture machine
    // (dps_postures.rs): a melee-range threat now carrying the Mage's root
    // triggers KITE on the next posture evaluation.

    info!(
        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
        combatant.team,
        combatant.class.name(),
        frost_nova_targets.len()
    );

    true
}

/// Try to cast Polymorph on the CC target (non-kill target).
fn try_polymorph(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Polymorph;
    let def = abilities.get_unchecked(&ability);

    // PRICED: choose by expected value; the kill-target guard below is derived
    // from `T_eff` rather than asserted, so sheeping a unit nobody is hitting is
    // allowed and sheeping one under fire is not.
    let cc_target = if ctx.cc_policy.is_priced() && MAGE_PRICED_POLYMORPH {
        match pick_polymorph_target(combatant, ctx, abilities) {
            Some(t) => t,
            None => {
                builder.reject(ability, RejectionReason::NoValidTarget);
                return false;
            }
        }
    } else {
        let Some(cc_target) = combatant.cc_target else {
            builder.reject(ability, RejectionReason::NoValidTarget);
            return false;
        };

        // Don't polymorph the kill target — any damage will break it immediately.
        if combatant.target == Some(cc_target) {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "cc_target equals kill target — would break on damage".into(),
                },
            );
            return false;
        }
        cc_target
    };

    let Some(target_info) = ctx.combatants.get(&cc_target) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_pos = target_info.position;

    if ctx.is_dr_immune(cc_target, DRCategory::Incapacitates) {
        builder.reject(
            ability,
            RejectionReason::DRImmune {
                category: DRCategory::Incapacitates,
            },
        );
        return false;
    }

    // Check if target is already CC'd
    let already_ccd_type = ctx.active_auras
        .get(&cc_target)
        .and_then(|auras| {
            auras.iter().find_map(|a| {
                if matches!(
                    a.effect_type,
                    AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                ) {
                    Some(a.effect_type)
                } else {
                    None
                }
            })
        });

    if let Some(cc_type) = already_ccd_type {
        builder.reject(ability, RejectionReason::TargetAlreadyCCd { cc_type });
        return false;
    }

    // GCD check (defensive — outer function already checked).
    if combatant.global_cooldown > 0.0 {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "global cooldown active".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts {
        check_target_immune: true,
        check_friendly_dots: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((cc_target, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((cc_target, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(cc_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, cc_target, cast_time));

    let target_tuple = ctx.combatants
        .get(&cc_target)
        .map(|info| info.log_id());
    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on cc_target",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Frostbolt on the current target.
fn try_frostbolt(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Frostbolt;
    let def = abilities.get_unchecked(&ability);

    let Some(target_entity) = combatant.target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_pos = target_info.position;

    let distance_to_target = my_pos.distance(target_pos);

    // While kiting, only cast if at safe distance. Kiting is now posture-state
    // (dps_postures.rs) rather than the legacy `kiting_timer`; the equivalent
    // world-state condition is "a Mage-owned root/slow is on an enemy within
    // safe-kiting distance" — proximity-gated so Frostbolt's own never-breaking
    // slow on a kited-away enemy doesn't permanently suppress hard-casts.
    let kiting =
        super::dps_postures::mage_impaired_enemy(ctx, entity, my_pos, Some(SAFE_KITING_DISTANCE));
    if kiting && distance_to_target < SAFE_KITING_DISTANCE {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "kiting and below safe distance".into(),
            },
        );
        return false;
    }

    if combatant.global_cooldown > 0.0 {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "global cooldown active".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts {
        check_target_immune: true,
        check_friendly_cc: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| info.log_id());
    log_ability_use(combat_log, combatant.team, combatant.slot, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
