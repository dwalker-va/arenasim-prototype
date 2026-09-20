//! Auto-attack system: melee swings, wand shots, auto shots, Heroic Strike, rage generation.

use super::super::abilities::{AbilityType, SpellSchool};
use super::super::ability_config::AbilityDefinitions;
use super::super::components::*;
use super::super::constants::{
    CRIT_DAMAGE_MULTIPLIER, DUAL_WIELD_MISS_CHANCE, OFFHAND_DAMAGE_MULTIPLIER,
};
use super::super::map_config::ActiveMapGeometry;
use super::super::map_geometry::has_line_of_sight;
use super::super::match_config;
use super::super::utils::{combat_log_id, get_next_fct_offset};
use super::super::{AUTO_SHOT_RANGE, FCT_HEIGHT, HUNTER_DEAD_ZONE, MELEE_RANGE, WAND_RANGE};
use super::damage::{
    apply_damage_with_absorb, get_divine_shield_damage_penalty, get_physical_damage_reduction,
    roll_crit,
};
use crate::combat::log::{CombatLog, CombatLogEventType};
use bevy::prelude::*;
use bevy_egui::egui;

/// Auto-attack system: Process attacks based on attack speed timers.
///
/// Each combatant has an attack timer that counts up. When it reaches
/// the attack interval (1.0 / attack_speed), they check if they're in
/// range and attack their target.
///
/// **Range Check**: Only melee attacks for now, must be within MELEE_RANGE.
/// **WoW Mechanic**: Cannot auto-attack while casting (checked via `CastingState`).
///
/// Damage is applied immediately and stats are updated for both attacker and target.
/// All attacks are logged to the combat log for display.
pub fn combat_auto_attack(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    abilities: Res<AbilityDefinitions>,
    mut combatants: Query<(
        Entity,
        &Transform,
        &mut Combatant,
        Option<&CastingState>,
        Option<&ChannelingState>,
        Option<&mut ActiveAuras>,
    )>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
    auto_attack_pet_query: Query<&Pet>,
    map_geometry: Res<ActiveMapGeometry>,
) {
    // Don't deal damage during victory celebration
    if celebration.is_some() {
        return;
    }
    let dt = time.delta_secs();

    // Update match time in combat log (starts from beginning, including prep phase)
    combat_log.match_time += dt;

    // Don't allow auto-attacks until gates open
    if !countdown.gates_opened {
        return;
    }

    // Build a snapshot of positions for range checks
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _, _)| (entity, transform.translation))
        .collect();

    // Build a snapshot of combatant info for logging and alive checks
    // Tuple: (team, display_name, is_alive, slot_label, kind)
    // `slot_label` is the OWNER-relative 0-based slot used to build the unique
    // combat-log id: a combatant's own slot, or a pet's owner slot
    // (pet.slot - PET_SLOT_BASE) so a pet lines up with its owner's number.
    //
    // `kind` is the ONLY thing in here that answers "how does this combatant
    // auto-attack". Neither the class nor `CharacterClass::is_melee()` is
    // carried, so no site in this system can reach back for the class ladder
    // that `kind` replaced — range, dead zone, line of sight, both proc gates,
    // the swing visual and the log name all read this one value and therefore
    // cannot disagree with one another.
    let combatant_info: std::collections::HashMap<Entity, (u8, String, bool, u8, AutoAttackKind)> =
        combatants
            .iter()
            .map(|(entity, _, combatant, _, _, _)| {
                let (display_name, slot_label, kind) =
                    if let Ok(pet) = auto_attack_pet_query.get(entity) {
                        // Pets carry no equipment, so there is no socket to derive
                        // from: a pet's auto-attack is its own pet type's. The
                        // ranged arm reads the OWNER's class, as it always has.
                        let melee = pet.pet_type.is_melee();
                        (
                            pet.pet_type.name().to_string(),
                            combatant.owner_relative_slot(),
                            if melee {
                                AutoAttackKind::Melee
                            } else if combatant.class == match_config::CharacterClass::Hunter {
                                AutoAttackKind::Shot
                            } else {
                                AutoAttackKind::Wand
                            },
                        )
                    } else {
                        (
                            combatant.class.name().to_string(),
                            combatant.slot,
                            combatant.auto_attack_kind,
                        )
                    };
                (
                    entity,
                    (
                        combatant.team,
                        display_name,
                        combatant.is_alive(),
                        slot_label,
                        kind,
                    ),
                )
            })
            .collect();

    // Weapon-poison proc table: Rogues coated with Crippling Poison and the
    // per-swing application chance from the ability config. A successful roll on
    // a landed swing applies/refreshes the Crippling slow on the target.
    let crippling_chance: std::collections::HashMap<Entity, f32> = combatants
        .iter()
        .filter_map(|(entity, _, combatant, _, _, _)| {
            if combatant.class == match_config::CharacterClass::Rogue
                && combatant.rogue_poison == match_config::RoguePoison::Crippling
            {
                abilities
                    .get(&AbilityType::CripplingPoison)
                    .and_then(|def| def.application_chance)
                    .map(|chance| (entity, chance))
            } else {
                None
            }
        })
        .collect();

    // Auto-attacks must not shatter friendly crowd control. The AI ability path
    // already guards casts via `pre_cast_ok(check_friendly_cc)`, but
    // auto-attacks bypass that guard. Two tiers, each mapping a target entity to
    // the team of the caster who placed the CC (only one caster team recorded
    // per target — a target carrying the same CC class from two teams at once
    // does not occur):
    //  - `incap_cc_team`: break-on-ANY-damage incapacitates (Freezing Trap /
    //    Polymorph, threshold 0.0). NO attacker may break these — most visibly a
    //    Hunter's melee pet sitting on a trapped target.
    //  - `root_cc_team`: damage-breakable Roots (Spider Web, Frost Nova). A PET
    //    must not break these: it webs a target to peel it off the owner, and
    //    meleeing through the Web both defeats the peel and shatters it. A ranged
    //    player legitimately nukes a rooted target (root + nuke), so this tier is
    //    pet-only. Stuns/Fears are excluded — those are offensive setups the pet
    //    should keep attacking through.
    let caster_team = |a: &Aura| {
        a.caster
            .and_then(|c| combatant_info.get(&c))
            .map(|(team, ..)| *team)
    };
    let incap_cc_team: std::collections::HashMap<Entity, u8> = combatants
        .iter()
        .filter_map(|(entity, _, _, _, _, auras)| {
            let auras = auras?;
            auras
                .auras
                .iter()
                .find_map(|a| {
                    (a.break_on_damage_threshold == 0.0)
                        .then(|| caster_team(a))
                        .flatten()
                })
                .map(|team| (entity, team))
        })
        .collect();
    let root_cc_team: std::collections::HashMap<Entity, u8> = combatants
        .iter()
        .filter_map(|(entity, _, _, _, _, auras)| {
            let auras = auras?;
            auras
                .auras
                .iter()
                .find_map(|a| {
                    // `> 0.0` (not `>= 0.0`): a Root at threshold 0.0 is an
                    // any-damage break and belongs to the incap tier above, which
                    // blocks ALL attackers — keep the two tiers a clean partition.
                    (a.effect_type == AuraType::Root && a.break_on_damage_threshold > 0.0)
                        .then(|| caster_team(a))
                        .flatten()
                })
                .map(|team| (entity, team))
        })
        .collect();

    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();

    // Track damage per target for batching floating combat text.
    // BTreeMap (not HashMap) so iteration order is deterministic by Entity —
    // FCT entity spawn order would otherwise vary across runs due to Rust's
    // randomized HashMap hasher, breaking byte-identical determinism for
    // self-mirror matchups (same class on both teams).
    let mut damage_per_target: std::collections::BTreeMap<Entity, f32> =
        std::collections::BTreeMap::new();
    // Track damage per target for aura breaking. Same BTreeMap rationale as
    // above — the iteration at the bottom of this function spawns commands
    // whose order can ripple into downstream entity allocation.
    let mut damage_per_aura_break: std::collections::BTreeMap<Entity, f32> =
        std::collections::BTreeMap::new();

    for (attacker_entity, transform, mut combatant, casting_state, channeling_state, auras) in
        combatants.iter_mut()
    {
        if !combatant.is_alive() {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while stunned, feared, or polymorphed
        let is_incapacitated = super::super::utils::is_incapacitated(auras.as_deref());
        if is_incapacitated {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while casting
        if casting_state.is_some() {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while channeling
        if channeling_state.is_some() {
            continue;
        }

        // WoW Mechanic: Cannot auto-attack while stealthed (Rogues must use abilities)
        if combatant.stealthed {
            continue;
        }

        // Update attack timer
        combatant.attack_timer += dt;

        // The off hand keeps its own clock, off its own weapon's speed, so the
        // two hands drift apart over a match instead of landing in lockstep.
        // Everything about the off hand is gated on `is_dual_wielding`, which
        // is false for every single-wielding combatant — so a match with no
        // second weapon in it ticks no extra state and draws no extra RNG.
        let dual_wielding = combatant.is_dual_wielding();
        if dual_wielding {
            combatant.offhand_timer += dt;
        }

        // Check if ready to attack and has a target
        let attack_interval = effective_attack_interval(&combatant, auras.as_deref());
        let main_hand_ready = combatant.attack_timer >= attack_interval;
        let off_hand_ready = dual_wielding
            && combatant.offhand_timer >= effective_offhand_interval(&combatant, auras.as_deref());
        if main_hand_ready || off_hand_ready {
            if let Some(target_entity) = combatant.target {
                // Skip if target is dead (will be retargeted next frame)
                if !combatant_info
                    .get(&target_entity)
                    .is_some_and(|(_, _, alive, _, _)| *alive)
                {
                    continue;
                }
                // Don't shatter our own team's CC. The timer keeps building so
                // the attack resumes the instant the CC ends.
                //  - incapacitates (Freezing Trap / Polymorph): no attacker.
                //  - Roots (Spider Web): pets only — a ranged owner still nukes
                //    a rooted target.
                let attacker_is_pet = auto_attack_pet_query.get(attacker_entity).is_ok();
                if incap_cc_team.get(&target_entity) == Some(&combatant.team)
                    || (attacker_is_pet
                        && root_cc_team.get(&target_entity) == Some(&combatant.team))
                {
                    continue;
                }
                // Check if target is in range before attacking
                if let Some(&target_pos) = positions.get(&target_entity) {
                    let my_pos = transform.translation;

                    // The derived kind from the snapshot (pets carry no
                    // equipment, so they get their own pet type's kind).
                    let &(_, _, _, _, attacker_kind) = &combatant_info[&attacker_entity];
                    // Range comes from the EQUIPPED weapon, not the class. A
                    // live socket with no weapon in it means no auto-attack.
                    let attack_range = match attacker_kind {
                        AutoAttackKind::Melee => MELEE_RANGE,
                        AutoAttackKind::Shot => AUTO_SHOT_RANGE,
                        AutoAttackKind::Wand => WAND_RANGE,
                        AutoAttackKind::None => continue,
                    };
                    let distance = my_pos.distance(target_pos);
                    // Hunter dead zone: the ranged Auto Shot can't fire within 8
                    // yards. Keying it on `Shot` rather than on the class is
                    // what excludes a melee pet (Spider/Boar), which inherits
                    // the Hunter class but attacks in melee; that previously
                    // needed a separate `!attacker_is_melee` guard beside the
                    // class check.
                    if attacker_kind == AutoAttackKind::Shot && distance < HUNTER_DEAD_ZONE {
                        continue;
                    }
                    // Line-of-sight gate: ranged autos (Hunter Auto Shot,
                    // caster wand shots) require an unobstructed line to the
                    // target. Occlusion skips the swing exactly like an
                    // out-of-range tick — the timer keeps building so the shot
                    // fires the instant the target clears cover. MELEE autos are
                    // deliberately excluded: two melee units flanking a
                    // thin obstacle edge can still trade hits. Empty obstacle
                    // lists → always clear, so this is a byte-identical no-op on
                    // BasicArena / obstacle-free maps.
                    if attacker_kind != AutoAttackKind::Melee
                        && !has_line_of_sight(&map_geometry.volumes, my_pos, target_pos)
                    {
                        continue;
                    }
                    if distance <= attack_range {
                        // Roll crit before damage reduction (include dynamic crit bonus from auras)
                        let crit_bonus = super::get_crit_chance_bonus(auras.as_deref());
                        // Apply physical damage reduction from curses (Curse of Weakness: -20%)
                        let damage_reduction = get_physical_damage_reduction(auras.as_deref());
                        // Apply Divine Shield outgoing damage penalty (50%)
                        let ds_penalty = get_divine_shield_damage_penalty(auras.as_deref());

                        if main_hand_ready {
                            // Dual wield's price, charged to BOTH hands as it is in
                            // Classic. The sim has no general miss mechanic, so this
                            // roll happens ONLY while a second weapon is equipped —
                            // a single-wielding attacker draws nothing here and its
                            // match is unchanged.
                            let missed =
                                dual_wielding && game_rng.random_f32() < DUAL_WIELD_MISS_CHANCE;
                            if !missed {
                                // Calculate total damage (base + bonus from Heroic Strike, etc.)
                                let base_damage =
                                    combatant.attack_damage + combatant.next_attack_bonus_damage;
                                // Windfury Totem: a MELEE attacker carrying its own WindfuryBuff
                                // aura has a chance (= aura magnitude) for one bonus swing.
                                // Gated to melee SWINGS (R14/AE3) — see
                                // `windfury_bonus_chance`.
                                // Captured here because `auras` is borrowed again below.
                                //
                                // MAIN HAND ONLY — this roll sits in the
                                // main-hand branch and deliberately has no twin
                                // in the off-hand branch below. In Classic the
                                // totem is a temporary WEAPON ENCHANT, and a
                                // Rogue spends its off-hand enchant slot on a
                                // poison — so the buff lands on the main hand.
                                // That was also the efficient play: Windfury
                                // was proc-per-minute, so a fixed budget of
                                // procs was worth more on the weapon with the
                                // higher top end. Main-hand-only is the
                                // realistic outcome of the enchant-slot
                                // interaction, so we model the outcome and skip
                                // the slot. See docs/design/wow-mechanics.md.
                                let windfury_chance =
                                    windfury_bonus_chance(attacker_kind, auras.as_deref());
                                let is_crit =
                                    roll_crit(combatant.crit_chance + crit_bonus, &mut game_rng);
                                let crit_damage = if is_crit {
                                    base_damage * CRIT_DAMAGE_MULTIPLIER
                                } else {
                                    base_damage
                                };
                                let total_damage =
                                    (crit_damage * (1.0 - damage_reduction) * ds_penalty).max(0.0);
                                let has_bonus = combatant.next_attack_bonus_damage > 0.0;

                                attacks.push((
                                    attacker_entity,
                                    target_entity,
                                    total_damage,
                                    has_bonus,
                                    is_crit,
                                ));

                                // Windfury Totem proc: a successful roll pushes a duplicate
                                // (bonus) swing that resolves like a normal weapon hit. Both
                                // the proc roll and the bonus swing's crit roll draw from the
                                // seeded game_rng, so match determinism is preserved. This branch
                                // is only reached when a WindfuryBuff aura is present, so existing
                                // (totem-free) matches draw zero extra RNG and stay byte-identical.
                                if let Some(wf_chance) = windfury_chance {
                                    if game_rng.random_f32() < wf_chance {
                                        // Bonus swing uses base weapon damage (the Heroic Strike
                                        // bonus is consumed by the primary swing) and re-rolls crit.
                                        let wf_base = combatant.attack_damage;
                                        let wf_is_crit = roll_crit(
                                            combatant.crit_chance + crit_bonus,
                                            &mut game_rng,
                                        );
                                        let wf_crit_damage = if wf_is_crit {
                                            wf_base * CRIT_DAMAGE_MULTIPLIER
                                        } else {
                                            wf_base
                                        };
                                        let wf_total = (wf_crit_damage
                                            * (1.0 - damage_reduction)
                                            * ds_penalty)
                                            .max(0.0);
                                        attacks.push((
                                            attacker_entity,
                                            target_entity,
                                            wf_total,
                                            false,
                                            wf_is_crit,
                                        ));

                                        // Signature Windfury VFX: a wind funnel swirls up
                                        // around the proccing melee ally. Spawned here like
                                        // FloatingCombatText; the mesh is built only in
                                        // graphical mode (rendering/effects.rs, registered
                                        // solely in states/mod.rs), so headless stays
                                        // mesh-free and deterministic.
                                        commands.spawn((
                                            WindfuryTornado {
                                                target: attacker_entity,
                                                lifetime: 0.6,
                                                initial_lifetime: 0.6,
                                                spin: 0.0,
                                            },
                                            PlayMatchEntity,
                                        ));
                                    }
                                }
                                // Break stealth on auto-attack
                                if combatant.stealthed {
                                    combatant.stealthed = false;
                                    info!(
                                        "Team {} {} breaks stealth with auto-attack!",
                                        combatant.team,
                                        combatant.class.name()
                                    );
                                }

                                // Warriors generate Rage from auto-attacks
                                if combatant.resource_type == ResourceType::Rage {
                                    let rage_gain = 10.0; // Gain 10 rage per auto-attack
                                    combatant.current_mana = (combatant.current_mana + rage_gain)
                                        .min(combatant.max_mana);
                                }
                            }

                            // A missed swing is still a swing: it costs the timer
                            // and it consumes the queued Heroic Strike bonus. Only
                            // the damage, and the rage that damage would have
                            // generated, are lost.
                            combatant.attack_timer = 0.0;

                            // Consume the bonus damage after queueing the attack
                            combatant.next_attack_bonus_damage = 0.0;
                        }

                        if off_hand_ready {
                            let missed = game_rng.random_f32() < DUAL_WIELD_MISS_CHANCE;
                            if !missed {
                                // The off hand carries no Heroic Strike bonus:
                                // that ability buffs "your next swing", and the
                                // main hand is the one it was queued against.
                                let base_damage = combatant.offhand_damage;
                                let is_crit =
                                    roll_crit(combatant.crit_chance + crit_bonus, &mut game_rng);
                                let crit_damage = if is_crit {
                                    base_damage * CRIT_DAMAGE_MULTIPLIER
                                } else {
                                    base_damage
                                };
                                let total_damage =
                                    (crit_damage * (1.0 - damage_reduction) * ds_penalty).max(0.0);
                                attacks.push((
                                    attacker_entity,
                                    target_entity,
                                    total_damage,
                                    false,
                                    is_crit,
                                ));

                                // Rage tracks the damage a swing deals, and an
                                // off-hand swing deals a fraction of one — so
                                // it pays the same fraction of the flat
                                // per-swing rage. Full rate would let a second
                                // weapon double a Warrior's rage income.
                                if combatant.resource_type == ResourceType::Rage {
                                    let rage_gain = 10.0 * OFFHAND_DAMAGE_MULTIPLIER;
                                    combatant.current_mana = (combatant.current_mana + rage_gain)
                                        .min(combatant.max_mana);
                                }
                            }
                            combatant.offhand_timer = 0.0;
                        }
                    }
                    // If not in range, timer keeps building up so they attack immediately when in range
                }
            }
        }
    }

    // Apply damage to targets and track damage dealt.
    // The maps/sets below all use BTreeMap/BTreeSet rather than HashMap/HashSet
    // so iteration order is deterministic by Entity. `frost_armor_procs` in
    // particular drives `commands.spawn(AuraPending)` calls below, where the
    // call order determines entity ID allocation and ripples into downstream
    // query iteration — a pre-existing source of self-mirror non-determinism
    // before this fix.
    let mut damage_dealt_updates: Vec<(Entity, f32)> = Vec::new();
    let mut absorbed_per_target: std::collections::BTreeMap<Entity, f32> =
        std::collections::BTreeMap::new();

    // Track crit status per target for FCT display
    let mut crit_per_target: std::collections::BTreeMap<Entity, bool> =
        std::collections::BTreeMap::new();

    // Track Frost Armor procs: attacker entities to apply slows to after the loop.
    let mut frost_armor_procs: std::collections::BTreeSet<Entity> =
        std::collections::BTreeSet::new();

    // Build a map of targets with breakable CC from friendly casters.
    let mut friendly_cc_team: std::collections::BTreeMap<Entity, u8> =
        std::collections::BTreeMap::new();
    for (entity, _, combatant, _, _, auras) in combatants.iter() {
        if let Some(auras) = auras {
            for aura in &auras.auras {
                // Only care about CC auras that break on damage
                if aura.break_on_damage_threshold >= 0.0
                    && matches!(aura.effect_type, AuraType::Polymorph | AuraType::Fear)
                {
                    // Look up the caster's team
                    if let Some(caster_entity) = aura.caster {
                        if let Some(&(caster_team, _, _, _, _)) = combatant_info.get(&caster_entity)
                        {
                            // Only track if the CC is from the opposing team of the target
                            // (i.e., the CC caster is an enemy of the CC'd target)
                            if caster_team != combatant.team {
                                friendly_cc_team.insert(entity, caster_team);
                            }
                        }
                    }
                }
            }
        }
    }

    for (attacker_entity, target_entity, damage, has_bonus, is_crit) in attacks {
        // If any attack to this target crits, mark the FCT as crit
        crit_per_target
            .entry(target_entity)
            .and_modify(|c| *c = *c || is_crit)
            .or_insert(is_crit);
        // Dying-blow semantics: every attack queued by an attacker who was alive
        // at frame start lands, even if the attacker died earlier in this loop.
        // Skipping those attacks made the winner of a simultaneous-lethal
        // exchange depend on entity iteration order (Team 1's shot processed
        // first, cancelling Team 2's counter-shot) — a systematic side bias in
        // mirror matchups. Mutual lethal now kills both, and check_match_end
        // records the draw.

        // Bug fix: Don't auto-attack targets with breakable CC from a friendly caster.
        // This prevents, e.g., a Warlock pet from breaking its team's Polymorph.
        if let Some(&cc_caster_team) = friendly_cc_team.get(&target_entity) {
            if let Some(&(attacker_team, _, _, _, _)) = combatant_info.get(&attacker_entity) {
                if attacker_team == cc_caster_team {
                    continue;
                }
            }
        }

        if let Ok((_, _, mut target, _, _, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (actual_damage, absorbed) = apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                    SpellSchool::Physical,
                );

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Frost Armor proc: the chill fires back at an attacker who
                // struck in MELEE. Keyed on the landed swing's derived kind, so
                // a caster's wand shot cannot trigger it and a melee weapon
                // swing always can, whatever class made it.
                if let Some(&(_, _, _, _, attacker_kind)) = combatant_info.get(&attacker_entity) {
                    if attacker_kind == AutoAttackKind::Melee {
                        if let Some(ref target_auras_ref) = target_auras {
                            if target_auras_ref
                                .auras
                                .iter()
                                .any(|a| a.effect_type == AuraType::FrostArmorBuff)
                            {
                                frost_armor_procs.insert(attacker_entity);
                            }
                        }
                    }
                }

                // Crippling Poison proc: a coated Rogue's landed swing has a
                // chance to apply/refresh the slow. Refreshed in place so it
                // never diminishes (poisons sidestep the slow DR category).
                if let Some(&chance) = crippling_chance.get(&attacker_entity) {
                    if game_rng.random_f32() < chance {
                        let fresh = apply_or_refresh_crippling(
                            &mut commands,
                            &abilities,
                            attacker_entity,
                            target_entity,
                            target_auras.as_deref_mut(),
                        );
                        if fresh {
                            if let Some((_, tname, _, _, _)) = combatant_info.get(&target_entity) {
                                combat_log.log(
                                    CombatLogEventType::CrowdControl,
                                    format!(
                                        "Crippling Poison applied to {} ({:.0}% slow)",
                                        tname, 70.0
                                    ),
                                );
                            }
                        }
                    }
                }

                // Track damage for aura breaking (only actual damage, not absorbed)
                *damage_per_aura_break.entry(target_entity).or_insert(0.0) += actual_damage;

                // Batch damage for floating combat text (sum all damage to same target)
                *damage_per_target.entry(target_entity).or_insert(0.0) += actual_damage;
                *absorbed_per_target.entry(target_entity).or_insert(0.0) += absorbed;

                // Collect attacker damage for later update (include absorbed damage - attacker dealt it)
                damage_dealt_updates.push((attacker_entity, actual_damage + absorbed));

                // One landed auto-attack = one swing signal for the graphical
                // animation layer. Spawned here in the APPLY loop — not at the
                // queue site like WindfuryTornado — so an attack dropped above
                // by the friendly-CC guard or a same-frame death never
                // telegraphs a phantom release stroke. Inert in headless: like
                // FloatingCombatText, the consuming systems live in
                // rendering/effects.rs and are registered only in states/mod.rs.
                if let Some(&(_, _, _, _, attacker_kind)) = combatant_info.get(&attacker_entity) {
                    commands.spawn((
                        AutoAttackSwing {
                            attacker: attacker_entity,
                            target: target_entity,
                            // The swing is ranged iff the weapon it comes
                            // from is a ranged one.
                            ranged: attacker_kind != AutoAttackKind::Melee,
                        },
                        PlayMatchEntity,
                    ));
                }

                // Log the attack with structured data
                if let (
                    Some((attacker_team, attacker_name, _, attacker_slot, attacker_kind)),
                    Some((target_team, target_name, _, target_slot, _)),
                ) = (
                    combatant_info.get(&attacker_entity),
                    combatant_info.get(&target_entity),
                ) {
                    // The log name is chosen by the same derived kind as the
                    // range, so the two can never disagree.
                    let attack_name = if has_bonus {
                        "Heroic Strike" // Enhanced auto-attack
                    } else {
                        match attacker_kind {
                            AutoAttackKind::Melee => "Auto Attack",
                            AutoAttackKind::Shot => "Auto Shot",
                            AutoAttackKind::Wand => "Wand Shot",
                            // Unreachable: the range gate `continue`s on None.
                            AutoAttackKind::None => "Auto Attack",
                        }
                    };
                    let attacker_id = combat_log_id(*attacker_team, *attacker_slot, attacker_name);
                    let target_id = combat_log_id(*target_team, *target_slot, target_name);

                    let verb = if is_crit { "CRITS" } else { "hits" };
                    let message = if absorbed > 0.0 {
                        format!(
                            "{}'s {} {} {} for {:.0} damage ({:.0} absorbed)",
                            attacker_id, attack_name, verb, target_id, actual_damage, absorbed
                        )
                    } else {
                        format!(
                            "{}'s {} {} {} for {:.0} damage",
                            attacker_id, attack_name, verb, target_id, actual_damage
                        )
                    };

                    let is_killing_blow = !target.is_alive();
                    combat_log.log_damage(
                        attacker_id.clone(),
                        target_id.clone(),
                        attack_name.to_string(),
                        actual_damage + absorbed, // Total damage dealt (including absorbed)
                        is_killing_blow,
                        is_crit,
                        message,
                    );

                    // Log death with killer tracking (only on first death to prevent duplicates)
                    if is_killing_blow {
                        // Mark target as dead to prevent duplicate death processing across systems
                        let was_already_dead = if let Ok((_, _, mut dead_target, _, _, _)) =
                            combatants.get_mut(target_entity)
                        {
                            let already = dead_target.is_dead;
                            dead_target.is_dead = true;
                            already
                        } else {
                            true // entity gone, treat as already dead
                        };

                        if was_already_dead {
                            continue;
                        }

                        // Cancel any in-progress cast or channel so dead combatants can't finish spells
                        commands.entity(target_entity).remove::<CastingState>();
                        commands.entity(target_entity).remove::<ChannelingState>();

                        let death_message = format!("{} has been eliminated", target_id);
                        combat_log.log_death(target_id, Some(attacker_id), death_message);
                    }
                }
            }
        }
    }

    // Apply Frost Armor procs: chill melee attackers who hit a target with FrostArmorBuff
    // Only apply if the attacker doesn't already have the chill (prevents DR escalation)
    for attacker_entity in frost_armor_procs {
        // Check if attacker already has the Frost Armor chill active
        let already_has_frost_slow =
            if let Ok((_, _, _, _, _, Some(attacker_auras))) = combatants.get(attacker_entity) {
                attacker_auras
                    .auras
                    .iter()
                    .any(|a| a.compound == Some(CompoundDebuff::FrostArmorChill))
            } else {
                false
            };
        if already_has_frost_slow {
            continue;
        }
        // ONE pending, for the chill's FACE. `apply_pending_auras` brings the
        // attack-speed rider in with it, at whatever duration the face ends up
        // with — see the compound note there.
        commands.spawn(AuraPending {
            target: attacker_entity,
            aura: frost_armor_movement_slow_aura(),
        });
    }

    // Spawn floating combat text for each target that took damage (batched)
    for (target_entity, total_damage) in damage_per_target {
        let target_was_crit = crit_per_target
            .get(&target_entity)
            .copied()
            .unwrap_or(false);
        if let Some(&target_pos) = positions.get(&target_entity) {
            // Spawn floating text slightly above the combatant
            let text_position = target_pos + Vec3::new(0.0, FCT_HEIGHT, 0.0);

            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity)
            {
                get_next_fct_offset(&mut fct_state)
            } else {
                // Fallback to center if state not found
                (0.0, 0.0)
            };

            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", total_damage),
                    color: egui::Color32::WHITE, // White for auto-attacks
                    lifetime: 1.5,               // Display for 1.5 seconds
                    vertical_offset: offset_y,
                    is_crit: target_was_crit,
                },
                PlayMatchEntity,
            ));

            // Spawn light blue floating combat text for absorbed damage
            if let Some(&total_absorbed) = absorbed_per_target.get(&target_entity) {
                if total_absorbed > 0.0 {
                    let (absorb_offset_x, absorb_offset_y) =
                        if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                            get_next_fct_offset(&mut fct_state)
                        } else {
                            (0.0, 0.0)
                        };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position
                                + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                            text: format!("{:.0} absorbed", total_absorbed),
                            color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                            lifetime: 1.5,
                            vertical_offset: absorb_offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }
            }
        }
    }

    // Update attacker damage dealt stats
    for (attacker_entity, damage) in damage_dealt_updates {
        if let Ok((_, _, mut attacker, _, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += damage;
        }
    }

    // Track damage for aura breaking
    for (target_entity, total_damage) in damage_per_aura_break {
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: total_damage,
        });
    }
}

