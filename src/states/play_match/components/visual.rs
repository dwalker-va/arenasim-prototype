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

/// Visual effect for a successful dispel — a twisting ribbon that spirals up off
/// the dispelled combatant's head and fades. Distinct from `DispelBurst` (the
/// expanding sphere, still used by Concussive Shot and Master's Call): the ribbon's
/// unique silhouette + upward rise make it unmistakable as a cleanse and draw the
/// eye to *which* combatant lost a buff. Spawned only on a successful dispel.
#[derive(Component)]
pub struct DispelRibbon {
    /// The entity that was dispelled (ribbon anchors above this target's head)
    pub target: Entity,
    /// The class of the dispeller (affects color: Priest = white/silver, Paladin = golden)
    pub caster_class: CharacterClass,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade/rise progress
    pub initial_lifetime: f32,
    /// Spin accumulator (seconds) driving the ribbon's slow Y-axis rotation
    pub spin: f32,
}

/// Visual effect for Psychic Scream — a self-centered expanding shadow burst
/// around the caster that conveys the AoE fear radius. Spawned on cast, expands
/// outward to roughly the scream radius and fades over its lifetime. Distinct
/// from `DispelBurst`: centered on the caster (not a dispelled target), larger
/// terminal scale, and shadow-violet to read as the Shadow-school AoE fear.
#[derive(Component)]
pub struct ScreamBurst {
    /// The caster — the burst follows this entity for its short life.
    pub caster: Entity,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the expand/fade curve.
    pub initial_lifetime: f32,
}

/// Flashy impact burst for Death Coil — a bright skull-green pop on the *target*
/// when the coil lands. Death Coil is often used as a point-blank self-peel
/// against melee (near-zero projectile travel), so the traveling sphere reads as
/// too subtle; this burst pops on the victim and is visible regardless of range.
/// Follows the target for its short life, starts as an intense flash, then
/// expands and fades. Distinct from `ScreamBurst` (caster-centered, violet) and
/// `DispelBurst` (small): target-centered, vivid green, with a hot initial flash.
#[derive(Component)]
pub struct DeathCoilBurst {
    /// The struck target — the burst follows this entity for its short life.
    pub target: Entity,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the expand/fade curve.
    pub initial_lifetime: f32,
}

/// Visual effect for Berserker Rage activation — the TBC-style flat black
/// "angry face" mask that flashes at the Warrior's head. Billboarded to the
/// camera, pops in with a scale overshoot, holds, then collapses. Spawned as a
/// bare marker by `process_berserker_rage` (headless-safe); the graphical
/// systems attach the textured quad and spawn the companion [`BerserkGlow`].
#[derive(Component)]
pub struct BerserkMask {
    /// The Warrior — the mask follows this entity's head for its short life.
    pub caster: Entity,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the pop/hold/collapse curve.
    pub initial_lifetime: f32,
}

/// Companion effect to [`BerserkMask`] — the hot red-orange emissive glow
/// behind the mask. Separate top-level entity (not a child) so both pieces use
/// the same flat follow-the-caster idiom as every other effect here. Spawned
/// by the graphical mask-spawn system only, never by combat code.
#[derive(Component)]
pub struct BerserkGlow {
    /// The Warrior — the glow follows this entity's head for its short life.
    pub caster: Entity,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the pulse/fade curve.
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

/// Affliction family for DoT drip indicators. The drip color is game
/// language, not per-ability decoration: green = poison, red = bleed.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DripKind {
    /// Green drips — Serpent Sting today; future rogue poisons join the table.
    Poison,
    /// Red drips — Rend today; future Rupture/Garrote join the table.
    Bleed,
}

/// Continuous drip emitter attached (logically) to a combatant carrying a
/// mapped DoT. One emitter per (target, kind); spawns falling `DotDrip`
/// particles on an interval until the mapped aura is gone.
#[derive(Component)]
pub struct DotDripEmitter {
    /// The afflicted combatant the drips fall from.
    pub target: Entity,
    /// Which affliction family (and therefore color) this emitter renders.
    pub kind: DripKind,
    /// Seconds accumulated toward the next drip spawn.
    pub spawn_accumulator: f32,
    /// Count of drips spawned — doubles as the jitter seed.
    pub drips_spawned: u32,
}

