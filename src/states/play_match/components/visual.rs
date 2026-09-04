use bevy::prelude::*;
use bevy_egui::egui;
use super::super::abilities::{AbilityType, SpellSchool};
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

/// Where on the victim a shared impact plays.
///
/// The Classic client attaches the arrows' impact to chest attachment 34 and
/// Mind Blast's to head attachment 20; the two heights are what separate a
/// body hit from a mind hit at a glance. See `rendering/effects/school_impact.rs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpactAnchor {
    Chest,
    Head,
}

/// A landed ability playing the shared, school-coloured impact on its victim.
///
/// The third generic hook — the receiver-side counterpart of the casting orb
/// and `InstantAbilityFired`. Spawned by combat code at the site where the
/// ability RESOLVES (`process_projectile_hits` for projectiles, the
/// instant-effect landing in `process_casting` for Mind Blast), so it exists in
/// both modes like `BoltImpact`; rendered only in graphical mode. Purely
/// cosmetic: it reads combat state, writes none, and draws no `game_rng`.
#[derive(Component)]
pub struct SchoolImpact {
    /// The victim. The burst TRACKS it, so a target that keeps running carries
    /// its hit.
    pub target: Entity,
    /// What landed. The style is chosen by school, but an ability may
    /// override its school's row (Mana Burn is Shadow without being Mind
    /// Blast) — see `landing_style`.
    pub ability: AbilityType,
    pub school: SpellSchool,
    pub anchor: ImpactAnchor,
    /// Unit vector from the victim back toward where the hit came from.
    /// Debris splashes back along it.
    pub from: Vec3,
    /// Damage dealt (health plus absorbed) as a fraction of the victim's max
    /// health; `0.0` for an aura-only landing. Scales the burst, so a hard hit
    /// reads as a hard hit for every ability at once.
    pub magnitude: f32,
    /// Cosmetic only — never read by sim code.
    pub is_crit: bool,
    pub age: f32,
}

impl SchoolImpact {
    /// Which abilities land through the shared impact, and where — the single
    /// list the two spawn sites and the projectile audit derive from.
    ///
    /// `None` for anything with a bespoke landing (the two bolts' `BoltImpact`,
    /// Death Coil's `DeathCoilBurst`, Lightning Bolt's own burst), for Web —
    /// whose source has no impact kit at all, only the root STATE that
    /// `hard_cc.rs` already draws — and for everything that is not a landing.
    /// Every projectile in `abilities.ron` must reach SOME impact;
    /// `tests/school_impact_visual_probes.rs` checks that against the config.
    pub fn anchor_for(ability: AbilityType) -> Option<ImpactAnchor> {
        match ability {
            AbilityType::AimedShot
            | AbilityType::ArcaneShot
            | AbilityType::ConcussiveShot
            | AbilityType::SerpentSting
            // `holysmite_low_chest.m2` on attachment 34, per the client data.
            | AbilityType::HolyShock
            // `manaburn_chest.m2` on attachment 34 — its own model, so it
            // overrides the Shadow row (see `landing_style`).
            | AbilityType::ManaBurn => Some(ImpactAnchor::Chest),
            AbilityType::MindBlast => Some(ImpactAnchor::Head),
            _ => None,
        }
    }
}

/// What a flat, per-landing piece of a shared impact is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpactRole {
    /// The star flash at the point of contact.
    Flash,
    /// The expanding soft-rimmed band, for schools that have one.
    Ring,
    /// A dark blended mass behind the flash — the only piece that DARKENS.
    Blot,
}

#[derive(Component)]
pub struct ImpactSprite {
    pub role: ImpactRole,
    /// Full-size radius in yards; the growth curves scale around it.
    pub radius: f32,
}

/// One piece of a landing's debris, in the rig's own frame.
#[derive(Component)]
pub struct ImpactMote {
    pub kind: crate::states::play_match::rendering::SprayKind,
    pub velocity: Vec3,
    /// Downward acceleration; negative rises.
    pub gravity: f32,
    pub spin: f32,
    pub age: f32,
    pub life: f32,
    pub radius: f32,
}

/// Graphical-only state a `SchoolImpact` rig carries while it plays.
#[derive(Component)]
pub struct ImpactRig {
    pub mote_mesh: Handle<Mesh>,
    /// Material for a smoulder's emitted motes, when the style has one.
    pub smoulder_material: Option<Handle<StandardMaterial>>,
    /// Fractional motes owed since the last one was emitted.
    pub emit_carry: f32,
    /// How many the smoulder has emitted, seeding their scatter.
    pub emitted: u32,
}

