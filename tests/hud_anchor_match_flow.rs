//! The health bar block's anchor observed in a REAL match — a stealthed Rogue
//! walking in on a Mage — booted into `PlayMatch` through the real
//! `StatesPlugin` schedule the way `--replay` does, at a render rate that is
//! not a multiple of the 60Hz sim.
//!
//! ## Why this exists
//!
//! The sim moves units once per 60Hz tick; the display renders faster. The
//! health bar, cast bar and status labels (STEALTH, STUN, ...) used to hang
//! from the raw `Transform`, so a walking unit's label stood still on frames
//! with no tick and jumped a whole tick's travel on frames with one, while the
//! smoothly-following camera slid it backwards in between. Measured in the
//! running client at ~120fps on this exact approach: +2.19px, -0.14px,
//! +2.19px, -0.14px per frame — text that is crisp in any one frame and fuzzy
//! in motion.
//!
//! The anchor is now interpolated between the last two ticks
//! (`hud_anchor_translation`). This pins the property that fixes the shimmer:
//! while the unit walks steadily, the anchor advances by the unit's speed times
//! the FRAME's duration on every frame — never zero, never a whole tick.
//!
//! Runs with no window or GPU (same shape as `tests/hit_reaction_match_flow.rs`).
//! Visual-only systems never touch the sim, so the match is the seeded one.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{Combatant, PreviousSimTranslation};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    hud_anchor_translation, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// ~143fps: never a multiple of the 60Hz tick, so frames with no tick and
/// frames with one interleave irregularly, as they do on a real display.
const FRAME: Duration = Duration::from_millis(7);
/// Past the gates and through the stealthed approach.
const MAX_FRAMES: usize = 3000;
/// A tick's travel counts as a steady walk when it is at least this long...
const WALKING: f32 = 0.01;
/// ...and differs from the tick before by no more than this.
const STEADY: f32 = 1e-4;

#[derive(Resource, Default)]
struct Seen {
    /// The Rogue's translation after every sim tick.
    ticks: Vec<Vec3>,
    /// Per rendered frame while the Rogue is stealthed: how many ticks had run,
    /// and the anchor the health bar block hangs from.
    frames: Vec<(usize, Vec3)>,
}

fn boot(cfg: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    // The `--replay` entrance: straight into the match with the seed set.
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(FixedLast, observe_tick)
        // After the fixed loop, as `render_health_bars` sees the world.
        .add_systems(Update, observe_frame);
    app
}

fn rogue<'a>(
    combatants: impl Iterator<
        Item = (
            &'a Combatant,
            &'a Transform,
            Option<&'a PreviousSimTranslation>,
        ),
    >,
) -> Option<(
    &'a Combatant,
    &'a Transform,
    Option<&'a PreviousSimTranslation>,
)> {
    combatants
        .into_iter()
        .find(|(c, _, _)| c.class == CharacterClass::Rogue)
}

fn observe_tick(
    mut seen: ResMut<Seen>,
    combatants: Query<(&Combatant, &Transform, Option<&PreviousSimTranslation>)>,
) {
    if let Some((_, tf, _)) = rogue(combatants.iter()) {
        seen.ticks.push(tf.translation);
    }
}

fn observe_frame(
    mut seen: ResMut<Seen>,
    fixed: Res<Time<Fixed>>,
    combatants: Query<(&Combatant, &Transform, Option<&PreviousSimTranslation>)>,
) {
    let Some((c, tf, previous)) = rogue(combatants.iter()) else {
        return;
    };
    if !c.stealthed {
        return;
    }
    let anchor = hud_anchor_translation(tf.translation, previous, fixed.overstep_fraction());
    let ticks = seen.ticks.len();
    seen.frames.push((ticks, anchor));
}

#[test]
fn stealth_label_anchor_advances_every_frame_at_the_units_speed() {
    let mut app =
        boot(r#"{"team1":["Rogue"],"team2":["Mage"],"map":"BasicArena","random_seed":42}"#);
    for _ in 0..MAX_FRAMES {
        app.update();
    }
    let tick_secs = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let seen = std::mem::take(&mut *app.world_mut().resource_mut::<Seen>());
    let frame_secs = FRAME.as_secs_f32();

    let mut steady_frames = 0;
    for pair in seen.frames.windows(2) {
        let ((k0, a0), (k1, a1)) = (pair[0], pair[1]);
        // Both frames' anchors lie on the segments between ticks k1-3 .. k1.
        if k1 < 3 || k1 - k0 > 1 {
            continue;
        }
        let p = &seen.ticks[k1 - 3..k1];
        let (d1, d2) = (p[1] - p[0], p[2] - p[1]);
        if d2.length() < WALKING || (d2 - d1).length() > STEADY {
            continue;
        }
        steady_frames += 1;
        let expected = d2 * (frame_secs / tick_secs);
        let actual = a1 - a0;
        assert!(
            (actual - expected).length() <= 0.05 * expected.length(),
            "frame after tick {k1}: the anchor moved {actual:?}, expected {expected:?} \
             ({:.0}% of a tick's travel for {:.0}% of a tick's time) — the HUD is \
             stepping with the sim instead of gliding",
            100.0 * actual.length() / d2.length(),
            100.0 * frame_secs / tick_secs,
        );
    }
    assert!(
        steady_frames >= 200,
        "only {steady_frames} frames caught the stealthed Rogue walking steadily — \
         the seed no longer exercises the approach"
    );
}
