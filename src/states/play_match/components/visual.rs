use super::super::abilities::{AbilityType, SpellSchool};
use super::super::match_config::CharacterClass;
use super::auras::AuraType;
use super::combatant::AutoAttackKind;
use bevy::prelude::*;
use bevy_egui::egui;

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

/// The segment an entity the sim moves is drawn along between ticks
/// (`rendering::interpolation`). Graphical-only; the sim never reads it.
#[derive(Component, Clone, Copy, Debug)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct RenderInterpolation {
    /// Translation at the start of the latest sim tick.
    pub previous: Vec3,
    /// Translation the latest sim tick left — the sim's own value.
    pub current: Vec3,
    /// Facing at the start of the latest sim tick.
    pub previous_rotation: Quat,
    /// Facing the latest sim tick left — the sim's own value.
    pub current_rotation: Quat,
    /// What this frame drew (translation, rotation), while the interpolated
    /// values are in `Transform`.
    pub drawn: Option<(Vec3, Quat)>,
}

/// Where on the victim a shared impact plays.
///
/// The Classic client attaches the Hunter shots' impact to chest attachment 34 and
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
/// both modes like `BoltImpact`; rendered only in graphical mode. The interrupt
/// landings and Heroic Strike's are spawned graphical-side instead, off the
/// markers the sim tags (`InterruptedBy`, `HeroicStrikeSwing`). Purely
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
            | AbilityType::ManaBurn
            // `ice_impactdd_med_chest.m2` on attachment 34 — its impact kit
            // (214) resolves to the SAME model as Frostbolt's landing, so the
            // stock Frost row is the faithful rendition and there is no
            // `landing_style` override. See
            // docs/design/2026-09-06-frost-shock-client-data.md.
            | AbilityType::FrostShock => Some(ImpactAnchor::Chest),
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
    /// Fractional particles owed per client emitter (`ImpactStyle::emitters`),
    /// one slot per emitter.
    pub emitter_carry: Vec<f32>,
    /// One colour-ramp palette per client emitter, in emitter order.
    pub palettes: Vec<std::sync::Arc<[Handle<StandardMaterial>]>>,
}

/// Signature Lightning Bolt strike: an instant forked "flash-crack" arc drawn
/// from caster to target at the moment the cast lands.
///
/// Spawned deterministically in the shared casting-completion path (no `game_rng`
/// draw), so it is byte-neutral in headless. The
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

/// Moonfire's landing — a moon orb overhead and a thin beam dropping onto the
/// victim (`moonfire_impact_base.m2`, kit 3293, @Base).
///
/// Spawned deterministically at the Moonfire landing in `process_casting` (no
/// `game_rng` draw), so it is byte-neutral in headless like
/// [`LightningBoltStrike`]. The graphical-only systems in
/// `rendering/effects/moonfire.rs` dress and animate it. Moonfire does NOT
/// route through [`SchoolImpact`]: this is its whole landing, and its DoT has
/// no sustained visual (the client has none; the aura icon carries it).
#[derive(Component)]
pub struct MoonfireLanding {
    /// The victim. The landing tracks it while it plays.
    pub target: Entity,
    /// Victim position at the landing — where the landing stays if the victim
    /// is gone.
    pub origin: Vec3,
}

/// Component for tracking death fall animation.
/// When a combatant dies, this component is added to animate them falling over.
#[derive(Component)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
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
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
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
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct FearedVisual;

/// Marker: a Druid's body is drawn as Travel Form, the pill on all fours
/// (`rendering/effects/shapeshift.rs`). Single source of truth for the form's
/// look — the hidden standing capsule, the form rig, the bound gait and the
/// hidden weapons all key off it, never off `ActiveAuras`.
///
/// The form owns its OWN restore slot ([`TravelFormBodyMesh`]) and never
/// touches the shared [`OriginalMesh`] / [`OriginalBodyMaterial`] pair, because
/// Fear lands on a shifted Druid: the form swaps the body's MESH and Fear its
/// MATERIAL, so the two compose in either order instead of excluding each other.
/// Polymorph cannot land on a shifted Druid (Travel Form is immune), and the
/// form defers to a sheep that is already up (`Without<PolymorphedVisual>`),
/// with the mirror guard on the polymorph system.
///
/// SparseSet, because it is inserted on the combatant on the frame clock: a
/// table-stored component would move the combatant between archetypes at
/// display rate and reorder the sim's queries with it.
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct TravelFormVisual {
    /// The [`TravelFormRig`] drawing the form, a child of the unit's
    /// [`VisualBody`].
    pub rig: Entity,
}

/// The standing capsule mesh Travel Form took off the [`VisualBody`], stored on
/// that body until the form ends. The form's own slot: see [`TravelFormVisual`].
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct TravelFormBodyMesh(pub Handle<Mesh>);

/// The pivot of a Druid's Travel Form body: a child of its [`VisualBody`] at
/// the lying pill's centre, laid along the unit's heading (+Z). The bound's
/// nose-up/nose-down rock is this entity's rotation; the bob rides the body's
/// own local Y, which the rig inherits.
#[derive(Component)]
pub struct TravelFormRig {
    /// The SIM entity, so the rig is never confused with another Druid's.
    pub owner: Entity,
    /// The current rock, radians, nose up positive.
    pub rock: f32,
    /// The body material the parts were last dressed in. When the body's
    /// material changes (a Fear's husk lands or lifts), the parts follow it.
    pub dressed_in: Option<Handle<StandardMaterial>>,
}

/// One primitive of the Travel Form body (the lying pill, head, an ear, the
/// tail), a child of its [`TravelFormRig`]. `shade` darkens the body's
/// material for this part (1.0 wears the body's material itself).
#[derive(Component)]
pub struct TravelFormPart {
    pub shade: f32,
}

/// The shapeshift puff (`druidmorph_impact_base.m2`, kit 3610), playing at a
/// Druid's feet as it shifts in or out. Emits world-space client particles
/// for [`SHIFT_PUFF_SECS`](crate::states::play_match::SHIFT_PUFF_SECS), then
/// retires; its particles finish their own lives.
#[derive(Component)]
pub struct ShiftPuff {
    /// The Druid the puff follows while it emits.
    pub owner: Entity,
    /// Where it emits from: the Druid's feet, refreshed while the Druid exists.
    pub origin: Vec3,
    /// The Druid's heading, yaw only: the client emitters' forward offsets.
    pub facing: Quat,
    pub age: f32,
    /// Particles owed per emitter, carried between frames.
    pub carry: [f32; 4],
    pub emitted: u32,
}

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
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
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

/// Immolate's apply-moment flame burst, as a one-frame marker.
///
/// Spawned deterministically at the Immolate landing in `process_casting` (no
/// `game_rng` draw), so the burst's look can never move a match. The
/// graphical-only `spawn_immolate_apply_bursts` (`rendering/effects/flame.rs`)
/// turns it into [`FlameParticle`]s with a visual-only RNG and despawns it.
#[derive(Component)]
pub struct ImmolateApplyBurst {
    /// Victim position at the landing; the burst does not follow the victim.
    pub origin: Vec3,
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

/// Which per-spell heal landing a [`HealImpact`] plays.
///
/// From the Classic Era client data (build 1.15.9.69547 — see
/// `docs/design/2026-09-06-heal-impact-client-data.md`): several abilities
/// share one implementation. Holy Shock's heal is byte-identical to Priest
/// Heal in the client (visual 135, kit 232), Lesser Healing Wave and Healing
/// Wave are the same visual 58, and Flash of Light — impact-less for players
/// in the source — borrows Holy Light's head shower the way the non-player
/// FoL variants (visuals 6622/7379) do, at reduced intensity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealImpactKind {
    /// `flashheal_base.m2`: gold ray-fan flash + lens flare, then rising motes.
    FlashHeal,
    /// `heal_low_base.m2`: no flash — a narrow rising stream of gold motes.
    /// Priest Heal's model; Holy Shock's heal lands with it verbatim.
    HealStream,
    /// `holylight_low_head.m2`: head glow + a falling curtain of gold stars
    /// and soft light-puffs. The only head-attached heal in the set.
    HolyLight,
    /// Holy Light's shower at reduced intensity and duration — the sanctioned
    /// borrow the non-player Flash of Light variants use.
    FlashOfLight,
    /// `restoration_impact_base.m2`: green/gold torso glow, orbiting
    /// butterflies, rising gold stars. Nature's one heal landing.
    HealingWave,
    /// The Healing Stream Totem tick blip: a minimal one-shot of rising
    /// Nature-green sparkles in a ring around the bearer's feet, once per
    /// HoT tick.
    ///
    /// AUTHORED, not transcribed (see
    /// `docs/design/2026-09-07-healing-stream-totem-client-data.md`): the
    /// client draws NO per-tick visual at all — Healing Stream's only
    /// target-side identity is a persistent aura-state loop of
    /// `lesserheal_base.m2`, Priest Lesser Heal's gold impact model borrowed
    /// verbatim. Gold would alias Shaman sustain with Priest landings and a
    /// persistent loop is constant noise, so this keeps the client's SHAPE
    /// (rising motes from the Base attach, the source's 0.56 area as the
    /// ring's radial depth, source emitter speeds) recolored on the AS-10
    /// Nature vocabulary and cut to a per-tick one-shot, deliberately far
    /// below Healing Wave's swirl scale. The ring clears the body capsule
    /// (`TOTEM_PULSE_RING_RADIUS`) so the rise reads from the ground instead
    /// of being depth-rejected inside the body.
    TotemPulse,
}

