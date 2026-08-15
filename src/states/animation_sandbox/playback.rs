//! What can be played in the sandbox, and the transport that plays it.
//!
//! ## Entries are abilities, not animation subsystems
//!
//! The user picks "Frostbolt", not "casting orb" — that matches how the game is
//! thought about, and playing a whole ability shows how its effects COMPOSE
//! (orb grows, flash, projectile travels, impact) rather than showing one
//! subsystem in isolation. Body motion that no ability drives (walk bob, death
//! sink, victory bounce) has no home under an ability-only list, so it forms a
//! second category rather than being dropped.
//!
//! ## Families decide how an entry starts
//!
//! A hard cast is started by inserting a [`CastingState`]; the match's own
//! `process_casting` then resolves it, which is what keeps the preview faithful.
//! Instant abilities are applied inline inside each class AI's `try_*`
//! functions with no shared entry point, so they are **not yet playable** — the
//! extraction that makes them playable is Phase B of the plan. They are listed
//! rather than hidden so the gap is visible instead of looking like full
//! coverage.

use bevy::prelude::*;

use super::super::match_config::CharacterClass;
use super::super::play_match::abilities::AbilityType;
use super::super::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use super::super::play_match::components::{
    ActiveAuras, Celebrating, CastingState, ChannelingState, Combatant, DRTracker,
    DeathAnimation, MatchResults, PlayMatchEntity, VictoryCelebration, VisualBody,
};
use super::{SandboxEntity, SandboxStage};

/// Seconds the match's victory clock is seeded with when the sandbox plays the
/// winner bounce, and the floor it is never allowed to fall below.
///
/// `update_victory_celebration` is the ONLY thing that animates `Celebrating`,
/// and it early-returns unless a [`VictoryCelebration`] resource exists — so the
/// sandbox has to supply one or the entry plays nothing at all. But that same
/// system transitions to `GameState::Results` the moment the clock reaches zero,
/// which would eject the user from the sandbox. The clock is therefore REWOUND
/// to the top whenever it falls to the floor, so it drives the bounce forever
/// and can never fire the transition.
///
/// Rewinding rather than clamping matters: `update_victory_celebration` derives
/// its bounce phase from `CELEBRATION_SECS - time_remaining`, so a clamped clock
/// stops advancing and the bounce FREEZES at whatever height it held. The rewind
/// distance is `CELEBRATION_SECS - CELEBRATION_FLOOR_SECS` = 4.0s, an exact
/// multiple of the 0.5s bounce period, so the wrap is phase-continuous and the
/// bounce shows no seam.
const CELEBRATION_SECS: f32 = 5.0;
const CELEBRATION_FLOOR_SECS: f32 = 1.0;
/// Period of the celebration bounce (`update_victory_celebration` runs it at
/// 2 Hz). The rewind distance must be a whole number of these; the invariant is
/// enforced by test rather than computed from, so this is test-only.
#[cfg(test)]
const CELEBRATION_BOUNCE_PERIOD_SECS: f32 = 0.5;

/// Body motion that no ability triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAnimation {
    WalkBob,
    /// Weapon swings driven by the real attack timer. Its own entry because the
    /// sandbox never sets a target, and `combat_auto_attack` needs one — so
    /// without this there was no way to preview the swing animations at all,
    /// which matters most now that weapons are visible.
    AutoAttack,
    DeathSink,
    VictoryBounce,
}

impl BodyAnimation {
    pub const ALL: [BodyAnimation; 4] = [
        BodyAnimation::WalkBob,
        BodyAnimation::AutoAttack,
        BodyAnimation::DeathSink,
        BodyAnimation::VictoryBounce,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BodyAnimation::WalkBob => "Walk bob",
            BodyAnimation::AutoAttack => "Auto attack",
            BodyAnimation::DeathSink => "Death sink",
            BodyAnimation::VictoryBounce => "Victory bounce",
        }
    }

    /// How long one pass reads for. Death has a real animation duration; the
    /// other two are continuous, so these are display windows before a loop
    /// restarts rather than intrinsic lengths.
    pub fn duration(self) -> f32 {
        match self {
            BodyAnimation::WalkBob => 3.0,
            // Long enough for several swings at any weapon speed.
            BodyAnimation::AutoAttack => 6.0,
            BodyAnimation::DeathSink => DeathAnimation::DURATION,
            BodyAnimation::VictoryBounce => 3.0,
        }
    }
}