/// Signature Lightning Bolt strike: an instant forked "flash-crack" arc drawn
/// from caster to target at the moment the cast lands.
///
/// Spawned deterministically in the shared casting-completion path (no `game_rng`
/// draw, unlike Immolate's particles), so it is byte-neutral in headless. The
/// graphical-only systems in `rendering/effects/lightning_bolt.rs` consume it,
/// generate the jagged geometry with a visual-only RNG, and animate the flash
/// plus impact burst. `start`/`end` are snapshots taken at cast completion (the
/// strike is instant, so the endpoint is fixed).
#[derive(Component)]
pub struct LightningBoltStrike {
    /// Caster position (bolt start) at cast completion.
    pub start: Vec3,
    /// Target position (bolt end) at cast completion.
    pub end: Vec3,
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

/// The body material a polymorph's wool coat displaced, stored on the
/// [`VisualBody`] child beside [`OriginalMesh`] for the same reason that
/// component exists: nothing else records it, and `update_stealth_visuals`
/// edits the material asset in place, so the handle has to survive the swap.
/// Mirrors `OriginalWeaponMaterial`'s insert-at-swap / remove-at-restore
/// lifecycle.
#[derive(Component)]
pub struct OriginalBodyMaterial(pub Handle<StandardMaterial>);

/// One primitive of a polymorphed combatant's sheep body (head, ear, leg, ...),
/// spawned as a child of the victim's [`VisualBody`] while the aura lasts.
///
/// `owner` is the SIM entity, not the body child: restore despawns only the
/// parts belonging to the unit whose polymorph ended, so two sheep on the field
/// at once cannot strip each other.
#[derive(Component)]
pub struct SheepPart {
    pub owner: Entity,
}

/// Marker component indicating the combatant is currently feared (the terror
/// treatment is applied). Single source of truth for the Fear signature look:
/// the body-tint swap, the breathing shroud, and every exit-path restore key
/// off this marker's presence/absence — never re-derived from `ActiveAuras`.
/// Keyed on `AuraType::Fear`, so Death Coil's horror (a Fear-type aura) inherits
/// the treatment for free. Distinct from [`PolymorphedVisual`]: a unit can be
/// both feared and polymorphed (different DR categories), and the sheep look
/// wins while polymorphed (the fear system carries `Without<PolymorphedVisual>`).
#[derive(Component)]
pub struct FearedVisual;

/// The breathing shadow aura sphere spawned as a child of a feared combatant's
/// [`VisualBody`]. Mirrors [`SheepPart`]'s owner scoping: `owner` is the SIM
/// entity, so restore despawns exactly this unit's shroud and two
/// simultaneously-feared units never strip each other's.
#[derive(Component)]
pub struct FearShroud {
    pub owner: Entity,
}

/// Interval-timer state for the rising fear-mote emitter, attached to a feared
/// unit and gated by [`FearedVisual`]. Collapses the affliction detector +
/// emitter into one system: while the marker holds, motes spawn every
/// `FEAR_MOTE_INTERVAL`. When the marker is removed the unit simply stops being
/// iterated (no new motes), and any in-flight motes finish their own lifetime —
/// no owner-scoped despawn is needed. Mirrors [`DotDripEmitter`]'s
/// accumulator/count fields.
#[derive(Component, Default)]
pub struct FearMoteEmitter {
    /// Seconds accumulated toward the next mote spawn.
    pub spawn_accumulator: f32,
    /// Count of motes spawned — doubles as the visual-only jitter seed.
    pub motes_spawned: u32,
}

/// One rising shadow mote spawned by a [`FearMoteEmitter`]. A transient world
/// particle (NOT owner-scoped, NOT a child): it floats upward and fades over
/// its lifetime, then self-despawns. Mirrors [`DotDrip`] / [`FlameParticle`].
#[derive(Component)]
pub struct FearMote {
    /// Velocity vector (primarily upward with slight horizontal drift).
    pub velocity: Vec3,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the fade calculation.
    pub initial_lifetime: f32,
}

/// A glass-like shard flung from the fear shroud when it shatters on break — a
/// transient, unattached world particle with ballistic motion (gravity) and a
/// tumble, that fades and self-despawns. The dynamic replacement for the break
/// flash: the shroud appears to break apart and fall away.
#[derive(Component)]
pub struct FearShard {
    /// Current velocity (outward + up at spawn; gravity pulls it down each tick).
    pub velocity: Vec3,
    /// Tumble rate about each local axis (radians/sec).
    pub angular_velocity: Vec3,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the fade calculation.
    pub initial_lifetime: f32,
}

/// A brief shadow flash burst spawned at BOTH the Fear apply and the Fear break,
/// mirroring [`TransformPuff`]'s dual-direction role. Kept short-lived (~0.4s):
/// Fear breaks on ANY damage, so an apply and its break can land within a second
/// of each other and must each read as a distinct pop rather than one smear.
/// Grows and fades over its lifetime, then self-despawns.
#[derive(Component)]
pub struct FearFlash {
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the grow/fade curve.
    pub initial_lifetime: f32,
}

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

/// Graphical-only state a [`DispelRibbon`] carries while it plays: the spark
/// sprite and this ribbon's own class-coloured spark material, plus the
/// emitter accumulator for the play-out stream off the fixed top end.
#[derive(Component)]
pub struct DispelRibbonRig {
    pub spark_mesh: Handle<Mesh>,
    pub spark_material: Handle<StandardMaterial>,
    /// Fractional sparks owed since the last one was emitted.
    pub emit_carry: f32,
    /// How many sparks this ribbon has emitted, seeding their scatter.
    pub emitted: u32,
}

/// One spark streaming off a playing-out dispel ribbon's top end. A transient,
/// unattached world particle: rises, shrinks, self-expires.
#[derive(Component)]
pub struct DispelSpark {
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub radius: f32,
}

/// Visual effect for a polymorph transition — a cluster of pale cloud lobes that
/// puffs outward at the victim's torso. Spawned at BOTH the transform-in and the
/// restore, in the same style: the sim cannot distinguish an expiry from a damage
/// break, so one puff covers every direction.
///
/// Static by design — it carries the position it was spawned at rather than
/// following the victim, so it marks the point the transform happened instead of
/// dragging behind a fleeing sheep. Kept short-lived: polymorph breaks on ANY
/// damage, so a rapid apply-break pair must read as two distinct pops, not one
/// smear.
#[derive(Component)]
pub struct TransformPuff {
    /// World position the puff was spawned at (the victim's torso).
    pub position: Vec3,
    /// Time remaining before despawn (seconds).
    pub lifetime: f32,
    /// Initial lifetime for the expand/fade curve.
    pub initial_lifetime: f32,
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
    /// The owner's facing yaw last frame. Large one-frame facing jumps
    /// (gate-open first move, a hard target switch) are absorbed into
    /// `yaw_local` so the weapon holds its world bearing through the snap and
    /// then eases to the new aim, instead of whipping around with the body.
    pub prev_owner_yaw: f32,
    /// Smoothed windup parameter (0 to -1). The raw value is discontinuous
    /// while chasing: an overdue attack timer pins it at full windup the
    /// moment the target enters reach and drops it to rest the moment it
    /// leaves, which strobes the pose every few frames during pursuit.
    /// Easing at a bounded rate turns that into a deliberate raise/lower.
    pub windup_s: f32,
    /// Which named stroke the current release is playing. `Auto` between
    /// strokes and for every ordinary auto-attack; set alongside `release_t`
    /// by `consume_instant_ability_signals` for a signature ability, and reset
    /// to `Auto` when that stroke expires. Selects both the timing profile and
    /// the arc SHAPE in `animate_weapon_swings`.
    pub swing_style: SwingStyle,
    /// The swing parameter this socket last rendered at, published by
    /// `animate_weapon_swings` for `animate_body_lean` to consume.
    ///
    /// The body lean must be driven by the SAME value as the weapon or the two
    /// desync, and recomputing it would mean duplicating the windup
    /// eligibility gates, the easing and the release timing. Written once per
    /// frame by the swing animation; read-only everywhere else.
    pub last_s: f32,
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

/// Which named stroke a [`WeaponSocket`]'s current release is playing.
///
/// `Auto` is the ordinary auto-attack. A signature ability adds one variant
/// here plus one arm in `swing_style_for_ability` / [`SwingStyle::profile`]
/// (`rendering/effects/weapon_swing.rs`), instead of scattering new consts
/// through that file or widening `swing_param`'s call sites again.
///
/// One-shot: `animate_weapon_swings` resets the socket to `Auto` the frame its
/// release stroke expires, and `consume_swing_signals` clears it on any
/// ordinary auto — so a styled stroke can never leak into the next swing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SwingStyle {
    #[default]
    Auto,
    /// Warrior Mortal Strike: a rising diagonal that reverses the auto-attack's
    /// raise-and-chop — the blade drops low and behind, then rips up and across
    /// the body. Matches the ability's WoW animation ("bottom left to top
    /// right") and is a different arc PLANE, not a heavier version of the same
    /// swing.
    MortalStrike,
    /// Rogue Cheap Shot: a fast, shallow swing. The source plays plain
    /// `Attack1H` over 634ms — genuinely generic, and half the length of Kidney
    /// Shot's lunge. Its job is to be QUICK, which is the whole contrast with
    /// the finisher.
    CheapShot,
    /// Rogue Kidney Shot: a deep lunging thrust. The source plays
    /// `Attack1HPierce` over 1233ms — twice Cheap Shot's length, and a PIERCE
    /// rather than a swing. That shape difference, plus its unique magenta cast
    /// model, is the whole of what separates the two rogue stuns; they are
    /// byte-identical on the receiver side.
    KidneyShot,
    /// Paladin Hammer of Justice: an UPPERCUT. The mace drops low, then drives
    /// vertically up as the seal lands on the victim.
    ///
    /// Nearly sagittal, where Mortal Strike's signature is a 49-degree diagonal
    /// — the two must not be mistaken for each other, and a rise is the natural
    /// reading of a hammer of judgement being brought up.
    HammerOfJustice,
}

/// One instant ability performed by a caster, spawned by combat code at that
/// ability's own resolution site and consumed by the graphical gesture router
/// (`consume_instant_ability_signals`, `rendering/effects/instant_ability.rs`,
/// registered only in `states/mod.rs`).
///
/// This is the caster-side counterpart to `CastingState` -> casting orb: a hard
/// cast telegraphs itself for its whole duration, an instant does not, so an
/// instant that wants an actor-side animation states it here. Mirrors
/// [`AutoAttackSwing`] and [`CastEnding`]: a bare marker entity, spawned
/// unconditionally in BOTH modes (headless spawns it and never reads it) per
/// `cosmetic-marker-cross-mode-spawn-parity.md`.
///
/// Deliberately ability-AGNOSTIC: core never learns which instants have a
/// signature. The graphical router decides, so a new signature costs one match
/// arm there and touches no combat code.
///
/// **Each spawn site owns its own gate and documents it.** The
/// `QueuedInstantAttack` drain spawns only inside the landed-hit `is_alive`
/// gate, like `AutoAttackSwing`, so a same-frame death never telegraphs a
/// phantom strike. The class-AI sites spawn on the committed-use branch,
/// because the caster performed the gesture whether or not every aura stuck.
#[derive(Component)]
pub struct InstantAbilityFired {
    pub caster: Entity,
    /// The single unit the gesture is aimed at, or `None` for a caster-centred
    /// effect. An AoE has no one target, and picking one out of the victim list
    /// would be an arbitrary lie that the geometry would then be anchored on.
    pub target: Option<Entity>,
    pub ability: AbilityType,
    /// Cosmetic only — scales the flourish. Never read by sim code. `false` for
    /// aura-only abilities, which roll no crit.
    pub is_crit: bool,
}

impl InstantAbilityFired {
    /// Every ability whose combat path spawns this marker — the single list.
    ///
    /// The animation sandbox runs neither the class AIs nor the
    /// `QueuedInstantAttack` drain, so it must spawn the marker itself for
    /// exactly this set. Deriving the sandbox's behaviour from this one
    /// predicate is what stops the two drifting.
    ///
    /// TWO audits guard it, because they catch different mistakes.
    /// `animation_sandbox/playback.rs` asserts every ability listed here
    /// classifies as `EntryFamily::Residue`, so a LISTED ability always
    /// previews. `tests/instant_ability_audit.rs` scans the source for real
    /// spawn sites and checks the list against them, so an ability given a
    /// spawn site but forgotten HERE fails too — which the family check alone
    /// cannot see, because a `commands.spawn` in class AI is invisible to it.
    pub fn is_spawned_for(ability: AbilityType) -> bool {
        use AbilityType::*;
        matches!(
            ability,
            // Resolved through the `QueuedInstantAttack` drain in combat_ai.rs.
            MortalStrike | Ambush | SinisterStrike
            // Instant AND aura-only: applied inline in class AI, entering
            // neither generic caster hook (A2).
            | CheapShot | KidneyShot | HammerOfJustice | FrostNova
        )
    }