/// How long a Frost Armor proc chills its victim.
pub const FROST_ARMOR_PROC_DURATION: f32 = 5.0;

/// The RIDER effects a compound debuff brings along with its face.
///
/// Called by `apply_pending_auras` at the moment the face actually lands, so a
/// rider cannot outlive, out-stack or survive the rejection of the debuff it
/// belongs to. The riders' durations are overwritten there with the face's
/// post-diminishing-returns duration; what this returns is everything else
/// about them.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** A compound whose
/// riders were forgotten here is a debuff that silently does half of what it
/// says, and the catalog would still print both effects.
pub fn compound_riders(compound: CompoundDebuff) -> Vec<Aura> {
    match compound {
        CompoundDebuff::FrostArmorChill => vec![frost_armor_attack_speed_aura()],
    }
}

/// The Frost Armor chill: ONE debuff, two effects.
///
/// A melee attacker who strikes a Mage wearing Frost Armor is slowed AND swings
/// slower. The two effects are separate [`Aura`]s because that is the only way
/// the movement solver and the swing timer can each see the half they act on —
/// and they carry [`CompoundDebuff::FrostArmorChill`] so everything that treats
/// a debuff as a unit (the frames, the catalog, a dispel) sees one thing.
///
/// The whole debuff, face first, at its UNDIMINISHED durations — what the
/// encyclopedia's page describes and what the tests assert against. The sim
/// never calls this: it queues the face alone and lets `apply_pending_auras`
/// pull the riders in through [`compound_riders`], which is what keeps the two
/// halves from being applied apart or with different lifetimes.
///
/// Note the name: the chill and the Mage's own self-buff are both "Frost
/// Armor" on the frames, which is why the catalog disambiguates and the audit
/// keys on name PLUS mechanic.
pub fn frost_armor_chill_auras() -> [Aura; 2] {
    [
        frost_armor_movement_slow_aura(),
        frost_armor_attack_speed_aura(),
    ]
}