/// One thing the user can select and play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEntry {
    Ability(AbilityType),
    Body(BodyAnimation),
}

/// How an entry is started — the *application mechanism*, not `cast_time`. This
/// is the keystone classification (KTD1): it decides which start path an entry
/// takes and whether it is playable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFamily {
    /// Routed through `process_casting` by inserting a `CastingState` with the
    /// ability's own `cast_time` (0.0 for instants). Covers hard casts AND
    /// single-target damage/aura/projectile instants (M1) — the same code path.
    Cast,
    /// Channeled — inserts a `ChannelingState` (M2). Drain Life.
    Channel,
    /// Inserts a bespoke `*Pending` or movement component whose resolver / a
    /// sandbox-owned driver produces the visual (M3).
    Component,
    /// Spawns a world entity (totem, trap) or issues a `PetCommand` (M4).
    Entity,
    /// M1 for the aura plus a directly-spawned caster cosmetic (Psychic Scream).
    Residue,
    /// Body motion started by inserting the driving component.
    Body,
    /// Defined as data (`abilities.ron`) but with no application code, so it has
    /// nothing to preview. Wind Shear only.
    Unsupported,
}

impl EntryFamily {
    /// Whether this mechanism's start path is wired yet. Extended as each
    /// mechanism's unit lands; `Unsupported` is never playable.
    pub fn is_playable(self) -> bool {
        matches!(
            self,
            EntryFamily::Cast | EntryFamily::Channel | EntryFamily::Body
        )
    }
}

/// Aura types applied to an ENEMY (debuffs / CC). Used by the target rule
/// (KTD5): an offensive entry aims at the dummy, a friendly buff at the caster.
fn is_hostile_aura(aura: super::super::play_match::components::AuraType) -> bool {
    use super::super::play_match::components::AuraType::*;
    matches!(
        aura,
        MovementSpeedSlow
            | Root
            | Stun
            | DamageOverTime
            | SpellSchoolLockout
            | HealingReduction
            | Fear
            | Polymorph
            | CastTimeIncrease
            | Incapacitate
            | AttackPowerReduction
            | AttackSpeedSlow
            | Silence
            | WeakenedSoul
    )
}

/// Whether a Cast/Channel entry should aim at the dummy (offensive/relational)
/// rather than the caster (self/friendly buff). Damage, mana burn, an interrupt,
/// or a hostile aura mark it offensive.
fn entry_targets_dummy(config: &AbilityConfig) -> bool {
    if config.damage_base_max > 0.0 || config.mana_burn_amount > 0.0 || config.is_interrupt {
        return true;
    }
    match config.applies_aura.as_ref() {
        Some(aura) => is_hostile_aura(aura.aura_type),
        None => false,
    }
}

/// Classifies an ability by its application mechanism (KTD1). Config fields
/// resolve channels and the Cast default; a small explicit table handles the
/// component/entity/residue/unsupported cases config alone can't distinguish.
fn mechanism_for(ability: AbilityType, config: &AbilityConfig) -> EntryFamily {
    use AbilityType::*;
    match ability {
        // M3 — bespoke `*Pending` (resolvers run in the sandbox) and the two
        // movement instants (sandbox-owned dash driver, KTD6).
        DivineShield | BerserkerRage | HolyShock | DispelMagic | PaladinCleanse | Purge
        | DevourMagic | Charge | Disengage => EntryFamily::Component,
        // M4 — world-entity drops and pet-command dispatch.
        AirTotem | WaterTotem | EarthTotem | FireTotem | FreezingTrap | FrostTrap | SpiderWeb
        | BoarCharge | MastersCall => EntryFamily::Entity,
        // M1 aura + a directly-spawned caster cosmetic.
        PsychicScream => EntryFamily::Residue,
        // Data-only (no application code) / no distinct visual beyond the swing.
        WindShear | HeroicStrike => EntryFamily::Unsupported,
        // Config-derived default: channels, else Cast (hard casts and every
        // single-target damage/aura/projectile instant resolve through a
        // `CastingState`).
        _ => {
            if config.channel_duration.is_some() {
                EntryFamily::Channel
            } else {
                EntryFamily::Cast
            }
        }
    }
}

/// A selectable row in the entry list.
pub struct EntryListing {
    pub entry: SandboxEntry,
    pub family: EntryFamily,
    pub label: String,
}

