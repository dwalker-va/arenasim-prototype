//! Probes for the cast-side heal visuals
//! (`rendering/effects/heal_cast.rs`) — the per-hand glow-and-wisp rigs that
//! replace the generic casting orb for hard-cast heals.
//!
//! These assert WORLD-SPACE GEOMETRY through `GlobalTransform`, not stored
//! fields: that both hand glows light up at the rig's real spell-hand
//! sockets (at the real spawn height — combatant capsule centres sit at
//! world y = 1.0 over the floor plane at y = 0), that the wisps orbit and
//! stream at the blessed widths and lengths, that Nature sheds leaves and
//! launches its water-ring/gold-spark jet, that Holy re-flares its own glow
//! at launch, that the cast posture leans back then surges forward, and that
//! an interrupted cast stops EVERYTHING dead with zero survivors.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` (the latter
//! is load-bearing: without it `GlobalTransform` never propagates and every
//! assertion would read a child's LOCAL pose). The harness registers the
//! full graphical chain — endings consumer, spawn, update, cleanup, posture,
//! billboard — in production order, with a real `Camera3d` wherever the
//! billboard pass finalizes the probed pose.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    CastEnding, CastEndingKind, CastingOrb, CastingState, Combatant, HealCastBurstKind,
    HealCastBurstMote, HealCastHand, HealCastKind, HealCastLeaf, HealCastPhase, HealCastPiece,
    HealCastPieceRole, HealCastPosture, VisualBody,
};
use arenasim::states::play_match::{
    billboard_heal_cast_glows, cleanup_heal_cast_glows, consume_cast_ending_signals,
    consume_heal_cast_endings, heal_cast_glow_sizes, heal_cast_wisp_length,
    spawn_casting_orbs, spawn_heal_cast_glows, spell_hand_local,
    update_heal_cast_glows, update_heal_cast_posture, FLASH_OF_LIGHT_HAS_LAUNCH_FLASH,
    HEAL_CAST_WISP_LIFETIMES, HEAL_CAST_WISP_ORBIT_RADIUS, HOLY_CAST_WISP_WIDTH,
    HOLY_LAUNCH_FLARE_SECS, NATURE_CAST_LEAF_SPEED, NATURE_CAST_WISP_WIDTH,
    NATURE_LAUNCH_BURST_SECS, NATURE_LAUNCH_SPREAD, SPELL_HAND_X, SPELL_HAND_Y,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

struct Harness {
    app: App,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.insert_resource(AbilityDefinitions::default());
        // Production order: the FixedUpdate consumers run before the Update
        // group each frame; within the group spawn -> update -> cleanup ->
        // posture -> billboard, and the orb spawner runs alongside so the
        // orb-vs-hand-glow routing is probed against the real pair.
        app.add_systems(
            Update,
            (
                consume_heal_cast_endings,
                consume_cast_ending_signals,
                spawn_casting_orbs,
                spawn_heal_cast_glows,
                update_heal_cast_glows,
                cleanup_heal_cast_glows,
                update_heal_cast_posture,
                billboard_heal_cast_glows,
            )
                .chain(),
        );
        Harness { app }
    }

    fn spawn_camera(&mut self, from: Vec3, look_at: Vec3) -> Quat {
        let transform = Transform::from_translation(from).looking_at(look_at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform.rotation
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// A caster with the REAL rendered hierarchy — sim entity at the real
    /// game height (capsule centre 1.0 above the floor plane) with a
    /// `VisualBody` child, exactly as `spawn_combatant` builds it.
    fn spawn_caster(&mut self, class: CharacterClass, at: Vec3) -> (Entity, Entity) {
        let caster = self
            .app
            .world_mut()
            .spawn((Combatant::new(0, 0, class), Transform::from_translation(at)))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        self.app.world_mut().entity_mut(caster).add_child(body);
        (caster, body)
    }

    fn begin_cast(&mut self, caster: Entity, ability: AbilityType) {
        let cast_time = AbilityDefinitions::default().get_unchecked(&ability).cast_time;
        self.app
            .world_mut()
            .entity_mut(caster)
            .insert(CastingState::new(ability, caster, cast_time));
    }

    fn end_cast(&mut self, caster: Entity, kind: CastEndingKind) {
        // Mirror the core resolution sites: the state goes (or is flagged
        // interrupted) and a CastEnding marker is spawned.
        match kind {
            CastEndingKind::Interrupted => {
                let mut state = self
                    .app
                    .world_mut()
                    .get_mut::<CastingState>(caster)
                    .expect("interrupting a live cast");
                state.interrupted = true;
                state.interrupted_display_time = 0.5;
            }
            _ => {
                self.app.world_mut().entity_mut(caster).remove::<CastingState>();
            }
        }
        self.app.world_mut().spawn(CastEnding { caster, kind });
    }

    fn rigs(&mut self) -> Vec<(f32, HealCastPhase, Vec3, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&HealCastHand, &Transform, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(r, t, g)| (r.side, r.phase, t.translation, g.translation()))
            .collect()
    }

    fn pieces(&mut self) -> Vec<(HealCastPieceRole, f32, GlobalTransform, f32)> {
        let mut q = self.app.world_mut().query::<(
            &HealCastPiece,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let rows: Vec<(HealCastPieceRole, f32, GlobalTransform, Handle<StandardMaterial>)> = q
            .iter(self.app.world())
            .map(|(p, g, m)| (p.role, p.base_alpha, *g, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(role, base, g, handle)| {
                let alpha = materials
                    .get(&handle)
                    .map(|m| m.base_color.alpha())
                    .unwrap_or(0.0);
                (role, base, g, alpha)
            })
            .collect()
    }

    fn leaves(&mut self) -> Vec<(Vec3, Vec3)> {
        let mut q = self.app.world_mut().query::<(&HealCastLeaf, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(l, g)| (l.velocity, g.translation()))
            .collect()
    }

    fn bursts(&mut self) -> Vec<(HealCastBurstKind, Vec3, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&HealCastBurstMote, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(b, g)| (b.kind, b.velocity, g.translation()))
            .collect()
    }

    fn body_rotation(&mut self, body: Entity) -> Quat {
        self.app.world().get::<Transform>(body).unwrap().rotation
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&C>();
        q.iter(self.app.world()).count()
    }
}

fn assert_min(label: &str, actual: usize, min: usize) {
    assert!(
        actual >= min,
        "{label}: expected at least {min}, saw {actual} — the probe went vacuous"
    );
}

/// World endpoints of a piece's unit quad along its local Y (the long axis).
fn quad_y_endpoints(g: &GlobalTransform) -> (Vec3, Vec3) {
    (
        g.transform_point(Vec3::new(0.0, -0.5, 0.0)),
        g.transform_point(Vec3::new(0.0, 0.5, 0.0)),
    )
}

// ── routing ────────────────────────────────────────────────────────────────

/// Every hard-cast heal in the config must reach a cast-side family; every
/// instant and non-heal must reach none.
#[test]
fn every_hard_cast_heal_reaches_a_cast_family() {
    let defs = AbilityDefinitions::default();
    let mut silent = Vec::new();
    for (ability, config) in defs.iter() {
        if config.is_heal() && config.cast_time > 0.0 && HealCastKind::for_ability(*ability).is_none()
        {
            silent.push(*ability);
        }
    }
    assert!(
        silent.is_empty(),
        "hard-cast heals with no cast-side visual: {silent:?} — add an arm \
         to HealCastKind::for_ability"
    );
    // The blessed family split: Holy for Priest+Paladin, Nature for Shaman.
    assert_eq!(HealCastKind::for_ability(AbilityType::FlashHeal), Some(HealCastKind::Holy));
    assert_eq!(HealCastKind::for_ability(AbilityType::HolyLight), Some(HealCastKind::Holy));
    assert_eq!(
        HealCastKind::for_ability(AbilityType::FlashOfLight),
        Some(HealCastKind::Holy)
    );
    assert_eq!(
        HealCastKind::for_ability(AbilityType::LesserHealingWave),
        Some(HealCastKind::Nature)
    );
    // Instants and non-heals keep their own vocabulary.
    assert_eq!(HealCastKind::for_ability(AbilityType::HolyShock), None);
    assert_eq!(HealCastKind::for_ability(AbilityType::Frostbolt), None);
    assert_eq!(HealCastKind::for_ability(AbilityType::MindBlast), None);
}

/// The orb handoff: a heal cast spawns hand rigs and NO casting orb; a
/// non-heal cast keeps the orb and grows NO hand rigs.
#[test]
fn heals_replace_the_casting_orb_and_nothing_else_does() {
    let mut h = Harness::new();
    let (healer, _) = h.spawn_caster(CharacterClass::Priest, Vec3::new(0.0, 1.0, 0.0));
    let (mage, _) = h.spawn_caster(CharacterClass::Mage, Vec3::new(5.0, 1.0, 0.0));
    h.begin_cast(healer, AbilityType::FlashHeal);
    h.begin_cast(mage, AbilityType::Frostbolt);
    h.tick(3);

    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "the heal lights BOTH hands, the bolt neither");
    let orbs = h.count::<CastingOrb>();
    assert_eq!(orbs, 1, "exactly the Frostbolt's orb survives the handoff");
}

// ── the spell-hand sockets ─────────────────────────────────────────────────

/// Both hands light up AT the rig's spell-hand sockets, mirrored about the
/// spine, at the real spawn height — and rigidly attached: the rig's LOCAL
/// pose is the socket, and its world pose is that socket composed through
/// the caster's transform, clear of the 0.5-yd capsule.
#[test]
fn hand_glows_sit_at_the_rig_hand_sockets() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, 1.0, -2.0);
    let (caster, _) = h.spawn_caster(CharacterClass::Priest, at);
    h.begin_cast(caster, AbilityType::FlashHeal);
    h.tick(4);

    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "both hands always — the source attaches 21 AND 22");
    let sides: Vec<f32> = rigs.iter().map(|(s, ..)| *s).collect();
    assert!(sides.contains(&1.0) && sides.contains(&-1.0), "one rig per hand: {sides:?}");

    for (side, phase, local, world) in &rigs {
        assert!(matches!(phase, HealCastPhase::Loop));
        // The LOCAL pose is the socket, exactly.
        let socket = spell_hand_local(*side);
        assert!(
            local.distance(socket) < 1e-6,
            "hand {side} parked at local {local}, socket is {socket}"
        );
        // The WORLD pose is the socket through the caster's transform (the
        // eased cast posture may pitch it by a few hundredths of a yard).
        let expected = at + socket;
        assert!(
            world.distance(expected) < 0.06,
            "hand {side} rendered at {world}, expected ~{expected}"
        );
        // Real spawn height: the hands hover mid-body over the floor plane
        // (world y = 0), never at the feet and never below the floor.
        assert!(
            (world.y - (1.0 + SPELL_HAND_Y)).abs() < 0.06,
            "hand {side} at world height {} — not at the capsule's hand line",
            world.y
        );
        // Clear of the capsule: the glow must not be swallowed by the body.
        let spine_r = Vec2::new(world.x - at.x, world.z - at.z).length();
        assert!(
            spine_r > 0.5 && (spine_r - SPELL_HAND_X).abs() < 0.35,
            "hand {side} at {spine_r}yd off the spine — inside the capsule \
             or off the hand line"
        );
    }
}

