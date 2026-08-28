//! Game state management
//!
//! Defines the core game states and transitions between them.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

pub mod match_config;
pub mod main_menu;
pub mod animation_sandbox;
pub mod arena_layout_debug;
pub mod configure_match_ui;
pub mod play_match;
pub mod results_ui;
pub mod view_combatant_ui;
pub mod armory_ui;

pub use match_config::MatchConfig;

/// The core game states representing the main screens/modes of the game.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Main menu - entry point, navigate to other states
    #[default]
    MainMenu,
    /// Options menu - video/audio settings
    Options,
    /// Keybindings menu - control remapping
    Keybindings,
    /// Match configuration - team setup, map selection
    ConfigureMatch,
    /// View combatant details - stats, abilities, gear, talents
    ViewCombatant,
    /// Active match - the autobattle simulation
    PlayMatch,
    /// Post-match results - statistics and breakdown
    Results,
    /// Armory - browse all equipment in the game
    Armory,
    /// Animation sandbox - play combat animations on demand on an inert caster
    AnimationSandbox,
}

use play_match::systems::{
    CombatSystemPhase, configure_combat_system_ordering, add_core_combat_systems,
};

/// Run condition for the shared combat VISUAL layer.
///
/// The world-space visual-effect systems in this file are not specific to a
/// match — they render whatever the combat resolution systems produce. Both
/// [`GameState::PlayMatch`] and [`GameState::AnimationSandbox`] produce that,
/// so both need the effect layer, and gating them from one definition keeps the
/// two states from drifting apart the way headless and graphical registration
/// historically did (see `tests/registration_audit.rs`).
///
/// This deliberately does NOT cover match chrome — HUD, team frames, the combat
/// panel, speech-bubble rendering, banter, gate bars, selection, countdown, or
/// `update_play_match`. Those stay on the narrow `PlayMatch` gate because the
/// sandbox is required to render none of them (plan R13, R14).
/// Written as a plain `-> bool` system rather than a combinator expression so
/// it is a `Copy` fn item usable directly at every `run_if` site.
fn in_combat_scene(state: Res<State<GameState>>) -> bool {
    matches!(
        state.get(),
        GameState::PlayMatch | GameState::AnimationSandbox
    )
}