/// The movement-slow half of [`frost_armor_chill_auras`] — the chill's FACE,
/// and so the effect a dispel is classified against. Public for the catalog,
/// which needs each effect's own numbers; the sim applies the pair.
pub fn frost_armor_movement_slow_aura() -> Aura {
    Aura {
        effect_type: AuraType::MovementSpeedSlow,
        duration: FROST_ARMOR_PROC_DURATION,
        magnitude: 0.7, // 30% slow (0.7 = 70% speed)
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: "Frost Armor".to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: Some(SpellSchool::Frost),
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
        compound: Some(CompoundDebuff::FrostArmorChill),
    }
}

/// The attack-speed half of [`frost_armor_chill_auras`] — a RIDER: it does not
/// need to be dispellable in its own right, because the chill comes off by its
/// face.
pub fn frost_armor_attack_speed_aura() -> Aura {
    Aura {
        effect_type: AuraType::AttackSpeedSlow,
        duration: FROST_ARMOR_PROC_DURATION,
        magnitude: 0.25,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: "Frost Armor".to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: Some(SpellSchool::Frost),
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
        compound: Some(CompoundDebuff::FrostArmorChill),
    }
}

/// Effective auto-attack interval for a combatant: base `1.0 / attack_speed`,
/// stretched by each `AttackSpeedSlow` aura (magnitude clamped at 0.75 to
/// prevent division by near-zero).
///
/// Shared by the swing timer in `combat_auto_attack` above AND the graphical
/// windup animation (`rendering/effects.rs`), so the anticipation window can
/// never drift from the sim's real cadence. Pure — safe to call from graphical
/// systems without touching sim state.
pub fn effective_attack_interval(combatant: &Combatant, auras: Option<&ActiveAuras>) -> f32 {
    swing_interval(combatant.attack_speed, auras)
}