    /// Whether this ability's gesture is anchored on the caster rather than a
    /// victim — the `target: None` cases.
    pub fn is_caster_centred(ability: AbilityType) -> bool {
        matches!(ability, AbilityType::FrostNova)
    }
}

/// One heal that a [`AuraType::HealingReduction`] debuff cut down, spawned in
/// core at each of the three sites that already apply the reduction: healing
/// another target and the self-heal path (`combat_core/casting.rs`) and Holy
/// Shock (`effects/holy_shock.rs`). Consumed by the graphical
/// `spawn_heal_fracture` (`rendering/effects/mortal_wounds.rs`).
///
/// This is how Mortal Wounds is shown: the debuff has no body treatment at
/// rest, and states itself at the moment it costs someone something — the
/// incoming heal column visibly sheds the share it refused. Keyed on the aura
/// TYPE at the reduction site, so Hunter's Aimed Shot (identical 10s/0.65
/// debuff) gets the same treatment with no Hunter-side code.
#[derive(Component)]
pub struct HealingRefused {
    /// Who was being healed.
    pub target: Entity,
    /// Fraction of the heal the debuff refused, in `0..1` (0.35 for a single
    /// Mortal Strike). Scales the ash so a bigger cut sheds more.
    pub refused_fraction: f32,
}

/// How a hard cast or channel ended, for the casting-orb ending animation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastEndingKind {
    /// The cast completed and its effect actually landed (mana was charged).
    Landed,
    /// The cast reached completion but fizzled — target dead, despawned, or
    /// out of line of sight at the resolution gates (no mana charged).
    Fizzled,
    /// The cast or channel was cut short: ability interrupt (Pummel/Kick),
    /// crowd control (stun/fear/polymorph), or Silence.
    Interrupted,
}

