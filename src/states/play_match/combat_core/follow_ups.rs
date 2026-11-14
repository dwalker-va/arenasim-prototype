//! What a hit sets off on other units: transfers, reflects and saves.
//!
//! `apply_damage_with_absorb` holds one target. A transfer (Blessing of
//! Sacrifice, Soul Link) lands on another unit, a reflect (Retribution Aura,
//! a reflecting shield) on the attacker, and a save's heal goes through
//! `apply_healing`, which the funnel cannot reach. So the funnel pushes each
//! as a [`DamageFollowUp`] onto the caller's [`DamageFollowUps`]; the caller
//! spawns them ([`DamageFollowUps::spawn`]); and [`process_damage_follow_ups`]
//! lands them at the end of the frame, through the same funnel, logging what
//! they do and the deaths they cause.
//!
//! Nothing is pushed unless a modifier aura or a talent asks for it, and an
//! empty batch spawns nothing, so a match without them never meets this
//! module.
//!
//! **Dying-blow semantics.** Every queued hit of a frame still lands; a
//! follow-up lands after them, in the order the frame queued it. A save is
//! checked per blow, so it spares the holder the FIRST lethal blow of its
//! window and a second lethal blow in the same frame kills; a mutual lethal
//! in which one side holds a save is that side's survival, not a draw.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::super::abilities::SpellSchool;
use super::super::components::*;
use super::super::utils::combat_log_id_for;
use super::{
    apply_damage_with_absorb, apply_healing, blocks_all_damage, credit_damage_dealt,
    credit_healing_done, credited_damage, share_to_health, Hit, HitKind,
};
use crate::combat::log::{CombatLog, CombatLogEventType};

/// Who receives a transferred share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferReceiver {
    /// A named unit: a [`AuraType::DamageTransfer`]'s caster.
    Unit(Entity),
    /// The holder's own pet (Soul Link).
    OwnPet,
}

/// One effect a hit sets off beyond its victim.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageFollowUp {
    /// `amount` of the holder's damage goes to `receiver` instead.
    Transfer {
        holder: Entity,
        receiver: TransferReceiver,
        amount: f32,
        /// The school of the hit it was split from: what the receiver's
        /// scoped damage-taken modifiers read.
        school: SpellSchool,
        /// Whether the receiver's own immunity, damage-taken modifiers and
        /// absorbs apply to it (Blessing of Sacrifice), or it cannot be
        /// prevented (Soul Link). See [`HitKind::share`].
        preventable: bool,
        /// The unit that dealt the hit, credited with the share.
        dealer: Option<Entity>,
        /// The ability that dealt the hit ("Mortal Strike"): the share is
        /// part of its damage, and is credited to its row.
        ability: String,
        /// What transferred it ("Blessing of Sacrifice", "Soul Link").
        by: String,
    },
    /// `amount` of `school` damage is dealt back to `to`, the attacker that
    /// struck `from`, credited to `dealer` (the reflecting aura's caster).
    Reflect {
        to: Entity,
        from: Entity,
        dealer: Option<Entity>,
        amount: f32,
        school: SpellSchool,
        /// What reflected it ("Retribution Aura").
        source: String,
    },
    /// A killing blow on `holder` was prevented by `by`.
    Saved {
        holder: Entity,
        by: String,
        /// Who cast the save, credited with its heal.
        caster: Option<Entity>,
        /// The heal it lands, as a fraction of the holder's maximum health
        /// (Guardian Spirit), through `apply_healing`.
        heal_fraction: Option<f32>,
        /// The damage cut `(reduction, duration)` the save leaves, when the
        /// funnel could not land it itself on a holder with no `ActiveAuras`
        /// (Cheat Death).
        aura: Option<(f32, f32)>,
    },
}

/// The follow-ups one damage system's hits set off, spawned once it is done.
#[derive(Default)]
pub struct DamageFollowUps(Vec<DamageFollowUp>);