/// A landed heal playing its per-spell, Classic-faithful landing on the
/// recipient — the healing counterpart of [`SchoolImpact`].
///
/// Spawned by combat code at the site where the heal RESOLVES (the
/// cast-completion heal branch in `process_casting`, and
/// `process_holy_shock_heals` for Holy Shock's instant heal), so it exists in
/// both modes; rendered only in graphical mode
/// (`rendering/effects/heal_impact.rs`). Purely cosmetic: it reads combat
/// state, writes none, and draws no `game_rng`.
#[derive(Component)]
pub struct HealImpact {
    /// The recipient. The landing TRACKS it, so a healed runner carries it.
    pub target: Entity,
    pub kind: HealImpactKind,
    pub age: f32,
}

impl HealImpact {
    /// Which landing a cast plays on its target — the single routing table
    /// the spawn sites derive from. Every direct heal has one
    /// (`tests/heal_impact_visual_probes.rs` checks every `is_heal()` ability
    /// in the config reaches SOME landing); Innervate is the one non-heal
    /// that lands one, because the client gives it the heal's kit.
    pub fn kind_for(ability: AbilityType) -> Option<HealImpactKind> {
        match ability {
            AbilityType::FlashHeal => Some(HealImpactKind::FlashHeal),
            // Holy Shock's heal reuses Heal's visual verbatim in the client.
            AbilityType::HolyShock => Some(HealImpactKind::HealStream),
            AbilityType::HolyLight => Some(HealImpactKind::HolyLight),
            AbilityType::FlashOfLight => Some(HealImpactKind::FlashOfLight),
            // LHW and Healing Wave are one visual in the client.
            AbilityType::LesserHealingWave => Some(HealImpactKind::HealingWave),
            // Swiftmend and Innervate land with this same kit in the client
            // (kit 101, `restoration_impact_base.m2`, shared with Healing
            // Wave — `docs/design/2026-10-03-druid-client-data.md`). Client-
            // faithful, not a borrow. Innervate heals nothing, so its landing
            // spawns beside the heal branch in `process_casting`.
            AbilityType::Swiftmend | AbilityType::Innervate => Some(HealImpactKind::HealingWave),
            _ => None,
        }
    }

    /// Which landing an aura-tick heal plays — the routing table for heals
    /// that arrive as AURA ticks rather than resolved casts, spawned at the
    /// tick application site (`process_hot_ticks`). This closes the hole
    /// [`Self::kind_for`] structurally cannot cover: a HoT heals through an
    /// aura, so its ability config has no healing fields, `is_heal()` is
    /// false, and a config-field audit never sees it (Healing Stream Totem
    /// healed 111 times in one 3v3 log with zero visuals).
    ///
    /// EXHAUSTIVE over [`AuraType`] on purpose — no wildcard arm — so adding
    /// a new aura type forces a decision here at compile time: either its
    /// ticks heal the bearer and it names a landing, or it goes in the
    /// explicit non-healing group. A heal over time is then routed by its RON
    /// `name:` through [`HotVisual::for_hot`], because two HoTs can share the
    /// type and still draw differently: Healing Stream pulses per tick, while
    /// the Druid's Rejuvenation and Lifebloom draw nothing on a tick (their
    /// identity is a landing and a sustained state). `name` is the aura's
    /// `ability_name`. `tests/heal_impact_visual_probes.rs` pins the mapping
    /// and proves the tick site actually spawns it.
    pub fn kind_for_hot_tick(aura: AuraType, name: &str) -> Option<HealImpactKind> {
        match aura {
            // The one aura type whose ticks heal the bearer: which HoT it is
            // decides whether a tick draws.
            AuraType::HealingOverTime => match HotVisual::for_hot(name)? {
                HotVisual::TickPulse(kind) => Some(kind),
                HotVisual::LandingSwirl | HotVisual::SustainedPulse => None,
            },
            // Every other aura type's ticks do not heal the bearer. DoT
            // leeches (Death Coil, Drain Life) heal the CASTER at their own
            // sites, not through the bearer's aura tick.
            AuraType::MovementSpeedSlow
            | AuraType::Root
            | AuraType::Stun
            | AuraType::MaxHealthIncrease
            | AuraType::DamageOverTime
            | AuraType::SpellSchoolLockout
            | AuraType::HealingReduction
            | AuraType::Fear
            | AuraType::MaxManaIncrease
            | AuraType::AttackPowerIncrease
            | AuraType::ShadowSight
            | AuraType::Absorb
            | AuraType::WeakenedSoul
            | AuraType::Polymorph
            | AuraType::DamageReduction
            | AuraType::CastTimeIncrease
            | AuraType::DamageTakenReduction
            | AuraType::DamageImmunity
            | AuraType::Incapacitate
            | AuraType::SpellResistanceBuff
            | AuraType::ArmorIncrease
            | AuraType::AttackPowerReduction
            | AuraType::CritChanceIncrease
            | AuraType::ManaRegenIncrease
            | AuraType::AttackSpeedSlow
            | AuraType::LockoutDurationReduction
            | AuraType::FrostArmorBuff
            | AuraType::Silence
            | AuraType::WeaponPoison
            | AuraType::SpellPowerIncrease
            | AuraType::WindfuryBuff
            | AuraType::FearImmunity
            | AuraType::Cyclone
            | AuraType::TravelForm => None,
        }
    }
}

// ============================================================================
// Druid heals over time and Mark of the Wild (AS-212)
// ============================================================================
//
// From `docs/design/2026-10-03-druid-client-data.md` (Rulings 1): the client
// draws Rejuvenation as a one-shot ribbon swirl when it LANDS and nothing on
// its ticks; Lifebloom as a sustained pulse over the head while it lives and a
// gold burst when it BLOOMS; Mark of the Wild as a brief glyph above the head.
// The renderer is `rendering/effects/druid_heals.rs`.

/// Rejuvenation's aura name (the RON `name:` string).
pub const REJUVENATION_AURA: &str = "Rejuvenation";
/// Lifebloom's aura name (the RON `name:` string).
pub const LIFEBLOOM_AURA: &str = "Lifebloom";
/// Mark of the Wild's aura name (the RON `name:` string).
pub const MARK_OF_THE_WILD_AURA: &str = "Mark of the Wild";

/// What a heal-over-time aura draws — the ONE place a HoT meets its visual,
/// routed by the aura's RON name the way `DotStateVisual::for_dot` routes a
/// DoT. [`HealImpact::kind_for_hot_tick`] (the tick site) and the
/// lands-silently audit both ask it, so a HoT routed nowhere fails the build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotVisual {
    /// Each tick lands a heal impact: Healing Stream Totem's blip.
    TickPulse(HealImpactKind),
    /// Rejuvenation: a ribbon swirl when the aura lands (or is refreshed), and
    /// nothing on its ticks — the client has no sustained state for it.
    LandingSwirl,
    /// Lifebloom: a sustained pulse over the head for as long as the aura
    /// lives, and a gold burst when it blooms. Nothing on its ticks.
    SustainedPulse,
}

impl HotVisual {
    /// The visual for a heal over time named `name`, or `None` when nothing
    /// draws it. `None` is what the lands-silently audit fails on, so it never
    /// means "handled elsewhere".
    pub fn for_hot(name: &str) -> Option<HotVisual> {
        match name {
            REJUVENATION_AURA => Some(HotVisual::LandingSwirl),
            LIFEBLOOM_AURA => Some(HotVisual::SustainedPulse),
            _ if name == super::totems::TotemElement::Water.buff_name() => {
                Some(HotVisual::TickPulse(HealImpactKind::TotemPulse))
            }
            _ => None,
        }
    }
}

/// Which one-shot an aura plays on its bearer when it LANDS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuraLandingKind {
    /// Rejuvenation: five green ribbons orbit the body for 3 s
    /// (`rejuvenation_impact_base.m2`, kit 56).
    RejuvenationSwirl,
    /// Mark of the Wild: the paw glyph above the head for 0.667 s
    /// (`markofwild_impact_head.m2`, kit 542).
    MarkOfTheWildGlyph,
}

impl AuraLandingKind {
    /// The landing an aura of `aura_type` named `name` plays, or `None`.
    pub fn for_aura(aura_type: AuraType, name: &str) -> Option<AuraLandingKind> {
        match aura_type {
            AuraType::HealingOverTime => match HotVisual::for_hot(name)? {
                HotVisual::LandingSwirl => Some(AuraLandingKind::RejuvenationSwirl),
                HotVisual::TickPulse(_) | HotVisual::SustainedPulse => None,
            },
            AuraType::MaxHealthIncrease if name == MARK_OF_THE_WILD_AURA => {
                Some(AuraLandingKind::MarkOfTheWildGlyph)
            }
            _ => None,
        }
    }
}

