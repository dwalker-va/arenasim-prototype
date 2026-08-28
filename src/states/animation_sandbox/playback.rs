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
    ActiveAuras, AuraPending, AuraType, BerserkerRagePending, Celebrating, CastingState, ChannelingState,
    ChargingState, Combatant, DRTracker, DeathAnimation, DisengagingState, DispelPending,
    DivineShieldPending, HolyShockDamagePending, HolyShockHealPending, InstantAbilityFired,
    MatchResults, Pet,
    PetType, PlayMatchEntity, ScreamBurst, Totem, TotemElement, TrapType, VictoryCelebration,
    VisualBody,
};
use super::super::play_match::class_ai::hunter::spawn_trap;
use super::super::play_match::class_ai::pet_ai::{
    execute_boar_charge, execute_masters_call, execute_spell_lock, execute_spider_web,
};
use super::super::play_match::class_ai::shaman::{totem_spec, totem_spacing_offset};
use super::super::play_match::spawn_pet;
use super::super::play_match::{DISENGAGE_SPEED, MELEE_RANGE, TOTEM_DURATION, TOTEM_RADIUS};
use crate::combat::log::CombatLog;
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
    /// M1 for the outcome — a zero-duration `CastingState`, so `process_casting`
    /// applies damage, floating text and the aura from the same functions a
    /// match uses — PLUS the caster cosmetic that path cannot produce, spawned
    /// directly by [`spawn_sandbox_cosmetic`] (KTD3).
    ///
    /// Covers every ability whose visual is emitted inline by code the sandbox
    /// does not run: the class AIs (gated `in_state(GameState::PlayMatch)`) and
    /// `combat_ai.rs`'s `QueuedInstantAttack` drain. Leaving one of these on the
    /// `Cast` default fails SILENTLY — the preview still shows damage, so it
    /// looks fine, right up until the ability's signature never fires.
    ///
    /// This absorbed the former `InstantMelee` family, which was the identical
    /// mechanism under a name that had stopped being true: Hammer of Justice is
    /// a 10yd Holy stun and Frost Nova a caster-centred PBAoE, neither of them
    /// melee.
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
            EntryFamily::Cast
                | EntryFamily::Channel
                | EntryFamily::Component
                | EntryFamily::Entity
                | EntryFamily::Residue
                | EntryFamily::Body
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