/// One falling drop spawned by a `DotDripEmitter`. Mirrors `FlameParticle`:
/// constant velocity, shrink over lifetime, despawn at zero.
#[derive(Component)]
pub struct DotDrip {
    /// Which affliction family — picks the drop color at visual-spawn time.
    pub kind: DripKind,
    /// Velocity vector (primarily downward).
    pub velocity: Vec3,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for shrink calculation.
    pub initial_lifetime: f32,
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

/// Drives a subtle vertical bob on combatant/pet capsules while they are moving.
/// `phase` advances by horizontal distance traveled, so slowed units bob slowly
/// and stationary units do not bob at all.
///
/// Lives on the SIM entity (it needs that entity's post-movement XZ), but the bob
/// it drives is written to the [`VisualBody`] child — see that type for why.
#[derive(Component)]
pub struct WalkAnim {
    pub phase: f32,
    pub previous_xz: Vec2,
    /// Seconds since the sim last moved this unit. The sim steps positions in
    /// FixedUpdate, so at render rates above the tick rate every other frame
    /// sees zero movement — treating those frames as "idle" snapped the bob
    /// offset to rest and back every frame, strobing the body and anything
    /// attached to it. Idle is declared only after this exceeds a real pause
    /// (~0.1s), so the bob holds its height between ticks.
    pub idle_time: f32,
}

/// The rendered body of a combatant or pet: a CHILD entity carrying `Mesh3d`,
/// `MeshMaterial3d` and [`OriginalMesh`], with a local `Transform` relative to
/// its parent.
///
/// **This exists to keep graphical animation out of the simulation's state.**
/// Gameplay range checks use `Vec3::distance`, which includes `y`. The walk bob,
/// the death sink and the victory bounce all used to write `translation.y`
/// directly on the combatant entity, so a ±0.10 visual bob perturbed real range
/// checks and a seed stopped reproducing between the client and headless (see
/// `design-docs/2026-08-01-nagrand-camp-handoff.md` §3.3). Those animations now
/// write this child's LOCAL transform, which nothing in the simulation reads.
///
/// The parent's `Transform` is therefore the unit's logical position, written
/// only by simulation systems. Keep it that way: a graphical system that needs
/// to move a unit visually should move its `VisualBody`, never its parent.
///
/// `rest_y` is the child's neutral local height — the offset that makes the mesh
/// sit correctly on the ground given wherever the sim puts the parent. It is not
/// always 0: pets spawn at the `y` headless uses (0.75) so the two modes agree,
/// while the pet capsule is tuned to render lower.
#[derive(Component)]
pub struct VisualBody {
    pub rest_y: f32,
}

/// Which weapon model a [`WeaponSocket`] holds. Decides the glTF asset, the
/// mount pose, and the swing arc. Class-keyed for v1 (see the attack-animations
/// plan KTD6); an equipment-keyed lookup can replace the mapping later without
/// touching the animation layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeaponKind {
    TwoHandAxe,
    Dagger,
    Bow,
    Mace,
    Shield,
}

/// Which hand position a [`WeaponSocket`] occupies. The Paladin's shield is
/// held statically; the Rogue's daggers alternate hands cosmetically — the
/// sim has a single attack timer, so each landed auto swings whichever dagger
/// is flagged `winds_up_next`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeaponHand {
    Main,
    Off,
}

