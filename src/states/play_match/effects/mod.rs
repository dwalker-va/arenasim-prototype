//! Ability Effect Processing Systems
//!
//! This module contains systems for processing pending ability effects.
//! Unlike auras (which have duration and tick over time), these are
//! one-shot effects that execute immediately after being triggered.
//!
//! ## Pattern
//!
//! Abilities spawn `*Pending` components to queue effects. These systems
//! process those pending components and apply the actual game effects.

pub mod holy_shock;
pub mod dispels;
pub mod divine_shield;
pub mod berserker_rage;
pub mod backlash;
pub mod mana_burn;

pub use holy_shock::{process_holy_shock_damage, process_holy_shock_heals};
pub use dispels::process_dispels;
pub use mana_burn::process_mana_burn;
pub use divine_shield::process_divine_shield;
pub use berserker_rage::process_berserker_rage;
pub use backlash::*;