/// One cast/channel ending, spawned in core at the resolution site (mirrors
/// [`AutoAttackSwing`]): a bare marker entity, inert in headless, consumed and
/// despawned by the graphical casting-orb systems in `rendering/effects.rs`
/// (registered only in `states/mod.rs`). Spawned at the OUTCOME site rather
/// than inferred from `CastingState` removal because pass 1 of
/// `process_casting` removes the component before pass 2 decides landed vs
/// fizzled — the two endings are indistinguishable from component state alone.
/// Caster death and match end deliberately spawn NO marker (silent vanish —
/// the death/celebration animation owns that moment).
#[derive(Component)]
pub struct CastEnding {
    pub caster: Entity,
    pub kind: CastEndingKind,
}

/// Lifecycle phase of a [`CastingOrb`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastingOrbPhase {
    /// Hard cast in progress — the orb grows with cast progress.
    Growing,
    /// Channel in progress — the orb holds at full intensity.
    Holding,
    /// Ending: shrink/dissipate after an interrupt or fizzle.
    Sputter,
    /// Ending: brief release pulse after a landed completion.
    Flash,
}

/// The gathering-orb casting animation: one free-standing world-space entity
/// per casting/channeling combatant (drain-life-beam follow pattern — NOT a
/// `VisualBody` child), colored via [`AbilityConfig::cast_color`]. Growing and
/// Holding read live cast state; the Sputter/Flash endings are driven by
/// consumed [`CastEnding`] markers, and a state-gone-with-no-marker caster
/// (death, match end, natural channel end) despawns the orb silently.
///
/// [`AbilityConfig::cast_color`]: crate::states::play_match::ability_config::AbilityConfig::cast_color
#[derive(Component)]
pub struct CastingOrb {
    /// The combatant this orb hovers in front of.
    pub caster: Entity,
    /// 0..1 growth captured continuously; an ending animates from this value.
    pub intensity: f32,
    pub phase: CastingOrbPhase,
    /// Seconds remaining in the current ending phase (Sputter/Flash only).
    pub ending_remaining: f32,
    /// Countdown to the next mote spawn.
    pub mote_spawn_timer: f32,
    /// Monotonic mote counter — drives the deterministic golden-angle spread
    /// of mote start offsets (no RNG: visual code never touches `game_rng`).
    pub mote_index: u32,
    /// Total cast duration captured at spawn, so growth tracks the LIVE cast
    /// time incl. CastTimeIncrease auras, not the base config value.
    pub cast_total: f32,
}