/// Builds the entry list for one caster class.
///
/// Ability rows come from the class's own ability list joined against the
/// loaded [`AbilityDefinitions`], so the list tracks the config data rather
/// than a hand-maintained copy of it.
pub fn entries_for_class(class: CharacterClass, defs: &AbilityDefinitions) -> Vec<EntryListing> {
    let mut listings: Vec<EntryListing> = super::super::view_combatant_ui::get_class_abilities(class)
        .into_iter()
        .filter_map(|ability| {
            let config = defs.get(&ability)?;
            let family = mechanism_for(ability, config);
            Some(EntryListing {
                entry: SandboxEntry::Ability(ability),
                family,
                label: config.name.clone(),
            })
        })
        .collect();

    listings.extend(BodyAnimation::ALL.into_iter().map(|body| EntryListing {
        entry: SandboxEntry::Body(body),
        family: EntryFamily::Body,
        label: body.label().to_string(),
    }));

    listings
}

/// Transport state: what is selected, whether it is running, and where it is.
#[derive(Resource)]
pub struct SandboxPlayback {
    pub selected: Option<SandboxEntry>,
    pub family: Option<EntryFamily>,
    pub looping: bool,
    pub playing: bool,
    /// Seconds elapsed within the current pass.
    pub elapsed: f32,
    /// Length of one pass, for the position readout.
    pub duration: f32,
    /// Set by the UI to (re)start the selected entry on the next tick.
    pub restart_requested: bool,
    /// Set by the UI to stop the current entry and return the caster to rest on
    /// the next tick.
    pub stop_requested: bool,
    /// Set by the UI to advance exactly one fixed tick while paused.
    pub step_requested: bool,
    /// Speed to return to when the transport unpauses.
    ///
    /// Held so Resume and Play restore the rung actually being watched. Assuming
    /// 1x would silently throw away a 0.1x setting the moment the user paused to
    /// look at something.
    pub resume_speed: f32,
}

impl Default for SandboxPlayback {
    fn default() -> Self {
        Self {
            selected: None,
            family: None,
            looping: false,
            playing: false,
            elapsed: 0.0,
            duration: 0.0,
            restart_requested: false,
            stop_requested: false,
            step_requested: false,
            resume_speed: 1.0,
        }
    }
}

impl SandboxPlayback {
    /// Selects an entry without starting it.
    pub fn select(&mut self, entry: SandboxEntry, family: EntryFamily) {
        self.selected = Some(entry);
        self.family = Some(family);
        self.elapsed = 0.0;
        self.playing = false;
    }

    /// Stops the current entry and asks the driver to tear its body state down.
    ///
    /// Clearing `playing` alone is NOT enough: [`drive_playback`] returns early
    /// when it is false, so a `Celebrating` / `DeathAnimation` / `CastingState`
    /// left on the caster would never be removed — and the victory clock the
    /// bounce needs would keep running toward its Results transition.
    pub fn stop(&mut self) {
        self.playing = false;
        self.restart_requested = false;
        self.stop_requested = true;
    }

    /// Stops playback AND drops the selection.
    ///
    /// Used when the caster class changes: entries are per-class, so a
    /// selection made against the previous class would otherwise stay live in
    /// the transport and let `Play` cast, say, a Mage's Frostbolt on a Warrior.
    pub fn clear_selection(&mut self) {
        self.stop();
        self.selected = None;
        self.family = None;
        self.elapsed = 0.0;
        self.duration = 0.0;
    }
}

/// Tail held after a pass finishes before a loop restarts, so the last frames
/// of an effect are seen rather than being cut off by the next pass.
pub const LOOP_TAIL_SECS: f32 = 0.6;