// ── the glow and the wisps ─────────────────────────────────────────────────

/// Holy hands: two glow layers at the blessed 0.88/0.54 sizes, billboarded
/// to the camera through the full parent chain, and three wide gold wisps
/// whose trails have the blessed width and per-lifetime lengths and whose
/// heads actually ORBIT the hand.
#[test]
fn holy_glow_layers_and_wisps_at_blessed_geometry() {
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, 1.0, 4.0);
    let (caster, _) = h.spawn_caster(CharacterClass::Paladin, at);
    let cam_rot = h.spawn_camera(Vec3::new(6.0, 6.0, 20.0), at);
    h.begin_cast(caster, AbilityType::HolyLight);
    h.tick(30); // bloom + posture ease complete

    let pieces = h.pieces();
    let sizes = heal_cast_glow_sizes(HealCastKind::Holy);
    let mut outer = 0;
    let mut core = 0;
    for (role, _, g, _) in &pieces {
        let (a, b) = quad_y_endpoints(g);
        let extent = a.distance(b);
        match role {
            HealCastPieceRole::GlowOuter => {
                outer += 1;
                assert!(
                    (extent - sizes[0]).abs() < 0.08,
                    "outer glow renders {extent}yd across, blessed {}",
                    sizes[0]
                );
            }
            HealCastPieceRole::GlowCore => {
                core += 1;
                assert!(
                    (extent - sizes[1]).abs() < 0.06,
                    "core glow renders {extent}yd across, blessed {}",
                    sizes[1]
                );
            }
            HealCastPieceRole::Wisp { .. } => {}
        }
        // Billboarded through the FULL parent chain (facing composes the
        // caster rotation AND the body's posture pitch): the rendered quad
        // normal must land on the camera's view axis.
        if !matches!(role, HealCastPieceRole::Wisp { .. }) {
            let origin = g.translation();
            let normal = (g.transform_point(Vec3::Z) - origin).normalize();
            let view = cam_rot * Vec3::Z;
            assert!(
                normal.dot(view) > 0.995,
                "glow normal {normal} is off the camera axis {view}"
            );
        }
    }
    assert_eq!(outer, 2, "one outer glow per hand");
    assert_eq!(core, 2, "one core glow per hand");

    // The wisps: blessed width, per-lifetime trail lengths, orbiting heads.
    // Wisps live on BOTH hands, so each is measured against its NEAREST hand.
    let rigs = h.rigs();
    let nearest_hand = |p: Vec3, rigs: &[(f32, HealCastPhase, Vec3, Vec3)]| -> Vec3 {
        rigs.iter()
            .map(|(_, _, _, hand)| *hand)
            .min_by(|a, b| a.distance(p).partial_cmp(&b.distance(p)).unwrap())
            .unwrap()
    };
    let mut wisp_offsets_before = Vec::new();
    for (role, _, g, _) in &pieces {
        if let HealCastPieceRole::Wisp { index } = role {
            let (a, b) = quad_y_endpoints(g);
            let length = a.distance(b);
            let expected = heal_cast_wisp_length(*index);
            assert!(
                (length - expected).abs() < 0.05,
                "wisp {index} streams {length}yd, blessed {expected} \
                 (lifetime {})",
                HEAL_CAST_WISP_LIFETIMES[*index as usize]
            );
            let width = g
                .transform_point(Vec3::new(0.5, 0.0, 0.0))
                .distance(g.transform_point(Vec3::new(-0.5, 0.0, 0.0)));
            assert!(
                (width - HOLY_CAST_WISP_WIDTH).abs() < 0.04,
                "wisp {index} is {width}yd wide, blessed {HOLY_CAST_WISP_WIDTH}"
            );
            // Trail centres hang within orbit radius + half a trail of the hand.
            let center = g.translation();
            let hand = nearest_hand(center, &rigs);
            let reach = center.distance(hand);
            assert!(
                reach < HEAL_CAST_WISP_ORBIT_RADIUS + expected * 0.5 + 0.12,
                "wisp {index} centre strayed {reach}yd from the hand"
            );
            wisp_offsets_before.push((*index, center - hand));
        }
    }
    assert_eq!(wisp_offsets_before.len(), 6, "three wisps per hand");

    // The swirl: a quarter-second later every wisp's offset from its hand
    // has rotated visibly (1 Hz orbit -> ~86 degrees in 15 frames).
    h.tick(15);
    let rigs_after = h.rigs();
    let pieces_after = h.pieces();
    let mut advanced = 0;
    for (role, _, g, _) in &pieces_after {
        if let HealCastPieceRole::Wisp { index } = role {
            let after = g.translation() - nearest_hand(g.translation(), &rigs_after);
            let Some((_, before)) = wisp_offsets_before
                .iter()
                .find(|(i, v)| i == index && v.distance(after) < 2.0)
            else {
                continue;
            };
            let angle = before.angle_between(after);
            if angle > 0.5 {
                advanced += 1;
            }
        }
    }
    assert_min("orbiting wisps", advanced, 4);
}

