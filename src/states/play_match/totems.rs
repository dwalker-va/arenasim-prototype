//! Totem Pulse System (Shaman)
//!
//! `totem_pulse_system` owns the full totem lifecycle, modeled on
//! `traps::slow_zone_system`:
//! - **Step 0 — one-per-element dedup**: across all live totems, for each
//!   `(owner, element)` keep the one with the GREATEST `duration_remaining` and
//!   despawn the rest. A freshly recast totem has full duration and wins, so
//!   recast-replaces-early falls out for free.
//! - **Tick**: decrement `duration_remaining`; despawn at 0.
//! - **Pulse**: apply-or-refresh the totem's buff aura on every ally
//!   (`team == owner_team`) within `radius`, mirroring how the slow zone
//!   refreshes the Frost Trap slow (find the existing aura by `ability_name`
//!   and reset its short refresh window; else push a fresh one). Direct
//!   `ActiveAuras` mutation respects U2's stacking guard (one buff per type).

use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use crate::combat::log::{CombatLog, CombatLogEventType};
use super::components::*;

/// Short window (seconds) the buff aura is refreshed to each pulse. Allies that
/// leave the radius keep the buff for at most this long before it expires.
const TOTEM_BUFF_REFRESH_WINDOW: f32 = 2.0;

/// Build the beneficial aura a totem pulses. `break_on_damage_threshold: -1.0`
/// (never breaks); `ability_name` is the element's stable buff name (refresh
/// key). HoT buffs tick every second; flat buffs (`tick_interval 0.0`) don't.
fn make_totem_aura(
    aura_type: AuraType,
    magnitude: f32,
    owner: Entity,
    spell_school: super::abilities::SpellSchool,
    buff_name: &str,
) -> Aura {
    let tick_interval = if aura_type == AuraType::HealingOverTime { 1.0 } else { 0.0 };
    Aura {
        effect_type: aura_type,
        duration: TOTEM_BUFF_REFRESH_WINDOW,
        magnitude,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval,
        time_until_next_tick: tick_interval,
        caster: Some(owner),
        ability_name: buff_name.to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: Some(spell_school),
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
    }
}