/// The same, for the OFF hand, off its own weapon's speed.
///
/// Only meaningful while [`Combatant::is_dual_wielding`] — that predicate is
/// what guarantees the speed is non-zero, so the reciprocal below is safe.
pub fn effective_offhand_interval(combatant: &Combatant, auras: Option<&ActiveAuras>) -> f32 {
    swing_interval(combatant.offhand_speed, auras)
}

/// One swing interval from one weapon speed.
///
/// The two hands share this rather than each spelling out the arithmetic,
/// **and the operation order below is load-bearing**: it multiplies the
/// reciprocal by each slow in turn. Folding the slows together first and
/// multiplying once is algebraically the same and is NOT the same in `f32`,
/// which would shift the main hand's interval by an ULP and, through the
/// timer comparison, potentially every match in the project's baselines.
fn swing_interval(speed: f32, auras: Option<&ActiveAuras>) -> f32 {
    let mut attack_interval = 1.0 / speed;
    if let Some(auras) = auras {
        for aura in auras.auras.iter() {
            if aura.effect_type == AuraType::AttackSpeedSlow {
                // magnitude = slow amount (e.g., 0.25 = 25% slower → 1.33x interval)
                let clamped = aura.magnitude.min(0.75);
                attack_interval *= 1.0 / (1.0 - clamped);
            }
        }
    }
    attack_interval
}