/// One mote streaming into its parent orb's focus point. Travels a straight
/// lerp from a deterministic start offset to the orb, then despawns (drain-
/// particle idiom aimed at the orb instead of along a beam).
#[derive(Component)]
pub struct CastingOrbMote {
    pub orb: Entity,
    /// 0..1 travel progress toward the orb center.
    pub progress: f32,
    /// Progress units per second.
    pub speed: f32,
    /// World-space offset from the orb center where this mote started.
    pub start_offset: Vec3,
}

/// The pre-stealth material of one weapon-mesh descendant, remembered so the
/// stealth fade can restore it exactly on unstealth. glTF materials are
/// SHARED assets across every spawned instance of the model, so the fade
/// must swap in a per-instance clone rather than mutate in place — mutating
/// would fade every copy of that weapon in the arena.
#[derive(Component)]
pub struct OriginalWeaponMaterial(pub Handle<StandardMaterial>);

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

/// Which restraint object a rooted unit wears. Selected from the aura's
/// `spell_school` (Frost Nova is `Frost`, Spider Web is `Nature`), so a future
/// root inherits a treatment with no code change.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootStyle {
    /// Faceted ice crystals stabbing up around the feet.
    Ice,
    /// A webbed sheet over the shins — spokes out to a hem pinned on the floor,
    /// crossed by concentric rings.
    Web,
}

