//! A landed heal's floating number: the one rule for whether it floats, and
//! the one place a heal number is built.
//!
//! **The rule.** A heal floats the amount it landed, rounded as the combat
//! log prints it, in the heal green. A heal whose landed amount rounds to
//! nothing — its target was at full health, so all of it was overheal —
//! floats no number at all, and takes no slot of its unit's floating-text
//! pattern: a "+0" reads as noise. The combat log is not this module's: each
//! site logs its heal as it always has.
//!
//! Every heal number is spawned through [`spawn_heal_number`] (or, for a site
//! that has already taken its pattern slot, [`spawn_heal_number_in_slot`]):
//! - a direct heal: `process_casting`, the instant area heals in
//!   `instant_landings`, and Holy Shock's heal (`process_holy_shock_heals`);
//! - a heal over time tick: `process_hot_ticks`, Healing Stream Totem's
//!   pulses and an on-use item's heal included;
//! - a bloom: `process_blooms`;
//! - a leech heal: `combat_core::leech::land`, Drain Life included;
//! - a killing-blow save's heal: `consume_killing_blow_signals`.
//!
//! `tests/heal_numbers.rs` holds every site to the rule, and fails on a heal
//! number built anywhere else.
//!
//! Like every floating number, a heal number is spawned in both modes and
//! drawn only in graphical mode; nothing in the sim reads it.

use bevy::prelude::*;

use super::components::{FloatingCombatText, FloatingTextState, PlayMatchEntity};
use super::utils::get_next_fct_offset;
use super::FCT_HEIGHT;

/// How long a heal number floats, in seconds.
pub const HEAL_NUMBER_SECS: f32 = 1.5;

/// Where a heal's number floats: its recipient's position, and the
/// recipient's floating-text pattern (`None` for a unit without one, whose
/// number floats unoffset).
pub struct HealNumberAt<'a> {
    pub position: Vec3,
    pub pattern: Option<&'a mut FloatingTextState>,
}

/// The text a heal that landed `landed` floats, or `None` when it floats no
/// number: the rule, in one place. The amount is rounded as the combat log
/// rounds it, so the number is the log's.
pub fn heal_number_text(landed: f32) -> Option<String> {
    let text = format!("+{landed:.0}");
    (text != "+0" && text != "+-0").then_some(text)
}

/// Float the number of a heal that landed `landed` over the unit `at` names,
/// in the next slot of its pattern. A heal that floats no number
/// ([`heal_number_text`]) spawns nothing and takes no slot. Returns whether a
/// number floats.
pub fn spawn_heal_number(
    commands: &mut Commands,
    at: HealNumberAt,
    landed: f32,
    is_crit: bool,
) -> bool {
    let Some(text) = heal_number_text(landed) else {
        return false;
    };
    let (dx, dy) = at.pattern.map_or((0.0, 0.0), get_next_fct_offset);
    spawn(
        commands,
        at.position + Vec3::new(dx, FCT_HEIGHT + dy, 0.0),
        dy,
        text,
        is_crit,
        HEAL_NUMBER_SECS,
    );
    true
}

/// Float a heal's number at `world_position`, a slot its site has already
/// taken (a killing-blow save's heal, whose "Saved" stacks over it), for
/// `lifetime` seconds. A heal that floats no number spawns nothing. Returns
/// whether a number floats.
pub fn spawn_heal_number_in_slot(
    commands: &mut Commands,
    world_position: Vec3,
    vertical_offset: f32,
    landed: f32,
    is_crit: bool,
    lifetime: f32,
) -> bool {
    let Some(text) = heal_number_text(landed) else {
        return false;
    };
    spawn(
        commands,
        world_position,
        vertical_offset,
        text,
        is_crit,
        lifetime,
    );
    true
}

fn spawn(
    commands: &mut Commands,
    world_position: Vec3,
    vertical_offset: f32,
    text: String,
    is_crit: bool,
    lifetime: f32,
) {
    commands.spawn((
        FloatingCombatText {
            world_position,
            text,
            color: crate::ui::theme::fct::HEAL,
            lifetime,
            vertical_offset,
            is_crit,
        },
        PlayMatchEntity,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;

    #[test]
    fn a_heal_that_rounds_to_nothing_floats_no_number() {
        assert_eq!(heal_number_text(0.0), None);
        assert_eq!(heal_number_text(0.3), None);
        assert_eq!(heal_number_text(1.0).as_deref(), Some("+1"));
        assert_eq!(heal_number_text(126.4).as_deref(), Some("+126"));
    }

    /// The pattern slot `spawn_heal_number` leaves next, and the numbers it
    /// spawned, for a heal that landed `landed` on a unit whose next slot is 1.
    fn spawned(landed: f32) -> (u8, Vec<String>) {
        let mut world = World::new();
        let mut queue = CommandQueue::default();
        let mut pattern = FloatingTextState {
            next_pattern_index: 1,
        };
        let mut commands = Commands::new(&mut queue, &world);
        spawn_heal_number(
            &mut commands,
            HealNumberAt {
                position: Vec3::ZERO,
                pattern: Some(&mut pattern),
            },
            landed,
            false,
        );
        queue.apply(&mut world);
        let texts = world
            .query::<&FloatingCombatText>()
            .iter(&world)
            .map(|t| t.text.clone())
            .collect();
        (pattern.next_pattern_index, texts)
    }

    /// A heal that floats no number takes no slot of its unit's pattern, so
    /// the next number over that unit stands where it would have.
    #[test]
    fn a_heal_that_floats_no_number_takes_no_slot() {
        assert_eq!(spawned(0.2), (1, vec![]));
        // Control: a heal that lands something takes the slot.
        assert_eq!(spawned(5.0), (2, vec!["+5".to_string()]));
    }
}