/// Starts, advances, and loops the selected entry.
pub fn drive_playback(
    mut commands: Commands,
    time: Res<Time>,
    mut playback: ResMut<SandboxPlayback>,
    stage: Res<SandboxStage>,
    defs: Res<AbilityDefinitions>,
    mut bodies: Query<(&mut Transform, &VisualBody)>,
    children: Query<&Children>,
    celebration: Option<ResMut<VictoryCelebration>>,
    mut auras: Query<&mut ActiveAuras>,
    mut combatants: Query<&mut Combatant>,
    leftovers: Query<Entity, (With<PlayMatchEntity>, Without<SandboxEntity>)>,
) {
    let Some(caster) = stage.caster else { return };

    // Floor the victory clock BEFORE anything can return early. Every path out
    // of this system leaves the resource in place for at least a frame, and a
    // clock that reaches zero transitions to `GameState::Results`.
    if let Some(mut celebration) = celebration {
        if celebration.time_remaining < CELEBRATION_FLOOR_SECS {
            celebration.time_remaining = CELEBRATION_SECS;
        }
    }

    if playback.stop_requested {
        playback.stop_requested = false;
        playback.playing = false;
        clear_body_state(&mut commands, &stage, &children, &mut bodies, &mut auras, &mut combatants, &leftovers);
        return;
    }

    if playback.restart_requested {
        playback.restart_requested = false;
        clear_body_state(&mut commands, &stage, &children, &mut bodies, &mut auras, &mut combatants, &leftovers);
        if start_entry(&mut commands, &playback, caster, stage.dummy, &defs) {
            playback.playing = true;
            playback.elapsed = 0.0;
            playback.duration = entry_duration(&playback, &defs);
            // Auto-attack is the ONLY entry that wants a live target:
            // `combat_auto_attack` skips any combatant without one. Every other
            // entry leaves it cleared so nothing swings under the animation
            // being judged.
            if playback.selected == Some(SandboxEntry::Body(BodyAnimation::AutoAttack)) {
                if let (Some(dummy), Ok(mut combatant)) =
                    (stage.dummy, combatants.get_mut(caster))
                {
                    combatant.target = Some(dummy);
                }
            }
        }
        return;
    }

    if !playback.playing {
        return;
    }

    playback.elapsed += time.delta_secs();

    if playback.elapsed >= playback.duration + LOOP_TAIL_SECS {
        if playback.looping {
            playback.restart_requested = true;
        } else {
            playback.playing = false;
            clear_body_state(&mut commands, &stage, &children, &mut bodies, &mut auras, &mut combatants, &leftovers);
        }
    }
}

/// Extra pass time held after an aura-applying cast resolves, so the applied
/// state is actually watchable. A CC's/DoT's/buff's subject only EXISTS after
/// the cast lands: at `cast_time` alone the pass ends one loop tail (0.6s) after
/// the aura applies, far too short to read (e.g. a sheep's hop, a fear panic
/// run, a shield bubble). Generalizes the old per-ability Polymorph/Fear holds
/// to every aura entry (KTD5); non-aura casts (pure damage) need no hold.
const AURA_HOLD_SECS: f32 = 4.0;

/// Length of one pass of the selected entry.
fn entry_duration(playback: &SandboxPlayback, defs: &AbilityDefinitions) -> f32 {
    match playback.selected {
        Some(SandboxEntry::Ability(ability)) => {
            let Some(config) = defs.get(&ability) else {
                return 1.0;
            };
            // Channels run for their channel duration; aura entries hold past
            // cast_time so the applied state reads; everything else is cast_time.
            if let Some(channel) = config.channel_duration {
                channel
            } else if config.applies_aura.is_some() {
                config.cast_time + AURA_HOLD_SECS
            } else {
                config.cast_time
            }
        }
        Some(SandboxEntry::Body(body)) => body.duration(),
        None => 0.0,
    }
}