/// Marker: this unit is rooted and wearing the feet treatment.
///
/// The SINGLE source of truth for every visual keyed on `AuraType::Root` — the
/// rig's lifetime and its retract arm both key off this marker, never off a
/// second system re-deriving state from `ActiveAuras` (predicates that each
/// re-derive drift apart; see `aura-driven-visual-exit-paths.md`). Carrying the
/// style makes a style CHANGE — a Web root replaced by a Frost Nova within one
/// tick — a detectable rebuild rather than silent drift.
///
/// Composes with [`StunnedVisual`]: Root and Stun are separate DR categories
/// occupying disjoint space, and both must show at once.
#[derive(Component)]
pub struct RootedVisual {
    pub style: RootStyle,
}

/// Marker: this unit is stunned and wearing the overhead whirl. See
/// [`RootedVisual`] for the ownership rule.
#[derive(Component)]
pub struct StunnedVisual;

/// Which hard-CC treatment a [`CcRig`] carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CcKind {
    Root,
    Stun,
}

/// One crowd-control rig: a WORLD-SPACE hub that follows its owner, with every
/// primitive of the treatment as its own child.
///
/// Deliberately NOT a child of the `VisualBody` (whose local y belongs to the
/// gaits and the victory bounce, which would lift a ground piece off the floor)
/// and deliberately NOT a child of the sim entity (whose yaw is sim-written,
/// which would make the whirl's spin fight the unit's facing snaps).
///
/// `owner` is the SIM entity and `kind` disambiguates, mirroring
/// `FearShroud { owner }`: the retract arm filters on BOTH, so two
/// simultaneously rooted units never strip each other's rig and a root and a
/// stun on one unit never strip each other's. Children are UNMARKED — `despawn`
/// is recursive, the same reason `SheepPart`'s siblings are untagged.
#[derive(Component)]
pub struct CcRig {
    pub owner: Entity,
    pub kind: CcKind,
    /// Seconds since spawn. Drives the grow ease, the spin and the bob — all off
    /// `Res<Time>`, never off sim displacement (`fixed-timestep-visual-strobe`).
    pub age: f32,
    /// `Some` once the exit was armed: the rig plays out its retract, then
    /// despawns. A retracting rig is not "held", so a re-application spawns a
    /// fresh one rather than reviving it.
    pub retract: Option<f32>,
    /// Seconds to wait before the rig starts growing. Non-zero only for a
    /// Frost Nova victim, so its crystals rise as the wavefront reaches it
    /// rather than the instant the aura lands (see `NovaFreezeDelay`).
    pub delay: f32,
    /// Vertical offset from the owner's SIM y to this rig's anchor, resolved
    /// once at spawn from the owner's `VisualBody::rest_y` (which is the
    /// sim-to-render correction, and is large and negative for pets). Used by
    /// the Stun whirl; the Root rig ignores it and pins to the floor instead.
    pub lift: f32,
}

/// The one-shot ring marking the instant a hard CC lands. Per VICTIM, so a Frost
/// Nova catching three enemies pops three rings and the AoE reads as an AoE with
/// no caster-side hook.
#[derive(Component)]
pub struct CcFlare {
    /// Seconds remaining before despawn.
    pub lifetime: f32,
    /// Scale the ring expands to — wider on the ground than overhead.
    pub end_scale: f32,
    /// Seconds to hold invisible before the ring starts, mirroring the
    /// [`CcRig::delay`] of the rig it accompanies.
    ///
    /// A Frost Nova catching victims at different distances gives each a
    /// different delay, so the freeze propagates outward with the wavefront.
    /// Without the same delay here, every victim's "landing" ring pops at once
    /// and then their crystals rise seconds apart — the flare contradicting the
    /// propagation it is supposed to announce. Zero for a root from any other
    /// source, which lands everywhere at once.
    pub delay: f32,
}

/// One sparkle in a stunned unit's overhead whirl.
///
/// The beads are camera-facing quads, not spheres — geometry cannot produce a
/// soft-edged glow, so the falloff lives in a procedural sparkle texture's
/// alpha. This marker exists so the billboard system can find them, and because
/// they are children of a hub that SPINS, the billboard must counter-rotate by
/// the hub's own rotation rather than simply copying the camera's.
#[derive(Component)]
pub struct CcBead;