/// Totem lifecycle + buff pulse. See module docs.
pub fn totem_pulse_system(
    mut commands: Commands,
    time: Res<Time>,
    mut combat_log: ResMut<CombatLog>,
    mut totems: Query<(Entity, &mut Totem, &Transform)>,
    mut combatants: Query<(Entity, &Combatant, &Transform, Option<&mut ActiveAuras>), Without<Totem>>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't pulse totems during victory celebration (mirrors slow_zone_system).
    if celebration.is_some() {
        return;
    }

    let dt = time.delta_secs();

    // --- Step 0: one-per-(owner, element) dedup -----------------------------
    // Keep the greatest-duration totem per (owner, element); deterministic
    // tie-break by lower entity id (ties only arise on a same-frame recast,
    // which GCD prevents — included for total ordering). Losers are despawned.
    let mut winners: BTreeMap<(Entity, usize), (Entity, f32)> = BTreeMap::new();
    for (entity, totem, _) in totems.iter() {
        let key = (totem.owner, totem.element.index());
        winners
            .entry(key)
            .and_modify(|(win_e, win_d)| {
                if totem.duration_remaining > *win_d
                    || (totem.duration_remaining == *win_d && entity < *win_e)
                {
                    *win_e = entity;
                    *win_d = totem.duration_remaining;
                }
            })
            .or_insert((entity, totem.duration_remaining));
    }
    let keep: BTreeSet<Entity> = winners.values().map(|(e, _)| *e).collect();
    for (entity, _, _) in totems.iter() {
        if !keep.contains(&entity) {
            commands.entity(entity).despawn();
        }
    }

    // --- Tick + pulse (winners only) ----------------------------------------
    for (entity, mut totem, totem_transform) in totems.iter_mut() {
        if !keep.contains(&entity) {
            continue; // loser — already queued for despawn above
        }

        totem.duration_remaining -= dt;
        if totem.duration_remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Snapshot the totem's fields so the ally loop can borrow `combatants`
        // mutably without aliasing the `totems` borrow.
        let owner_team = totem.owner_team;
        let owner = totem.owner;
        let aura_type = totem.aura_type;
        let magnitude = totem.magnitude;
        let spell_school = totem.spell_school;
        let radius = totem.radius;
        let buff_name = totem.element.buff_name();
        let totem_pos = totem_transform.translation;

        for (ally_entity, ally, ally_transform, active_auras) in combatants.iter_mut() {
            if !ally.is_alive() {
                continue;
            }
            if ally.team != owner_team {
                continue;
            }
            if totem_pos.distance(ally_transform.translation) > radius {
                continue;
            }

            if let Some(mut auras) = active_auras {
                // Refresh the existing totem buff (match on type + stable name)
                // or push a fresh one — mirrors slow_zone_system's refresh.
                if let Some(existing) = auras.auras.iter_mut().find(|a| {
                    a.effect_type == aura_type && a.ability_name == buff_name
                }) {
                    existing.duration = TOTEM_BUFF_REFRESH_WINDOW;
                } else {
                    auras.auras.push(make_totem_aura(
                        aura_type, magnitude, owner, spell_school, buff_name,
                    ));
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!(
                            "[TOTEM] {} buffs Team {} {}",
                            buff_name, ally.team, ally.class.name()
                        ),
                    );
                }
            } else {
                // Ally has no ActiveAuras yet — add the component with the buff.
                commands.entity(ally_entity).try_insert(ActiveAuras {
                    auras: vec![make_totem_aura(
                        aura_type, magnitude, owner, spell_school, buff_name,
                    )],
                });
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "[TOTEM] {} buffs Team {} {}",
                        buff_name, ally.team, ally.class.name()
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod totem_lifecycle_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use super::super::abilities::SpellSchool;
    use super::super::constants::TOTEM_DURATION;

    fn fire(owner: Entity, duration: f32) -> Totem {
        Totem {
            owner_team: 1,
            owner,
            element: TotemElement::Fire,
            radius: 10.0,
            duration_remaining: duration,
            aura_type: AuraType::SpellPowerIncrease,
            magnitude: 18.0,
            spell_school: SpellSchool::Fire,
        }
    }

    fn water(owner: Entity, duration: f32) -> Totem {
        Totem {
            owner_team: 1,
            owner,
            element: TotemElement::Water,
            radius: 10.0,
            duration_remaining: duration,
            aura_type: AuraType::HealingOverTime,
            magnitude: 8.0,
            spell_school: SpellSchool::Nature,
        }
    }

    fn new_world() -> World {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(Time::<()>::default());
        world
    }

    /// AE2 (covers R12): recasting the SAME element replaces the old totem (only
    /// the greater-duration one survives), while a totem of a DIFFERENT element
    /// is untouched. Models a Fire recast (the new full-duration totem alongside
    /// the expiring old one) with an independent Water totem; one pulse of
    /// `totem_pulse_system` must despawn the old Fire and keep both the new Fire
    /// and the Water.
    #[test]
    fn recast_replaces_same_element_other_element_untouched() {
        let mut world = new_world();
        let owner = world.spawn_empty().id();

        // Old Fire (being replaced — partially elapsed) and the freshly recast
        // Fire (full duration). Same (owner, element) → dedup keeps the newer.
        let fire_old = world.spawn((fire(owner, 12.0), Transform::default())).id();
        let fire_new = world.spawn((fire(owner, TOTEM_DURATION), Transform::default())).id();
        // A different element slot — must NOT be affected by the Fire recast.
        let water_totem = world.spawn((water(owner, TOTEM_DURATION), Transform::default())).id();

        world.run_system_once(totem_pulse_system).expect("totem_pulse_system ran");

        let live: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Totem>>();
            q.iter(&world).collect()
        };

        assert!(
            !live.contains(&fire_old),
            "the replaced (lower-duration) Fire totem must be despawned on recast"
        );
        assert!(
            live.contains(&fire_new),
            "the freshly recast (full-duration) Fire totem must survive"
        );
        assert!(
            live.contains(&water_totem),
            "the Water totem (different element slot) must be untouched by a Fire recast"
        );
        // Exactly one Fire + one Water remain (no stacking).
        assert_eq!(live.len(), 2, "expected exactly one Fire + one Water totem live");
    }

    /// AE5 (covers R13): a Totem is NOT a Combatant — it carries only a `Totem`
    /// (+ `Transform`) and never a `Combatant`. Target acquisition iterates the
    /// `Combatant` set, so a totem can never enter the targetable set: enemies
    /// cannot target, damage, or destroy it. Asserted structurally — the totem
    /// is absent from a `With<Combatant>` query and present in `With<Totem>`.
    #[test]
    fn totem_is_not_a_combatant_and_thus_untargetable() {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let totem = world.spawn((fire(owner, TOTEM_DURATION), Transform::default())).id();

        let combatants: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Combatant>>();
            q.iter(&world).collect()
        };
        assert!(
            !combatants.contains(&totem),
            "a Totem must not satisfy a Combatant query — it would become targetable"
        );

        let totems: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Totem>>();
            q.iter(&world).collect()
        };
        assert_eq!(
            totems,
            vec![totem],
            "the spawned totem must be the sole entity carrying a Totem component"
        );
    }
}