/// Starts the entry. Returns false when it could not start (no target for an
/// ability that needs one, or an instant that has no seam yet).
fn start_entry(
    commands: &mut Commands,
    playback: &SandboxPlayback,
    caster: Entity,
    dummy: Option<Entity>,
    defs: &AbilityDefinitions,
) -> bool {
    match playback.selected {
        Some(SandboxEntry::Ability(ability)) => {
            let Some(config) = defs.get(&ability) else {
                return false;
            };
            // Target rule (KTD5): offensive/relational entries aim at the dummy
            // (self is the honest fallback when none is staged, and the UI
            // separately disables relational entries dummy-off); self/friendly
            // buffs aim at the caster, so the buff visual lands on the right unit.
            let target = if entry_targets_dummy(config) {
                dummy.unwrap_or(caster)
            } else {
                caster
            };
            match playback.family {
                // M1 / hard casts — resolve through process_casting.
                Some(EntryFamily::Cast) => {
                    commands
                        .entity(caster)
                        .insert(CastingState::new(ability, target, config.cast_time));
                    true
                }
                // M2 channels — shared entry point (mirror warlock.rs:819);
                // process_channeling resolves the ticks and drives the beam.
                Some(EntryFamily::Channel) => {
                    commands.entity(caster).insert(ChannelingState {
                        ability,
                        duration_remaining: config.channel_duration.unwrap_or(5.0),
                        time_until_next_tick: config.channel_tick_interval,
                        tick_interval: config.channel_tick_interval,
                        target,
                        interrupted: false,
                        interrupted_display_time: 0.0,
                        ticks_applied: 0,
                    });
                    true
                }
                _ => false,
            }
        }
        Some(SandboxEntry::Body(body)) => {
            match body {
                BodyAnimation::DeathSink => {
                    commands.entity(caster).insert(DeathAnimation {
                        progress: 0.0,
                        fall_direction: Vec3::X,
                    });
                }
                BodyAnimation::VictoryBounce => {
                    commands
                        .entity(caster)
                        .insert(Celebrating { bounce_offset: 0.0 });
                    // `Celebrating` is inert on its own: the bounce is written
                    // by `update_victory_celebration`, which bails without this
                    // resource — and `update_walk_animation` explicitly cedes
                    // the Y axis to it, so the caster would stand frozen.
                    commands.insert_resource(VictoryCelebration {
                        winner: None,
                        time_remaining: CELEBRATION_SECS,
                        match_results: MatchResults {
                            winner: None,
                            duration_secs: 0.0,
                            team1_combatants: Vec::new(),
                            team2_combatants: Vec::new(),
                            pet_damage_links: Default::default(),
                        },
                    });
                }
                // Both of these are driven from `position_caster`, which owns
                // the combatant transform: the bob needs real movement, and the
                // swing needs the caster inside its own weapon's range.
                BodyAnimation::WalkBob | BodyAnimation::AutoAttack => {}
            }
            true
        }
        None => false,
    }
}

/// Removes body-animation state and returns the visual body to rest.
///
/// `animate_death` writes the `VisualBody` child's transform, so removing the
/// component alone would leave the caster lying on the floor for the next pass.
fn clear_body_state(
    commands: &mut Commands,
    stage: &SandboxStage,
    children: &Query<&Children>,
    bodies: &mut Query<(&mut Transform, &VisualBody)>,
    auras: &mut Query<&mut ActiveAuras>,
    combatants: &mut Query<&mut Combatant>,
    leftovers: &Query<Entity, (With<PlayMatchEntity>, Without<SandboxEntity>)>,
) {
    let Some(caster) = stage.caster else { return };

    if let Ok(mut e) = commands.get_entity(caster) {
        e.remove::<DeathAnimation>();
        e.remove::<Celebrating>();
        e.remove::<CastingState>();
        e.remove::<ChannelingState>();
    }
    // Paired with the insert in `start_entry`: left behind, the match's victory
    // clock keeps ticking down to its `GameState::Results` transition.
    commands.remove_resource::<VictoryCelebration>();

    // Auras are the real state carrier, and clearing them is what returns the
    // units to a PRISTINE look rather than just an idle one. Several visuals are
    // driven by aura presence and only revert when the aura goes: the Polymorph
    // mesh swap (`update_polymorph_visuals` restores the capsule when the aura
    // is gone — otherwise you get a sheep holding an axe, because the weapon
    // sockets are unaffected by the swap), the Unstable Affliction glow, DoT
    // drips, shield bubbles, and ice blocks. Without this, previewing Polymorph
    // and then switching entries left the dummy transformed indefinitely.
    for unit in stage.caster.into_iter().chain(stage.dummy) {
        if let Ok(mut active) = auras.get_mut(unit) {
            active.auras.clear();
        }
        if let Ok(mut combatant) = combatants.get_mut(unit) {
            combatant.target = None;
        }
    }

    // Effects spawned by the shared resolution systems are tagged
    // `PlayMatchEntity`, not `SandboxEntity` — projectiles still in flight,
    // impact bursts, casting orbs, floating text. The staged units carry BOTH
    // markers, so excluding `SandboxEntity` is what keeps this from despawning
    // the caster and dummy along with the debris.
    for entity in leftovers.iter() {
        commands.entity(entity).despawn();
    }

    if let Ok(kids) = children.get(caster) {
        for child in kids.iter() {
            if let Ok((mut transform, body)) = bodies.get_mut(child) {
                *transform = Transform::from_xyz(0.0, body.rest_y, 0.0);
            }
        }
    }
}

/// Distance a melee caster stands from the dummy for the auto-attack entry.
/// Comfortably inside `MELEE_RANGE` (2.5) so no swing is dropped for being a
/// hair out of range.
const MELEE_STANDOFF: f32 = 2.0;

