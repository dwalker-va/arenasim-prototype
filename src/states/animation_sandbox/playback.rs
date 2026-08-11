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
use super::super::play_match::ability_config::AbilityDefinitions;
use super::super::play_match::components::{
    ActiveAuras, Celebrating, CastingState, ChannelingState, Combatant, DeathAnimation,
    MatchResults, PlayMatchEntity, VictoryCelebration, VisualBody,
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

/// How an entry has to be started, which is also what decides whether it is
/// playable at all in this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFamily {
    /// Started by inserting a `CastingState`; the sim resolves it.
    HardCast,
    /// Applied inline inside class AI code today. Listed, not yet playable.
    Instant,
    /// Started by inserting the component that drives the motion.
    Body,
}

impl EntryFamily {
    pub fn is_playable(self) -> bool {
        !matches!(self, EntryFamily::Instant)
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
            let family = if config.cast_time > 0.0 {
                EntryFamily::HardCast
            } else {
                EntryFamily::Instant
            };
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

/// Extra pass time for Polymorph, whose subject only EXISTS after the cast
/// resolves: at `cast_time` alone the pass ends one loop tail (0.6s) after the
/// victim becomes a sheep, which is not long enough to read the hop.
const POLYMORPH_HOLD_SECS: f32 = 4.0;

/// Length of one pass of the selected entry.
fn entry_duration(playback: &SandboxPlayback, defs: &AbilityDefinitions) -> f32 {
    match playback.selected {
        Some(SandboxEntry::Ability(ability)) => {
            let cast_time = defs.get(&ability).map(|c| c.cast_time).unwrap_or(1.0);
            if ability == AbilityType::Polymorph {
                cast_time + POLYMORPH_HOLD_SECS
            } else {
                cast_time
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
            if playback.family != Some(EntryFamily::HardCast) {
                return false;
            }
            let Some(config) = defs.get(&ability) else {
                return false;
            };
            // Self-targeting is the honest fallback when no dummy is staged:
            // the cast itself still plays, and the UI separately disables the
            // relational entries rather than letting them look complete.
            let target = dummy.unwrap_or(caster);
            commands
                .entity(caster)
                .insert(CastingState::new(ability, target, config.cast_time));
            true
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
    mut movers: Query<&mut Transform, With<Combatant>>,
) {
    let Some(caster) = stage.caster else { return };
    let entry = playback.playing.then_some(playback.selected).flatten();

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
                let dummy_x = -stage.caster_home.x;
                Vec3::new(dummy_x - MELEE_STANDOFF, stage.caster_home.y, 0.0)
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

    // The dummy is staged mirrored across the origin from the caster, which is
    // also how the auto-attack branch above finds it.
    let Some(dummy) = stage.dummy else { return };
    let dummy_home = Vec3::new(-stage.caster_home.x, stage.caster_home.y, 0.0);
    let dummy_target = match entry {
        Some(SandboxEntry::Ability(AbilityType::Polymorph)) => {
            circle_walk(dummy_home, playback.elapsed, SHEEP_RADIUS, SHEEP_ANGULAR_SPEED)
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
    mut combatants: Query<&mut Combatant>,
) {
    for entity in stage.caster.into_iter().chain(stage.dummy) {
        if let Ok(mut combatant) = combatants.get_mut(entity) {
            combatant.current_health = combatant.max_health;
            combatant.current_mana = combatant.max_mana;
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
    fn instants_are_listed_but_not_playable_in_this_phase() {
        // Hiding them would make partial coverage look like full coverage.
        assert!(!EntryFamily::Instant.is_playable());
        assert!(EntryFamily::HardCast.is_playable());
        assert!(EntryFamily::Body.is_playable());
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
        assert!(mage
            .iter()
            .any(|l| l.family == EntryFamily::HardCast));
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
            EntryFamily::HardCast,
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