impl DamageFollowUps {
    /// Queue a follow-up.
    pub fn push(&mut self, follow_up: DamageFollowUp) {
        self.0.push(follow_up);
    }

    /// Whether nothing was queued.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The follow-ups queued, in order.
    pub fn as_slice(&self) -> &[DamageFollowUp] {
        &self.0
    }

    /// Spawn every queued follow-up for [`process_damage_follow_ups`].
    /// Spawns nothing when nothing was queued.
    pub fn spawn(self, commands: &mut Commands) {
        for follow_up in self.0 {
            commands.spawn(PendingDamageFollowUp(follow_up));
        }
    }
}

/// A follow-up waiting for [`process_damage_follow_ups`].
#[derive(Component)]
pub struct PendingDamageFollowUp(pub DamageFollowUp);

/// A killing blow prevented: what the shared "Saved" cue (AS-354) reads.
/// Cosmetic; spawned in both modes, read only by the graphical client.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct KillingBlowPrevented {
    pub holder: Entity,
    /// What saved it ("Guardian Spirit", "Cheat Death").
    pub by: String,
}

/// Land every follow-up the frame's hits queued: transfers on their
/// receivers, reflects on their attackers, saves' heals and markers. A
/// follow-up's own hit can queue a save, which lands in the same pass.
///
/// A transfer never kills its receiver: a share it could not survive (or one
/// whose receiver is dead or gone) stays with the holder, and a transfer
/// aura that could not pass its share on ends. What it could survive is what
/// the share would put on its health ([`share_to_health`]): a receiver immune
/// to it bears any share.
pub fn process_damage_follow_ups(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    dampening: Res<ArenaDampening>,
    pending: Query<(Entity, &PendingDamageFollowUp)>,
    mut combatants: Query<(
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&DamageTakenThisFrame>,
    )>,
    pets: Query<(Entity, &Pet)>,
) {
    let mut queue: VecDeque<DamageFollowUp> = VecDeque::new();
    for (entity, follow_up) in pending.iter() {
        commands.entity(entity).despawn();
        queue.push_back(follow_up.0.clone());
    }
    let pet_of = |e: Entity| pets.get(e).ok().map(|(_, p)| p);
    while let Some(follow_up) = queue.pop_front() {
        let mut more = DamageFollowUps::default();
        match follow_up {
            DamageFollowUp::Transfer {
                holder,
                receiver,
                amount,
                school,
                preventable,
                dealer,
                ability,
                by,
            } => {
                let kind = HitKind::share(preventable);
                let receiver_entity = match receiver {
                    TransferReceiver::Unit(e) => Some(e),
                    TransferReceiver::OwnPet => pets
                        .iter()
                        .filter(|(_, p)| p.owner == holder)
                        .map(|(e, _)| e)
                        .find(|e| combatants.get(*e).is_ok_and(|(c, _, _)| c.is_alive())),
                };
                let bearable = receiver_entity.is_some_and(|e| {
                    combatants.get(e).is_ok_and(|(c, auras, _)| {
                        c.is_alive()
                            && c.current_health > share_to_health(kind, amount, school, auras)
                    })
                });
                let (lands_on, returned) = match receiver_entity {
                    Some(e) if bearable => (e, false),
                    _ => (holder, true),
                };
                if returned {
                    // The transfer ends: its receiver cannot bear the share.
                    if let (TransferReceiver::Unit(r), Ok((_, Some(mut auras), _))) =
                        (receiver, combatants.get_mut(holder))
                    {
                        auras.auras.retain(|a| {
                            !(a.effect_type == AuraType::DamageTransfer && a.caster == Some(r))
                        });
                    }
                }
                // Whether the receiver's immunity blocks the share, read
                // before it lands, for its log line.
                let immune = !returned
                    && kind == HitKind::Transferred
                    && combatants
                        .get(lands_on)
                        .is_ok_and(|(_, auras, _)| blocks_all_damage(auras));
                let hit = Hit {
                    amount,
                    school,
                    source: &by,
                    row: &by,
                    attacker: dealer,
                    victim: lands_on,
                    dealt: 1.0,
                    // A share handed back meets its holder's absorbs.
                    kind: if returned { HitKind::Returned } else { kind },
                    // A share met (or skipped) armor on the unit it left.
                    mechanic: None,
                };
                let landed = land(
                    hit,
                    &mut commands,
                    &mut combat_log,
                    &mut combatants,
                    &pet_of,
                    &mut more,
                );
                if let Some(landed) = landed {
                    let Landed {
                        actual,
                        absorbed,
                        ref victim_id,
                        ..
                    } = landed;
                    let holder_id = combatants
                        .get(holder)
                        .map(|(c, _, _)| combat_log_id_for(c, pet_of(holder)))
                        .unwrap_or_default();
                    let message = if returned {
                        format!(
                            "[TRANSFER] {by} cannot pass on {actual:.0} damage: {victim_id} takes it"
                        )
                    } else if immune && actual == 0.0 && absorbed == 0.0 {
                        format!("[TRANSFER] {victim_id} is immune to {holder_id}'s damage ({by})")
                    } else if absorbed > 0.0 {
                        format!(
                            "[TRANSFER] {victim_id} takes {actual:.0} of {holder_id}'s damage \
                             ({by}, {absorbed:.0} absorbed)"
                        )
                    } else {
                        format!("[TRANSFER] {victim_id} takes {actual:.0} of {holder_id}'s damage ({by})")
                    };
                    record(
                        &mut combat_log,
                        &mut combatants,
                        dealer,
                        &landed,
                        &ability,
                        message,
                    );
                }
            }
            DamageFollowUp::Reflect {
                to,
                from,
                dealer,
                amount,
                school,
                source,
            } => {
                let hit = Hit {
                    amount,
                    school,
                    source: &source,
                    row: &source,
                    attacker: dealer,
                    victim: to,
                    dealt: 1.0,
                    kind: HitKind::Reflected,
                    mechanic: None,
                };
                let landed = land(
                    hit,
                    &mut commands,
                    &mut combat_log,
                    &mut combatants,
                    &pet_of,
                    &mut more,
                );
                if let Some(landed) = landed {
                    let from_id = combatants
                        .get(from)
                        .map(|(c, _, _)| combat_log_id_for(c, pet_of(from)))
                        .unwrap_or_default();
                    let message = format!(
                        "[REFLECT] {} takes {:.0} {school:?} damage from {from_id}'s {source}",
                        landed.victim_id, landed.actual
                    );
                    record(
                        &mut combat_log,
                        &mut combatants,
                        dealer,
                        &landed,
                        &source,
                        message,
                    );
                }
            }
            DamageFollowUp::Saved {
                holder,
                by,
                caster,
                heal_fraction,
                aura,
            } => {
                // Who the save's heal is credited to: its caster's log id,
                // read before the holder is borrowed.
                let caster_id = caster.and_then(|c| {
                    combatants
                        .get(c)
                        .ok()
                        .map(|(cc, _, _)| combat_log_id_for(cc, pet_of(c)))
                });
                let Ok((mut combatant, auras, _)) = combatants.get_mut(holder) else {
                    continue;
                };
                let holder_id = combat_log_id_for(&combatant, pet_of(holder));
                // A later blow in the save's own frame can still kill its
                // holder: the save prevented one blow and then has no one to
                // heal, so its line says so and no heal line follows.
                let survived = combatant.is_alive();
                let message = if survived {
                    format!("[SAVED] {by} prevents a killing blow on {holder_id}")
                } else {
                    format!(
                        "[SAVED] {by} prevents a killing blow on {holder_id}, who is killed by \
                         another blow"
                    )
                };
                combat_log.log(CombatLogEventType::MatchEvent, message);
                commands.spawn((
                    KillingBlowPrevented {
                        holder,
                        by: by.clone(),
                    },
                    PlayMatchEntity,
                ));
                let mut healed = 0.0;
                if let Some(fraction) = heal_fraction.filter(|_| survived) {
                    let amount = combatant.max_health * fraction;
                    healed = apply_healing(
                        &mut commands,
                        holder,
                        &mut combatant,
                        auras.as_deref(),
                        &dampening,
                        amount,
                    );
                    let message = format!("[SAVED] {by} heals {holder_id} for {healed:.0}");
                    // A row in its caster's breakdown, under the save's name;
                    // a save with no caster in the match credits no one.
                    match caster_id {
                        Some(caster_id) => {
                            combat_log.log_healing(caster_id, holder_id, by, healed, false, message)
                        }
                        None => combat_log.log(CombatLogEventType::Healing, message),
                    }
                }
                if let Some((reduction, duration)) = aura {
                    commands.spawn(AuraPending {
                        target: holder,
                        aura: super::cheat_death_aura(reduction, duration),
                    });
                }
                if let Some(Ok((mut c, _, _))) = caster.map(|c| combatants.get_mut(c)) {
                    credit_healing_done(&mut c, healed);
                }
            }
        }
        queue.extend(more.0);
    }
}