/// Radius and angular speed of the circle a staged unit walks for the
/// position-driven entries. The radius is deliberately small: at 3yd the unit
/// walked clean out of the camera presets' shot, which made the entries whose
/// whole subject is locomotion the hardest ones to actually watch.
const WALK_RADIUS: f32 = 1.4;
const WALK_ANGULAR_SPEED: f32 = 1.4;

/// The sheep's circle. Slower than the caster's walk because the hop is what is
/// being judged, and in a match the polymorph wander runs at 20% movement speed
/// — a sheep sprinting a circle would misrepresent the gait it drives.
const SHEEP_RADIUS: f32 = 1.0;
const SHEEP_ANGULAR_SPEED: f32 = 0.9;

/// Walks a unit around a circle centred one radius BEHIND `home`, so the walk
/// starts exactly at `home` and eases away from it.
///
/// Phase comes from the pass clock, not absolute elapsed time. Keying the angle
/// off `time.elapsed_secs()` put the unit at an arbitrary point on the circle on
/// its first frame, teleporting it up to 2 * radius and driving the gait
/// systems' per-frame XZ delta into their `WALK_MAX_PHASE_STEP` clamp.
fn circle_walk(home: Vec3, elapsed: f32, radius: f32, angular_speed: f32) -> Vec3 {
    let angle = elapsed * angular_speed;
    Vec3::new(
        home.x - radius + angle.cos() * radius,
        home.y,
        home.z + angle.sin() * radius,
    )
}

/// Places the staged units for whichever entry is playing.
///
/// Owns the combatant transforms so the position-driven entries share a single
/// writer:
///
/// - **Walk bob** — `update_walk_animation` keys the bob off real per-tick XZ
///   movement (gating it on "moved since last frame" strobed the body at render
///   rates above the tick rate), so showing the bob means actually moving the
///   unit rather than faking component state.
/// - **Auto attack** — `combat_auto_attack` only swings inside the attacker's
///   own range, so a melee caster left at the staged 16yd separation would
///   never swing at all.
/// - **Polymorph** — `update_sheep_hop` is distance-driven for the same reason,
///   so the DUMMY (the unit that turns into a sheep) is what walks here.
///
/// Every other entry returns both units to their staged positions, so the next
/// one plays centred and the camera presets keep framing it.
pub fn position_caster(
    playback: Res<SandboxPlayback>,
    stage: Res<SandboxStage>,
    config: Res<super::SandboxConfig>,
    defs: Res<AbilityDefinitions>,
    mut movers: Query<&mut Transform, With<Combatant>>,
) {
    let Some(caster) = stage.caster else { return };
    let entry = playback.playing.then_some(playback.selected).flatten();

    // The dummy is staged mirrored across the origin from the caster; both the
    // auto-attack approach and the dummy's own staging below key off this.
    let dummy_home = Vec3::new(-stage.caster_home.x, stage.caster_home.y, 0.0);

    let target = match entry {
        Some(SandboxEntry::Body(BodyAnimation::WalkBob)) => circle_walk(
            stage.caster_home,
            playback.elapsed,
            WALK_RADIUS,
            WALK_ANGULAR_SPEED,
        ),
        Some(SandboxEntry::Body(BodyAnimation::AutoAttack)) => {
            // Melee must close. Ranged is already in range at the staged
            // separation and must NOT close: the Hunter's Auto Shot is
            // cancelled inside its dead zone, so walking it in would silence
            // the very animation being previewed.
            if config.caster_class.is_melee() {
                dummy_home - Vec3::X * MELEE_STANDOFF
            } else {
                stage.caster_home
            }
        }
        _ => stage.caster_home,
    };

    if let Ok(mut transform) = movers.get_mut(caster) {
        if transform.translation != target {
            transform.translation = target;
        }
    }

    let Some(dummy) = stage.dummy else { return };
    let dummy_target = match entry {
        // CC entries: the dummy is the VICTIM, and its distance-driven gait
        // (sheep hop / panic run) needs it to actually move or the gait shows
        // nothing. Hold it still until the cast lands: driving the circle by
        // (elapsed - cast_time) keeps the dummy at home (circle angle 0 = home)
        // through the cast, then walks it out from home once the aura applies —
        // so the sandbox no longer shows the victim fleeing before it is hit.
        Some(SandboxEntry::Ability(ability @ (AbilityType::Polymorph | AbilityType::Fear))) => {
            let cast_time = defs.get(&ability).map(|c| c.cast_time).unwrap_or(1.5);
            let walk_elapsed = (playback.elapsed - cast_time).max(0.0);
            // The panic run reads faster / more erratic than the sheep hop, so
            // Fear circles at the caster's brisker walk pace, Polymorph at the
            // sheep's deliberately slow one.
            let (radius, speed) = match ability {
                AbilityType::Polymorph => (SHEEP_RADIUS, SHEEP_ANGULAR_SPEED),
                _ => (WALK_RADIUS, WALK_ANGULAR_SPEED),
            };
            circle_walk(dummy_home, walk_elapsed, radius, speed)
        }
        _ => dummy_home,
    };

    if let Ok(mut transform) = movers.get_mut(dummy) {
        if transform.translation != dummy_target {
            transform.translation = dummy_target;
        }
    }
}