/// An aura that LANDED — a fresh application or a refresh — playing its
/// one-shot on the bearer.
///
/// Spawned by `apply_pending_auras` at the two points an aura lands, so it
/// exists in both modes; rendered only in graphical mode. Purely cosmetic,
/// like [`HealImpact`]: it reads combat state, writes none, and draws no
/// `game_rng`.
#[derive(Component, Clone, Copy, Debug)]
pub struct AuraLanding {
    /// The bearer. The landing TRACKS it.
    pub target: Entity,
    pub kind: AuraLandingKind,
}

/// What an aura's BLOOM draws when it lands, routed by the aura's RON name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BloomVisual {
    /// Lifebloom's gold flower-burst at the chest (`lifebloom_impact.m2`,
    /// kit 6965).
    GoldBurst,
}

impl BloomVisual {
    /// The burst a bloom from the aura named `name` plays, or `None`.
    pub fn for_bloom(name: &str) -> Option<BloomVisual> {
        match name {
            LIFEBLOOM_AURA => Some(BloomVisual::GoldBurst),
            _ => None,
        }
    }
}

/// A bloom that LANDED, playing its burst on the bearer.
///
/// Spawned by `process_blooms`, the one site every bloom lands at, after that
/// system's own alive check — so it follows the sim's bloom rules exactly:
/// expiry, a purge and a dispel bloom; a refresh and the bearer's death do
/// not. It is not a heal site. Purely cosmetic, like [`AuraLanding`].
#[derive(Component, Clone, Copy, Debug)]
pub struct BloomBurst {
    pub target: Entity,
    pub kind: BloomVisual,
}

/// The four Druid effects the AS-160 bench signed off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DruidEffect {
    RejuvenationSwirl,
    LifebloomPulse,
    LifebloomBloom,
    MarkOfTheWildGlyph,
}

/// The rig one Druid effect plays from (graphical only). A one-shot (the
/// swirl, the bloom, the glyph) emits for its window and retires when its last
/// particle has died. A [`DruidEffect::LifebloomPulse`] rig is ONE caster's
/// Lifebloom on one bearer — two Druids' Lifeblooms are two auras and draw as
/// two pulses — and lives exactly as long as that aura does on a living
/// bearer.
#[derive(Component, Debug)]
pub struct DruidEffectRig {
    pub target: Entity,
    pub effect: DruidEffect,
    /// The Lifebloom's caster, for a pulse; `None` for the one-shots.
    pub caster: Option<Entity>,
    pub age: f32,
    /// Particles owed per emitter (fractional carry).
    pub carry: Vec<f32>,
    /// Particles emitted so far — the scatter seed.
    pub emitted: u32,
}

/// One Rejuvenation ribbon: a camera-facing band whose mesh is rebuilt each
/// frame along the last stretch of its orbit (graphical only).
#[derive(Component, Debug)]
pub struct RejuvenationRibbon {
    /// The swirl rig it belongs to.
    pub rig: Entity,
    /// 0..5 — sets its phase, height, direction and green.
    pub index: usize,
    pub mesh: Handle<Mesh>,
}

/// One of Mark of the Wild's two crossed glyph plates (graphical only).
#[derive(Component, Debug)]
pub struct MarkOfTheWildPlate {
    pub rig: Entity,
    /// 0 = red-orange, 1 = gold.
    pub layer: usize,
}

/// A particle from one of the Druid effects' emitters (graphical only). The
/// entity carries the position; its children are the sprite and, for an
/// emitter with a tail, the streak behind it.
#[derive(Component, Debug)]
pub struct DruidParticle {
    /// The rig that emitted it. A particle whose rig is gone is retired with
    /// it, which is how a sustained state's particles leave with its aura.
    pub rig: Entity,
    pub effect: DruidEffect,
    /// Index into the effect's emitter table.
    pub emitter: usize,
    pub age: f32,
    pub life: f32,
    pub velocity: Vec3,
    /// The bearer it follows, and where the bearer was last frame.
    pub follow: Option<(Entity, Vec3)>,
    /// Sprite roll, radians.
    pub angle: f32,
    /// Stature scale (a pet bearer draws smaller).
    pub stature: f32,
    /// The emitter's colour/alpha palettes over a life: sprite, then streak.
    pub sprite_palette: std::sync::Arc<[Handle<StandardMaterial>]>,
    pub streak_palette: std::sync::Arc<[Handle<StandardMaterial>]>,
}

/// The sprite or streak child of a [`DruidParticle`].
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DruidParticlePart {
    Sprite,
    Streak,
}

/// A flat, per-landing piece of a heal impact (graphical only).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HealSpriteRole {
    /// One of Flash Heal's radiating gradient light rays, rolled to `angle`
    /// in the billboard plane.
    Ray { angle: f32 },
    /// The central lens-flare flash under Flash Heal's rays.
    LensFlare,
    /// Holy Light's glow bloom at the head.
    HeadGlow,
    /// One of Healing Wave's green/gold torso glow layers — halo quads whose
    /// radii clear the combatant capsule so the wrap reads AROUND the body.
    TorsoGlow,
    /// Healing Wave's green pool of light at the recipient's feet. Lies flat
    /// on the ground and is never billboarded.
    UnderGlow,
}

#[derive(Component)]
pub struct HealSprite {
    pub role: HealSpriteRole,
    /// Full-size radius in yards (for rays: full length).
    pub radius: f32,
    pub base_alpha: f32,
}

/// What a heal landing's motes look like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealMoteKind {
    /// A small star flash (`star5a` / `yellow_star_dim`).
    Star,
    /// A vertically stretched streak (`ribbonblur1bd_gold_side`).
    Ribbon,
    /// A soft, wide light-puff (`clouds8x8fade`).
    Puff,
}

/// One rising or falling gold mote of a heal landing, in the rig's frame.
#[derive(Component)]
pub struct HealMote {
    pub kind: HealMoteKind,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub radius: f32,
}

/// One butterfly wing of the Healing Wave swirl. Wings are direct children
/// of the rig; the animate system computes the whole butterfly pose (orbit
/// position, heading, flap) from these fields each frame.
#[derive(Component)]
pub struct HealButterflyWing {
    /// Which butterfly of the swirl this wing belongs to.
    pub index: u32,
    /// -1.0 for the left wing, +1.0 for the right.
    pub side: f32,
}

/// Graphical-only state a [`HealImpact`] rig carries while it plays.
#[derive(Component)]
pub struct HealImpactRig {
    /// Fractional motes owed per emitter since the last one was spawned.
    pub carry: [f32; 8],
    /// How many motes the rig has emitted, seeding their scatter.
    pub emitted: u32,
    pub quad: Handle<Mesh>,
    pub star_material: Handle<StandardMaterial>,
    pub ribbon_material: Handle<StandardMaterial>,
    pub puff_material: Handle<StandardMaterial>,
}

/// Which cast-side family a heal's hand glow plays while the caster winds up.
///
/// From the Classic Era client data (build 1.15.9.69547 — see
/// `docs/design/2026-09-06-cast-side-heal-client-data.md`): every heal's
/// entire cast-side show lives on the caster's two spell hands (attach 21/22,
/// always both, always symmetric), and the vocabulary splits cleanly by
/// school. Holy (Priest + Paladin) is `holy_precast_low_hand.m2` — a gold
/// two-layer glow ball with three wide gold ribbon wisps; Nature (Shaman) is
/// `nature_precast_low_hand.m2` — the same swirl skeleton re-dressed green
/// with thin star-threads plus a lazy shed of leaves.
///
/// The Druid draws these hands on EVERY cast, heal or not
/// (`docs/design/2026-10-03-druid-client-data.md`): the Nature pair on all
/// but Moonfire, whose hands are the arcane sparkle of `magic_cast_hand.m2`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealCastKind {
    /// Gold glow + wide gold ribbon wisps; launch is a one-shot re-flare of
    /// the SAME glow (kit 270 resolves to the precast model verbatim).
    Holy,
    /// Green glow + thin green star-thread wisps + leaf drift; launch is a
    /// dedicated ~0.3s water-ring and gold-spark jet (`nature_cast_hand.m2`).
    Nature,
    /// Moonfire's cast kit 730, `magic_cast_hand.m2`: a 1000 ms one-shot of
    /// cyan motes, star twinkles and a brief ring disc on each hand. The
    /// client gives it no precast loop, so it only ever plays as a launch.
    Arcane,
}