/// Whether an entry's visual REQUIRES a staged dummy — a relational entry that
/// would otherwise play an empty or self-targeted pass. Drives the UI's
/// dummy-off disable (AE3). Totems / traps (a fixed ground drop), self-buffs,
/// and Master's Call (cleanses the caster) do NOT need one; the four
/// dummy-targeting pet commands have no self fallback (no dummy → no pet), and
/// every other offensive/relational entry aims at the dummy.
pub(crate) fn entry_needs_dummy(entry: SandboxEntry, defs: &AbilityDefinitions) -> bool {
    let SandboxEntry::Ability(ability) = entry else {
        return false;
    };
    use AbilityType::*;
    match ability {
        SpiderWeb | BoarCharge | SpellLock | DevourMagic => true,
        AirTotem | WaterTotem | EarthTotem | FireTotem | FreezingTrap | FrostTrap | MastersCall => {
            false
        }
        _ => defs.get(&ability).map(entry_targets_dummy).unwrap_or(false),
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
        | Charge | Disengage => EntryFamily::Component,
        // M4 — world-entity drops and pet-dispatched abilities (Hunter pets +
        // the Warlock's Felhunter), all driven by drive_sandbox_pet.
        AirTotem | WaterTotem | EarthTotem | FireTotem | FreezingTrap | FrostTrap | SpiderWeb
        | BoarCharge | MastersCall | SpellLock | DevourMagic => EntryFamily::Entity,
        // M1 for the outcome plus a directly-spawned caster cosmetic. Every
        // ability whose visual is emitted by code the sandbox does not run
        // belongs here, signature or not: the family is about the application
        // MECHANISM (KTD1), and one classified as `Cast` would silently fail to
        // fire a flourish the day it gets one. Kept in sync with combat code by
        // `InstantAbilityFired::is_spawned_for` — see the audit test below.
        PsychicScream
        | MortalStrike | Ambush | SinisterStrike
        | CheapShot | KidneyShot | HammerOfJustice | FrostNova => EntryFamily::Residue,
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
    let mut abilities = super::super::view_combatant_ui::get_class_abilities(class);
    // The Hunter's three pet-command abilities are the PET's, so they are not in
    // `get_class_abilities` (the View Combatant screen lists the Hunter's own
    // abilities). The sandbox previews them via the staged pet, so append them
    // here — sandbox-only, without touching the shared class-ability list.
    if class == CharacterClass::Hunter {
        abilities.extend([
            AbilityType::SpiderWeb,
            AbilityType::BoarCharge,
            AbilityType::MastersCall,
        ]);
    }
    // The Warlock's Felhunter has two abilities (Spell Lock, Devour Magic),
    // likewise not in the shared class list — appended for the same reason.
    if class == CharacterClass::Warlock {
        abilities.extend([AbilityType::SpellLock, AbilityType::DevourMagic]);
    }
    let mut listings: Vec<EntryListing> = abilities
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
        // Snapshot the caster's identity/stats for component-mechanism entries
        // whose `*Pending` carries them to its resolver (Holy Shock scales off
        // spell power / crit; Divine Shield / Berserker Rage / dispels need
        // team/slot/class).
        let caster_info = combatants.get(caster).ok().map(|c| CasterInfo {
            team: c.team,
            slot: c.slot,
            class: c.class,
            spell_power: c.spell_power,
            crit_chance: c.crit_chance,
        });
        if start_entry(&mut commands, &playback, caster, stage.caster_home, stage.dummy, &defs, caster_info) {
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
            // A dispel needs a dispellable aura present BEFORE process_dispels
            // runs, or no DispelBurst spawns. Stage a magic buff on the dummy
            // directly into its ActiveAuras (immediate — an AuraPending would
            // resolve AFTER process_dispels in the Phase-1 order and be missed).
            // Cleared between passes by clear_body_state.
            if let (Some(SandboxEntry::Ability(ab)), Some(dummy)) =
                (playback.selected, stage.dummy)
            {
                if matches!(
                    ab,
                    AbilityType::DispelMagic
                        | AbilityType::PaladinCleanse
                        | AbilityType::Purge
                        | AbilityType::DevourMagic
                ) {
                    if let Some(def) = defs.get(&AbilityType::ArcaneIntellect) {
                        if let Some(pending) = AuraPending::from_ability(dummy, caster, def) {
                            if let Ok(mut active) = auras.get_mut(dummy) {
                                active.auras.push(pending.aura);
                            }
                        }
                    }
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
            // Channels run for their channel duration; aura entries and the
            // component/residue mechanisms (bubble, dash, holy flash, scream
            // burst) hold past cast_time so the applied state reads; a pure
            // damage cast is just cast_time.
            let holds = config.applies_aura.is_some()
                || matches!(
                    playback.family,
                    Some(EntryFamily::Component)
                        | Some(EntryFamily::Residue)
                        | Some(EntryFamily::Entity)
                );
            if let Some(channel) = config.channel_duration {
                channel
            } else if holds {
                config.cast_time + AURA_HOLD_SECS
            } else {
                config.cast_time
            }
        }
        Some(SandboxEntry::Body(body)) => body.duration(),
        None => 0.0,
    }
}

/// Spawns the cosmetic marker this ability's combat path emits inline, which no
/// `process_casting` route can produce.
///
/// The sandbox runs neither the class AIs (gated `in_state(GameState::PlayMatch)`)
/// nor `combat_ai.rs`'s `QueuedInstantAttack` drain, so for a `Residue` entry it
/// has to emit the marker itself. Spawning the marker ALONE is not enough — an
/// ability with no signature yet has nothing listening, and the preview would
/// lose its damage number — so the caller pairs this with the zero-duration
/// `CastingState` that lets `process_casting` produce the real outcome.
///
/// The `InstantAbilityFired` arm is driven by
/// [`InstantAbilityFired::is_spawned_for`], the same predicate combat code is
/// audited against, so the sandbox cannot drift from what a match spawns.
fn spawn_sandbox_cosmetic(
    commands: &mut Commands,
    ability: AbilityType,
    caster: Entity,
    target: Entity,
) {
    // Psychic Scream's burst predates the shared marker and keys on
    // `Added<ScreamBurst>` rather than on the ability, so it stays bespoke.
    if ability == AbilityType::PsychicScream {
        commands.spawn((
            ScreamBurst {
                caster,
                lifetime: 0.6,
                initial_lifetime: 0.6,
            },
            PlayMatchEntity,
        ));
        return;
    }
    if InstantAbilityFired::is_spawned_for(ability) {
        commands.spawn((
            InstantAbilityFired {
                caster,
                target: if InstantAbilityFired::is_caster_centred(ability) {
                    None
                } else {
                    Some(target)
                },
                ability,
                is_crit: false,
            },
            PlayMatchEntity,
        ));
    }
}

/// Caster stat snapshot for component-mechanism entries whose `*Pending`
/// carries the caster's identity/stats to its resolver.
#[derive(Clone, Copy)]
struct CasterInfo {
    team: u8,
    slot: u8,
    class: CharacterClass,
    spell_power: f32,
    crit_chance: f32,
}

/// Starts an M3 (component-insert) entry. `*Pending` components are spawned as
/// standalone `PlayMatchEntity` entities whose existing resolvers (all NON
/// decide-gated) produce the visual; the two movement instants insert their dash
/// component for the sandbox-owned `drive_sandbox_dash` to advance, because
/// their real driver `move_to_target` is AI-decision-gated off here (KTD6).
fn start_component_entry(
    commands: &mut Commands,
    ability: AbilityType,
    caster: Entity,
    dummy: Option<Entity>,
    caster_info: Option<CasterInfo>,
) -> bool {
    let Some(info) = caster_info else {
        return false;
    };
    match ability {
        AbilityType::DivineShield => {
            commands.spawn((
                DivineShieldPending {
                    caster,
                    caster_team: info.team,
                    caster_slot: info.slot,
                    caster_class: info.class,
                },
                PlayMatchEntity,
            ));
        }
        AbilityType::BerserkerRage => {
            commands.spawn((
                BerserkerRagePending {
                    caster,
                    caster_team: info.team,
                    caster_slot: info.slot,
                    caster_class: info.class,
                },
                PlayMatchEntity,
            ));
        }
        AbilityType::HolyShock => {
            // Damage on the dummy is the more legible preview; heal self if the
            // dummy is off.
            if let Some(dummy) = dummy {
                commands.spawn((
                    HolyShockDamagePending {
                        caster_spell_power: info.spell_power,
                        caster_crit_chance: info.crit_chance,
                        caster_team: info.team,
                        caster_slot: info.slot,
                        caster_class: info.class,
                        target: dummy,
                    },
                    PlayMatchEntity,
                ));
            } else {
                commands.spawn((
                    HolyShockHealPending {
                        caster_spell_power: info.spell_power,
                        caster_crit_chance: info.crit_chance,
                        caster_team: info.team,
                        caster_slot: info.slot,
                        caster_class: info.class,
                        target: caster,
                    },
                    PlayMatchEntity,
                ));
            }
        }
        AbilityType::DispelMagic | AbilityType::PaladinCleanse | AbilityType::Purge => {
            let target = dummy.unwrap_or(caster);
            let (log_prefix, removes_poison, heal_on_success): (
                &'static str,
                bool,
                Option<(Entity, f32)>,
            ) = match ability {
                AbilityType::PaladinCleanse => ("[CLEANSE]", true, None),
                AbilityType::Purge => ("[PURGE]", false, None),
                _ => ("[DISPEL]", false, None),
            };
            commands.spawn((
                DispelPending {
                    target,
                    dispeller: caster,
                    log_prefix,
                    caster_class: info.class,
                    heal_on_success,
                    aura_type_filter: None,
                    removes_poison,
                },
                PlayMatchEntity,
            ));
        }
        AbilityType::Charge => {
            commands
                .entity(caster)
                .insert(ChargingState { target: dummy.unwrap_or(caster) });
        }
        AbilityType::Disengage => {
            // The caster stages at -x with the dummy at +x, so the retreat leap
            // is in -x.
            commands.entity(caster).insert(DisengagingState {
                direction: Vec3::new(-1.0, 0.0, 0.0),
                distance_remaining: 12.0,
            });
        }
        _ => return false,
    }
    true
}

/// Starts an M4 (entity-spawn) entry. Totems (and traps, U5b) are world-entity
/// drops whose existing resolvers / visual systems run in the sandbox; they are
/// tagged `PlayMatchEntity` so `clear_body_state`'s leftover sweep reclaims them
/// between passes. Spawned from the SAME data gameplay uses (`shaman::totem_spec`)
/// so the preview can never drift from the real totem.
fn start_entity_entry(
    commands: &mut Commands,
    ability: AbilityType,
    caster: Entity,
    caster_home: Vec3,
    caster_info: Option<CasterInfo>,
) -> bool {
    let Some(info) = caster_info else {
        return false;
    };
    use AbilityType::*;
    let element = match ability {
        AirTotem => Some(TotemElement::Air),
        WaterTotem => Some(TotemElement::Water),
        EarthTotem => Some(TotemElement::Earth),
        FireTotem => Some(TotemElement::Fire),
        _ => None,
    };
    if let Some(element) = element {
        let (_, aura_type, magnitude, spell_school) = totem_spec(element);
        let drop = caster_home + totem_spacing_offset(element);
        commands.spawn((
            Transform::from_translation(Vec3::new(drop.x, 0.0, drop.z)),
            Totem {
                owner_team: info.team,
                owner: caster,
                element,
                radius: TOTEM_RADIUS,
                duration_remaining: TOTEM_DURATION,
                aura_type,
                magnitude,
                spell_school,
            },
            PlayMatchEntity,
        ));
        return true;
    }

    // Traps — thrown toward the dummy's staged spot (mirror across origin), so
    // the launch arc reads. Reuses hunter::spawn_trap (same code gameplay uses).
    let trap_type = match ability {
        FreezingTrap => Some(TrapType::Freezing),
        FrostTrap => Some(TrapType::Frost),
        _ => None,
    };
    if let Some(trap_type) = trap_type {
        let landing = Vec3::new(-caster_home.x, 0.0, 0.0);
        spawn_trap(commands, caster, info.team, caster_home, landing, trap_type);
        return true;
    }

    // Pet commands are driven by `drive_sandbox_pet` (it needs `CombatLog` + the
    // spawned pet, unavailable here) — just mark the entry started so the
    // transport plays and that system takes over.
    if matches!(
        ability,
        SpiderWeb | BoarCharge | MastersCall | SpellLock | DevourMagic
    ) {
        return true;
    }

    false
}

/// Starts the entry. Returns false when it could not start (no target for an
/// ability that needs one, or a mechanism whose start path is not wired yet).
fn start_entry(
    commands: &mut Commands,
    playback: &SandboxPlayback,
    caster: Entity,
    caster_home: Vec3,
    dummy: Option<Entity>,
    defs: &AbilityDefinitions,
    caster_info: Option<CasterInfo>,
) -> bool {
    match playback.selected {
        Some(SandboxEntry::Ability(ability)) => {
            let Some(config) = defs.get(&ability) else {
                return false;
            };
            // Target rule (KTD5): offensive/relational entries aim at the dummy
            // (self is the honest fallback when none is staged); self/friendly
            // buffs aim at the caster, so the buff visual lands on the right unit.
            // NOTE: relational entries are NOT yet greyed out when the dummy is
            // off — that gate (AE3) is deferred. A Cast relational entry then
            // self-targets and still renders; a pet-command entry (Entity) has
            // no target and no-ops until a dummy is staged.
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
                // M3 — bespoke `*Pending` / movement components.
                Some(EntryFamily::Component) => {
                    start_component_entry(commands, ability, caster, dummy, caster_info)
                }
                // M4 — world-entity drops (totems, traps) and pet commands.
                Some(EntryFamily::Entity) => {
                    start_entity_entry(commands, ability, caster, caster_home, caster_info)
                }
                Some(EntryFamily::Residue) => {
                    commands
                        .entity(caster)
                        .insert(CastingState::new(ability, target, config.cast_time));
                    spawn_sandbox_cosmetic(commands, ability, caster, target);
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
        e.remove::<ChargingState>();
        e.remove::<DisengagingState>();
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
            // Revert the MaxHealth/MaxMana stat bakes BEFORE the blunt clear.
            // `auras.clear()` drops the aura without running the `-= magnitude`
            // revert that normal expiry does (auras.rs), so a looping stat buff
            // (PW: Fortitude, Arcane Intellect) would otherwise accumulate its
            // bonus every pass — the staged unit's max HP/mana climbing without
            // bound. Mirrors the expiry revert; other buffs (AP/crit/SP) are
            // computed from active auras at use-time and need no revert here.
            if let Ok(mut combatant) = combatants.get_mut(unit) {
                for aura in active.auras.iter() {
                    match aura.effect_type {
                        AuraType::MaxHealthIncrease => combatant.max_health -= aura.magnitude,
                        AuraType::MaxManaIncrease => combatant.max_mana -= aura.magnitude,
                        _ => {}
                    }
                }
            }
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
    dashing: Query<(), Or<(With<ChargingState>, With<DisengagingState>)>>,
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
        // While the caster is mid-dash (Charge/Disengage), `drive_sandbox_dash`
        // owns its transform — resetting it to home here would zero the dash's
        // per-frame progress (KTD6).
        let is_dashing = dashing.get(caster).is_ok();
        if !is_dashing && transform.translation != target {
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
        Some(SandboxEntry::Ability(
            ability @ (AbilityType::Polymorph | AbilityType::Fear | AbilityType::PsychicScream),
        )) => {
            let cast_time = defs.get(&ability).map(|c| c.cast_time).unwrap_or(1.5);
            let walk_elapsed = (playback.elapsed - cast_time).max(0.0);
            // The panic run reads faster / more erratic than the sheep hop, so
            // Fear / Psychic Scream circle at the caster's brisker walk pace,
            // Polymorph at the sheep's deliberately slow one.
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

/// Sandbox-owned dash driver for the two movement instants (KTD6).
///
/// In a match, `move_to_target` advances `ChargingState` / `DisengagingState` —
/// but that system is AI-decision-gated (`in_state(PlayMatch)`) and does NOT run
/// in the sandbox, so a Charge/Disengage entry would insert its component and
/// then sit still. This mirrors the charge/disengage motion from
/// `combat_core/movement.rs` (minus the obstacle resolve — the sandbox floor has
/// none) so the two dashes animate. `position_caster` cedes the caster transform
/// while a dash component is present, so this is the sole writer during a dash.
pub fn drive_sandbox_dash(
    mut commands: Commands,
    time: Res<Time>,
    stage: Res<SandboxStage>,
    mut movers: Query<(
        &mut Transform,
        &Combatant,
        Option<&ChargingState>,
        Option<&DisengagingState>,
    )>,
) {
    // Copy the charge target's position out before the mutable borrows below.
    let charge_target_pos = stage
        .dummy
        .and_then(|d| movers.get(d).ok())
        .map(|(t, ..)| t.translation);
    let dt = time.delta_secs();

    // Both the caster (Charge / Disengage) and the Hunter pet (Boar Charge) can
    // carry a dash component whose real driver (`move_to_target`) is gated off
    // here, so this drives whichever of the two has one this frame.
    for who in [stage.caster, stage.pet].into_iter().flatten() {
        let Ok((mut transform, combatant, charging, disengaging)) = movers.get_mut(who) else {
            continue;
        };

        if charging.is_some() {
            let Some(target_pos) = charge_target_pos else {
                commands.entity(who).remove::<ChargingState>();
                continue;
            };
            let from = transform.translation;
            let flat = Vec3::new(target_pos.x - from.x, 0.0, target_pos.z - from.z);
            if flat.length() <= MELEE_RANGE {
                commands.entity(who).remove::<ChargingState>();
                continue;
            }
            let dir = flat.normalize_or_zero();
            if dir != Vec3::ZERO {
                let speed = combatant.base_movement_speed * 4.0; // CHARGE_SPEED_MULTIPLIER
                transform.translation = from + dir * speed * dt;
                transform.rotation = Quat::from_rotation_y(dir.x.atan2(dir.z));
            }
        } else if let Some(dis) = disengaging {
            if dis.distance_remaining > 0.0 {
                let amount = DISENGAGE_SPEED * dt;
                transform.translation += dis.direction * amount;
                let remaining = dis.distance_remaining - amount;
                if remaining <= 0.0 {
                    commands.entity(who).remove::<DisengagingState>();
                } else {
                    commands.entity(who).try_insert(DisengagingState {
                        direction: dis.direction,
                        distance_remaining: remaining,
                    });
                }
            } else {
                commands.entity(who).remove::<DisengagingState>();
            }
        }
    }
}

/// Sandbox-owned pet driver for the three pet-command abilities (KTD6): Spider
/// Web, Boar Charge, Master's Call. Their consumer `pet_ai_system` is
/// AI-decision-gated off here, so this spawns the matching pet on demand, fires
/// the SAME `execute_*` helper gameplay uses (single source of truth), and lets
/// `drive_sandbox_dash` drive Boar Charge's dash. The pet is tagged
/// `PlayMatchEntity`, so `clear_body_state`'s leftover sweep reclaims it between
/// passes; this rebinds/respawns it each pass.
#[allow(clippy::too_many_arguments)]
pub fn drive_sandbox_pet(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut combat_log: ResMut<CombatLog>,
    playback: Res<SandboxPlayback>,
    mut stage: ResMut<SandboxStage>,
    defs: Res<AbilityDefinitions>,
    mut combatants: Query<&mut Combatant>,
    positions: Query<&Transform>,
    pets: Query<(Entity, &Pet)>,
) {
    let Some(caster) = stage.caster else { return };

    // A bound pet may have been swept between passes — drop the stale handle.
    if let Some(pet) = stage.pet {
        if pets.get(pet).is_err() {
            stage.pet = None;
        }
    }

    // Which pet-command ability is playing, and the pet type + target it needs.
    let desired: Option<(PetType, AbilityType, Entity)> = playback
        .playing
        .then_some(playback.selected)
        .flatten()
        .and_then(|entry| match entry {
            SandboxEntry::Ability(a @ AbilityType::SpiderWeb) => {
                stage.dummy.map(|d| (PetType::Spider, a, d))
            }
            SandboxEntry::Ability(a @ AbilityType::BoarCharge) => {
                stage.dummy.map(|d| (PetType::Boar, a, d))
            }
            // Master's Call cleanses the owner (freedom), so it targets the caster.
            SandboxEntry::Ability(a @ AbilityType::MastersCall) => Some((PetType::Bird, a, caster)),
            // The Warlock's Felhunter interrupts / devours the enemy.
            SandboxEntry::Ability(a @ (AbilityType::SpellLock | AbilityType::DevourMagic)) => {
                stage.dummy.map(|d| (PetType::Felhunter, a, d))
            }
            _ => None,
        });

    let Some((pet_type, ability, target)) = desired else {
        // Not a pet-command entry — just drop the handle. clear_body_state's
        // leftover sweep (and restage_on_config_change) already owns the pet
        // despawn on every transition that ends a pet entry — select, stop,
        // completion, loop restart, and class change all run one of them — so
        // despawning here too would double-despawn the just-swept entity and log
        // a spurious "entity does not exist" warning. The top-of-system staleness
        // check keeps stage.pet consistent if a sweep already fired.
        stage.pet = None;
        return;
    };

    // Already spawned + fired; drive_sandbox_dash owns any Boar Charge dash.
    if stage.pet.is_some() {
        return;
    }

    if let Some((pet, _)) = pets.iter().find(|(_, p)| p.owner == caster) {
        // The spawn resolved — bind it and fire the ability ONCE from the same
        // code gameplay uses.
        let Some(def) = defs.get(&ability) else { return };
        let pet_pos = positions.get(pet).map(|t| t.translation).unwrap_or(stage.caster_home);
        if let Ok(mut pet_combatant) = combatants.get_mut(pet) {
            match ability {
                AbilityType::SpiderWeb => execute_spider_web(
                    &mut commands,
                    &mut combat_log,
                    def,
                    pet,
                    &mut pet_combatant,
                    pet_pos,
                    target,
                ),
                AbilityType::BoarCharge => {
                    execute_boar_charge(&mut commands, &mut combat_log, def, pet, &mut pet_combatant, target)
                }
                AbilityType::MastersCall => {
                    execute_masters_call(&mut commands, &mut combat_log, def, pet, &mut pet_combatant, target)
                }
                AbilityType::SpellLock => execute_spell_lock(
                    &mut commands,
                    &mut combat_log,
                    &defs,
                    pet,
                    &mut pet_combatant,
                    target,
                    &def.name,
                ),
                // Devour Magic has no extracted executor (its logic is embedded
                // in the AI's try_devour_magic), but its effect is a DispelPending
                // with the Felhunter as dispeller — the same contract the other
                // dispels spawn. The dummy's dispellable aura is staged in
                // drive_playback (shared with the Warlock/Priest/Paladin dispels).
                AbilityType::DevourMagic => {
                    commands.spawn(DispelPending {
                        target,
                        dispeller: pet,
                        log_prefix: "[DEVOUR]",
                        caster_class: pet_combatant.class,
                        heal_on_success: Some((pet, 20.0)),
                        aura_type_filter: None,
                        removes_poison: false,
                    });
                }
                _ => {}
            }
            stage.pet = Some(pet);
        }
    } else if let Ok(caster_combatant) = combatants.get(caster) {
        // Spawn the matching pet at the caster; bound + fired next frame (the
        // spawn is not queryable this frame).
        spawn_pet(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut combat_log,
            caster,
            caster_combatant,
            stage.caster_home,
            pet_type,
        );
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
        // Every ability whose combat path spawns an `InstantAbilityFired` — a
        // marker emitted by code the sandbox does not run. Classifying one of
        // these as `Cast` fails silently: the preview still shows damage, so it
        // looks fine, right up until the ability's signature never fires.
        // Driven off the same predicate combat code uses, so the two cannot
        // drift: adding a spawn site without a family arm fails HERE.
        let defs = AbilityDefinitions::default();
        let mut checked = 0;
        for (&ability, config) in defs.iter() {
            if !InstantAbilityFired::is_spawned_for(ability) {
                continue;
            }
            checked += 1;
            assert_eq!(
                mechanism_for(ability, config),
                EntryFamily::Residue,
                "{ability:?} spawns InstantAbilityFired, so the sandbox must \
                 spawn it too — see spawn_sandbox_cosmetic"
            );
            assert!(mechanism_for(ability, config).is_playable());
        }
        // Guard against the predicate silently emptying and the loop above
        // passing vacuously.
        assert_eq!(checked, 7, "expected the seven marker-spawning abilities");
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
    fn every_ability_is_a_sandbox_entry_in_some_class() {
        // Guard against the silent-drop gap: `get_class_abilities` is
        // hand-maintained and the sandbox iterates it (plus the pet appends),
        // NOT the AbilityType enum — so a new variant that is not wired into
        // some class's list is un-previewable with no compile error. This test
        // is that missing check (the pattern tests/registration_audit.rs uses
        // for system wiring). If it fails, add the ability to the right class in
        // `view_combatant_ui::get_class_abilities`, or — for a pet ability — to
        // the pet appends in `entries_for_class`.
        let defs = AbilityDefinitions::default();
        for (&ability, _) in defs.iter() {
            let listed = CharacterClass::all().iter().any(|&class| {
                entries_for_class(class, &defs)
                    .iter()
                    .any(|l| l.entry == SandboxEntry::Ability(ability))
            });
            assert!(
                listed,
                "{ability:?} is not a sandbox entry for any class — wire it into \
                 get_class_abilities or the entries_for_class pet appends"
            );
        }
    }

    #[test]
    fn pet_owning_classes_list_their_pet_abilities() {
        // Pet abilities aren't in get_class_abilities (they're the pet's), but
        // the sandbox appends them so they're selectable and preview via a
        // staged pet (drive_sandbox_pet).
        let defs = AbilityDefinitions::default();
        let cases = [
            (
                CharacterClass::Hunter,
                &[
                    AbilityType::SpiderWeb,
                    AbilityType::BoarCharge,
                    AbilityType::MastersCall,
                ][..],
            ),
            (
                CharacterClass::Warlock,
                &[AbilityType::SpellLock, AbilityType::DevourMagic][..],
            ),
        ];
        for (class, pet_abilities) in cases {
            let entries = entries_for_class(class, &defs);
            for &ab in pet_abilities {
                assert!(
                    entries.iter().any(|l| l.entry == SandboxEntry::Ability(ab)
                        && l.family == EntryFamily::Entity),
                    "{class:?} entries missing playable {ab:?}"
                );
            }
        }
    }

    #[test]
    fn relational_entries_need_a_dummy_but_self_and_drops_do_not() {
        // AE3: only entries whose visual requires a second unit are gated
        // dummy-off. Pet commands that target the enemy + offensive casts need
        // one; drops, self-buffs, and Master's Call (self-cleanse) do not.
        let defs = AbilityDefinitions::default();
        let needs = |a: AbilityType| entry_needs_dummy(SandboxEntry::Ability(a), &defs);
        assert!(needs(AbilityType::SpiderWeb)); // pet command -> dummy
        assert!(needs(AbilityType::DevourMagic)); // Felhunter -> dummy
        assert!(needs(AbilityType::Frostbolt)); // projectile at enemy
        assert!(needs(AbilityType::DrainLife)); // beam at enemy
        assert!(needs(AbilityType::Corruption)); // DoT on enemy
        assert!(!needs(AbilityType::EarthTotem)); // ground drop
        assert!(!needs(AbilityType::FreezingTrap)); // ground drop
        assert!(!needs(AbilityType::MastersCall)); // cleanses the caster
        assert!(!needs(AbilityType::IceBarrier)); // self buff
        assert!(!needs(AbilityType::DivineShield)); // self component
        assert!(!entry_needs_dummy(
            SandboxEntry::Body(BodyAnimation::WalkBob),
            &defs
        ));
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