/// Windfury Totem bonus-swing chance for this attacker. Returns `Some(magnitude)`
/// ONLY when the attacker is melee and carries a `WindfuryBuff` aura (R14/AE3):
/// the totem may pulse the buff onto every ally in radius, but the proc is inert
/// for ranged/caster allies who are wanding or auto-shooting. Returns `None`
/// (no bonus swing) for a ranged attacker even if it carries the buff.
pub(crate) fn windfury_bonus_chance(
    attacker_kind: AutoAttackKind,
    auras: Option<&ActiveAuras>,
) -> Option<f32> {
    if attacker_kind != AutoAttackKind::Melee {
        return None;
    }
    auras.and_then(|a| {
        a.auras
            .iter()
            .find(|aura| aura.effect_type == AuraType::WindfuryBuff)
            .map(|aura| aura.magnitude)
    })
}

/// Apply or refresh the Crippling Poison slow on `target`. Returns true if it was
/// freshly applied (vs. just refreshed) — the caller logs only the initial proc.
///
/// Refresh-in-place when the debuff is already present (no DR, no stacking);
/// push directly when the target already has an `ActiveAuras` component;
/// otherwise defer via `AuraPending` (the rare first-debuff-on-a-fresh-target
/// case, which respects immunity through the normal aura pipeline).
fn apply_or_refresh_crippling(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    attacker: Entity,
    target: Entity,
    target_auras: Option<&mut ActiveAuras>,
) -> bool {
    let def = abilities.get_unchecked(&AbilityType::CripplingPoison);
    let refresh_to = def.applies_aura.as_ref().map(|a| a.duration).unwrap_or(8.0);
    if let Some(auras) = target_auras {
        if let Some(existing) = auras.auras.iter_mut().find(|a| {
            a.effect_type == AuraType::MovementSpeedSlow && a.ability_name == "Crippling Poison"
        }) {
            existing.duration = refresh_to;
            return false;
        }
        if let Some(pending) = AuraPending::from_ability(target, attacker, def) {
            auras.auras.push(pending.aura);
        }
        true
    } else {
        if let Some(pending) = AuraPending::from_ability(target, attacker, def) {
            commands.spawn(pending);
        }
        true
    }
}

