use bevy::prelude::*;

// ============================================================================
// Pet Types
// ============================================================================

/// Pet type enum (extensible for future demons and hunter pets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PetType {
    Felhunter,
    Spider,
    Boar,
    Bird,
}

impl PetType {
    pub fn name(&self) -> &'static str {
        match self {
            PetType::Felhunter => "Felhunter",
            PetType::Spider => "Spider",
            PetType::Boar => "Boar",
            PetType::Bird => "Bird",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            PetType::Felhunter => Color::srgb(0.4, 0.75, 0.4), // Green (demon)
            PetType::Spider => Color::srgb(0.5, 0.4, 0.3),     // Brown
            PetType::Boar => Color::srgb(0.6, 0.4, 0.3),       // Dark brown
            PetType::Bird => Color::srgb(0.6, 0.7, 0.8),       // Light grey-blue
        }
    }

    pub fn preferred_range(&self) -> f32 {
        match self {
            PetType::Felhunter => 2.0, // Melee
            PetType::Spider => 2.0,    // Melee
            PetType::Boar => 2.0,      // Melee
            PetType::Bird => 2.0,      // Melee
        }
    }

    pub fn movement_speed(&self) -> f32 {
        match self {
            PetType::Felhunter => 5.5,
            PetType::Spider => 5.5,
            PetType::Boar => 6.0, // Slightly faster — aggressive
            PetType::Bird => 5.5,
        }
    }

    pub fn is_melee(&self) -> bool {
        match self {
            PetType::Felhunter => true,
            PetType::Spider => true,
            PetType::Boar => true,
            PetType::Bird => true,
        }
    }

}

/// Marker component for pet entities. Links pet to its owner.
#[derive(Component, Clone)]
pub struct Pet {
    pub owner: Entity,
    pub pet_type: PetType,
}

// ============================================================================
// Hunter Components
// ============================================================================

/// Trap type enum for Hunter traps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapType {
    /// Freezing Trap — incapacitates first enemy contact (breaks on damage)
    Freezing,
    /// Frost Trap — creates a persistent slow zone on trigger
    Frost,
}

impl TrapType {
    pub fn name(&self) -> &'static str {
        match self {
            TrapType::Freezing => "Freezing Trap",
            TrapType::Frost => "Frost Trap",
        }
    }
}

/// Component for Hunter traps placed on the ground.
/// Traps have an arming delay, then trigger on enemy proximity.
#[derive(Component)]
pub struct Trap {
    /// Which type of trap this is
    pub trap_type: TrapType,
    /// Team of the hunter who placed this trap
    pub owner_team: u8,
    /// Entity of the hunter who placed this trap
    pub owner: Entity,
    /// Time remaining before the trap is armed (seconds). 0 = armed.
    pub arm_timer: f32,
    /// Trigger radius — enemies within this distance of the trap trigger it
    pub trigger_radius: f32,
    /// Whether this trap has been triggered (pending despawn)
    pub triggered: bool,
}

/// A trap that has been lobbed and is traveling through the air to its landing position.
/// On arrival, despawns and spawns a regular Trap entity at the landing position.
#[derive(Component)]
pub struct TrapLaunchProjectile {
    pub trap_type: TrapType,
    pub owner_team: u8,
    pub owner: Entity,
    /// Hunter's position at launch (needed for lerp — Transform is mutated each frame)
    pub origin: Vec3,
    /// World-space target position (post-clamp)
    pub landing_position: Vec3,
    /// Precomputed horizontal distance for progress calculation
    pub total_distance: f32,
    /// Accumulated horizontal travel distance
    pub distance_traveled: f32,
}

/// Component for persistent slow zones created by Frost Trap.
/// Enemies inside the zone receive a refreshing movement speed slow.
#[derive(Component)]
pub struct SlowZone {
    /// Team of the hunter who created this zone
    pub owner_team: u8,
    /// Entity of the hunter who created this zone
    pub owner: Entity,
    /// Radius of the slow zone
    pub radius: f32,
    /// Time remaining before the zone expires (seconds)
    pub duration_remaining: f32,
    /// Slow magnitude (movement speed multiplier, e.g., 0.4 = 60% slow)
    pub slow_magnitude: f32,
}

/// Component tracking an active Disengage (Hunter backward leap).
#[derive(Component)]
pub struct DisengagingState {
    /// Direction of the leap (normalized, away from nearest enemy)
    pub direction: Vec3,
    /// Distance remaining to travel
    pub distance_remaining: f32,
}

// ============================================================================
// Hunter Visual Components
// ============================================================================

/// Expanding burst sphere when a trap triggers.
/// Spawned at the trap's position, expands and fades over 0.3s.
#[derive(Component)]
pub struct TrapBurst {
    /// Which type of trap triggered (affects color)
    pub trap_type: TrapType,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for progress calculation
    pub initial_lifetime: f32,
}

/// Translucent ice cuboid around a Freezing Trap target.
/// Follows the target's position and despawns when Incapacitate aura breaks.
#[derive(Component)]
pub struct IceBlockVisual {
    /// The entity frozen inside the ice block
    pub target: Entity,
    /// Skip cleanup check until expired (gives apply_pending_auras time to process)
    pub grace_timer: f32,
}

/// Wind streak trail left behind during Disengage leap.
/// Static position, fades over its lifetime.
#[derive(Component)]
pub struct DisengageTrail {
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}

/// Speed streak trail behind Boar during charge.
/// Static position, fades over its lifetime.
#[derive(Component)]
pub struct ChargeTrail {
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}