/// Keeps the staged units alive and castable across repeated plays.
///
/// Deliberately a sandbox-only restore rather than an immunity aura: a new
/// `DamageImmunity`-style mechanic would be simulation-visible and could leak
/// into matches. Restoring health here leaves the damage path itself untouched.
pub fn sustain_staged_units(
    stage: Res<SandboxStage>,
    mut combatants: Query<(&mut Combatant, Option<&mut DRTracker>)>,
) {
    for entity in stage.caster.into_iter().chain(stage.dummy) {
        if let Ok((mut combatant, dr)) = combatants.get_mut(entity) {
            combatant.current_health = combatant.max_health;
            combatant.current_mana = combatant.max_mana;
            // Diminishing returns escalate per CC application and the reset
            // timer re-arms each time — a looping CC entry (Polymorph, Fear)
            // re-applies every ~6s, so by the third pass the dummy is IMMUNE and
            // the effect silently stops. Resetting every staged unit's tracker
            // here covers any such entry without per-ability wiring. Same sustain
            // rationale as the health restore above: keep the entry replayable,
            // leave sim code alone.
            if let Some(mut dr) = dr {
                dr.reset();
            }
            // Rogues spawn stealthed (`Combatant::new`), and in a match the
            // class AI breaks stealth on its opener. No class AI runs here, so
            // a staged Rogue would stay stealthed forever — and stealth fades
            // both the body and the weapon materials, so every animation it
            // plays, the auto-attack swing most visibly, renders invisible.
            //
            // If a stealth preview is ever added as its own entry, this has to
            // become conditional on that entry rather than unconditional.
            combatant.stealthed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wired_mechanisms_are_playable_and_unsupported_never_is() {
        // Wired so far: Cast (hard casts + M1 instants), Channel, Body.
        // Component/Entity/Residue land in later units; Unsupported never plays.
        assert!(EntryFamily::Cast.is_playable());
        assert!(EntryFamily::Channel.is_playable());
        assert!(EntryFamily::Body.is_playable());
        assert!(!EntryFamily::Unsupported.is_playable());
    }

    #[test]
    fn mechanism_classification_matches_application_shape() {
        use AbilityType::*;
        let defs = AbilityDefinitions::default();
        let mech = |a: AbilityType| mechanism_for(a, defs.get(&a).unwrap());
        // Channel, not a not-playable "instant" — regression on the cast_time
        // classifier bug (KTD1).
        assert_eq!(mech(DrainLife), EntryFamily::Channel);
        // Frost Shock is cast_time 0 but Cast-routable, so it is playable.
        assert_eq!(mech(FrostShock), EntryFamily::Cast);
        assert!(mech(FrostShock).is_playable());
        // Representative mechanism buckets.
        assert_eq!(mech(IceBarrier), EntryFamily::Cast); // self buff
        assert_eq!(mech(Corruption), EntryFamily::Cast); // offensive aura
        assert_eq!(mech(EarthTotem), EntryFamily::Entity); // drop
        assert_eq!(mech(FreezingTrap), EntryFamily::Entity); // drop
        assert_eq!(mech(SpiderWeb), EntryFamily::Entity); // pet command
        assert_eq!(mech(DivineShield), EntryFamily::Component);
        assert_eq!(mech(Charge), EntryFamily::Component); // movement
        assert_eq!(mech(PsychicScream), EntryFamily::Residue);
        assert_eq!(mech(WindShear), EntryFamily::Unsupported);
    }

    #[test]
    fn every_ability_classifies_and_only_two_are_unsupported() {
        // Wind Shear is data-only (no application code); Heroic Strike's
        // next-swing bonus has no distinct cast visual (preview via Auto attack).
        // Everything else maps to a real, previewable mechanism.
        let defs = AbilityDefinitions::default();
        let unsupported: Vec<AbilityType> = defs
            .iter()
            .filter(|(a, c)| mechanism_for(**a, c) == EntryFamily::Unsupported)
            .map(|(a, _)| *a)
            .collect();
        assert_eq!(unsupported.len(), 2, "unexpected Unsupported set: {unsupported:?}");
        assert!(unsupported.contains(&AbilityType::WindShear));
        assert!(unsupported.contains(&AbilityType::HeroicStrike));
    }

    #[test]
    fn target_rule_sends_buffs_to_caster_and_offense_to_dummy() {
        let defs = AbilityDefinitions::default();
        let cfg = |a: AbilityType| defs.get(&a).unwrap();
        assert!(!entry_targets_dummy(cfg(AbilityType::IceBarrier))); // self buff
        assert!(!entry_targets_dummy(cfg(AbilityType::ArcaneIntellect))); // ally buff
        assert!(entry_targets_dummy(cfg(AbilityType::Corruption))); // DoT
        assert!(entry_targets_dummy(cfg(AbilityType::MortalStrike))); // damage
    }

    #[test]
    fn every_class_lists_its_abilities_plus_the_body_animations() {
        let defs = AbilityDefinitions::default();
        for &class in CharacterClass::all() {
            let listings = entries_for_class(class, &defs);
            let bodies = listings
                .iter()
                .filter(|l| l.family == EntryFamily::Body)
                .count();
            assert_eq!(bodies, BodyAnimation::ALL.len(), "{class:?} body entries");
            assert!(
                listings.len() > bodies,
                "{class:?} listed no abilities at all"
            );
        }
    }

    #[test]
    fn a_class_with_hard_casts_has_at_least_one_playable_ability() {
        // Phase A's usefulness depends on this: a caster class must offer
        // something that actually plays.
        let defs = AbilityDefinitions::default();
        let mage = entries_for_class(CharacterClass::Mage, &defs);
        assert!(mage.iter().any(|l| l.family == EntryFamily::Cast));
    }

    #[test]
    fn the_victory_clock_outlasts_a_bounce_pass_without_reaching_its_floor() {
        // `update_victory_celebration` derives the bounce phase from
        // `CELEBRATION_SECS - time_remaining`, and the floor freezes that phase.
        // If a pass could outlive the unfloored part of the clock, the bounce
        // would stall visibly partway through — and if the floor were removed,
        // the clock would hit zero and eject the user to the Results screen.
        let pass = BodyAnimation::VictoryBounce.duration() + LOOP_TAIL_SECS;
        assert!(pass < CELEBRATION_SECS - CELEBRATION_FLOOR_SECS);
        assert!(CELEBRATION_FLOOR_SECS > 0.0);
    }

    #[test]
    fn the_victory_clock_rewind_is_phase_continuous() {
        // The clock is REWOUND at the floor rather than clamped there, because a
        // clamped clock stops advancing and freezes the bounce mid-air. The
        // rewind distance must be a whole number of bounce periods, or the wrap
        // shows as a visible hitch every few seconds.
        let rewind = CELEBRATION_SECS - CELEBRATION_FLOOR_SECS;
        let periods = rewind / CELEBRATION_BOUNCE_PERIOD_SECS;
        assert!(
            (periods - periods.round()).abs() < 1e-5,
            "rewind of {rewind}s is {periods} bounce periods, not a whole number"
        );
    }

    #[test]
    fn clearing_the_selection_stops_playback_and_drops_the_entry() {
        // Entries are per-class, so a stale selection surviving a caster change
        // would let `Play` cast an ability the staged class does not have.
        let mut playback = SandboxPlayback::default();
        playback.select(
            SandboxEntry::Ability(AbilityType::Frostbolt),
            EntryFamily::Cast,
        );
        playback.playing = true;
        playback.restart_requested = true;

        playback.clear_selection();

        assert_eq!(playback.selected, None);
        assert_eq!(playback.family, None);
        assert!(!playback.playing);
        assert!(!playback.restart_requested);
        assert!(playback.stop_requested, "the driver must tear body state down");
    }
}