/// Plugin for managing game states and transitions
pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize match config resource
            .init_resource::<MatchConfig>()
            // Initialize class icon resources
            .init_resource::<configure_match_ui::ClassIcons>()
            .init_resource::<configure_match_ui::ClassIconHandles>()
            // Initialize ability icon resources for view combatant screen
            .init_resource::<view_combatant_ui::AbilityIcons>()
            .init_resource::<view_combatant_ui::AbilityIconHandles>()
            // Initialize item icon resources for view combatant screen
            .init_resource::<view_combatant_ui::ItemIcons>()
            .init_resource::<view_combatant_ui::ItemIconHandles>()
            // Initialize hunter pet icon resources for view combatant screen
            .init_resource::<view_combatant_ui::HunterPetIcons>()
            .init_resource::<view_combatant_ui::HunterPetIconHandles>()
            // Initialize armory filter state
            .init_resource::<armory_ui::ArmoryFilters>()
            // Player selection (click-to-select) — graphical-only
            .init_resource::<play_match::Selection>()
            // Kill-target call watcher (banter) — graphical-only. Owned for the
            // app lifetime and reset at the PlayMatch state boundary, the same
            // lifecycle `Selection` uses, so `play_match/mod.rs` needs no
            // per-match insert/remove pair for it.
            .init_resource::<play_match::CallWatcher>()
            // Banter beat queues (graphical-only), same app-lifetime-plus-
            // state-boundary-reset lifecycle as the watcher above.
            .init_resource::<play_match::BanterScheduler>()
            // Main menu systems (defined in main_menu module): ambient 3D
            // arena backdrop (setup/orbit/cleanup) + the egui menu overlay
            .add_systems(OnEnter(GameState::MainMenu), main_menu::setup_menu_scene)
            .add_systems(OnExit(GameState::MainMenu), main_menu::cleanup_menu_scene)
            .add_systems(
                Update,
                (main_menu::orbit_menu_camera, main_menu::main_menu_ui)
                    .run_if(in_state(GameState::MainMenu)),
            )
            // Options menu systems (now using egui)
            .add_systems(
                Update,
                options_ui.run_if(in_state(GameState::Options)),
            )
            .add_systems(
                Update,
                keybindings_ui.run_if(in_state(GameState::Keybindings)),
            )
            // Configure match systems (defined in configure_match_ui module).
            // The live arena preview is a render-to-texture scene set up/torn
            // down with the state; `update_map_preview` rebuilds it on map
            // change. Graphical-only (no headless registration).
            .add_systems(OnEnter(GameState::ConfigureMatch), configure_match_ui::setup_map_preview)
            .add_systems(OnExit(GameState::ConfigureMatch), configure_match_ui::cleanup_map_preview)
            .add_systems(
                Update,
                (
                    configure_match_ui::load_class_icons,
                    configure_match_ui::update_map_preview,
                    configure_match_ui::configure_match_ui,
                )
                    .chain()
                    .run_if(in_state(GameState::ConfigureMatch)),
            )
            // View combatant systems (defined in view_combatant_ui module)
            .add_systems(
                Update,
                (
                    view_combatant_ui::load_ability_icons,
                    view_combatant_ui::load_item_icons,
                    view_combatant_ui::load_hunter_pet_icons,
                    view_combatant_ui::view_combatant_ui,
                )
                    .chain()
                    .run_if(in_state(GameState::ViewCombatant)),
            )
            // Armory systems (defined in armory_ui module).
            // Reuses view_combatant_ui::load_item_icons — the loader's internal
            // `loaded: bool` guard makes the second registration idempotent.
            .add_systems(
                Update,
                (
                    view_combatant_ui::load_item_icons,
                    armory_ui::armory_ui,
                )
                    .chain()
                    .run_if(in_state(GameState::Armory)),
            )
            // Animation sandbox systems (defined in animation_sandbox module).
            // Graphical-only: no headless registration, and it reaches no
            // simulation registration of its own — see add_sandbox_combat_systems.
            // Spell icons are normally inserted by `setup_play_match`, which
            // never runs in the sandbox. `init_resource` only fills a gap, so
            // the per-match `insert_resource` still wins and match behaviour is
            // unchanged — this just stops the sandbox reading a resource that
            // does not exist yet.
            .init_resource::<play_match::SpellIcons>()
            .init_resource::<play_match::SpellIconHandles>()
            .init_resource::<animation_sandbox::SandboxConfig>()
            .init_resource::<animation_sandbox::SandboxStage>()
            .init_resource::<animation_sandbox::playback::SandboxPlayback>()
            .init_resource::<animation_sandbox::ui::PendingCameraPreset>()
            .add_systems(
                OnEnter(GameState::AnimationSandbox),
                animation_sandbox::setup_sandbox,
            )
            .add_systems(
                OnExit(GameState::AnimationSandbox),
                (
                    animation_sandbox::cleanup_sandbox,
                    animation_sandbox::ui::reset_time_on_exit,
                ),
            )
            .add_systems(
                Update,
                (
                    // Both loaders self-guard on an internal `loaded` flag, so
                    // registering them here as well as in their own states is
                    // idempotent — the same trick the Armory uses for item icons.
                    play_match::load_spell_icons,
                    configure_match_ui::load_class_icons,
                    animation_sandbox::restage_on_config_change,
                    animation_sandbox::playback::sustain_staged_units,
                    animation_sandbox::playback::drive_playback,
                    animation_sandbox::playback::position_caster,
                    animation_sandbox::playback::drive_sandbox_dash,
                    animation_sandbox::playback::drive_sandbox_pet,
                    animation_sandbox::ui::sandbox_ui,
                    animation_sandbox::ui::apply_camera_preset,
                )
                    .chain()
                    .run_if(in_state(GameState::AnimationSandbox)),
            )
            // Single-stepping injects a virtual-time delta, so it MUST land in
            // `First` (after `TimeSystem` overwrites delta, before
            // `RunFixedMainLoop` spends it on the fixed accumulator). In
            // `Update` it was both too late for this frame's fixed loop and
            // overwritten before the next one — a Step that advanced nothing.
            .add_systems(
                First,
                animation_sandbox::ui::apply_step_request
                    .after(bevy::time::TimeSystem)
                    .run_if(in_state(GameState::AnimationSandbox)),
            )
            // Play match systems (defined in play_match module)
            .add_systems(OnEnter(GameState::PlayMatch), play_match::setup_play_match);

        // Configure combat system phase ordering and add core combat systems
        // These are shared between graphical and headless modes
        configure_combat_system_ordering(app);
        // Resolution runs in both combat scenes; the AI and match clock only in
        // a real match. One registration, so no system's `SystemTypeSet` becomes
        // ambiguous and the `spawn_projectile_visuals` ordering below stays legal.
        add_core_combat_systems(app, in_combat_scene, in_state(GameState::PlayMatch));

        // SCHEDULES: the sim runs in `FixedUpdate`, visuals in `Update`.
        //
        // Bevy's `Main` order is First -> PreUpdate -> StateTransition ->
        // RunFixedMainLoop -> Update -> PostUpdate -> Last, so `FixedUpdate`
        // runs BEFORE `Update` within a frame, zero or more times. Two
        // consequences, both of which this file previously got wrong:
        //
        // 1. An ordering constraint in `Update` that names a `CombatSystemPhase`
        //    is CROSS-SCHEDULE and therefore silently VOID — the set has no
        //    members in `Update`, so no edge is created and Bevy does not
        //    complain. The `.after(CombatSystemPhase::*)` constraints further
        //    down are left in place as documentation of intent, and they happen
        //    to be satisfied structurally (the sim already ran this frame), but
        //    they are NOT enforced. Do not rely on one.
        // 2. `.before(CombatSystemPhase::*)` in `Update` is void AND inverted —
        //    it asks for something the schedule order makes impossible. The
        //    camera/input block below used to carry
        //    `.before(ResourcesAndAuras)`; it has been dropped rather than left
        //    to imply an ordering that never held.
        //
        // The rule: anything that affects the SIMULATION belongs in
        // `FixedUpdate` with a real phase constraint. `Update` is for systems
        // that should run once per rendered frame.
        // Camera drag/zoom/pan is split out of the match-chrome block below and
        // widened, so the Animation Sandbox gets real camera control rather than
        // only its framing presets — you cannot judge an animation from four
        // fixed angles. One registration, widened, so no `SystemTypeSet` becomes
        // ambiguous (see `add_core_combat_systems`).
        app.add_systems(
                Update,
                (
                    play_match::handle_camera_input,
                    play_match::update_camera_position,
                )
                    .chain()
                    .run_if(in_combat_scene),
            )
            .add_systems(
                Update,
                (
                    play_match::handle_time_controls,
                    // pick_selected_combatant consumes the pending_pick flag set
                    // by handle_camera_input on click-release; must run after it.
                    play_match::pick_selected_combatant,
                    // sync_selection_ring spawns/despawns the ring when the
                    // Selection resource changes — runs after picking.
                    play_match::sync_selection_ring,
                    play_match::animate_gate_bars,
                    play_match::update_play_match,
                )
                    .chain()
                    .after(play_match::handle_camera_input)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Graphical-only, but it must run IN the sim schedule: it attaches
            // meshes to projectiles the same tick `process_casting` spawns them,
            // before `move_projectiles` moves them and `process_projectile_hits`
            // can despawn them. Registered in `Update` it was ordered against a
            // set with no members there, so a projectile that spawned and hit
            // inside one rendered frame was never drawn at all. Draws no RNG, so
            // moving it into `FixedUpdate` cannot shift the sim's draw order.
            .add_systems(
                FixedUpdate,
                play_match::spawn_projectile_visuals
                    .in_set(CombatSystemPhase::CombatAndMovement)
                    .after(play_match::process_channeling)
                    .before(play_match::move_projectiles)
                    .run_if(in_combat_scene),
            )
            // Match end is a SIM decision, so it belongs on the sim clock. In
            // `Update` it was evaluated once per rendered frame — coarser than a
            // tick, and at a cadence that varied with frame rate — while headless
            // has always run its equivalent (`headless_check_match_end`) in
            // `FixedUpdate`. Same class of bug as the match-clock fix in 3a16a46.
            // Headless never registers this system, so no baseline moves.
            .add_systems(
                FixedUpdate,
                play_match::check_match_end
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Weapon-swing signal consumption is graphical-only but must run IN
            // the sim schedule (same rationale as `spawn_projectile_visuals`
            // above): `FixedUpdate` can tick several times per rendered frame,
            // and a landed-attack marker consumed one tick late would desync
            // the release stroke from its hit. Runs after CombatResolution so
            // it sees the markers `combat_auto_attack` spawned this tick.
            .add_systems(
                FixedUpdate,
                play_match::consume_swing_signals
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Landed instant-melee signals (Mortal Strike's signature stroke +
            // flourish). Same FixedUpdate rationale as `consume_swing_signals`
            // above. `.after` it so that when an ordinary auto and a Mortal
            // Strike land on the same tick, the SIGNATURE wins the socket —
            // otherwise the auto's `swing_style = Auto` reset could land last
            // and silently downgrade the special to a normal swing.
            .add_systems(
                FixedUpdate,
                play_match::consume_instant_ability_signals
                    .after(CombatSystemPhase::CombatResolution)
                    .after(play_match::consume_swing_signals)
                    .run_if(in_combat_scene),
            )
            // Mortal Wounds heal fracture: also a core-spawned marker consumer,
            // so it takes the same FixedUpdate slot as its siblings above
            // rather than Update. Several heals can resolve in one rendered
            // frame, and the ash burst reads the target's LIVE transform — a
            // consumer running at render rate would site every burst in that
            // frame at one position instead of each at its own.
            .add_systems(
                FixedUpdate,
                play_match::spawn_heal_fracture
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Weapon swing animation + cosmetic arrows: per-rendered-frame
            // cosmetic transforms, ordinary Update visual group.
            .add_systems(
                Update,
                (
                    play_match::animate_weapon_swings,
                    play_match::update_cosmetic_arrows,
                    play_match::update_weapon_stealth_fade,
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Body lean into the swing. Its own registration rather than a
            // nested tuple: the ordering constraint is a real dependency (it
            // consumes the `last_s` the swing publishes, and would trail the
            // weapon by a frame otherwise), so it reads better stated than
            // implied by position — and `registration_audit`'s scanner cannot
            // see through a nested `.chain()` sub-tuple.
            .add_systems(
                Update,
                play_match::animate_body_lean
                    .after(play_match::animate_weapon_swings)
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Cast-ending signal consumption: same rationale as
            // `consume_swing_signals` above — FixedUpdate can tick several
            // times per rendered frame, and a CastEnding marker consumed one
            // tick late would let cleanup_casting_orbs mistake a landed/
            // fizzled cast for a silent-vanish removal. Runs after
            // CombatResolution so it sees markers spawned this tick.
            .add_systems(
                FixedUpdate,
                play_match::consume_cast_ending_signals
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Casting orb (gathering-orb cast animation): spawn/animate/motes/
            // cleanup — separate group to avoid tuple size limits. `.chain()`
            // enforces spawn->update->motes->cleanup ordering within the
            // frame, so a just-spawned orb is positioned before motes stream
            // toward it (otherwise motes could target the default-ZERO
            // translation for a frame).
            .add_systems(
                Update,
                (
                    play_match::spawn_casting_orbs,
                    play_match::update_casting_orbs,
                    play_match::spawn_casting_orb_motes,
                    play_match::update_casting_orb_motes,
                    play_match::cleanup_casting_orbs,
                )
                    .chain()
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Combat resolution, death, and visual effects (after core combat)
            .add_systems(
                Update,
                (
                    play_match::update_stealth_visuals,
                    play_match::trigger_death_animation,
                    play_match::animate_death,
                    play_match::update_victory_celebration,
                    play_match::update_floating_combat_text,
                    play_match::update_speech_bubbles,
                    play_match::cleanup_expired_floating_text,
                    play_match::spawn_spell_impact_visuals,
                    play_match::update_spell_impact_effects,
                    play_match::cleanup_expired_spell_impacts,
                    play_match::animate_shadow_sight_orbs,  // Pulsing orb animation
                    play_match::animate_orb_consumption,    // Orb pickup shrink/move animation
                    play_match::update_shield_bubbles,      // Spawn/despawn shield bubbles
                    play_match::follow_shield_bubbles,      // Update bubble positions
                    // Polymorph BEFORE Fear, chained so a sync point flushes
                    // PolymorphedVisual before Fear evaluates its
                    // `Without<PolymorphedVisual>` filter. A unit hit by both on the
                    // same frame would otherwise get BOTH markers (each insert is a
                    // deferred Command the other can't see that frame) and then
                    // deadlock — both treatments exclude a double-marked unit, so
                    // neither could ever restore. Chaining keeps at most one marker
                    // set (sheep wins the tie). See
                    // tests/fear_visual_probes.rs::simultaneous_fear_and_polymorph_do_not_deadlock.
                    (
                        play_match::update_polymorph_visuals, // Sheep body swap when polymorphed
                        play_match::update_fear_visuals,      // Shadow-husk tint when feared
                    )
                        .chain(),
                    // Fear sub-effects nested to keep the outer tuple within Bevy's 20-limit.
                    (
                        play_match::update_fear_shroud,        // Breathing fear shroud pulse
                        play_match::update_fear_mote_emitters, // Spawn rising fear motes per feared unit
                        play_match::update_fear_motes,         // Float/fade fear motes
                        play_match::cleanup_fear_motes,        // Despawn expired fear motes
                        play_match::update_fear_flashes,       // Grow/fade apply flash
                        play_match::cleanup_fear_flashes,      // Despawn expired fear flashes
                        play_match::update_fear_shards,        // Fall/tumble/fade shroud shatter shards
                        play_match::cleanup_fear_shards,       // Despawn expired shatter shards
                    ),
                    play_match::spawn_flame_visuals,        // Visual meshes for flame particles
                    play_match::update_flame_particles,     // Move/fade flame particles
                    // Lightning Bolt signature flash-crack, nested to keep the
                    // outer tuple within Bevy's 20-item .add_systems limit.
                    // Graphical-only (never registered in systems.rs) — headless
                    // stays byte-identical.
                    (
                        play_match::spawn_lightning_bolt,
                        play_match::update_lightning_bolt,
                        play_match::cleanup_lightning_bolt,
                    ),
                    // Mortal Strike signature: weapon trail, impact flash and
                    // sparks, plus the Mortal Wounds heal fracture. Both live
                    // in ONE nested tuple — the outer tuple is at Bevy's
                    // 20-item .add_systems limit, so a second sibling here does
                    // not compile. Graphical-only.
                    (
                        (
                            play_match::update_mortal_strike_trail,
                            // Fires the held flash/sparks when the blade
                            // arrives. Chained ahead of the updaters so a burst
                            // spawned this frame is not aged before it renders.
                            play_match::update_mortal_strike_impacts,
                            play_match::update_mortal_strike_flash,
                            play_match::update_mortal_strike_sparks,
                            play_match::cleanup_mortal_strike,
                        )
                            .chain(),
                        // `spawn_heal_fracture` is NOT here — it consumes a
                        // core-spawned marker and belongs in FixedUpdate with
                        // the other marker consumers (see below). These two are
                        // ordinary per-frame particle motion.
                        (
                            play_match::update_heal_fracture,
                            play_match::cleanup_heal_fracture,
                        ),
                    ),
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Hard-CC receiver treatment: ice crystals / web sheet at a rooted
            // unit's feet, and the whirl over a stunned unit's head. Its own
            // group because the block above is AT Bevy's 20-item .add_systems
            // tuple limit, so a 21st sibling there does not compile.
            //
            // Graphical-only — never registered in `add_core_combat_systems`.
            // Keyed purely on the VICTIM's aura, which is what makes it faithful
            // in the animation sandbox too: Frost Nova, Cheap Shot, Kidney Shot
            // and Hammer of Justice are all applied inline in class AI and never
            // enter `process_casting`, but every path converges on `AuraPending`
            // -> `apply_pending_auras`, which does run under `in_combat_scene`.
            .add_systems(
                Update,
                (
                    // Rogue stun crescents, consumed from the same
                    // `InstantAbilityFired` marker the hard-CC treatment's
                    // victims key on.
                    play_match::update_crescent_flares,
                    play_match::cleanup_crescent_flares,
                    // Hammer of Justice's ground streak and victim rune. No
                    // weapon stroke — the source has no hammer.
                    play_match::update_holy_streaks,
                    play_match::update_justice_runes,
                    play_match::cleanup_holy_justice,
                    play_match::update_hard_cc_visuals,
                    play_match::update_cc_rigs,
                    // After `update_cc_rigs`, which writes the hub rotation the
                    // billboard has to cancel.
                    play_match::billboard_cc_beads,
                    play_match::update_cc_flares,
                    play_match::cleanup_cc_rigs,
                    play_match::cleanup_cc_flares,
                )
                    .chain()
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Pet mesh tilt must run after movement sets Y-facing rotation
            .add_systems(
                Update,
                play_match::apply_pet_mesh_tilt
                    .after(CombatSystemPhase::CombatAndMovement)
                    .run_if(in_combat_scene),
            )
            // Healing light column visual effects (separate group to avoid tuple size limits)
            .add_systems(
                Update,
                (
                    play_match::spawn_healing_light_visuals,    // Spawn healing light columns
                    play_match::update_healing_light_columns,   // Update position/fade
                    play_match::cleanup_expired_healing_lights, // Remove expired columns
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Dispel burst visual effects (separate group to avoid tuple size limits)
            // Still used by Concussive Shot impact and Master's Call — NOT the dispel.
            .add_systems(
                Update,
                (
                    play_match::spawn_dispel_visuals,          // Spawn burst when dispel succeeds
                    play_match::update_dispel_bursts,          // Expand sphere and fade
                    play_match::cleanup_expired_dispel_bursts, // Remove expired bursts
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Transform puff visual effects (separate group to avoid tuple size limits)
            // The cloud pop at both ends of a polymorph — graphical only.
            .add_systems(
                Update,
                (
                    play_match::spawn_transform_puff_visuals, // Attach cloud lobes to new puffs
                    play_match::update_transform_puffs,       // Expand, rise and fade
                    play_match::cleanup_expired_transform_puffs, // Remove expired puffs
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Dispel ribbon visual effects (separate group to avoid tuple size limits)
            // The spiraling "you got cleansed" indicator — graphical only.
            .add_systems(
                Update,
                (
                    play_match::spawn_dispel_ribbon_visuals,    // Attach ribbon mesh when a dispel succeeds
                    play_match::update_dispel_ribbons,          // Rise off the head, spin, and fade
                    play_match::cleanup_expired_dispel_ribbons, // Remove expired ribbons
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Psychic Scream burst visuals (separate group to avoid tuple size limits)
            .add_systems(
                Update,
                (
                    play_match::spawn_scream_burst,            // Attach mesh when a scream marker appears
                    play_match::update_scream_bursts,          // Expand the AoE ring and fade
                    play_match::cleanup_expired_scream_bursts, // Remove expired bursts
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Death Coil impact burst (separate group to avoid tuple size limits)
            .add_systems(
                Update,
                (
                    play_match::spawn_death_coil_burst,            // Attach mesh when a coil-impact marker appears
                    play_match::update_death_coil_bursts,          // Flash, punch outward, fade
                    play_match::cleanup_expired_death_coil_bursts, // Remove expired bursts
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Berserker Rage activation visuals (separate group to avoid tuple size limits)
            // The TBC-style black angry mask + red glow at the Warrior's head.
            .add_systems(
                Update,
                (
                    play_match::spawn_berserk_mask_visuals,     // Attach glyph quad + glow when a mask marker appears
                    play_match::update_berserk_masks,           // Follow head, billboard, pop/hold/collapse
                    play_match::cleanup_expired_berserk_masks,  // Remove expired masks and glows
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Unstable Affliction visuals: DoT glow, backlash burst, silenced text
            // (graphical only — never registered in headless systems.rs).
            .add_systems(
                Update,
                (
                    play_match::spawn_ua_glow_for_afflicted,   // Detect UA aura and spawn glow
                    play_match::spawn_ua_glow_visuals,         // Build mesh for new glows
                    play_match::update_ua_glow,                // Pulse and follow target
                    play_match::cleanup_ua_glow,               // Despawn when UA is gone
                    play_match::spawn_backlash_burst_visuals,  // Build mesh for new bursts
                    play_match::update_backlash_bursts,        // Expand and fade
                    play_match::cleanup_expired_backlash_bursts, // Remove expired bursts
                    // Silence visibility uses the standard CC pattern: [CC] log entry
                    // plus the HUD aura icon — no bespoke floating text.
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // DoT drip indicators: green poison / red bleed drops on afflicted
            // targets (graphical only — never registered in headless systems.rs).
            .add_systems(
                Update,
                (
                    play_match::spawn_drip_emitters_for_afflicted, // Detect mapped DoTs, spawn emitters
                    play_match::update_drip_emitters,              // Tick emitters, spawn drips, cleanup
                    play_match::spawn_drip_visuals,                // Build mesh for new drips
                    play_match::update_drips,                      // Fall, shrink, despawn
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Windfury Totem proc effect: a spinning wind funnel around the melee
            // ally that just landed a bonus swing (graphical only — the marker is
            // spawned in core like FCT; the mesh is built only here).
            .add_systems(
                Update,
                (
                    play_match::spawn_windfury_tornado_visuals,  // Build funnel mesh for new procs
                    play_match::update_windfury_tornados,        // Spin fast, follow ally, fade
                    play_match::cleanup_expired_windfury_tornados, // Despawn when expired
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Drain Life beam visual effects (separate group to avoid tuple size limits)
            .add_systems(
                Update,
                (
                    play_match::spawn_drain_life_beams,     // Spawn beam when Drain Life starts
                    play_match::update_drain_life_beams,    // Update beam position/rotation
                    play_match::spawn_drain_particles,      // Spawn particles along beam
                    play_match::update_drain_particles,     // Move particles toward caster
                    play_match::cleanup_drain_life_beams,   // Remove beam when channel ends
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Trap visual effects (ground circles + trigger bursts)
            .add_systems(
                Update,
                (
                    play_match::spawn_trap_visuals,              // Ground circle on new traps
                    play_match::update_trap_visuals,             // Arming pulse → armed shimmer
                    play_match::spawn_trap_burst_visuals,        // Burst sphere on trigger
                    play_match::update_and_cleanup_trap_bursts,  // Expand + fade + despawn
                    play_match::spawn_trap_launch_visuals,       // Glowing sphere on launched traps
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Ice block + slow zone visual effects
            .add_systems(
                Update,
                (
                    play_match::spawn_ice_block_visuals,     // Cuboid around frozen targets
                    play_match::update_ice_blocks,           // Follow target position
                    play_match::cleanup_ice_blocks,          // Despawn when aura breaks
                    play_match::spawn_slow_zone_visuals,     // Cyan disc on slow zones
                    play_match::update_slow_zone_visuals,    // Pulse + fade out
                    play_match::spawn_totem_visuals,         // Element-colored pillar on new totems
                    play_match::update_totem_visuals,        // Pulse + fade out
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Disengage trail + charge trail visual effects
            .add_systems(
                Update,
                (
                    play_match::spawn_disengage_trail,                 // Wind streak on Disengage
                    play_match::update_and_cleanup_disengage_trails,   // Fade + despawn
                    play_match::spawn_charge_trail,                    // Boar charge streak
                    play_match::update_and_cleanup_charge_trails,      // Fade + despawn
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Walking animation: vertical bob on moving combatants/pets, and
            // the hop that replaces it while a unit is polymorphed. Must run
            // after movement has settled so the post-movement XZ is read.
            .add_systems(
                Update,
                (
                    play_match::update_walk_animation,
                    play_match::update_sheep_hop,
                    play_match::update_fear_run,
                )
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_combat_scene),
            )
            // Kill-target call watcher (banter, graphical-only). An explicit
            // per-team diff of `MatchConfig`, NOT Bevy change detection —
            // `ResMut` deref marks the whole resource changed whether or not a
            // field moved, and `is_changed()` fires on the first run after
            // insert (KTD4). Ordinary `Update`: it reads the config and the
            // countdown's gate flag, touches no sim state, and only needs to
            // run before the beat scheduler that consumes its queue.
            .add_systems(
                Update,
                play_match::watch_kill_target_calls.run_if(in_state(GameState::PlayMatch)),
            )
            // Banter beat scheduler (graphical-only). Drains the watcher's
            // queue, resolves an exchange per change, and spawns each beat's
            // speech bubble as it falls due. Ordered `.after` the watcher so a
            // call change is picked up on the frame it is detected — both
            // systems take `ResMut<CallWatcher>`, so Bevy would otherwise
            // serialise them in an arbitrary order and the opening exchange
            // would sometimes start a frame late. It writes nothing but
            // `SpeechBubble` entities, which no sim system reads.
            .add_systems(
                Update,
                play_match::play_banter_beats
                    .after(play_match::watch_kill_target_calls)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // UI rendering systems
            .add_systems(
                Update,
                // Chained: egui systems are serialized on EguiContexts anyway,
                // and render_team_frames must run AFTER render_combat_panel —
                // it anchors to available_rect(), which only reflects panels
                // already shown this frame.
                (
                    play_match::load_spell_icons,
                    play_match::load_emoji_icons,
                    play_match::render_time_controls,
                    play_match::render_camera_controls,
                    play_match::render_combat_panel,
                    play_match::render_countdown,
                    play_match::render_victory_celebration,
                    play_match::render_health_bars,
                    play_match::render_team_frames,
                    play_match::render_speech_bubbles,
                )
                    .chain()
                    .run_if(in_state(GameState::PlayMatch)),
            )
            // Floating combat text renders in the sandbox too (not just matches):
            // it is world-space damage/heal feedback, not HUD chrome, and for
            // FCT-only abilities (Holy Shock, the melee strikes) it is the only
            // visible confirmation that the hit landed. R13 excludes the HUD /
            // team frames / combat log / speech bubbles from the sandbox — not
            // this. `update_floating_combat_text` already runs under
            // `in_combat_scene`; this widens the egui draw to match.
            .add_systems(
                Update,
                play_match::render_floating_combat_text.run_if(in_combat_scene),
            )
            // Selection ring follow & cleanup — runs after combat resolution
            // so the ring tracks post-movement positions on the same frame
            // (matches the `follow_shield_bubbles` pattern).
            .add_systems(
                Update,
                play_match::follow_selection_ring
                    .after(CombatSystemPhase::CombatResolution)
                    .run_if(in_state(GameState::PlayMatch)),
            )
            .add_systems(
                OnExit(GameState::PlayMatch),
                play_match::reset_selection_on_exit,
            )
            // Back to the "never observed" sentinel so the next match reports
            // its own opening call change, and so an unconsumed change cannot
            // leak into the next match's queue.
            .add_systems(
                OnExit(GameState::PlayMatch),
                play_match::reset_call_watcher_on_exit,
            )
            // Drop every queued beat, the scheduler clock, and the occurrence
            // counters, so nothing carries into the next match.
            .add_systems(
                OnExit(GameState::PlayMatch),
                play_match::reset_banter_scheduler_on_exit,
            )
            .add_systems(OnExit(GameState::PlayMatch), play_match::cleanup_play_match)
            // Results systems (defined in results_ui module)
            .add_systems(
                Update,
                results_ui::results_ui.run_if(in_state(GameState::Results)),
            );
    }
}

// ============================================================================
// Options Menu (egui)
// ============================================================================

fn options_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut settings: ResMut<crate::settings::GameSettings>,
    pending_restart: Res<crate::settings::PendingSettingsRestart>,
) {
    // Use try_ctx_mut to gracefully handle window close (the context
    // dies with the primary window; ctx_mut panics on the final frame)
    let Some(ctx) = contexts.try_ctx_mut() else { return; };
    
    // Configure style for a dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 20,
                    bottom: 20,
                })
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);
            
            // Back button - positioned in top-left
            let back_rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 20.0),
                egui::vec2(80.0, 36.0)
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui.button(egui::RichText::new("BACK").size(20.0)).clicked() {
                    next_state.set(GameState::MainMenu);
                }
            });
            
            // Title - centered relative to full width
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("OPTIONS")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(60.0);

            // Center the options panel
            ui.vertical_centered(|ui| {
                // Create a fixed-width panel for options
                ui.allocate_ui_with_layout(
                    egui::vec2(600.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        // Window Mode Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.label(
                                egui::RichText::new("Window Mode")
                                    .size(24.0)
                                    .color(egui::Color32::from_rgb(230, 204, 153)),
                            );
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("(Requires restart)")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                for mode in crate::settings::WindowModeOption::all() {
                                    let is_selected = settings.window_mode == mode;
                                    let button = egui::Button::new(
                                        egui::RichText::new(mode.as_str())
                                            .size(18.0)
                                            .color(if is_selected {
                                                egui::Color32::from_rgb(255, 255, 255)
                                            } else {
                                                egui::Color32::from_rgb(180, 180, 180)
                                            })
                                    )
                                    .min_size(egui::vec2(280.0, 40.0))
                                    .fill(if is_selected {
                                        egui::Color32::from_rgb(60, 60, 80)
                                    } else {
                                        egui::Color32::from_rgb(40, 40, 50)
                                    });

                                    if ui.add(button).clicked() {
                                        settings.window_mode = mode;
                                    }
                                }
                            });
                            
                            ui.add_space(10.0);
                        });

                        ui.add_space(20.0);

                        // Resolution Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.label(
                                egui::RichText::new("Resolution")
                                    .size(24.0)
                                    .color(egui::Color32::from_rgb(230, 204, 153)),
                            );
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("(Requires restart • Only applies in Windowed mode)")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                for resolution in crate::settings::ResolutionOption::all() {
                                    let is_selected = settings.resolution == resolution;
                                    let button = egui::Button::new(
                                        egui::RichText::new(resolution.as_str())
                                            .size(18.0)
                                            .color(if is_selected {
                                                egui::Color32::from_rgb(255, 255, 255)
                                            } else {
                                                egui::Color32::from_rgb(180, 180, 180)
                                            })
                                    )
                                    .min_size(egui::vec2(180.0, 40.0))
                                    .fill(if is_selected {
                                        egui::Color32::from_rgb(60, 60, 80)
                                    } else {
                                        egui::Color32::from_rgb(40, 40, 50)
                                    });

                                    if ui.add(button).clicked() {
                                        settings.resolution = resolution;
                                    }
                                }
                            });
                            
                            ui.add_space(10.0);
                        });

                        ui.add_space(20.0);

                        // VSync Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);
                            
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("VSync")
                                        .size(24.0)
                                        .color(egui::Color32::from_rgb(230, 204, 153)),
                                );
                                
                                ui.add_space(20.0);
                                
                                // Toggle switch
                                let vsync_label = if settings.vsync { "On" } else { "Off" };
                                if ui.add(
                                    egui::widgets::Checkbox::new(
                                        &mut settings.vsync,
                                        egui::RichText::new(vsync_label)
                                            .size(18.0)
                                    )
                                ).changed() {
                                    info!("VSync toggled to: {}", settings.vsync);
                                }
                            });
                            
                            ui.add_space(5.0);
                            
                            ui.label(
                                egui::RichText::new("Prevents screen tearing but may reduce performance • Applied immediately")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );
                            
                            ui.add_space(10.0);
                        });
                        
                        ui.add_space(20.0);

                        // Aura Icons Setting
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Show Aura Icons")
                                        .size(24.0)
                                        .color(egui::Color32::from_rgb(230, 204, 153)),
                                );

                                ui.add_space(20.0);

                                // Toggle switch
                                let aura_label = if settings.show_aura_icons { "On" } else { "Off" };
                                if ui.add(
                                    egui::widgets::Checkbox::new(
                                        &mut settings.show_aura_icons,
                                        egui::RichText::new(aura_label)
                                            .size(18.0)
                                    )
                                ).changed() {
                                    info!("Show Aura Icons toggled to: {}", settings.show_aura_icons);
                                }
                            });

                            ui.add_space(5.0);

                            ui.label(
                                egui::RichText::new("Shows buff/debuff icons below health bars • Toggle in-match with V")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );

                            ui.add_space(10.0);
                        });

                        ui.add_space(20.0);

                        // Controls / Keybindings button
                        ui.group(|ui| {
                            ui.set_min_width(580.0);
                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Controls")
                                        .size(24.0)
                                        .color(egui::Color32::from_rgb(230, 204, 153)),
                                );

                                ui.add_space(20.0);

                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("Configure Keybindings")
                                            .size(18.0)
                                    )
                                    .min_size(egui::vec2(200.0, 36.0))
                                ).clicked() {
                                    next_state.set(GameState::Keybindings);
                                }
                            });

                            ui.add_space(5.0);

                            ui.label(
                                egui::RichText::new("Customize keyboard controls")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                            );

                            ui.add_space(10.0);
                        });

                        // Restart notification
                        if pending_restart.restart_required {
                            ui.add_space(30.0);
                            
                            ui.group(|ui| {
                                ui.set_min_width(580.0);
                                ui.add_space(10.0);
                                
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("⚠")
                                            .size(24.0)
                                            .color(egui::Color32::from_rgb(230, 170, 80)),
                                    );
                                    
                                    ui.add_space(10.0);
                                    
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new("Restart Required")
                                                .size(20.0)
                                                .color(egui::Color32::from_rgb(230, 170, 80)),
                                        );
                                        ui.label(
                                            egui::RichText::new("Settings will be applied when you restart the application")
                                                .size(14.0)
                                                .color(egui::Color32::from_rgb(180, 180, 180)),
                                        );
                                    });
                                });
                                
                                ui.add_space(10.0);
                            });
                        }
                    }
                );
            });
        });
}

