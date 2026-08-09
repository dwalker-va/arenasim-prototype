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
    Celebrating, CastingState, Combatant, DeathAnimation, MatchResults, VictoryCelebration,
    VisualBody,
};
use super::SandboxStage;

/// Seconds the match's victory clock is seeded with when the sandbox plays the
/// winner bounce, and the floor it is never allowed to fall below.
///
/// `update_victory_celebration` is the ONLY thing that animates `Celebrating`,
/// and it early-returns unless a [`VictoryCelebration`] resource exists — so the
/// sandbox has to supply one or the entry plays nothing at all. But that same
/// system transitions to `GameState::Results` the moment the clock reaches zero,
/// which would eject the user from the sandbox. The clock is therefore seeded
/// above one bounce pass and floored every frame, so it drives the bounce and
/// can never fire the transition.
const CELEBRATION_SECS: f32 = 5.0;
const CELEBRATION_FLOOR_SECS: f32 = 1.0;

/// Body motion that no ability triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyAnimation {
    WalkBob,
    DeathSink,
    VictoryBounce,
}

impl BodyAnimation {
    pub const ALL: [BodyAnimation; 3] = [
        BodyAnimation::WalkBob,
        BodyAnimation::DeathSink,
        BodyAnimation::VictoryBounce,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BodyAnimation::WalkBob => "Walk bob",
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
#[derive(Resource, Default)]
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
const LOOP_TAIL_SECS: f32 = 0.6;

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
) {
    let Some(caster) = stage.caster else { return };

    // Floor the victory clock BEFORE anything can return early. Every path out
    // of this system leaves the resource in place for at least a frame, and a
    // clock that reaches zero transitions to `GameState::Results`.
    if let Some(mut celebration) = celebration {
        if celebration.time_remaining < CELEBRATION_FLOOR_SECS {
            celebration.time_remaining = CELEBRATION_FLOOR_SECS;
        }
    }

    if playback.stop_requested {
        playback.stop_requested = false;
        playback.playing = false;
        clear_body_state(&mut commands, caster, &children, &mut bodies);
        return;
    }

    if playback.restart_requested {
        playback.restart_requested = false;
        clear_body_state(&mut commands, caster, &children, &mut bodies);
        if start_entry(&mut commands, &playback, caster, stage.dummy, &defs) {
            playback.playing = true;
            playback.elapsed = 0.0;
            playback.duration = entry_duration(&playback, &defs);
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
            clear_body_state(&mut commands, caster, &children, &mut bodies);
        }
    }
}

/// Length of one pass of the selected entry.
fn entry_duration(playback: &SandboxPlayback, defs: &AbilityDefinitions) -> f32 {
    match playback.selected {
        Some(SandboxEntry::Ability(ability)) => {
            defs.get(&ability).map(|c| c.cast_time).unwrap_or(1.0)
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
                // The walk bob is driven by real movement, so the sandbox has
                // to actually move the unit — see `walk_the_caster`.
                BodyAnimation::WalkBob => {}
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
    caster: Entity,
    children: &Query<&Children>,
    bodies: &mut Query<(&mut Transform, &VisualBody)>,
) {
    if let Ok(mut e) = commands.get_entity(caster) {
        e.remove::<DeathAnimation>();
        e.remove::<Celebrating>();
        e.remove::<CastingState>();
    }
    // Paired with the insert in `start_entry`: left behind, the match's victory
    // clock keeps ticking down to its `GameState::Results` transition.
    commands.remove_resource::<VictoryCelebration>();
    if let Ok(kids) = children.get(caster) {
        for child in kids.iter() {
            if let Ok((mut transform, body)) = bodies.get_mut(child) {
                *transform = Transform::from_xyz(0.0, body.rest_y, 0.0);
            }
        }
    }
}

/// Walks the caster in a slow circle while the walk-bob entry plays.
///
/// `update_walk_animation` keys the bob off real per-tick XZ movement (a
/// deliberate choice — gating it on "moved since last frame" strobed the body
/// at render rates above the tick rate). So showing the bob means actually
/// moving the unit, not faking the component state.
pub fn walk_the_caster(
    playback: Res<SandboxPlayback>,
    stage: Res<SandboxStage>,
    mut movers: Query<&mut Transform, With<Combatant>>,
) {
    let Some(caster) = stage.caster else { return };
    let Ok(mut transform) = movers.get_mut(caster) else {
        return;
    };

    let walking =
        playback.playing && playback.selected == Some(SandboxEntry::Body(BodyAnimation::WalkBob));

    if !walking {
        // Restore the staged position so the next entry plays centred and the
        // camera presets keep framing the unit.
        if transform.translation != stage.caster_home {
            transform.translation = stage.caster_home;
        }
        return;
    }

    // Phase comes from the pass clock, not absolute elapsed time, and the circle
    // is centred one radius BEHIND the staged position — so the walk starts
    // exactly at `caster_home` and eases away from it. Keying the angle off
    // `time.elapsed_secs()` put the caster at an arbitrary point on the circle
    // on its first frame, which teleported it up to 2 * radius and drove
    // `update_walk_animation`'s per-frame XZ delta straight into its
    // `WALK_MAX_PHASE_STEP` clamp — a lurch, in the one entry whose whole
    // subject is how walking reads.
    const WALK_RADIUS: f32 = 3.0;
    const WALK_ANGULAR_SPEED: f32 = 0.8;
    let angle = playback.elapsed * WALK_ANGULAR_SPEED;
    transform.translation.x =
        stage.caster_home.x - WALK_RADIUS + angle.cos() * WALK_RADIUS;
    transform.translation.z = stage.caster_home.z + angle.sin() * WALK_RADIUS;
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