/// Nature hands: green glow at 0.76/0.36, thin 0.10-yd star-thread wisps,
/// and leaves puffing off each hand at the transcribed drift speed in
/// genuinely scattered directions.
#[test]
fn nature_hands_shed_drifting_leaves() {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, 1.0, 1.0);
    let (caster, _) = h.spawn_caster(CharacterClass::Shaman, at);
    h.spawn_camera(Vec3::new(0.0, 8.0, 22.0), at);
    h.begin_cast(caster, AbilityType::LesserHealingWave);
    h.tick(60); // ~0.96s: the leaf population is at steady state

    let sizes = heal_cast_glow_sizes(HealCastKind::Nature);
    let pieces = h.pieces();
    for (role, _, g, _) in &pieces {
        let (a, b) = quad_y_endpoints(g);
        let extent = a.distance(b);
        match role {
            HealCastPieceRole::GlowOuter => assert!(
                (extent - sizes[0]).abs() < 0.08,
                "nature outer glow {extent}yd, blessed {}",
                sizes[0]
            ),
            HealCastPieceRole::GlowCore => assert!(
                (extent - sizes[1]).abs() < 0.05,
                "nature core glow {extent}yd, blessed {}",
                sizes[1]
            ),
            HealCastPieceRole::Wisp { index } => {
                let width = g
                    .transform_point(Vec3::new(0.5, 0.0, 0.0))
                    .distance(g.transform_point(Vec3::new(-0.5, 0.0, 0.0)));
                assert!(
                    (width - NATURE_CAST_WISP_WIDTH).abs() < 0.02,
                    "nature wisp {index} is {width}yd wide, blessed \
                     {NATURE_CAST_WISP_WIDTH} — star-threads, not streamers"
                );
            }
        }
    }

    let leaves = h.leaves();
    assert_min("airborne leaves", leaves.len(), 10);
    let mut directions = Vec::new();
    for (velocity, pos) in &leaves {
        assert!(
            (velocity.length() - NATURE_CAST_LEAF_SPEED).abs() < 1e-4,
            "leaf drifts at {} u/s, transcribed {NATURE_CAST_LEAF_SPEED}",
            velocity.length()
        );
        // Leaves stay NEAR the hand — near-motionless drift, 0.8s life.
        let near_a_hand = h
            .rigs()
            .iter()
            .any(|(_, _, _, hand)| pos.distance(*hand) < 0.35);
        assert!(near_a_hand, "leaf at {pos} strayed off the hands");
        directions.push(velocity.normalize());
    }
    // Omni drift: the shed genuinely scatters (max pairwise angle far apart).
    let mut max_angle = 0.0_f32;
    for i in 0..directions.len() {
        for j in (i + 1)..directions.len() {
            max_angle = max_angle.max(directions[i].angle_between(directions[j]));
        }
    }
    assert!(
        max_angle > 1.5,
        "leaves puff in every direction; max pairwise angle {max_angle} rad"
    );
}