/// One follow-up hit, as it landed.
struct Landed {
    /// The damage to health.
    actual: f32,
    /// The damage absorbs soaked.
    absorbed: f32,
    /// The victim's log id.
    victim_id: String,
    /// The dealer's log id, while it is still in the match.
    dealer_id: Option<String>,
    /// Whether this hit killed its victim.
    killed: bool,
}

/// Land one follow-up hit on its victim through the funnel, with every damage
/// site's bookkeeping: rage, the aura-break tally and the death. `None` when
/// the victim is gone or already dead.
fn land<'p>(
    hit: Hit,
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    combatants: &mut Query<(
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&DamageTakenThisFrame>,
    )>,
    pet_of: &impl Fn(Entity) -> Option<&'p Pet>,
    follow_ups: &mut DamageFollowUps,
) -> Option<Landed> {
    let killer_id = hit.attacker.and_then(|a| {
        combatants
            .get(a)
            .ok()
            .map(|(c, _, _)| combat_log_id_for(c, pet_of(a)))
    });
    let (mut victim, auras, taken) = combatants.get_mut(hit.victim).ok()?;
    if !victim.is_alive() {
        return None;
    }
    let (actual, absorbed) = apply_damage_with_absorb(
        hit,
        &mut victim,
        auras.map(|a| a.into_inner()),
        follow_ups,
        combat_log,
    );
    if actual > 0.0 && victim.resource_type == ResourceType::Rage {
        let rage_gain = actual * 0.15;
        victim.current_mana = (victim.current_mana + rage_gain).min(victim.max_mana);
    }
    if actual > 0.0 {
        let before = taken.map_or(0.0, |t| t.amount);
        commands.entity(hit.victim).insert(DamageTakenThisFrame {
            amount: before + actual,
        });
    }
    let victim_id = combat_log_id_for(&victim, pet_of(hit.victim));
    let killed = !victim.is_alive() && !victim.is_dead;
    if killed {
        victim.is_dead = true;
        commands.entity(hit.victim).remove::<CastingState>();
        commands.entity(hit.victim).remove::<ChannelingState>();
        combat_log.log_death(
            victim_id.clone(),
            killer_id.clone(),
            format!("{victim_id} has been eliminated"),
        );
    }
    Some(Landed {
        actual,
        absorbed,
        victim_id,
        dealer_id: killer_id,
        killed,
    })
}

