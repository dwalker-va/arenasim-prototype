//! Hemorrhage's visual in a REAL match (AS-367): every Hemorrhage a Subtlety
//! Rogue makes swings Sinister Strike's stroke on the frame it is made; a
//! landed one — a fresh wound or a refresh of its stacks — holds Rupture's
//! bleed spray for the stroke's contact and fires it once, at its actual
//! victim's chest; a dodged or parried one sprays nothing; and its wound
//! raises no debuff band.
//!
//! ## Why this exists
//!
//! The bare-app probes (`hemorrhage_visual_probes.rs`) hand the router its
//! markers. Only the real schedule shows that the drain's marker names the
//! unit the strike hit, that `note_avoided_strikes` sees the Rogue's
//! `MeleeAvoided` inside its window, and that a refresh of the stacking
//! debuff adds nothing beside the strike's own spray. So this boots
//! `PlayMatch` through the real `StatesPlugin` schedule the way `--replay`
//! does (the `crusader_strike_match_flow.rs` shape).
//!
//! Runs with no window or GPU. Visual-only systems never touch the sim, so the
//! match is the seeded one, but for the staged fact of the avoidance arms: on
//! the frame the gates open, the Warrior is given a certain dodge (or parry),
//! so every Hemorrhage that meets it standing is avoided — and the Rogue,
//! whose wound never lands, keeps choosing Hemorrhage — while one it takes
//! stunned still lands.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::combat_core::{Avoidance, MeleeAvoided};
use arenasim::states::play_match::components::{
    ActiveAuras, AuraBand, AuraBandPolarity, ClientParticle, Combatant, ImpactAnchor,
    InstantAbilityFired, MatchCountdown, PlayMatchEntity, SwingStyle, WeaponHand, WeaponSocket,
    HEMORRHAGE_AURA,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::systems::CombatSystemPhase;
use arenasim::states::play_match::{
    consume_avoided_strike_signals, consume_instant_ability_signals, impact_origin,
    note_avoided_strikes, AbilityConfigPlugin, AvoidedStrikeSignal, BleedDroplet, BleedSpray,
    BleedSprayPending, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the strikes below; the match need not end.
const FRAMES: usize = 60 * 40;

/// A Subtlety Rogue (Hemorrhage, AS-419) against a Warrior on BasicArena:
/// the Warrior's Mortal Strike and the Rogue's poisons and stuns land beside
/// the wound, and nothing the Rogue lands routes a debuff to the band.
const CONFIG: &str = r#"{"team1":["Rogue"],"team2":["Warrior"],
    "team1_builds":[{"MasterOfSubtlety":2,"Initiative":2,"Hemorrhage":1}],
    "map":"BasicArena","random_seed":7}"#;

/// Sinister Strike's contact: when the spray is due after its strike.
fn contact() -> f32 {
    SwingStyle::SinisterStrike.impact_at()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Strike {
    frame: usize,
    caster: Entity,
    target: Entity,
}

#[derive(Clone, Debug)]
struct Held {
    frame: usize,
    victim: Entity,
    caster: Entity,
    aura: String,
    delay: f32,
}

#[derive(Clone, Copy, Debug)]
struct Fired {
    frame: usize,
    victim: Entity,
    at: Vec3,
    /// The victim's chest on that frame.
    chest: Vec3,
}

#[derive(Resource, Default)]
struct Seen {
    frame: usize,
    /// Hemorrhages the drain landed (its marker), and whether the victim
    /// already wore this Rogue's wound: a refresh of its stacks.
    landed: Vec<(Strike, bool)>,
    /// Sinister Strikes the drain landed: they swing the same stroke.
    sinister: Vec<usize>,
    /// Hemorrhages dodged or parried (the sim's `MeleeAvoided`).
    avoided: Vec<(Strike, Avoidance)>,
    /// The avoided-strike path's signals for a Hemorrhage.
    avoided_signals: Vec<Strike>,
    held: Vec<Held>,
    fired: Vec<Fired>,
    /// Frames on which the Rogue's main hand started Sinister Strike's stroke.
    strokes: Vec<usize>,
    /// Debuff bands raised on the Warrior.
    victim_bands: usize,
    /// Bleed entities seen, and those of them the match's cleanup would miss.
    bleed_entities: usize,
    untagged: usize,
    /// The first frame the Warrior was dead, if it died.
    victim_died: Option<usize>,
    /// The Warrior.
    warrior: Option<Entity>,
    /// Droplet-frames drawn at the match camera, and those drawn behind
    /// their victim's body or at another size than they have where they
    /// truly are.
    droplets_drawn: usize,
    misdrawn: usize,
}

#[derive(Resource, Clone, Copy)]
struct Stage(Option<Avoidance>);

fn boot(stage: Option<Avoidance>) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(CONFIG).unwrap();
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
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_resource(Stage(stage))
        .init_resource::<Seen>();
    // The markers live only inside these windows: `MeleeAvoided` from the
    // class AI to Phase 3, the drain's marker from Phase 2 to the router, the
    // avoided-strike signal from its noting to its consumer.
    app.add_systems(PreUpdate, stage_avoidance)
        .add_systems(
            FixedUpdate,
            observe_avoided
                .after(CombatSystemPhase::CombatAndMovement)
                .before(CombatSystemPhase::CombatResolution),
        )
        .add_systems(
            FixedUpdate,
            observe_signals
                .after(CombatSystemPhase::CombatResolution)
                .after(note_avoided_strikes)
                .before(consume_instant_ability_signals)
                .before(consume_avoided_strike_signals),
        )
        .add_systems(Last, observe_drawn)
        .add_systems(
            Last,
            (|mut seen: ResMut<Seen>| seen.frame += 1).after(observe_drawn),
        );
    app
}

/// The avoidance arms' staged fact: from the gates, the Warrior avoids every
/// melee attack it meets able to act.
fn stage_avoidance(
    mut done: Local<bool>,
    stage: Res<Stage>,
    countdown: Option<Res<MatchCountdown>>,
    mut combatants: Query<&mut Combatant>,
) {
    let Some(kind) = stage.0 else {
        return;
    };
    if *done || !countdown.is_some_and(|c| c.gates_opened) {
        return;
    }
    for mut c in combatants.iter_mut() {
        if c.team == 2 && c.class == CharacterClass::Warrior {
            match kind {
                Avoidance::Dodge => c.dodge_chance = 1.0,
                Avoidance::Parry => c.parry_chance = 1.0,
            }
            *done = true;
        }
    }
    assert!(*done, "no Warrior to stage");
}

fn observe_avoided(mut seen: ResMut<Seen>, avoided: Query<&MeleeAvoided>) {
    let frame = seen.frame;
    for a in avoided.iter() {
        if a.attack == "Hemorrhage" {
            seen.avoided.push((
                Strike {
                    frame,
                    caster: a.attacker,
                    target: a.defender,
                },
                a.avoidance,
            ));
        }
    }
}

fn observe_signals(
    mut seen: ResMut<Seen>,
    fired: Query<&InstantAbilityFired>,
    signals: Query<&AvoidedStrikeSignal>,
    auras: Query<&ActiveAuras>,
) {
    let frame = seen.frame;
    for f in fired.iter() {
        let Some(target) = f.target else {
            continue;
        };
        match f.ability {
            AbilityType::Hemorrhage => {
                // The wound this strike lands is still pending here; one the
                // victim already wears is a refresh.
                let refresh = auras.get(target).is_ok_and(|a| {
                    a.auras
                        .iter()
                        .any(|x| x.caster == Some(f.caster) && x.ability_name == HEMORRHAGE_AURA)
                });
                seen.landed.push((
                    Strike {
                        frame,
                        caster: f.caster,
                        target,
                    },
                    refresh,
                ));
            }
            AbilityType::SinisterStrike => seen.sinister.push(frame),
            _ => {}
        }
    }
    for s in signals.iter() {
        if s.ability == AbilityType::Hemorrhage {
            seen.avoided_signals.push(Strike {
                frame,
                caster: s.caster,
                target: s.target,
            });
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn observe_drawn(
    mut seen: ResMut<Seen>,
    held: Query<&BleedSprayPending, Added<BleedSprayPending>>,
    fired: Query<(&BleedSpray, &Transform), Added<BleedSpray>>,
    sockets: Query<&WeaponSocket, Changed<WeaponSocket>>,
    bands: Query<&AuraBand, Added<AuraBand>>,
    combatants: Query<(Entity, &Combatant, &Transform)>,
    bleed: Query<
        Has<PlayMatchEntity>,
        Or<(
            With<BleedSprayPending>,
            With<BleedSpray>,
            With<BleedDroplet>,
        )>,
    >,
    droplets: Query<(&BleedDroplet, &Transform, &ClientParticle)>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let frame = seen.frame;
    if seen.warrior.is_none() {
        seen.warrior = combatants
            .iter()
            .find(|(_, c, _)| c.team == 2 && c.class == CharacterClass::Warrior)
            .map(|(e, _, _)| e);
    }
    // Every droplet as drawn: never deeper than its victim's body (a
    // combatant's capsule, radius 0.5 and 0.75 either side of its origin),
    // and at the apparent size it has where it truly is (the bench's radius
    // keys 0.03 / 0.11 / 0.02 at life 0 / 0.25 / 1, drawn as a diameter).
    if let Some(cam) = camera.iter().next() {
        let (eye, fwd) = (cam.translation, cam.forward().as_vec3());
        for (d, tf, p) in droplets.iter() {
            let Ok((_, _, victim)) = combatants.get(d.victim) else {
                continue;
            };
            let (a, b) = (
                victim.translation - Vec3::Y * 0.75,
                victim.translation + Vec3::Y * 0.75,
            );
            let nearest = (a - eye).dot(fwd).min((b - eye).dot(fwd)) - 0.5;
            let (dt, dd) = ((d.truly - eye).dot(fwd), (tf.translation - eye).dot(fwd));
            let t = p.age / 0.5;
            let radius = if t < 0.25 {
                0.03 + (0.11 - 0.03) * t / 0.25
            } else {
                0.11 + (0.02 - 0.11) * (t - 0.25) / 0.75
            };
            seen.droplets_drawn += 1;
            if dd > nearest + 1e-4 || (tf.scale.x / dd - 2.0 * radius / dt).abs() > 1e-4 {
                seen.misdrawn += 1;
            }
        }
    }
    for h in held.iter() {
        seen.held.push(Held {
            frame,
            victim: h.victim,
            caster: h.caster,
            aura: h.aura.clone(),
            delay: h.delay,
        });
    }
    for (spray, tf) in fired.iter() {
        let chest = combatants
            .get(spray.victim)
            .map(|(_, _, v)| impact_origin(ImpactAnchor::Chest, v.translation, false))
            .unwrap_or(Vec3::NAN);
        seen.fired.push(Fired {
            frame,
            victim: spray.victim,
            at: tf.translation,
            chest,
        });
    }
    for s in sockets.iter() {
        let rogue = combatants
            .get(s.owner)
            .is_ok_and(|(_, c, _)| c.class == CharacterClass::Rogue);
        // The stroke's first frame: released this frame from a full windup.
        if rogue
            && s.hand == WeaponHand::Main
            && s.swing_style == SwingStyle::SinisterStrike
            && s.release_t.is_some_and(|t| t < 0.03)
        {
            seen.strokes.push(frame);
        }
    }
    for b in bands.iter() {
        let on_warrior = combatants
            .get(b.target)
            .is_ok_and(|(_, c, _)| c.team == 2 && c.class == CharacterClass::Warrior);
        if on_warrior && b.polarity == AuraBandPolarity::Debuff {
            seen.victim_bands += 1;
        }
    }
    for tagged in bleed.iter() {
        seen.bleed_entities += 1;
        if !tagged {
            seen.untagged += 1;
        }
    }
    if seen.victim_died.is_none()
        && combatants
            .iter()
            .any(|(_, c, _)| c.team == 2 && c.class == CharacterClass::Warrior && !c.is_alive())
    {
        seen.victim_died = Some(frame);
    }
}

/// Play the match, and once it has left `PlayMatch`, the number of bleed
/// entities the match's cleanup left behind.
fn play(stage: Option<Avoidance>) -> (Seen, Option<usize>) {
    let mut app = boot(stage);
    let mut left = None;
    for _ in 0..FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            app.update();
            let mut q = app.world_mut().query_filtered::<(), Or<(
                With<BleedSprayPending>,
                With<BleedSpray>,
                With<BleedDroplet>,
            )>>();
            left = Some(q.iter(app.world()).count());
            break;
        }
    }
    let seen = std::mem::take(&mut *app.world_mut().resource_mut::<Seen>());
    (seen, left)
}

/// The frames between a held spray (its delay as seen after its first
/// frame's count) and its contact: it fires on the first frame its count
/// reaches it.
fn due_after(delay: f32) -> usize {
    (delay / FRAME.as_secs_f32()).ceil() as usize
}

/// What every arm asserts: each landed Hemorrhage holds exactly one spray for
/// its own victim, on its own frame, at Sinister Strike's contact; each held
/// spray fires once, on its due frame, at its victim's chest, unless its
/// victim died first; each Hemorrhage, landed or avoided, starts the stroke
/// on its frame; and every bleed entity is the match's to clean up.
fn assert_every_landing_sprays_once(seen: &Seen, arm: &str) {
    eprintln!(
        "{arm}: landed {} (refresh {}), avoided {}, held {}, fired {}, strokes {}, sinister {}, bleed {}, drawn {}, died {:?}",
        seen.landed.len(),
        seen.landed.iter().filter(|(_, r)| *r).count(),
        seen.avoided.len(),
        seen.held.len(),
        seen.fired.len(),
        seen.strokes.len(),
        seen.sinister.len(),
        seen.bleed_entities,
        seen.droplets_drawn,
        seen.victim_died
    );
    let rogue = seen
        .landed
        .iter()
        .map(|(s, _)| s.caster)
        .chain(seen.avoided.iter().map(|(s, _)| s.caster))
        .next()
        .expect("a Hemorrhage was made");
    // One held spray per landing: same frame, the strike's own target, this
    // Rogue, the wound's name, the stroke's contact (seen after its first
    // frame's count, on the frame it was held).
    let held: Vec<(usize, Entity, Entity)> = seen
        .held
        .iter()
        .map(|h| (h.frame, h.victim, h.caster))
        .collect();
    let landed: Vec<(usize, Entity, Entity)> = seen
        .landed
        .iter()
        .map(|(s, _)| (s.frame, s.target, s.caster))
        .collect();
    assert_eq!(held, landed, "{arm}: one held spray per landed Hemorrhage");
    // ...each on the unit the strike hit: the Warrior.
    for (s, _) in &seen.landed {
        assert_eq!(Some(s.target), seen.warrior, "{arm}: struck {:?}", s.target);
    }
    for h in &seen.held {
        assert_eq!(h.aura, HEMORRHAGE_AURA, "{arm}");
        assert_eq!(h.caster, rogue, "{arm}");
        let frame = FRAME.as_secs_f32();
        assert!(
            (h.delay + frame - contact()).abs() < 1e-5,
            "{arm}: held {} after a frame of {}",
            h.delay,
            contact()
        );
    }
    // Each fires once, on its due frame, at its victim's chest — unless the
    // victim was dead by then.
    let mut expected = Vec::new();
    for h in &seen.held {
        let due = h.frame + due_after(h.delay);
        if seen.victim_died.is_some_and(|d| d <= due) {
            continue;
        }
        expected.push((due, h.victim));
    }
    let fired: Vec<(usize, Entity)> = seen.fired.iter().map(|f| (f.frame, f.victim)).collect();
    assert_eq!(
        fired, expected,
        "{arm}: each held spray fires once at contact, on its own victim"
    );
    for f in &seen.fired {
        assert!(
            f.at.distance(f.chest) < 1e-4,
            "{arm}: a spray at {} is not on its victim's chest {}",
            f.at,
            f.chest
        );
    }
    // The stroke: on the frame of every Hemorrhage, landed or avoided (and of
    // every Sinister Strike, which swings the same one), and on no other.
    let mut strikes: Vec<usize> = seen.landed.iter().map(|(s, _)| s.frame).collect();
    strikes.extend(seen.avoided.iter().map(|(s, _)| s.frame));
    strikes.extend(&seen.sinister);
    strikes.sort();
    strikes.dedup();
    assert_eq!(
        seen.strokes, strikes,
        "{arm}: Sinister Strike's stroke starts on each strike's frame"
    );
    // No band for the wound, fresh or refreshed.
    assert_eq!(
        seen.victim_bands, 0,
        "{arm}: the spray owns the wound's landing"
    );
    // Every bleed entity is tagged for the match's cleanup.
    assert_eq!(seen.untagged, 0, "{arm}: untagged bleed entities");
    // Every droplet is drawn in front of its victim at its true size.
    assert_eq!(seen.misdrawn, 0, "{arm}: of {} drawn", seen.droplets_drawn);
    assert_eq!(
        seen.droplets_drawn > 0,
        !seen.fired.is_empty(),
        "{arm}: droplets drawn {}",
        seen.droplets_drawn
    );
}

/// Every Hemorrhage that lands sprays once at its victim's chest, the
/// refreshes of its stacks included; nothing is avoided; and every bleed
/// entity goes with the match.
#[test]
fn every_landed_hemorrhage_sprays_once_on_its_victim_in_a_real_match() {
    let (seen, left) = play(None);
    assert!(seen.avoided.is_empty(), "the control avoids nothing");
    let refreshes = seen.landed.iter().filter(|(_, r)| *r).count();
    let fresh = seen.landed.len() - refreshes;
    assert!(
        refreshes >= 2 && fresh >= 1,
        "landings: {fresh} fresh, {refreshes} refreshes"
    );
    assert!(seen.fired.len() >= 3, "sprays fired: {}", seen.fired.len());
    assert!(seen.bleed_entities > 100, "{}", seen.bleed_entities);
    assert_every_landing_sprays_once(&seen, "control");
    // The match is won within the window, and its cleanup leaves no blood.
    assert!(seen.victim_died.is_some(), "the Warrior dies");
    assert_eq!(left, Some(0), "bleed entities outlived the match");
}

/// A dodged or parried Hemorrhage swings the stroke on its own frame, through
/// exactly one avoided-strike signal, and draws no blood: no landed marker,
/// no held spray, no spray.
#[test]
fn an_avoided_hemorrhage_strokes_once_and_sprays_nothing_in_a_real_match() {
    for kind in [Avoidance::Dodge, Avoidance::Parry] {
        let arm = format!("{kind:?}");
        let (seen, _) = play(Some(kind));
        assert!(
            seen.avoided.len() >= 3,
            "{arm}: avoided Hemorrhages {:?}",
            seen.avoided
        );
        assert!(
            seen.avoided.iter().all(|(_, k)| *k == kind),
            "{arm}: {:?}",
            seen.avoided
        );
        // Exactly one avoided-strike signal per avoided Hemorrhage, for the
        // strike's own caster and target.
        let avoided: Vec<Strike> = seen.avoided.iter().map(|(s, _)| *s).collect();
        assert_eq!(seen.avoided_signals, avoided, "{arm}");
        // ...and no landed marker on its frame.
        for (s, _) in &seen.avoided {
            assert!(
                !seen
                    .landed
                    .iter()
                    .any(|(l, _)| l.frame == s.frame && l.caster == s.caster),
                "{arm}: an avoided Hemorrhage also landed on frame {}",
                s.frame
            );
        }
        // Only the landings (taken stunned) hold sprays and spray.
        assert_every_landing_sprays_once(&seen, &arm);
    }
}
