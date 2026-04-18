use bevy::prelude::*;
use bevy_egui::egui;
use super::super::abilities::SpellSchool;
use super::super::match_config::CharacterClass;

// ============================================================================
// Visual Effect Components
// ============================================================================

/// Floating combat text component for damage/healing numbers.
/// These appear above combatants and float upward before fading out.
#[derive(Component)]
pub struct FloatingCombatText {
    /// World position where the text is anchored
    pub world_position: Vec3,
    /// The text to display (damage/healing amount)
    pub text: String,
    /// Color of the text (white for auto-attacks, yellow for abilities, green for healing)
    pub color: egui::Color32,
    /// Time remaining before text disappears (in seconds)
    pub lifetime: f32,
    /// Vertical offset accumulated over time (makes text float upward)
    pub vertical_offset: f32,
    /// Whether this was a critical strike (renders larger with "!" suffix)
    pub is_crit: bool,
}

/// Visual effect for spell impacts (Mind Blast, etc.)
/// Displays as an expanding sphere that fades out
#[derive(Component)]
pub struct SpellImpactEffect {
    /// World position where the effect should appear
    pub position: Vec3,
    /// Time remaining before effect disappears (in seconds)
    pub lifetime: f32,
    /// Initial lifetime for calculating fade/scale
    pub initial_lifetime: f32,
    /// Initial scale of the sphere
    pub initial_scale: f32,
    /// Final scale of the sphere (expands to this)
    pub final_scale: f32,
}

/// Component for tracking death fall animation.
/// When a combatant dies, this component is added to animate them falling over.
#[derive(Component)]
pub struct DeathAnimation {
    /// Animation progress (0.0 = start, 1.0 = complete)
    pub progress: f32,
    /// Fall direction (normalized, in XZ plane)
    pub fall_direction: Vec3,
}

impl DeathAnimation {
    /// Duration of the death fall animation in seconds
    pub const DURATION: f32 = 0.6;

    pub fn new(fall_direction: Vec3) -> Self {
        Self {
            progress: 0.0,
            fall_direction: fall_direction.normalize(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }
}

/// Component for shield bubble visual effects.
/// Attached to a sphere entity that visually represents an absorb shield around a combatant.
#[derive(Component)]
pub struct ShieldBubble {
    /// The combatant entity this bubble belongs to
    pub combatant: Entity,
    /// The spell school of the shield (affects color: Frost = blue, Holy = gold)
    pub spell_school: SpellSchool,
    /// Whether this is a damage immunity bubble (Divine Shield) vs absorb shield
    /// Immunity bubbles are larger, brighter gold, and have a pulse animation.
    pub is_immunity: bool,
}

/// Component that stores the original mesh handle for a combatant.
/// Used to restore the mesh when polymorph ends.
#[derive(Component)]
pub struct OriginalMesh(pub Handle<Mesh>);

/// Marker component indicating the combatant is currently polymorphed.
/// Used to track mesh swapping state.
#[derive(Component)]
pub struct PolymorphedVisual;

/// A rising flame particle for fire spell effects (e.g., Immolate).
/// Spawned at target location, rises upward while shrinking and fading.
#[derive(Component)]
pub struct FlameParticle {
    /// Velocity vector (primarily upward with slight horizontal drift)
    pub velocity: Vec3,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade/shrink calculation
    pub initial_lifetime: f32,
}

/// Drain Life beam effect connecting caster to target.
/// Created when a Drain Life channel starts, despawned when it ends.
#[derive(Component)]
pub struct DrainLifeBeam {
    /// The caster entity channeling Drain Life
    pub caster: Entity,
    /// The target entity being drained
    pub target: Entity,
    /// Timer for spawning particles along the beam
    pub particle_spawn_timer: f32,
}

/// A particle flowing along the Drain Life beam from target to caster.
#[derive(Component)]
pub struct DrainParticle {
    /// Progress along beam: 0.0 = at target, 1.0 = at caster
    pub progress: f32,
    /// Movement speed (progress units per second)
    pub speed: f32,
    /// Reference to the beam this particle belongs to
    pub beam: Entity,
}

/// Visual effect for healing spells - a translucent column of light at the target.
/// Spawned when a healing spell lands, fades over its lifetime.
#[derive(Component)]
pub struct HealingLightColumn {
    /// The entity being healed (column follows this target)
    pub target: Entity,
    /// The class of the healer (affects color: Priest = white-gold, Paladin = golden)
    pub healer_class: CharacterClass,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}

/// Visual effect for dispel spells - an expanding sphere burst at the target.
/// Spawned when a dispel successfully removes an aura, expands and fades over its lifetime.
#[derive(Component)]
pub struct DispelBurst {
    /// The entity that was dispelled (burst follows this target)
    pub target: Entity,
    /// The class of the dispeller (affects color: Priest = white/silver, Paladin = golden)
    pub caster_class: CharacterClass,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}

/// Visual effect indicating a combatant has Unstable Affliction active.
/// Pulses at ~0.5Hz (every 2s) in deep violet so it reads independently from
/// Corruption's faster green tendrils when both DoTs are stacked on the target.
#[derive(Component)]
pub struct UnstableAfflictionGlow {
    /// The afflicted target — glow follows this entity until UA expires/dispels.
    pub target: Entity,
    /// Phase accumulator (seconds) used to drive the pulse.
    pub phase: f32,
}

/// Visual effect spawned on the dispeller the frame UA backlash fires.
/// Distinct from `DispelBurst`: ~2x particle count, dark-violet shadow color,
/// snappier 0.3s lifetime — reads as "impact" rather than "sparkle".
#[derive(Component)]
pub struct BacklashBurst {
    /// The dispeller entity that took the backlash.
    pub target: Entity,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for fade calculation.
    pub initial_lifetime: f32,
}