impl HealCastKind {
    /// Which cast-side family an ability's hands play — the single routing
    /// table for the hand glows, mirroring [`HealImpact::kind_for`] on the
    /// impact side. A hard cast winds up through the precast loop and
    /// launches on landing (`spawn_heal_cast_glows`); a zero-length cast
    /// plays the launch alone as it lands (`spawn_instant_cast_hands`).
    /// `None` keeps the generic casting orb on a hard cast and draws nothing
    /// on an instant: every other class's instants are unrouted (Holy Shock
    /// never carries a `CastingState`; Frost Shock is a zero-length one).
    pub fn for_ability(ability: AbilityType) -> Option<HealCastKind> {
        match ability {
            AbilityType::FlashHeal | AbilityType::HolyLight | AbilityType::FlashOfLight => {
                Some(HealCastKind::Holy)
            }
            AbilityType::LesserHealingWave => Some(HealCastKind::Nature),
            // The Druid: kits 345/181 and 100/183 are the Shaman's two Nature
            // hand models, on every cast but Moonfire. Travel Form carries no
            // cast state; its shift (`ShapeshiftPending`) plays its hands.
            AbilityType::Rejuvenation
            | AbilityType::Lifebloom
            | AbilityType::Swiftmend
            | AbilityType::Innervate
            | AbilityType::MarkOfTheWild
            | AbilityType::EntanglingRoots
            | AbilityType::Cyclone
            | AbilityType::TravelForm => Some(HealCastKind::Nature),
            AbilityType::Moonfire => Some(HealCastKind::Arcane),
            _ => None,
        }
    }

    /// Whether the family has a precast loop for a hard cast to wind up
    /// through. Arcane has none in the client.
    pub fn has_precast(self) -> bool {
        !matches!(self, HealCastKind::Arcane)
    }
}

/// Lifecycle phase of a [`HealCastHand`] rig.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HealCastPhase {
    /// The precast loop — alive for exactly as long as the `CastingState`
    /// (the loop duration is the ACTUAL cast time, decided by gameplay).
    Loop,
    /// The one-shot launch, entered on a `CastEndingKind::Landed` marker.
    /// Holy re-flares its own glow; Nature swaps to the burst jet.
    Flare { remaining: f32 },
}

/// One spell-hand rig of a heal cast (graphical only): a CHILD of the
/// caster's [`VisualBody`] parked at the spell-hand socket, so it rides the
/// walk bob, the facing and the cast posture rigidly — composed in the socket
/// frame, never re-derived from sim movement (fixed-timestep strobe lesson).
/// Both hands always: the client attaches every heal's precast and launch
/// kits at attach 21 AND 22 with pure symmetric KMA rows.
#[derive(Component)]
pub struct HealCastHand {
    /// The sim combatant casting (NOT the `VisualBody` parent).
    pub caster: Entity,
    /// The `VisualBody` the rig hangs off — kept so the billboard pass can
    /// compose the parent world rotation without walking the hierarchy.
    pub body: Entity,
    pub kind: HealCastKind,
    /// +1 main-hand side, -1 off-hand side (mirrors [`WeaponHand`]'s mounts).
    pub side: f32,
    pub age: f32,
    pub phase: HealCastPhase,
    /// Whether a `Landed` ending plays the launch flare. True for every heal
    /// except Flash of Light when `FLASH_OF_LIGHT_HAS_LAUNCH_FLASH` is off.
    pub has_launch_flash: bool,
    /// Whether the rig drives the caster's cast posture. A hard cast's rigs
    /// do; an instant's one-shot hand flash leaves the torso alone.
    pub drives_posture: bool,
    /// Fractional leaves owed since the last spawn (Nature loop).
    pub leaf_carry: f32,
    /// Fractional launch-burst motes owed: `[water rings, gold sparks]`.
    pub burst_carry: [f32; 2],
    /// Monotonic mote counter, seeding position-hashed scatter (never RNG).
    pub emitted: u32,
    pub quad: Handle<Mesh>,
    /// The water-ring annulus mesh (Nature launch).
    pub ring_mesh: Handle<Mesh>,
    pub leaf_material: Handle<StandardMaterial>,
    pub spark_material: Handle<StandardMaterial>,
    pub ring_material: Handle<StandardMaterial>,
}

/// A flat piece of a heal-cast hand rig (graphical only).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HealCastPieceRole {
    /// The outer soft glow quad (`yellow_glow3a` / `green_glow3`).
    GlowOuter,
    /// The brighter core quad (`genericglow2c` / inner `green_glow3`).
    GlowCore,
    /// One of the three orbiting wisps (gold ribbons / green star-threads).
    Wisp { index: u32 },
    /// One of Moonfire's rising `cyan_glow3` motes (P0 of `magic_cast_hand.m2`).
    ArcaneMote { index: u32 },
    /// One of Moonfire's `star5a` twinkles (P1).
    ArcaneStar { index: u32 },
    /// Moonfire's `teleporttarget` ring disc, the kit's first 300 ms (P2).
    ArcaneRing,
}

#[derive(Component)]
pub struct HealCastPiece {
    pub role: HealCastPieceRole,
    pub base_alpha: f32,
}

/// One leaf puffing off a glowing Nature hand, in the rig's frame.
#[derive(Component)]
pub struct HealCastLeaf {
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
}

/// What a Nature launch-burst mote looks like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealCastBurstKind {
    /// An expanding water-ring sprite (`shockwavewater1`).
    WaterRing,
    /// A gold glow spark (`yellow_glow2/3`).
    GoldSpark,
}

/// One mote of the Nature cast-launch jet, in the rig's frame.
#[derive(Component)]
pub struct HealCastBurstMote {
    pub kind: HealCastBurstKind,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
}

/// The cast body posture of a healer mid-heal (graphical only): the
/// primitive-rig approximation of the omni cast pair — ReadySpellOmni's
/// hands-raised loop leans the torso slightly BACK for the whole cast,
/// SpellCastOmni's release surges it forward as the launch flares. Lives on
/// the CASTER entity; the posture system derives its target from the live
/// [`HealCastHand`] rigs and writes the `VisualBody`'s rotation, snapping
/// back to identity (and removing itself) the frame no rig remains — an
/// interrupted Classic heal's body loop stops dead, no failure flourish.
#[derive(Component)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct HealCastPosture {
    /// Current eased torso pitch, radians (negative = leaning back).
    pub pitch: f32,
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
/// expanding sphere, still used by Master's Call): the ribbon's
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

// ============================================================================
// Aura application — the shared family cue
// ============================================================================
//
// One brief, apply-moment band for every aura type whose application nothing
// else draws: the stat buffs (Arcane Intellect, the Mage armors, Power Word:
// Fortitude, the shouts, the Paladin auras), the Rogue's poison coating and
// every totem landing. Differentiated only by the aura's school tint and by
// polarity (a buff rises up the body, a debuff presses down it), so the NEXT
// aura of an existing type costs nothing and a new aura TYPE costs one arm in
// `AuraApplyRoute::for_aura`, which the compiler forces somebody to write.
//
// Graphical-only: detected off `ActiveAuras` transitions by
// `rendering/effects/aura_band.rs`, never spawned by core, so headless stays
// byte-identical by construction and every application path (pending auras,
// totem pulses, the Frost Trap zone, spawn-stamped poison) reaches the one
// classifier below.

/// What an aura's APPLICATION shows in the world — the single routing decision
/// for the shared [`AuraBand`] cue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuraApplyRoute {
    /// The shared family cue: an [`AuraBand`] sweeps the bearer's body.
    Band,
    /// A bespoke effect already draws this type's application. The band stays
    /// off it so it never doubles a treatment that exists.
    Owned(AuraApplyOwner),
}

/// The bespoke effect that owns an aura type's application moment — the named,
/// reviewable suppression set for [`AuraBand`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuraApplyOwner {
    /// Root crystals / web sheet and the stun whirl (`hard_cc.rs`).
    HardCc,
    /// Fear shroud, apply flash and flee run (`fear.rs`).
    FearShroud,
    /// Sheep body swap and transform puffs (`polymorph.rs`).
    Polymorph,
    /// Freezing Trap's ice block (`ice_block.rs`).
    IceBlock,
    /// Shield bubbles: absorbs, Divine Shield, and Weakened Soul, which only
    /// ever lands WITH Power Word: Shield's bubble (`shield_bubbles.rs`).
    ShieldBubble,
    /// Berserker Rage's mask (`berserk.rs`).
    BerserkMask,
    /// Curse of Weakness / Curse of Tongues apply apparitions — the only
    /// sources of these two types (`warlock_dots.rs`).
    CurseApparition,
    /// The DoT layer: Corruption / Unstable Affliction / Curse of Agony apply
    /// bursts and the Rend / Serpent Sting drips (`warlock_dots.rs`,
    /// `affliction.rs`). Per-DoT coverage inside that layer is the DoT
    /// family's own audit.
    DotLayer,
    /// Mortal Wounds: the debuff lands on a hit whose impact is already drawn
    /// (Mortal Strike's flourish, Aimed Shot's impact), and its tell is the
    /// heal fracture at the moment it bites (`mortal_wounds.rs`).
    MortalWounds,
    /// An interrupt's lockout: the victim's casting orb sputters
    /// (`casting_orbs.rs`).
    InterruptSputter,
    /// Unstable Affliction's dispel backlash burst, the only source of Silence
    /// (`affliction.rs`).
    BacklashBurst,
    /// Shadow Sight: the orb pickup animation (`shadow_sight` orbs).
    ShadowSightOrb,
    /// The slow family — movement slows and Frost Armor's paired attack-speed
    /// chill (`slow_ring.rs`). The bind ring and scuff draw the STATE while the
    /// victim moves; the apply moment is the hit a slow rides on (the bolt or
    /// arrow impact, the swing into Frost Armor, the Frost Trap zone's decal),
    /// plus Crippling Poison's proc flash, which has no hit of its own. A band
    /// here would double every one of those impacts.
    SlowRing,
}