/// Resource to track keybinding that is currently being rebound
#[derive(Resource, Default)]
struct RebindingState {
    action: Option<crate::keybindings::GameAction>,
    is_primary: bool,
}

/// Keybindings configuration UI
fn keybindings_ui(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut settings: ResMut<crate::settings::GameSettings>,
    mut rebinding_state: Local<Option<RebindingState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut keys_just_pressed: Local<Vec<KeyCode>>,
) {
    use crate::keybindings::{GameAction, Keybindings};
    
    // Initialize rebinding state if needed
    if rebinding_state.is_none() {
        *rebinding_state = Some(RebindingState::default());
    }
    
    // Collect all keys just pressed this frame (for rebinding)
    keys_just_pressed.clear();
    for key in keyboard.get_just_pressed() {
        keys_just_pressed.push(*key);
    }
    
    // Use try_ctx_mut to gracefully handle window close (the context
    // dies with the primary window; ctx_mut panics on the final frame)
    let Some(ctx) = contexts.try_ctx_mut() else { return; };
    
    // Configure style for a dark theme
    let mut style = (*ctx.style()).clone();
    style.visuals.window_fill = egui::Color32::from_rgb(20, 20, 30);
    style.visuals.panel_fill = egui::Color32::from_rgb(20, 20, 30);
    ctx.set_style(style);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(20, 20, 30))
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 20,
                    bottom: 20,
                })
        )
        .show(ctx, |ui| {
            ui.add_space(10.0);
            
            // Back button - positioned in top-left
            let back_rect = egui::Rect::from_min_size(
                egui::pos2(20.0, 20.0),
                egui::vec2(80.0, 36.0)
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(back_rect), |ui| {
                if ui.button(egui::RichText::new("BACK").size(20.0)).clicked() {
                    next_state.set(GameState::Options);
                }
            });
            
            // Title - centered relative to full width
            ui.vertical_centered(|ui| {
                ui.heading(
                    egui::RichText::new("KEYBINDINGS")
                        .size(42.0)
                        .color(egui::Color32::from_rgb(230, 204, 153)),
                );
            });

            ui.add_space(30.0);
            
            // Reset to defaults button
            ui.vertical_centered(|ui| {
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("Reset to Defaults")
                            .size(16.0)
                    )
                    .min_size(egui::vec2(180.0, 32.0))
                ).clicked() {
                    settings.keybindings.reset_to_defaults();
                }
            });

            ui.add_space(20.0);

            // Center the keybindings panel
            ui.vertical_centered(|ui| {
                // Create a fixed-width panel for keybindings
                ui.allocate_ui_with_layout(
                    egui::vec2(800.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        // Group actions by category
                        let mut actions_by_category: std::collections::HashMap<&str, Vec<GameAction>> = 
                            std::collections::HashMap::new();
                        
                        for action in GameAction::all() {
                            actions_by_category
                                .entry(action.category())
                                .or_insert_with(Vec::new)
                                .push(action);
                        }
                        
                        // Render each category
                        let categories = vec!["Navigation", "Camera", "Simulation", "Display"];
                        for category in categories {
                            if let Some(actions) = actions_by_category.get(category) {
                                ui.group(|ui| {
                                    ui.set_min_width(780.0);
                                    ui.add_space(10.0);
                                    
                                    ui.label(
                                        egui::RichText::new(category)
                                            .size(28.0)
                                            .color(egui::Color32::from_rgb(230, 204, 153)),
                                    );
                                    
                                    ui.add_space(10.0);
                                    
                                    // Render each action in this category
                                    for action in actions {
                                        let rebinding = rebinding_state.as_ref()
                                            .and_then(|rs| rs.action)
                                            .map_or(false, |a| a == *action);
                                        
                                        ui.horizontal(|ui| {
                                            // Action name
                                            ui.label(
                                                egui::RichText::new(action.description())
                                                    .size(18.0)
                                                    .color(egui::Color32::from_rgb(200, 200, 200))
                                            );
                                            
                                            ui.add_space(20.0);
                                            
                                            // Spacer to push buttons to the right
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                // Secondary key button
                                                let binding = settings.keybindings.get(*action);
                                                let secondary_text = binding
                                                    .and_then(|b| b.secondary)
                                                    .map(|k| Keybindings::key_name(k).to_string())
                                                    .unwrap_or_else(|| "-".to_string());
                                                
                                                let secondary_rebinding = rebinding && 
                                                    !rebinding_state.as_ref().unwrap().is_primary;
                                                
                                                let secondary_button = egui::Button::new(
                                                    egui::RichText::new(if secondary_rebinding {
                                                        "Press key..."
                                                    } else {
                                                        &secondary_text
                                                    })
                                                    .size(16.0)
                                                    .color(if secondary_rebinding {
                                                        egui::Color32::from_rgb(255, 200, 100)
                                                    } else {
                                                        egui::Color32::from_rgb(180, 180, 180)
                                                    })
                                                )
                                                .min_size(egui::vec2(120.0, 32.0))
                                                .fill(if secondary_rebinding {
                                                    egui::Color32::from_rgb(80, 60, 40)
                                                } else {
                                                    egui::Color32::from_rgb(40, 40, 50)
                                                });
                                                
                                                if ui.add(secondary_button).clicked() {
                                                    if let Some(rs) = rebinding_state.as_mut() {
                                                        rs.action = Some(*action);
                                                        rs.is_primary = false;
                                                    }
                                                }
                                                
                                                ui.add_space(10.0);
                                                
                                                // Primary key button
                                                let primary_text = binding
                                                    .map(|b| Keybindings::key_name(b.primary).to_string())
                                                    .unwrap_or_else(|| "Unbound".to_string());
                                                
                                                let primary_rebinding = rebinding && 
                                                    rebinding_state.as_ref().unwrap().is_primary;
                                                
                                                let primary_button = egui::Button::new(
                                                    egui::RichText::new(if primary_rebinding {
                                                        "Press key..."
                                                    } else {
                                                        &primary_text
                                                    })
                                                    .size(16.0)
                                                    .color(if primary_rebinding {
                                                        egui::Color32::from_rgb(255, 200, 100)
                                                    } else {
                                                        egui::Color32::from_rgb(255, 255, 255)
                                                    })
                                                )
                                                .min_size(egui::vec2(120.0, 32.0))
                                                .fill(if primary_rebinding {
                                                    egui::Color32::from_rgb(80, 60, 40)
                                                } else {
                                                    egui::Color32::from_rgb(60, 60, 80)
                                                });
                                                
                                                if ui.add(primary_button).clicked() {
                                                    if let Some(rs) = rebinding_state.as_mut() {
                                                        rs.action = Some(*action);
                                                        rs.is_primary = true;
                                                    }
                                                }
                                            });
                                        });
                                        
                                        ui.add_space(8.0);
                                    }
                                    
                                    ui.add_space(10.0);
                                });
                                
                                ui.add_space(20.0);
                            }
                        }
                    }
                );
            });
        });
    
    // Handle key press for rebinding
    if let Some(ref mut rs) = rebinding_state.as_mut() {
        if let Some(action) = rs.action {
            if !keys_just_pressed.is_empty() {
                let new_key = keys_just_pressed[0];
                
                // Check for conflicts
                if let Some(conflicting_action) = settings.keybindings.is_key_bound(new_key, Some(action)) {
                    info!("Key {:?} is already bound to {:?}", new_key, conflicting_action);
                    // For now, just warn. In a full implementation, you'd show a conflict dialog
                }
                
                // Update the binding
                if let Some(mut binding) = settings.keybindings.get(action).cloned() {
                    if rs.is_primary {
                        binding.primary = new_key;
                    } else {
                        binding.secondary = Some(new_key);
                    }
                    settings.keybindings.set(action, binding);
                }
                
                // Clear rebinding state
                rs.action = None;
            }
        }
    }
}

// ============================================================================
// Configure Match UI
// ============================================================================
// All Configure Match logic has been moved to src/states/configure_match_ui.rs
// See that module for team setup, character selection, and map controls.

// ============================================================================
// Play Match - 3D Combat Arena
// ============================================================================
// All Play Match logic has been moved to src/states/play_match.rs
// See that module for combat systems, combatant components, and match flow.

// ============================================================================
// Results Scene UI
// ============================================================================
// All Results screen logic has been moved to src/states/results_ui.rs
// See that module for match results display and statistics tables.