// ── the launches ───────────────────────────────────────────────────────────

/// Holy launch: a landed cast re-flares the SAME glow on both hands —
/// pieces survive, brighter than their loop alpha — then everything is gone
/// after the flare window. Flash of Light takes the same flare (the blessed
/// FLASH_OF_LIGHT_HAS_LAUNCH_FLASH override of the source's silence).
#[test]
fn holy_launch_reflares_the_same_glow_then_retires() {
    for ability in [AbilityType::FlashHeal, AbilityType::FlashOfLight] {
        assert!(FLASH_OF_LIGHT_HAS_LAUNCH_FLASH, "blessed spec: FoL flares");
        let mut h = Harness::new();
        let (caster, _) = h.spawn_caster(CharacterClass::Priest, Vec3::new(0.0, 1.0, 0.0));
        h.spawn_camera(Vec3::new(0.0, 6.0, 18.0), Vec3::new(0.0, 1.0, 0.0));
        h.begin_cast(caster, ability);
        h.tick(30);
        let loop_alpha: f32 = h
            .pieces()
            .iter()
            .filter(|(role, ..)| matches!(role, HealCastPieceRole::GlowCore))
            .map(|(_, _, _, alpha)| *alpha)
            .fold(0.0, f32::max);

        h.end_cast(caster, CastEndingKind::Landed);
        h.tick(2);

        let rigs = h.rigs();
        assert_eq!(rigs.len(), 2, "{ability:?}: the flare keeps both hands lit");
        for (side, phase, ..) in &rigs {
            assert!(
                matches!(phase, HealCastPhase::Flare { .. }),
                "{ability:?} hand {side} still in {phase:?} after landing"
            );
        }
        // The SAME pieces re-flare — still present, and BRIGHTER.
        let flare_pieces = h.pieces();
        assert_min("flaring pieces", flare_pieces.len(), 10);
        let flare_alpha: f32 = flare_pieces
            .iter()
            .filter(|(role, ..)| matches!(role, HealCastPieceRole::GlowCore))
            .map(|(_, _, _, alpha)| *alpha)
            .fold(0.0, f32::max);
        assert!(
            flare_alpha > loop_alpha * 1.2,
            "{ability:?}: the launch must RE-FLARE the glow: loop alpha \
             {loop_alpha} -> flare alpha {flare_alpha}"
        );

        // Spent: nothing survives the flare window.
        h.tick((HOLY_LAUNCH_FLARE_SECS / DT).ceil() as u32 + 3);
        assert_eq!(h.rigs().len(), 0, "{ability:?}: rigs retire after the flare");
        assert_eq!(h.pieces().len(), 0, "{ability:?}: pieces die with the rigs");
    }
}