impl AuraApplyRoute {
    /// Route an aura type's application.
    ///
    /// **EXHAUSTIVE on purpose — never add a `_ =>` arm.** A new aura type must
    /// be SEEN here and given an answer: the band, or a named bespoke owner. A
    /// wildcard would quietly file variant N+1 under one of
    /// those, and a silent aura application is exactly the defect this router
    /// exists to end. Pinned as a set equality over [`AuraType::ALL`] by
    /// `tests/aura_band_visual_probes.rs`.
    pub fn for_aura(aura: AuraType) -> Self {
        match aura {
            AuraType::MaxHealthIncrease
            | AuraType::MaxManaIncrease
            | AuraType::AttackPowerIncrease
            | AuraType::AttackPowerReduction
            | AuraType::DamageTakenReduction
            | AuraType::SpellResistanceBuff
            | AuraType::ArmorIncrease
            | AuraType::CritChanceIncrease
            | AuraType::ManaRegenIncrease
            | AuraType::LockoutDurationReduction
            | AuraType::FrostArmorBuff
            | AuraType::WeaponPoison
            | AuraType::SpellPowerIncrease
            | AuraType::HealingOverTime
            | AuraType::WindfuryBuff => AuraApplyRoute::Band,

            // The Druid's Cyclone and Travel Form have no bespoke treatment
            // yet (AS-160 scopes them), so the family cue draws them.
            AuraType::Cyclone | AuraType::TravelForm => AuraApplyRoute::Band,

            AuraType::Root | AuraType::Stun => AuraApplyRoute::Owned(AuraApplyOwner::HardCc),
            AuraType::Fear => AuraApplyRoute::Owned(AuraApplyOwner::FearShroud),
            AuraType::Polymorph => AuraApplyRoute::Owned(AuraApplyOwner::Polymorph),
            AuraType::Incapacitate => AuraApplyRoute::Owned(AuraApplyOwner::IceBlock),
            AuraType::Absorb | AuraType::DamageImmunity | AuraType::WeakenedSoul => {
                AuraApplyRoute::Owned(AuraApplyOwner::ShieldBubble)
            }
            AuraType::FearImmunity => AuraApplyRoute::Owned(AuraApplyOwner::BerserkMask),
            AuraType::DamageReduction | AuraType::CastTimeIncrease => {
                AuraApplyRoute::Owned(AuraApplyOwner::CurseApparition)
            }
            AuraType::DamageOverTime => AuraApplyRoute::Owned(AuraApplyOwner::DotLayer),
            AuraType::HealingReduction => AuraApplyRoute::Owned(AuraApplyOwner::MortalWounds),
            AuraType::SpellSchoolLockout => AuraApplyRoute::Owned(AuraApplyOwner::InterruptSputter),
            AuraType::Silence => AuraApplyRoute::Owned(AuraApplyOwner::BacklashBurst),
            AuraType::ShadowSight => AuraApplyRoute::Owned(AuraApplyOwner::ShadowSightOrb),

            AuraType::MovementSpeedSlow | AuraType::AttackSpeedSlow => {
                AuraApplyRoute::Owned(AuraApplyOwner::SlowRing)
            }
        }
    }
}

/// Which way an [`AuraBand`] sweeps. DERIVED from the sim's own exhaustive
/// hostility classifier ([`AuraType::is_hostile_effect`]) — polarity has one
/// correct answer per type, so it is never restated here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuraBandPolarity {
    /// A beneficial aura: the band rises from the feet past the crown, opening.
    Buff,
    /// A hostile aura: the band presses down from the crown to the feet,
    /// tightening.
    Debuff,
}

impl AuraBandPolarity {
    pub fn of(aura: AuraType) -> Self {
        if aura.is_hostile_effect() {
            AuraBandPolarity::Debuff
        } else {
            AuraBandPolarity::Buff
        }
    }
}

/// The aura-application cue: one soft ring that sweeps the bearer's body once,
/// tinted by the aura's school. Graphical-only (spawned by the renderer's
/// `detect_aura_applications`, never by core).
#[derive(Component, Clone, Debug)]
pub struct AuraBand {
    /// The unit the aura landed on — the band follows it.
    pub target: Entity,
    pub polarity: AuraBandPolarity,
    /// Seconds since the band was born.
    pub age: f32,
    /// Whether the bearer is a pet (smaller body, lower centre).
    pub is_pet: bool,
    /// The band's colour (the aura's school, or the neutral tint).
    pub tint: Color,
}

// ============================================================================
// Warlock DoT aura visuals (Corruption / Curse of Agony / Unstable Affliction)
// ============================================================================
//
// Aura-keyed state visuals from the measured Classic client data
// (`docs/design/2026-09-06-warlock-dot-client-data.md`) plus the authored UA
// redesign. All graphical-only: detected off `ActiveAuras`, never spawned by
// core, so headless stays byte-identical by construction. Systems live in
// `rendering/effects/warlock_dots.rs`.

/// Which shared apply-moment burst a [`DotApplyBurst`] plays. Corruption and
/// Unstable Affliction share ONE apply visual (client kit 117 — the
/// green→violet shadow ring + spark burst); Curse of Agony's apply is the
/// skull apparition, its own rig.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotApplyKind {
    /// Kit 117: expanding shadow ring + green/violet spark burst at the chest.
    /// Played verbatim for both Corruption and Unstable Affliction (the client
    /// gives UA no apply identity of its own — same SpellVisual 381).
    ShadowRing,
}

/// A one-shot Warlock-DoT apply burst playing on the victim (graphical only,
/// self-expiring). Spawned by the aura detector on the apply transition.
#[derive(Component)]
pub struct DotApplyBurst {
    /// The victim — the burst tracks it for its short life.
    pub target: Entity,
    pub kind: DotApplyKind,
    pub age: f32,
    /// Fractional sparks owed since the last one was emitted.
    pub spark_carry: f32,
    /// Sparks emitted so far — seeds the deterministic scatter.
    pub emitted: u32,
}

/// Corruption's persistent aura-state rig (client kit 535): the DARKENING
/// alpha-blend shroud over the victim's head/torso, re-blooming every
/// `PULSE_PERIOD`, plus murk-green swelling wisps and a green mote fizz.
/// Lives from aura-apply to aura-expire/dispel/death — no end flourish.
#[derive(Component)]
pub struct CorruptionShroudRig {
    /// The corrupted victim the rig follows.
    pub target: Entity,
    pub age: f32,
    /// Fractional wisps owed since the last one was emitted.
    pub wisp_carry: f32,
    /// Fractional fizz motes owed since the last one was emitted.
    pub fizz_carry: f32,
    /// Pieces emitted so far — seeds the deterministic scatter.
    pub emitted: u32,
}

/// Which Warlock curse an apparition belongs to. The client gives all three
/// curses a one-shot on-victim apparition at apply and (for Agony and
/// Weakness) nothing afterwards; the per-curse constants live in the
/// `CURSE_APPARITIONS` table in `rendering/effects/warlock_dots.rs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurseKind {
    /// Red-shell, yellow-core skull above the head (client kit 884).
    Agony,
    /// Violet skull-and-bone with a green core glow, above the head
    /// (client kit 719 — `curseofmannoroth_head.m2`; the filenames in this
    /// family lie, the kit joins do not).
    Weakness,
    /// Magenta-violet rune circle at the chest (client kit 503).
    Tongues,
}

impl CurseKind {
    /// Every curse, in table order — the detector and cleanup iterate this.
    pub const ALL: [CurseKind; 3] = [CurseKind::Agony, CurseKind::Weakness, CurseKind::Tongues];

    /// This curse's bit in [`CurseApparitionsFired`].
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// A curse's apply-only apparition on the victim, self-expiring at the end of
/// its spec's envelope. NOTHING follows for the rest of the curse — the
/// era-faithful behaviour for Agony and Weakness alike (see
/// `CURSE_SUSTAIN_WHISPER`).
#[derive(Component)]
pub struct CurseApparitionRig {
    /// The cursed victim the apparition plays on.
    pub target: Entity,
    /// Which curse's spec drives the envelope, palette and emitters.
    pub curse: CurseKind,
    pub age: f32,
    /// Fractional sparks/rune motes owed since the last one was emitted.
    pub spark_carry: f32,
    /// Fractional falling glow motes owed since the last one was emitted
    /// (Curse of Agony only — no other curse kit has a downward emitter).
    pub fall_carry: f32,
    /// Fractional swelling blooms owed since the last one was emitted.
    pub bloom_carry: f32,
    /// Pieces emitted so far — seeds the deterministic scatter.
    pub emitted: u32,
}

/// Latch on a CURSED combatant recording which curses' apparitions have
/// already fired for the current application, one bit per [`CurseKind`]. The
/// apparitions are apply-only, so this — not a live rig — is what stops the
/// detector re-firing every frame. A bit is cleared when its curse leaves the
/// victim, so a fresh curse fires a fresh apparition.
#[derive(Component, Default)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct CurseApparitionsFired {
    /// Bitmask over [`CurseKind::bit`].
    pub fired: u8,
}