/// A weapon held by a combatant: a child of the [`VisualBody`] carrying a glTF
/// `SceneRoot`. Purely graphical — spawned only by the graphical
/// `spawn_combatant` path, so headless never sees one.
///
/// The swing animation writes this entity's LOCAL `Transform` every frame
/// (never the sim parent's — see [`VisualBody`]). `rest` is the mount pose the
/// weapon returns to between swings; `release_t` is the seconds elapsed in the
/// current release stroke (`None` when no release is playing), set by the
/// swing-signal consumer when an auto-attack actually lands; `aim` is the
/// world-space point the current/last swing was aimed at, captured at the hit
/// frame so a dead or despawned target cannot orphan the stroke.
#[derive(Component)]
pub struct WeaponSocket {
    pub kind: WeaponKind,
    pub hand: WeaponHand,
    /// The sim combatant holding this weapon (NOT the `VisualBody` parent).
    pub owner: Entity,
    pub rest: Transform,
    pub release_t: Option<f32>,
    pub aim: Vec3,
    /// True when THIS socket telegraphs and plays the owner's next swing.
    /// Main hand at spawn; for dual daggers the signal consumer flips it
    /// between hands after each landed auto so the pair alternates. Always
    /// false for the shield.
    pub winds_up_next: bool,
    /// Smoothed aim correction, as a yaw angle LOCAL to the owner's facing
    /// (radians). The weapon is rigid to the body — when the body turns, the
    /// weapon turns with it instantly — and this angle eases toward the
    /// target bearing at a bounded rate. Smoothing in world space instead
    /// made the compensation sweep the weapon around the body every time the
    /// parent's tick-quantized facing snapped, which read as flashing while
    /// units moved.
    pub yaw_local: f32,
    /// Smoothed windup parameter (0 to -1). The raw value is discontinuous
    /// while chasing: an overdue attack timer pins it at full windup the
    /// moment the target enters reach and drops it to rest the moment it
    /// leaves, which strobes the pose every few frames during pursuit.
    /// Easing at a bounded rate turns that into a deliberate raise/lower.
    pub windup_s: f32,
}

/// One landed auto-attack, spawned in core at the damage-APPLY site (mirrors
/// [`FloatingCombatText`] / [`WindfuryTornado`]): a bare marker entity, inert in
/// headless, consumed and despawned by the graphical swing systems in
/// `rendering/effects.rs` (registered only in `states/mod.rs`). Spawned in the
/// apply loop rather than the queue loop so an attack dropped by the
/// friendly-CC guard or a same-frame death never telegraphs a phantom release.
#[derive(Component)]
pub struct AutoAttackSwing {
    pub attacker: Entity,
    pub target: Entity,
    /// True for ranged autos (Hunter Auto Shot AND caster Wand Shots). The
    /// consumer additionally gates the cosmetic arrow on the attacker holding a
    /// Bow-kind main hand, so wand shots and socketless attackers no-op.
    pub ranged: bool,
}

/// A purely cosmetic arrow for Hunter Auto Shot. Damage already landed
/// (hit-scan) when this spawns; the arrow just flies the visual. Never touches
/// the sim `Projectile` machinery — spawn/move/cleanup live in
/// `rendering/effects.rs`, registered only in `states/mod.rs`.
#[derive(Component)]
pub struct CosmeticArrow {
    /// World-space destination, captured at the hit frame.
    pub to: Vec3,
    /// Yards per second of cosmetic travel.
    pub speed: f32,
    /// Seconds remaining before a hard despawn (backstop if it never arrives).
    pub ttl: f32,
}

/// Marker component for the player's selection ring — a translucent torus
/// laid flat at the selected combatant's feet. One ring exists at most.
#[derive(Component)]
pub struct SelectionRing {
    /// The combatant entity this ring follows.
    pub target: Entity,
}

/// Transient Windfury Totem proc effect: a spinning wind funnel ("tornado") that
/// swirls up around a melee ally the instant it lands a Windfury bonus swing.
/// Spawned in core at the proc site (like FloatingCombatText); the
/// spawn/update/cleanup systems live in `rendering/effects.rs` and are
/// registered ONLY in `states/mod.rs`, so headless never builds the mesh.
#[derive(Component)]
pub struct WindfuryTornado {
    /// The combatant the funnel swirls around (followed each frame).
    pub target: Entity,
    /// Seconds remaining before despawn.
    pub lifetime: f32,
    /// Initial lifetime, for fade/grow progress.
    pub initial_lifetime: f32,
    /// Spin accumulator (seconds) driving the fast Y-axis rotation.
    pub spin: f32,
}
