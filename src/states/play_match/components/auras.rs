//! Aura and Status Effect Components
//!
//! This module re-exports aura-related types from the parent module
//! to provide a focused import path for aura functionality.
//!
//! ## Types
//! - `AuraType`: Enum of all possible aura effects
//! - `Aura`: Data structure for a single aura instance
//! - `ActiveAuras`: Component tracking all auras on a combatant
//! - `AuraPending`: Temporary component for queued aura applications
//!
//! ## Usage
//! ```ignore
//! use crate::states::play_match::components::auras::*;
//! // or
//! use crate::states::play_match::components::{ActiveAuras, Aura, AuraPending, AuraType};
//! ```

// This module serves as a namespace/organizational wrapper.
// The actual types are defined in the parent mod.rs and re-exported here.
// This allows gradual refactoring while maintaining backward compatibility.