/// Unstable Affliction's authored aura state (the client gives UA no identity
/// of its own): an additive violet torso glow pulsing at `UA_PULSE_PERIOD`
/// with a nervous flicker, discharging a crackle of jagged violet bolts and a
/// bright pop every `UA_CRACKLE_PERIOD`. Lives from aura-apply to
/// expire/dispel/death.
#[derive(Component)]
pub struct UaStateRig {
    /// The afflicted victim the rig follows.
    pub target: Entity,
    pub age: f32,
    /// Index of the last crackle cycle whose bolts were spawned, so each
    /// discharge fires exactly once. `u32::MAX` before the first.
    pub last_crackle_cycle: u32,
}

/// What a flat piece of a Warlock-DoT rig is — picks its animation arm.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DotSpriteRole {
    /// The apply burst's expanding shadow ring (a flat torus, never
    /// billboarded — it lies in the ground plane at chest height).
    ShadowRing,
    /// Corruption's darkening shroud shell (a 3D capsule, never billboarded).
    ShroudShell,
    /// The skull's cranium sphere (3D, not billboarded).
    SkullCranium,
    /// The skull's jaw sphere (3D, not billboarded).
    SkullJaw,
    /// One of the skull's dark eye sockets (3D, alpha-blend dark — reads as a
    /// hole in the additive shell).
    SkullEye,
    /// The skull's core glow behind the shell (billboarded quad) — yellow for
    /// Curse of Agony, green for Curse of Weakness.
    SkullCore,
    /// Curse of Weakness's bone beside the skull (a 3D capsule; the client's
    /// `bone_purple` submesh, the silhouette that tells CoW from CoA).
    SkullBone,
    /// One of Curse of Tongues' flat horizontal rune discs (a torus lying in
    /// the ground plane at chest height — never billboarded).
    RuneDisc,
    /// One of Curse of Tongues' upright glyph tablets standing on the rune
    /// ring, facing outward (oriented quad, not billboarded — its radial pose
    /// IS what makes the ring read as a ring).
    RuneTablet,
    /// UA's violet torso glow (billboarded quad).
    UaGlow,
    /// UA's crackle pop — the bright violet flash that must read through
    /// Corruption's shroud when stacked (billboarded quad).
    CracklePop,
    /// One segment of a jagged UA crackle bolt (oriented quad, not
    /// billboarded — its kinked world pose IS the jag).
    CrackleBolt,
}

/// A non-mote piece of a Warlock-DoT rig.
#[derive(Component)]
pub struct DotSprite {
    pub role: DotSpriteRole,
    /// Full-size radius in yards (for bolt segments: the segment length).
    pub radius: f32,
    pub base_alpha: f32,
    /// Seconds this piece stays alive, or `f32::INFINITY` to live with the
    /// rig. Bolt segments and pops use it to die between discharges.
    pub life: f32,
    /// Seconds lived (only meaningful for finite-life pieces).
    pub age: f32,
}

/// What an emitted Warlock-DoT mote looks like — picks material and fade arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotMoteKind {
    /// Apply-burst spark: green→violet→near-black over its life.
    ApplySpark,
    /// Corruption fizz: a small green mote streaming upward off the victim.
    Fizz,
    /// Curse apparition star spark, shrinking (CoA's red-orange `red_star2`
    /// crackle and CoW's green `starflash_grey`/`star5a` pair — same track,
    /// the rig's material carries the palette).
    SkullSpark,
    /// CoA glow mote sinking down over the victim's face/chest.
    SkullFall,
    /// Curse of Tongues rune mote: a slow, near-static violet mote shrinking
    /// off the chest sigil (client `aurarune_a`).
    RuneMote,
}

/// What an emitted Warlock-DoT wisp looks like — picks its scale/colour/alpha
/// tracks. Wisps are the pieces that GROW over their life (and so must fade
/// by alpha, not by shrinking), which is why each carries its own material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DotWispKind {
    /// Corruption's murk: near-black green swelling to sickly yellow-green
    /// (client kit 535, `clouds8x8fade`).
    CorruptionMurk,
    /// Curse of Weakness's violet bloom swelling around the skull (client kit
    /// 719, the additive `toonsmoke16` pair).
    CurseBloom,
    /// Curse of Tongues' rune glow: near-black swelling to violet (client kits
    /// 502/503, `genericglow_black`).
    RuneGlow,
}

/// One emitted mote of a Warlock-DoT rig, in the rig's local frame.
#[derive(Component)]
pub struct DotMote {
    pub kind: DotMoteKind,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    /// Full-size radius, yards.
    pub size: f32,
}

/// One swelling wisp: drifts slowly and GROWS over its life while its colour
/// ramps along its kind's track. Carries its own material so the ramp can be
/// written per-wisp (a growing piece cannot fade by shrinking the way the
/// shared-material motes do).
#[derive(Component)]
pub struct DotWisp {
    pub kind: DotWispKind,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    /// Pet-stature multiplier baked at spawn so a wisp on a Felhunter grows
    /// to pet proportions.
    pub stature: f32,
}

/// Shared graphical assets a Warlock-DoT rig carries: the unit quad and the
/// rig's shared mote materials (motes fade by SHRINKING, so one material
/// serves every mote of a kind — per-mote materials are reserved for pieces
/// whose COLOR ramps individually, i.e. wisps).
#[derive(Component)]
pub struct WarlockDotRigAssets {
    pub quad: Handle<Mesh>,
    /// Apply sparks / Corruption fizz / CoA star sparks, per rig kind.
    pub mote_material: Handle<StandardMaterial>,
    /// CoA falling glow motes; clones `mote_material` on other rigs.
    pub extra_material: Handle<StandardMaterial>,
    /// The shared soft radial-falloff sprite, carried on the rig because
    /// per-wisp materials are built in the animate system (which never sees
    /// `DotAssets`). An untextured additive quad has a hard edge — the
    /// round-2 "chunky green blocks" finding.
    pub soft_dot: Handle<Image>,
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
    /// The gait's CURRENT contribution to the [`VisualBody`] child's local Y,
    /// owned and rewritten by `apply_gait_offset` every frame.
    ///
    /// The body's Y is now composed — `rest_y + body_offset + flinch` (see
    /// [`HitFlinch`]) — so the gait can no longer recover its own contribution
    /// by reading the transform back: what it would read includes the dip. The
    /// settle-to-idle ease is the one place that used to do exactly that, so
    /// the channel it eases is kept here instead. Equivalent to the read-back
    /// whenever no flinch is live, which is why the composition did not move
    /// any existing gait.
    pub body_offset: f32,
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
/// `docs/design/2026-08-01-nagrand-camp-handoff.md` §3.3). Those animations now
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
/// mount pose, and the swing arc. Chosen from the equipped item's weapon type
/// by `weapon_model` in `play_match/mod.rs`, where the items drawn with a
/// stand-in silhouette are named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeaponKind {
    TwoHandAxe,
    Dagger,
    Bow,
    Mace,
    Shield,
    /// A caster's wand: held raised between shots and flicked per shot, never
    /// swung. The client's wand autos animate as `HoldThrown` -> `AttackThrown`
    /// (see the AS-132 client-data doc §4), which is why this is its own kind
    /// rather than a re-skinned melee weapon.
    Wand,
}

/// Which hand a [`WeaponSocket`] occupies, and which hand an
/// [`AutoAttackSwing`] came from. The sim swings each hand on its own timer
/// (`attack_timer` / `offhand_timer`), so a dual-wielder's daggers each follow
/// their own hand's swings rather than taking turns. A Hunter carrying melee
/// weapons holds two main-hand models — its bow on `attack_timer` and its blade
/// on `melee_timer` — told apart by their [`WeaponSet`]. The Paladin's shield
/// is held statically.
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
    /// The swinging hand's effective attack interval, captured when the
    /// current auto-attack stroke began. An auto's stroke timing scales with
    /// it (`weapon_stroke_profile`); freezing it at the hit keeps a slow that
    /// lands mid-stroke from re-timing the stroke already playing. Ignored by
    /// signature styles, whose timing belongs to the ability.
    pub stroke_interval: f32,
    /// The swing parameter this socket last rendered at, published by
    /// `animate_weapon_swings` for `animate_body_lean` to consume.
    ///
    /// The body lean must be driven by the SAME value as the weapon or the two
    /// desync, and recomputing it would mean duplicating the windup
    /// eligibility gates, the easing and the release timing. Written once per
    /// frame by the swing animation; read-only everywhere else.
    pub last_s: f32,
}