/// Credit a follow-up hit to `dealer` and log its line: a breakdown row of
/// the dealer's `ability` when the dealer is still in the match, a plain
/// line of no one's otherwise.
fn record(
    combat_log: &mut CombatLog,
    combatants: &mut Query<(
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&DamageTakenThisFrame>,
    )>,
    dealer: Option<Entity>,
    landed: &Landed,
    ability: &str,
    message: String,
) {
    if let Some(Ok((mut c, _, _))) = dealer.map(|d| combatants.get_mut(d)) {
        credit_damage_dealt(&mut c, landed.actual, landed.absorbed);
    }
    match &landed.dealer_id {
        Some(dealer_id) => combat_log.log_damage(
            dealer_id.clone(),
            landed.victim_id.clone(),
            ability.to_string(),
            credited_damage(landed.actual, landed.absorbed),
            landed.killed,
            false,
            message,
        ),
        None => combat_log.log(CombatLogEventType::Damage, message),
    }
}

/// End every damage transfer whose receiver can no longer take a share, so
/// the holder's hits stop splitting one off at step 4 of
/// `apply_damage_with_absorb`: a [`AuraType::DamageTransfer`] whose caster is
/// dead or gone ends, and a Soul Link whose pet is dead or gone is severed
/// (`Combatant::soul_link_severed`). Runs before the frame's damage; a
/// receiver that dies mid-frame is caught by [`process_damage_follow_ups`],
/// which hands the share back as a [`HitKind::Returned`] hit the holder's
/// absorbs soak. A no-op when no unit holds a transfer.
pub fn end_transfers_to_the_dead(
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(
        Entity,
        &mut Combatant,
        Option<&mut ActiveAuras>,
        Option<&Pet>,
    )>,
) {
    let holds_transfer = |c: &Combatant, auras: Option<&ActiveAuras>| {
        (c.talent_mechanics.soul_link().is_some() && !c.soul_link_severed)
            || auras.is_some_and(|a| {
                a.auras
                    .iter()
                    .any(|aura| aura.effect_type == AuraType::DamageTransfer)
            })
    };
    if !combatants
        .iter()
        .any(|(_, c, auras, _)| c.is_alive() && holds_transfer(c, auras))
    {
        return;
    }
    let alive: std::collections::BTreeSet<Entity> = combatants
        .iter()
        .filter(|(_, c, _, _)| c.is_alive())
        .map(|(e, ..)| e)
        .collect();
    let living_pet_owners: std::collections::BTreeSet<Entity> = combatants
        .iter()
        .filter(|(_, c, _, _)| c.is_alive())
        .filter_map(|(_, _, _, pet)| pet.map(|p| p.owner))
        .collect();
    let mut holders: Vec<(u8, u8, Entity)> = combatants
        .iter()
        .filter(|(_, c, auras, _)| c.is_alive() && holds_transfer(c, *auras))
        .map(|(e, c, _, _)| (c.team, c.slot, e))
        .collect();
    holders.sort();
    for (_, _, entity) in holders {
        let Ok((_, mut holder, auras, pet)) = combatants.get_mut(entity) else {
            continue;
        };
        let holder_id = combat_log_id_for(&holder, pet);
        if holder.talent_mechanics.soul_link().is_some()
            && !holder.soul_link_severed
            && !living_pet_owners.contains(&entity)
        {
            holder.soul_link_severed = true;
            combat_log.log(
                CombatLogEventType::MatchEvent,
                format!("[TALENTS] {holder_id}'s Soul Link ends: its demon is dead"),
            );
        }
        if let Some(mut auras) = auras {
            let mut ended: Vec<String> = Vec::new();
            auras.auras.retain(|a| {
                let orphaned = a.effect_type == AuraType::DamageTransfer
                    && !a.caster.is_some_and(|c| alive.contains(&c));
                if orphaned {
                    ended.push(a.ability_name.clone());
                }
                !orphaned
            });
            for name in ended {
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!("[TRANSFER] {holder_id}'s {name} ends: its caster is dead"),
                );
            }
        }
    }
}