/// Nature launch: the loop kit dies AT the landing and the dedicated burst
/// takes over — a tight jet of expanding water rings and gold sparks at the
/// transcribed speeds, gone ~0.3s later.
#[test]
fn nature_launch_fires_the_water_ring_and_spark_jet() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, 1.0, 2.0);
    let (caster, _) = h.spawn_caster(CharacterClass::Shaman, at);
    h.spawn_camera(Vec3::new(0.0, 7.0, 20.0), at);
    h.begin_cast(caster, AbilityType::LesserHealingWave);
    h.tick(40);
    assert_min("loop pieces before landing", h.pieces().len(), 10);

    h.end_cast(caster, CastEndingKind::Landed);
    h.tick(10); // ~0.16s into the 0.3s burst

    // The loop kit is DEAD: no glow, no wisps, no airborne leaves.
    assert_eq!(
        h.pieces().len(),
        0,
        "nature's launch replaces the loop kit outright"
    );
    assert_eq!(h.leaves().len(), 0, "airborne leaves die with the loop kit");

    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "the burst fires on BOTH hands");

    let bursts = h.bursts();
    let rings = bursts
        .iter()
        .filter(|(k, ..)| *k == HealCastBurstKind::WaterRing)
        .count();
    let sparks = bursts
        .iter()
        .filter(|(k, ..)| *k == HealCastBurstKind::GoldSpark)
        .count();
    assert_min("water rings", rings, 4);
    assert_min("gold sparks", sparks, 6);

    // A collimated jet at the transcribed speeds: every mote within the ~2°
    // spread of the shared jet line, at 0.261 (rings) / 0.278 (sparks) u/s.
    let jet = arenasim::states::play_match::nature_launch_jet_dir();
    for (kind, velocity, _) in &bursts {
        let speed = velocity.length();
        let expected = match kind {
            HealCastBurstKind::WaterRing => 0.261,
            HealCastBurstKind::GoldSpark => 0.278,
        };
        assert!(
            (speed - expected).abs() < 0.005,
            "{kind:?} travels {speed} u/s, transcribed {expected}"
        );
        let angle = velocity.normalize().angle_between(jet);
        assert!(
            angle <= NATURE_LAUNCH_SPREAD + 1e-3,
            "{kind:?} leaves the jet by {angle} rad (spread {NATURE_LAUNCH_SPREAD})"
        );
    }

    // Spent: the whole launch is over ~0.3s after the landing.
    h.tick((NATURE_LAUNCH_BURST_SECS / DT).ceil() as u32 + 3);
    assert_eq!(h.rigs().len(), 0, "the burst rigs retire");
    assert_eq!(h.bursts().len(), 0, "burst motes die with the rigs");
}