/// One crescent slash in a rogue stun's caster-side flare.
///
/// A camera-facing quad carrying the procedural arc texture from
/// `rendering/effects/rogue_crescents.rs`. `delay` staggers it within its fan —
/// the source pops Cheap Shot's four in two quick pairs and spreads Kidney
/// Shot's three much wider — so the whole fan spawns in one loop and each
/// crescent holds itself invisible until its turn.
#[derive(Component)]
pub struct CrescentFlare {
    /// World-space unit vector the slash SWEEPS along — across the caster's
    /// body, from its right to its left, perpendicular to the line of attack.
    ///
    /// The streak's long axis is turned to follow this once projected into the
    /// camera's plane. It is deliberately NOT the aim: a blade sweeps across a
    /// target rather than stabbing along the line to it, and the aim is close to
    /// the view axis for the usual over-the-shoulder camera, so projecting IT
    /// yields a near-vertical screen direction — the streaks then run head to
    /// toe down the body, which is what shipped before this was corrected.
    pub sweep: Vec3,
    /// Seconds before this crescent appears.
    pub delay: f32,
    /// Seconds since spawn, including the delay.
    pub age: f32,
    /// Seconds this crescent lives once it has appeared.
    pub lifetime: f32,
    /// Roll about the view axis, so a fan spreads across the screen rather than
    /// around the world.
    pub roll: f32,
    pub size: f32,
    pub color: Color,
    /// Mid-travel tint, for the source's early white-pink flash.
    pub color_mid: Color,
    pub color_end: Color,
    pub emissive: LinearRgba,
}

/// Hammer of Justice's ground wave: a flat gold arc sweeping outward from the
/// Paladin's own feet.
///
/// The NAME is historical. This began as a streak racing toward the victim, on
/// a reading of `HasMissile = 0` and a `SpecialUnarmed` animation name as "no
/// weapon motion at all". Reference imagery reversed that: nothing travels
/// between the two units, and the source draws a wavefront rolling out around
/// the caster. See `src/states/play_match/rendering/effects/holy_justice.rs`.
#[derive(Component)]
pub struct JusticeWave {
    pub age: f32,
    /// How far the wave rolls out, in yards. A FIXED radius — the wave is
    /// caster-centred, so unlike the streak it replaced it does not scale to
    /// the caster-target distance.
    pub length: f32,
    /// The caster's feet — the wave's fixed centre.
    pub origin: Vec3,
}

/// The golden seal that blooms on a Hammer of Justice victim's chest.
#[derive(Component)]
pub struct JusticeRune {
    pub age: f32,
}

/// One of Frost Nova's three expanding ground rings.
///
/// The geometry is a ragged unit-radius annulus built once at spawn; only the
/// uniform scale changes, because the wobble is fixed and the radius is not.
/// See `rendering/effects/frost_nova.rs`.
#[derive(Component)]
pub struct NovaRing {
    /// 0, 1 or 2 — decides the radius, the stagger and the wobble's phase.
    pub ring: u32,
    pub age: f32,
}

/// One ice crystal thrown up along Frost Nova's outer wavefront.
#[derive(Component)]
pub struct NovaShard {
    /// Nova-age at which the wave reaches this crystal's radius.
    pub born_at: f32,
    pub age: f32,
    /// Full height, jittered per crystal.
    pub height: f32,
}

/// How long a freshly-rooted unit should wait before its root crystals grow,
/// so the freeze propagates outward with Frost Nova's wavefront instead of
/// happening everywhere at once.
///
/// Inserted by the nova's graphical flourish on every enemy the wave will
/// reach, and CONSUMED (removed) by `update_hard_cc_visuals` when it builds the
/// Root rig. Purely cosmetic: if it is missing — a root from any other source,
/// or a race where the rig is built first — the rig simply grows immediately,
/// which is the pre-existing behaviour.
///
/// It carries its own expiry because it is inserted on everyone in RADIUS, and
/// the graphical side cannot know who the sim actually rooted. A target that is
/// immune (Divine Shield) or already dead gets no aura and therefore no rig, so
/// nothing would ever consume its delay — and it would then silently postpone
/// that unit's NEXT root, from any source, by up to a full wavefront. Expiring
/// it after the wave has passed keeps the stranding harmless.
#[derive(Component)]
pub struct NovaFreezeDelay {
    pub secs: f32,
    /// Seconds since insertion; the component is dropped once this passes the
    /// wavefront's own life, whether or not it was ever used.
    pub age: f32,
}

/// Which bespoke missile a projectile carries.
///
/// The two are one shared vocabulary — faceted body, twin helical ribbons, shed
/// sprites — parameterised into opposite silhouettes, which is the relationship
/// the Classic models themselves have. See `rendering/effects/spell_bolts.rs`.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoltKind {
    Frost,
    Shadow,
}