#[cfg(test)]
mod windfury_gate_tests {
    use super::super::super::components::{ActiveAuras, Aura};
    use super::*;

    fn windfury_auras(magnitude: f32) -> ActiveAuras {
        ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::WindfuryBuff,
                magnitude,
                ..Default::default()
            }],
        }
    }

    /// AE3 (covers R14 Windfury): the bonus swing is gated on the SWING being
    /// a melee weapon swing, not on the attacker's class. A totem pulses its
    /// buff onto everyone in radius, so every one of the four kinds can be
    /// carrying it; only `Melee` may convert it into a bonus swing.
    ///
    /// Enumerated exhaustively over `AutoAttackKind` rather than over a bool,
    /// because the bool this gate used to take could not tell a bow shot from
    /// a wand shot from a combatant with no weapon at all — and it read the
    /// CLASS ladder, so a melee-weapon swing by a class the ladder calls
    /// ranged (a Shaman with its mace) silently fell into the `None` arm.
    #[test]
    fn windfury_bonus_only_for_melee_swings() {
        let buffed = windfury_auras(0.2);

        // The melee swing converts the buff into a bonus-swing chance.
        assert_eq!(
            windfury_bonus_chance(AutoAttackKind::Melee, Some(&buffed)),
            Some(0.2),
            "a melee weapon swing carrying the Windfury buff must get the bonus-swing chance"
        );

        // Every non-melee kind inside the same radius carries the same buff
        // and must still get nothing from it.
        for kind in [
            AutoAttackKind::Shot,
            AutoAttackKind::Wand,
            AutoAttackKind::None,
        ] {
            assert_eq!(
                windfury_bonus_chance(kind, Some(&buffed)),
                None,
                "{kind:?} must get NO Windfury bonus swing even while carrying the totem buff"
            );
        }
    }

    /// A melee swing without the buff gets no bonus swing (the aura is the
    /// gate, not just the kind).
    #[test]
    fn windfury_bonus_none_without_buff() {
        let empty = ActiveAuras { auras: vec![] };
        assert_eq!(
            windfury_bonus_chance(AutoAttackKind::Melee, Some(&empty)),
            None
        );
        assert_eq!(windfury_bonus_chance(AutoAttackKind::Melee, None), None);
    }
}