/// Which weapon set a [`WeaponSocket`] belongs to, for a combatant that holds
/// two — a Hunter carrying melee weapons beside its bow — and so shows only one
/// set at a time. A socket without this component is always shown.
///
/// The rule, applied by `animate_weapon_swings`: the melee weapons come out
/// when the owner's target comes within melee reach (the melee windup band,
/// where its melee swing is about to fire), and the bow comes back out once the
/// target is at or beyond the Auto Shot minimum (`HUNTER_DEAD_ZONE`). Inside the
/// dead zone between the two the owner keeps whichever set it last had out, so
/// the swap cannot strobe while a target hovers at either edge. With no target,
/// the bow is out.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponSetSwap {
    /// The set this socket belongs to.
    pub set: WeaponSet,
    /// The set this socket's owner currently has out. Every socket of one
    /// owner latches from the same inputs, so their copies agree.
    pub out: WeaponSet,
}

impl WeaponSetSwap {
    /// A socket of `set`, spawned with the bow out.
    pub fn new(set: WeaponSet) -> Self {
        Self {
            set,
            out: WeaponSet::Ranged,
        }
    }

    /// Whether this socket is drawn right now.
    pub fn shown(&self) -> bool {
        self.set == self.out
    }
}

/// The two weapon sets a [`WeaponSetSwap`] chooses between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponSet {
    /// The bow (or gun, crossbow, thrown weapon): Auto Shot.
    Ranged,
    /// The hand weapons: the melee auto-attack.
    Melee,
}

impl WeaponSet {
    /// The set a weapon model belongs to.
    pub fn of(kind: WeaponKind) -> Self {
        match kind {
            WeaponKind::Bow | WeaponKind::Wand => WeaponSet::Ranged,
            WeaponKind::TwoHandAxe | WeaponKind::Dagger | WeaponKind::Mace | WeaponKind::Shield => {
                WeaponSet::Melee
            }
        }
    }
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
    /// The swing's kind — the attacker's derived [`AutoAttackKind`], or
    /// `Melee` for a Hunter's melee swing beside its bow — the same value that
    /// chose the range gate and the combat-log name, so a consumer can never
    /// disagree with the sim about what kind of attack just landed. Replaces
    /// an earlier `ranged: bool`, which could not tell a wand shot from an
    /// arrow.
    pub kind: AutoAttackKind,
    /// Whether the landed swing crit. Selects the deeper flinch
    /// (`CombatCritical`) and the bigger impact burst; cosmetic only.
    pub is_crit: bool,
    /// The hand that swung: `Off` only for a dual-wielder's off-hand swing,
    /// `Main` for everything else (a Windfury bonus swing included, since the
    /// totem procs off the main hand). The swing consumer releases the socket
    /// in THIS hand, so each weapon strikes exactly when its own timer lands.
    pub hand: WeaponHand,
}

/// Rides an [`AutoAttackSwing`] marker when the swing carried a queued Heroic
/// Strike (`next_attack_bonus_damage`), which is otherwise invisible: the bonus
/// rides the ordinary swing, and until this only the combat-log name knew.
/// Inserted on the same marker entity in the apply loop, so the sim spawns
/// nothing extra; read by the graphical `spawn_heroic_strike_flourish`
/// (`rendering/effects/heroic_strike.rs`) before `consume_swing_signals`
/// despawns the marker.
#[derive(Component, Clone, Copy, Debug)]
pub struct HeroicStrikeSwing;

/// A victim's hit reaction: a short downward compression of the
/// [`VisualBody`] child, one per landed auto-attack.
///
/// Lives on the SIM entity of the VICTIM (combatant or pet). Carries no
/// geometry of its own — `apply_gait_offset` composes
/// [`hit_flinch_offset`](crate::states::play_match::hit_flinch_offset) onto
/// the gait channel, so exactly one system writes the body's local Y and
/// there is no ordering hazard between the flinch and the walk bob / sheep
/// hop / panic run. That also makes the flinch visible on a MOVING victim,
/// which a separate writer ordered before the gait would not be.
///
/// Refreshed rather than stacked: a second hit inside the window restarts the
/// dip at the deeper of the two depths, so focus fire reads as a body being
/// held down rather than as an offset that keeps growing. That holds WITHIN a
/// tick as well as across one — `consume_hit_reactions` floors against the
/// depths it has already queued this tick, because its own inserts are not
/// visible in this component until the schedule ends.
///
/// Graphical-only: spawned by `consume_hit_reactions`, which is registered in
/// `states/mod.rs` and nowhere else.
#[derive(Component, Debug, Clone, Copy)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct HitFlinch {
    /// Seconds since the dip started.
    pub elapsed: f32,
    /// Total duration of this dip — per-VICTIM, because the client authors
    /// the wound animation per rig (human 1000 ms, wolf 667 ms).
    pub duration: f32,
    /// Peak downward displacement in arena units, already scaled for a crit.
    pub depth: f32,
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
    /// Warrior Pummel: a POMMEL STRIKE — the weapon flips tip-back about its
    /// grip while thrusting, so the haft's butt end slams into the target. The
    /// source is `SpecialUnarmed` (anim 118, a punch, no weapon motion), but a
    /// limbless capsule has no punch to throw, so the gesture deliberately
    /// moves onto the prop the caster holds — a bench-reviewed design call.
    Pummel,
    /// Rogue Kick: the literal `Kick` animation (anim 95) — a leg strike, kept
    /// source-faithful as body-only motion: the torso loads forward, then
    /// ROCKS BACK at full extension as the leg extends (the front-kick
    /// silhouette) while the weapon rides rigidly, never swinging. The
    /// backward body direction — opposite Pummel's forward drive — is what
    /// separates the two interrupts at a glance.
    Kick,
    /// Rogue Ambush: a fast dagger PIERCE from stealth. The source shares
    /// Backstab's visual wholesale (SpellVisualID 155 on every rank): caster
    /// anim `Attack1HPierce` (85) — the same anim FAMILY as Kidney Shot — with
    /// a 634ms cast model (`backstab_cast_base.m2`), half Kidney Shot's
    /// 1233ms. So the opener is a lunge that reads as SPEED where the
    /// finisher's reads as weight.
    Ambush,
    /// Rogue Sinister Strike: a plain one-hand slash. The source plays
    /// `Attack1H` (anim 17) — the same generic swing Cheap Shot uses — with no
    /// cast model at all, only a muted pink-violet weapon-trail procedural
    /// (SpellProceduralEffect type 8, 0xBD55C6; Kidney Shot carries the same
    /// effect in magenta). Stroke-only here: the tilt of its swing plane is
    /// what separates the spammed builder from Cheap Shot's near-sagittal jab.
    SinisterStrike,
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
            // The melee interrupts: spawned at the committed-use site in
            // combat_ai.rs's interrupt system (roadmap B). Wind Shear is
            // deliberately absent — the Shaman has no weapon sockets and its
            // answer is a victim-side effect, not an actor stroke.
            | Pummel | Kick
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

/// Rides a [`CastEnding`] marker when an interrupt ABILITY cut the cast
/// short, naming it and who used it. Spawned on the same marker entity at the
/// two ability-interrupt sites in `process_interrupts`, and nowhere else: a
/// cast broken by crowd control or Silence, or one that fizzled, carries none.
/// Read by the graphical `spawn_interrupt_landings`
/// (`rendering/effects/client_landings.rs`), which lands the interrupt's mark
/// on the victim. A component on the existing marker rather than a field of it,
/// so the sim spawns no extra entity and every other ending site is untouched.
#[derive(Component, Clone, Copy, Debug)]
pub struct InterruptedBy {
    /// The interrupt ability (Kick, Pummel, Spell Lock, Wind Shear).
    pub ability: AbilityType,
    /// Who used it.
    pub interrupter: Entity,
}