/// The emitter state a bespoke bolt carries while it flies.
///
/// Lives on the `Projectile` entity itself, so the whole rig — shard, sprites,
/// and the accumulators below — dies with the projectile on impact. Trail
/// segments and motes are deliberately NOT children: they are left behind in
/// world space and fade on their own clock.
#[derive(Component)]
pub struct BoltRig {
    pub kind: BoltKind,
    pub age: f32,
    /// Distance travelled since the last ribbon segment, in yards.
    pub ribbon_carry: f32,
    /// Fractional shed sprites owed since the last one was spawned.
    pub shed_carry: f32,
    /// How many sprites this bolt has shed, seeding their scatter.
    pub shed_count: u32,
    /// Where the bolt was last frame, so ribbon spacing can be measured along
    /// the step rather than sampled at frame boundaries.
    pub last_pos: Vec3,
    /// Per-bolt scatter seed. Visual only — never `game_rng`.
    pub seed: u32,
}

/// The rolling hub carrying Frostbolt's two shard cones.
#[derive(Component)]
pub struct BoltShard;

/// Shadow Bolt's opaque core — the one part of either bolt that is not a light.
#[derive(Component)]
pub struct BoltCore;

/// What a billboarded bolt sprite is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoltSpriteRole {
    /// Frostbolt's wide head flare.
    Flare,
    /// Frostbolt's tight additive core, just ahead of the shard's shoulder.
    TipGlow,
    /// Shadow Bolt's breathing glow.
    Halo,
    /// Shadow Bolt's churn layers, on the source's 200ms and 433ms loops.
    ChurnA,
    ChurnB,
}

/// A billboarded quad parented to a bolt.
#[derive(Component)]
pub struct BoltSprite {
    pub role: BoltSpriteRole,
    /// Radius in yards at full size; the pulses scale around it.
    pub radius: f32,
}

/// One segment of a bolt's ribbon trail, left behind in world space.
///
/// A STRETCHED band, not a dot: it spans `length` along `dir` so consecutive
/// segments overlap into a continuous ribbon. Round sprites cannot do this —
/// their alpha falls off radially, so however tightly they are spaced the
/// bright cores stay separate and the trail reads as a dotted line.
#[derive(Component)]
pub struct BoltTrail {
    pub age: f32,
    pub life: f32,
    /// Half-width of the band, in yards. This is what the fade shrinks.
    pub half_width: f32,
    /// Length along `dir`, in yards. Held CONSTANT as the segment fades —
    /// shrinking it would open gaps at the tail as the ribbon died.
    pub length: f32,
    /// Direction of travel when this segment was laid down.
    pub dir: Vec3,
}

/// One shed snowflake or shadow mote, drifting off the bolt's head.
#[derive(Component)]
pub struct BoltMote {
    pub age: f32,
    pub life: f32,
    pub radius: f32,
    pub velocity: Vec3,
}

/// A landed bolt, playing its burst on the victim.
///
/// Spawned by `process_projectile_hits` (so it exists in both modes, like
/// `DeathCoilBurst`) and rendered only in graphical mode. Purely cosmetic: it
/// reads combat state, writes none, and draws no `game_rng`.
#[derive(Component)]
pub struct BoltImpact {
    pub kind: BoltKind,
    /// The victim. The burst TRACKS it — the client attaches both impacts to
    /// chest attachment 34, so a target that keeps running carries its hit.
    pub target: Entity,
    /// Unit vector from the victim back toward where the bolt came from.
    ///
    /// Shadow Bolt's burst is bilateral — its two arcs straddle this axis — so
    /// without it the pair would spread along a fixed world axis and collapse
    /// to a line for half of all bearings.
    pub from: Vec3,
    pub age: f32,
}

/// What a billboarded piece of an impact is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoltImpactRole {
    /// Frostbolt's cyan star flash.
    Flash,
    /// Frostbolt's expanding shockwave ring.
    Ring,
    /// Shadow Bolt's brief additive core flash.
    Core,
    /// Shadow Bolt's dark blot — the source's Opaque batch, taken literally.
    /// The only piece of either burst that DARKENS rather than brightens.
    Blot,
    /// One of Shadow Bolt's two arcs.
    Arc,
}

#[derive(Component)]
pub struct BoltImpactSprite {
    pub role: BoltImpactRole,
    /// Full-size radius in yards; the growth curves scale around it.
    pub radius: f32,
    /// `+1` / `-1` for the two arcs, `0` for everything else.
    pub side: f32,
}

/// One ice chip thrown out by a Frostbolt impact, in the rig's own frame.
#[derive(Component)]
pub struct BoltImpactShard {
    pub velocity: Vec3,
    pub spin: f32,
}
