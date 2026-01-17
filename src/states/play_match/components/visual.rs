//! Visual Effect Components
//!
//! This module provides documentation for visual effect components.
//! The actual types are defined in the parent mod.rs for backward compatibility.
//!
//! ## Types
//! - `FloatingTextState`: Tracks FCT spawn patterns per combatant
//! - `FloatingCombatText`: Damage/healing numbers that float up and fade
//! - `SpellImpactEffect`: Expanding sphere effect for spell impacts
//! - `DeathAnimation`: Controls the death fall animation
//! - `ShieldBubble`: Visual sphere around shielded combatants
//! - `SpeechBubble`: Text bubble above combatants
//!
//! ## Usage
//! ```ignore
//! use crate::states::play_match::components::visual::*;
//! // or
//! use crate::states::play_match::components::{FloatingCombatText, ShieldBubble};
//! ```

// This module serves as a namespace/organizational wrapper.
// The actual types are defined in the parent mod.rs and re-exported here.
// This allows gradual refactoring while maintaining backward compatibility.