/// Rides a `Landed` [`CastEnding`] marker, naming the ability that landed.
/// Spawned on the same marker at the two landed sites in `process_casting`,
/// and read by the graphical `spawn_instant_cast_hands`, which plays a
/// zero-length cast's hand flash: such a cast is inserted and completed
/// inside one sim tick, so no render-frame system ever sees its
/// `CastingState`. A component on the existing marker (the [`InterruptedBy`]
/// idiom), so the sim spawns no extra entity; inert in headless.
#[derive(Component, Clone, Copy, Debug)]
pub struct LandedCast {
    pub ability: AbilityType,
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
/// `rendering/effects/hunter_shots.rs`, registered only in `states/mod.rs`.
///
/// The entity sits at the arrow's TIP and the shaft hangs back along local
/// -Z, so "arrived" means the point reached the victim. The victim's hit
/// reaction is HELD until then: the arrow leaves a [`RangedHitArrival`] behind
/// the frame it arrives.
#[derive(Component)]
pub struct CosmeticArrow {
    /// The victim. The arrow homes on its chest anchor every frame, so it
    /// ends AT the target however far it has run since the hit.
    pub target: Entity,
    /// Whether the landed shot crit, carried to the arrival so the held
    /// reaction plays the crit's deeper flinch and bigger burst.
    pub is_crit: bool,
    /// Last known aim point — flown to if the victim despawns mid-flight.
    pub to: Vec3,
    /// Yards per second of cosmetic travel.
    pub speed: f32,
    /// Distance travelled since the last ribbon segment, in yards.
    pub ribbon_carry: f32,
    /// Where the tip was last frame, so ribbon spacing is measured along the
    /// step rather than sampled at frame boundaries.
    pub last_pos: Vec3,
    pub ribbon_mesh: Handle<Mesh>,
    pub ribbon_material: Handle<StandardMaterial>,
}

/// A ranged auto (`AutoAttackKind::Shot` or `Wand`) whose projectile has just
/// reached its victim: a bare marker entity, consumed and despawned by
/// `hit_reaction::consume_ranged_hit_arrivals`, which plays the victim's
/// reaction — the flinch, plus the impact burst for a kind that throws one.
///
/// It exists because the sim resolves an auto's damage AT THE SWING and the
/// cosmetic projectile flies afterwards, so a reaction hung on the damage
/// would lead its own projectile by the whole flight. Left behind the frame
/// the projectile arrives — by `update_cosmetic_arrows` for an arrow,
/// `update_wand_missiles` for a wand bolt — or, for a shot that has nothing
/// in flight, at once by whichever consumer would have launched it
/// (`consume_swing_signals` for a Shot, `consume_hit_reactions` for a wand),
/// so every landed ranged auto still reacts exactly once. Graphical-only; the
/// sim never sees it.
#[derive(Component)]
pub struct RangedHitArrival {
    pub target: Entity,
    /// Which ranged auto arrived. Selects the reaction: a Shot throws the
    /// weapon-spark burst, a wand bolt the flinch alone.
    pub kind: AutoAttackKind,
    pub is_crit: bool,
    /// The direction the shot came FROM, as seen from the victim — the side
    /// of the silhouette it struck. Only its horizontal part is read.
    pub from: Vec3,
}

/// The emitter state a Hunter shot missile carries while it flies.
///
/// Lives on the `Projectile` entity itself, like `BoltRig`, so the core dies
/// with the projectile on impact. The particles and ribbon it sheds are NOT
/// children: they are left behind in world space and fade on their own clock.
#[derive(Component)]
pub struct HunterShotRig {
    pub kind: crate::states::play_match::rendering::HunterShotKind,
    /// Fractional particles owed per emitter since the last one was spawned.
    pub carry: [f32; 4],
    /// How many particles this missile has shed, seeding their scatter.
    pub emitted: u32,
    /// Distance travelled since the last ribbon segment, in yards.
    pub ribbon_carry: f32,
    /// Where the missile was last frame.
    pub last_pos: Vec3,
    /// Per-missile scatter seed. Visual only — never `game_rng`.
    pub seed: u32,
    pub quad: Handle<Mesh>,
    pub ribbon_material: Handle<StandardMaterial>,
    /// One colour-ramp palette per emitter, in emitter order.
    pub palettes: Vec<std::sync::Arc<[Handle<StandardMaterial>]>>,
}

/// A billboarded glow in a Hunter shot missile's core.
#[derive(Component)]
pub struct HunterShotCore;

/// How a client-emitter particle is oriented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticleFacing {
    /// A sprite turned to face the camera.
    Camera,
    /// Lying flat in the ground plane and turning about world up — the rune.
    Flat,
    /// Facing the camera and turning in the view plane — a seal rune closing
    /// on the chest (Spell Lock).
    Seal,
    /// Facing the camera, its long axis laid along the particle's own flight —
    /// a ribbon-blur or a streak (Holy Shock).
    Streak,
}

/// One particle of a transcribed client emitter
/// (`rendering/effects/hunter_shots.rs`).
///
/// Its colour and alpha ramp is carried by swapping between the emitter's
/// pre-built palette materials as it ages — one material per ramp step, shared
/// by every particle of that emitter — and its size ramp by scale, so nothing
/// per-particle is ever written to an asset.
#[derive(Component)]
pub struct ClientParticle {
    pub age: f32,
    pub life: f32,
    pub velocity: Vec3,
    /// Downward acceleration, yd/s².
    pub gravity: f32,
    /// Diameter at birth, midlife and death, yards.
    pub size: [f32; 3],
    /// Where in the life the middle ramp key sits, 0..1 (0.5 for most
    /// emitters; the source keys some earlier or later).
    pub mid: f32,
    pub palette: std::sync::Arc<[Handle<StandardMaterial>]>,
    /// The palette step currently on the mesh.
    pub step: usize,
    pub facing: ParticleFacing,
    /// `true` when a parent rig retires this particle (a landing's pieces die
    /// with its `SchoolImpact`); `false` for a world-space particle that
    /// despawns itself at the end of its life.
    pub owned: bool,
}

/// The drawn body of one Shaman totem (`rendering/effects/totems.rs`): the
/// carved post, its rune plate, the orbiting ribbons and the flame and death
/// emitters.
///
/// A top-level entity DETACHED from the gameplay [`Totem`](super::Totem), which
/// it stands beside but never touches: the sim despawns the totem the moment it
/// expires or is replaced, and the rig outlives it by the death sequence. It
/// notices the despawn itself and starts dying then.
#[derive(Component)]
pub struct TotemRig {
    /// The gameplay totem this rig draws. Once it is gone the rig dies.
    pub totem: Entity,
    pub element: super::TotemElement,
    /// Seconds since the rig was built.
    pub age: f32,
    /// Seconds into the death sequence, from the frame the totem despawned.
    pub death_age: Option<f32>,
    /// The tilting body under the rig; the post, rune plate and orbit hang
    /// off it.
    pub body: Entity,
    pub post: Entity,
    pub rune: Entity,
    pub orbit: Entity,
    /// This rig's own copy of its element's ribbon material, so its ribbons
    /// can fade out at death without fading every other totem's.
    pub ribbon_material: Handle<StandardMaterial>,
    /// Deterministic particle seed (the totem's entity index).
    pub seed: u32,
    /// Particles emitted so far, the running part of every particle seed.
    pub emitted: u32,
    /// Fractional particles owed to the flame, dust and smoke.
    pub carry: [f32; 3],
    /// Sprite and palettes for the flame, dust and smoke particles.
    pub quad: Handle<Mesh>,
    pub palettes: [std::sync::Arc<[Handle<StandardMaterial>]>; 3],
}

/// A child of a [`TotemRig`]: which piece it is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TotemPart {
    /// Pivots at the base; carries the birth wobble and the death tilt.
    Body,
    /// The carved post mesh, rising at birth by its vertical scale.
    Post,
    /// The circular glyph plate on the post's front.
    Rune,
    /// The ribbons, turning about the post's axis.
    Orbit,
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
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct RootedVisual {
    pub style: RootStyle,
}

/// Marker: this unit is stunned and wearing the overhead whirl. See
/// [`RootedVisual`] for the ownership rule.
#[derive(Component)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
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
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
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

// ============================================================================
// Slow treatment — the bind ring and scuff (`rendering/effects/slow_ring.rs`)
// ============================================================================

/// Per-victim state of the slow treatment, present exactly while the unit is
/// alive and carries a routed `MovementSpeedSlow`. Graphical-only: inserted and
/// removed by `update_slow_treatment`, never read by the sim.
#[derive(Component, Clone, Copy, Debug)]
#[component(storage = "SparseSet")] // frame-clock visual state on a sim entity (AS-175)
pub struct SlowTrailEmitter {
    /// Ground point the scuff was last laid to. Distance-paced from here, the
    /// way the charge trail lays its segments.
    pub last_emit: Vec3,
    /// Seconds since the last bind ring. Keeps counting while the unit stands
    /// still, so the first step after a stop pulses at once.
    pub since_pulse: f32,
    /// Whether a Crippling Poison slow was on the unit last frame — the edge
    /// its proc flash fires on.
    pub crippled: bool,
}

/// One pulse of the bind ring: a flat annulus at the victim's feet that grows
/// and fades over its life. It FOLLOWS the victim (see `slow_ring.rs`).
#[derive(Component, Clone, Copy, Debug)]
pub struct SlowBindRing {
    pub owner: Entity,
    /// Seconds since the pulse.
    pub age: f32,
    /// Body-size scale on the ring's diameter: 1.0 for a combatant, smaller
    /// for a pet.
    pub stature: f32,
    pub tint: Color,
    /// The soft outer stroke rather than the core band.
    pub halo: bool,
}

/// One segment of the scuff streak laid along a slowed victim's path. Stays
/// where it was laid and fades.
#[derive(Component, Clone, Copy, Debug)]
pub struct SlowScuff {
    /// Seconds since it was laid.
    pub age: f32,
    /// Body-size scale on its width: 1.0 for a combatant, smaller for a pet.
    pub stature: f32,
    pub tint: Color,
}