// ── the cast posture ───────────────────────────────────────────────────────

/// The capsule's omni cast pair: the torso leans BACK through the loop
/// (ReadySpellOmni), surges FORWARD at the launch (SpellCastOmni), and
/// returns exactly to identity when the show is over.
#[test]
fn cast_posture_leans_back_then_surges_forward() {
    let mut h = Harness::new();
    let (caster, body) = h.spawn_caster(CharacterClass::Priest, Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(h.body_rotation(body), Quat::IDENTITY);

    h.begin_cast(caster, AbilityType::FlashHeal);
    h.tick(30); // past the 0.25s ease
    // Leaning BACK: the head (+Y) tips away from the facing (+Z).
    let head = h.body_rotation(body) * Vec3::Y;
    assert!(
        head.z < -0.05,
        "loop posture must lean the torso back; head axis tips z {}",
        head.z
    );

    h.end_cast(caster, CastEndingKind::Landed);
    h.tick(6); // ~0.1s into the flare: the release surge peaks
    let head = h.body_rotation(body) * Vec3::Y;
    assert!(
        head.z > 0.03,
        "the launch must surge the torso FORWARD; head axis tips z {}",
        head.z
    );

    // Spent: the posture snaps home and removes itself.
    h.tick((HOLY_LAUNCH_FLARE_SECS / DT).ceil() as u32 + 4);
    assert_eq!(
        h.body_rotation(body),
        Quat::IDENTITY,
        "the body returns exactly to identity"
    );
    assert_eq!(h.count::<HealCastPosture>(), 0, "the posture retires itself");
}

// ── teardown ───────────────────────────────────────────────────────────────

/// An interrupted cast stops EVERYTHING dead — no sputter, no fade, no
/// interrupt flourish (faithful: the source fires the end edge and nothing
/// else). Zero surviving entities, body back at identity, within a frame.
#[test]
fn an_interrupted_cast_stops_everything_dead() {
    let mut h = Harness::new();
    let (caster, body) = h.spawn_caster(CharacterClass::Shaman, Vec3::new(0.0, 1.0, 0.0));
    h.spawn_camera(Vec3::new(0.0, 7.0, 20.0), Vec3::new(0.0, 1.0, 0.0));
    h.begin_cast(caster, AbilityType::LesserHealingWave);
    h.tick(45); // glow + wisps + a healthy leaf population, posture leaned
    assert_eq!(h.rigs().len(), 2);
    assert_min("pieces mid-cast", h.pieces().len(), 10);
    assert_min("leaves mid-cast", h.leaves().len(), 8);
    assert_ne!(h.body_rotation(body), Quat::IDENTITY);

    h.end_cast(caster, CastEndingKind::Interrupted);
    h.tick(2);

    assert_eq!(h.rigs().len(), 0, "interrupt: hand rigs stop dead");
    assert_eq!(h.pieces().len(), 0, "interrupt: glow and wisps stop dead");
    assert_eq!(h.leaves().len(), 0, "interrupt: airborne leaves stop dead");
    assert_eq!(h.bursts().len(), 0, "interrupt: no launch, no burst");
    assert_eq!(
        h.body_rotation(body),
        Quat::IDENTITY,
        "interrupt: the body loop stops dead too"
    );
    assert_eq!(h.count::<HealCastPosture>(), 0);
}

/// A fizzle (completion gates failed) is the same stop-dead teardown, and a
/// cast whose state silently vanishes (caster death / match end) is cleaned
/// up by the no-marker path.
#[test]
fn fizzles_and_silent_vanishes_tear_down_clean() {
    let mut h = Harness::new();
    let (caster, _) = h.spawn_caster(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.begin_cast(caster, AbilityType::HolyLight);
    h.tick(20);
    assert_eq!(h.rigs().len(), 2);
    h.end_cast(caster, CastEndingKind::Fizzled);
    h.tick(2);
    assert_eq!(h.rigs().len(), 0, "fizzle: no flare, stop dead");
    assert_eq!(h.pieces().len(), 0);

    // Silent vanish: the CastingState is removed with NO marker.
    let (caster2, _) = h.spawn_caster(CharacterClass::Priest, Vec3::new(4.0, 1.0, 0.0));
    h.begin_cast(caster2, AbilityType::FlashHeal);
    h.tick(20);
    assert_eq!(h.rigs().len(), 2);
    h.app
        .world_mut()
        .entity_mut(caster2)
        .remove::<CastingState>();
    h.tick(2);
    assert_eq!(h.rigs().len(), 0, "state-gone-with-no-marker: cleaned up");
    assert_eq!(h.pieces().len(), 0);
}
