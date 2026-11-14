//! Probes for Sacred Shield's visuals (AS-375): the cast (the instant Holy
//! hand flash and a gold glyph disc at the Paladin's feet), the landing on
//! the ally (a flat gold rune disc at the chest, streaks, two turning runes;
//! `rendering/effects/paladin_oneshots.rs`), and the absorb's gold card, held
//! while the absorb lasts (`rendering/effects/shield.rs`,
//! `ShieldKind::SacredShield`).
//!
//! These assert what is DRAWN, read off the live entities: world geometry
//! through `GlobalTransform`, and each piece's material (colour, blend mode,
//! texture) and mesh. The bench's tables are pinned in the modules' own
//! tests; here the production systems run in the order `states/mod.rs`
//! registers them, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//! (no window, no GPU). The last test plays a real match.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraLanding,
    AuraLandingKind, AuraType, CameraController, ClientParticle, Combatant, HealCastHand,
    HealCastKind, OneShotPieceRole, PaladinOneShot, PaladinOneShotKind, PaladinOneShotPiece,
    ParticleFacing, ShieldCard, ShieldKind, ShieldLayer, ShieldPhase, ShieldVisual, VisualBody,
    SACRED_SHIELD_AURA,
};
use arenasim::states::play_match::triggered_absorbs::{
    fire_triggered_absorbs, AbsorbSource, ShieldFrom, TriggeredAbsorbSlot,
};
use arenasim::states::play_match::{
    animate_client_particles, billboard_hunter_shots, consume_cast_ending_signals, is_live_shield,
    oneshot_glyph_texture, piece_pose, place_shield_cards, shield_tuning, spawn_instant_cast_hands,
    spawn_paladin_oneshots, star_flash_texture, update_auras, update_heal_cast_glows,
    update_paladin_oneshots, update_shield_visuals, GameRng, PWS_CLIENT_TO_RIG,
    PW_SHIELD_AURA_NAME, SACRED_SHIELD_AURA_NAME, SS_CARD_QUAD, SS_RIM_RGB, SS_STREAK_RGB,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// Half the capsule's height: a body's feet sit this far below its centre.
const HALF_HEIGHT: f32 = 1.25;

struct Harness {
    app: App,
}

/// One drawn piece of the absorb's rig: its layer, world pose and material.
struct Drawn {
    layer: ShieldLayer,
    g: Transform,
    material: StandardMaterial,
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
        app.insert_resource(ArenaDampening::default());
        app.insert_resource(CombatLog::default());
        app.insert_resource(GameRng::from_seed(7));
        // The sim's two systems the absorb's life runs through: the trigger
        // raises it, the aura tick runs it out.
        app.add_systems(Update, (update_auras, fire_triggered_absorbs).chain());
        app.add_systems(
            Update,
            (
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                spawn_paladin_oneshots,
                update_paladin_oneshots,
                update_heal_cast_glows,
                animate_client_particles,
                billboard_hunter_shots,
                update_shield_visuals,
            )
                .chain()
                .after(fire_triggered_absorbs),
        );
        app.add_systems(
            PostUpdate,
            place_shield_cards.before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it: the sim entity at its capsule
    /// centre and a `VisualBody` child.
    fn unit(&mut self, class: CharacterClass, at: Vec3) -> Entity {
        let world = self.app.world_mut();
        let unit = world
            .spawn((
                Combatant::new(1, 0, class),
                Transform::from_translation(at),
                ActiveAuras::default(),
            ))
            .id();
        let body = world
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        world.entity_mut(unit).add_child(body);
        unit
    }

    fn camera(&mut self, from: Vec3, at: Vec3) -> Transform {
        let transform = Transform::from_translation(from).looking_at(at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform
    }

    /// The unit's auras (the aura tick removes the component with its last
    /// aura, and `apply_pending_auras` puts it back, as here).
    fn auras(&mut self, unit: Entity) -> Mut<'_, ActiveAuras> {
        let world = self.app.world_mut();
        if world.get::<ActiveAuras>(unit).is_none() {
            world.entity_mut(unit).insert(ActiveAuras::default());
        }
        world.get_mut::<ActiveAuras>(unit).unwrap()
    }

    /// Sacred Shield's absorb, as `fire_triggered_absorbs` raises it.
    fn give_absorb(&mut self, unit: Entity, points: f32) {
        let arming = arming_aura(None);
        let shield =
            sacred_config().shield_aura(SACRED_SHIELD_AURA, ShieldFrom::Aura(&arming), points);
        self.auras(unit).auras.push(shield);
    }

    /// The 30 s buff landing from `caster`, as `apply_pending_auras` lands
    /// it: the aura, its armed slot, and the landing marker.
    fn arm(&mut self, unit: Entity, caster: Option<Entity>) {
        self.auras(unit).auras.push(arming_aura(caster));
        let world = self.app.world_mut();
        let mut c = world.get_mut::<Combatant>(unit).unwrap();
        let taken = c.damage_taken;
        c.triggered_absorbs.push(TriggeredAbsorbSlot::new(
            SACRED_SHIELD_AURA,
            AbsorbSource::Aura,
            sacred_config(),
            taken,
        ));
        world.spawn(AuraLanding {
            target: unit,
            kind: AuraLandingKind::SacredShieldDisc,
            caster,
        });
    }

    /// The bearer's absorbs (none once the aura tick has taken the last aura
    /// and the component with it).
    fn absorbs(&mut self, unit: Entity) -> Vec<Aura> {
        self.app
            .world()
            .get::<ActiveAuras>(unit)
            .map(|a| {
                a.auras
                    .iter()
                    .filter(|a| a.effect_type == AuraType::Absorb)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A one-shot rig's own clock, seconds.
    fn age_of(&mut self, kind: PaladinOneShotKind) -> f32 {
        let mut q = self.app.world_mut().query::<&PaladinOneShot>();
        q.iter(self.app.world())
            .find(|r| r.kind == kind)
            .map(|r| r.age)
            .expect("the one-shot is playing")
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / DT).ceil() as u32);
    }

    fn rigs(&mut self, kind: ShieldKind) -> Vec<ShieldVisual> {
        let mut q = self.app.world_mut().query::<&ShieldVisual>();
        q.iter(self.app.world())
            .filter(|r| r.kind == kind)
            .copied()
            .collect()
    }

    fn rig(&mut self, unit: Entity) -> Option<ShieldVisual> {
        self.rigs(ShieldKind::SacredShield)
            .into_iter()
            .find(|r| r.bearer == unit)
    }

    fn run_to(&mut self, unit: Entity, phase: ShieldPhase, at: f32) {
        for _ in 0..2000 {
            if let Some(r) = self.rig(unit) {
                if r.phase == phase && r.t >= at {
                    return;
                }
            }
            self.tick(1);
        }
        panic!("the card never reached {phase:?} at {at}s");
    }

    /// Every piece of `unit`'s rig of `kind`.
    fn drawn(&mut self, unit: Entity, kind: ShieldKind) -> Vec<Drawn> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == unit && r.kind == kind)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        kids.into_iter()
            .filter_map(|k| {
                Some(Drawn {
                    layer: world.get::<ShieldCard>(k)?.layer,
                    g: world.get::<GlobalTransform>(k)?.compute_transform(),
                    material: materials
                        .get(&world.get::<MeshMaterial3d<StandardMaterial>>(k)?.0)?
                        .clone(),
                })
            })
            .collect()
    }

    fn card(&mut self, unit: Entity, layer: ShieldLayer) -> Drawn {
        self.drawn(unit, ShieldKind::SacredShield)
            .into_iter()
            .find(|d| d.layer == layer)
            .unwrap_or_else(|| panic!("no {layer:?} on the card"))
    }

    fn image(&self, handle: &Handle<Image>) -> Image {
        self.app
            .world()
            .resource::<Assets<Image>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn mesh_vertices(&self, unit_piece: Entity) -> usize {
        let world = self.app.world();
        let mesh = world.get::<Mesh3d>(unit_piece).unwrap();
        world
            .resource::<Assets<Mesh>>()
            .get(&mesh.0)
            .unwrap()
            .count_vertices()
    }

    /// The one-shot pieces playing, as (role, entity, world pose, material).
    fn pieces(&mut self) -> Vec<(OneShotPieceRole, Entity, Transform, StandardMaterial)> {
        let mut q = self.app.world_mut().query::<(
            Entity,
            &PaladinOneShotPiece,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(e, p, g, m)| (p.role, e, g.compute_transform(), m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(r, e, g, m)| (r, e, g, materials.get(&m).unwrap().clone()))
            .collect()
    }

    fn piece(&mut self, role: OneShotPieceRole) -> (Entity, Transform, StandardMaterial) {
        let found: Vec<_> = self
            .pieces()
            .into_iter()
            .filter(|(r, ..)| *r == role)
            .collect();
        assert_eq!(found.len(), 1, "one {role:?}");
        let (_, e, g, m) = found.into_iter().next().unwrap();
        (e, g, m)
    }

    fn oneshot_rigs(&mut self) -> Vec<(PaladinOneShotKind, Entity)> {
        let mut q = self.app.world_mut().query::<&PaladinOneShot>();
        q.iter(self.app.world())
            .map(|r| (r.kind, r.bearer))
            .collect()
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity)> {
        let mut q = self.app.world_mut().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster))
            .collect()
    }
}

fn sacred_config() -> arenasim::states::play_match::triggered_absorbs::TriggeredAbsorbConfig {
    AbilityDefinitions::default()
        .get(&AbilityType::SacredShield)
        .and_then(|d| d.applies_aura.as_ref())
        .and_then(|a| a.triggered_absorb)
        .expect("Sacred Shield arms a triggered absorb")
}

fn arming_aura(caster: Option<Entity>) -> Aura {
    Aura {
        effect_type: AuraType::TriggeredAbsorb,
        duration: 30.0,
        ability_name: SACRED_SHIELD_AURA.to_string(),
        caster,
        ..Default::default()
    }
}

fn srgb_u8(c: Color) -> [u8; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

fn srgb(c: Color) -> [f32; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue]
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// The bench's linear keys (`keys`).
fn bench_keys(track: &[(f32, f32)], t: f32) -> f32 {
    if t <= track[0].0 {
        return track[0].1;
    }
    for pair in track.windows(2) {
        let ((t0, a), (t1, b)) = (pair[0], pair[1]);
        if t <= t1 {
            return a + (b - a) * (t - t0) / (t1 - t0);
        }
    }
    track[track.len() - 1].1
}

/// The card's weight as the signed-off bench draws it (its `customDraw`,
/// written out here, not read from the module): `ms` into `phase`.
fn bench_card(phase: ShieldPhase, ms: f32) -> f32 {
    match phase {
        ShieldPhase::Birth => bench_keys(
            &[
                (0.0, 0.26),
                (200.0, 0.26),
                (300.0, 1.0),
                (400.0, 0.38),
                (500.0, 0.26),
                (600.0, 0.38),
                (700.0, 0.26),
            ],
            ms,
        ),
        ShieldPhase::Loop => bench_keys(
            &[
                (0.0, 0.18),
                (100.0, 0.49),
                (200.0, 0.26),
                (300.0, 0.45),
                (400.0, 0.16),
                (533.0, 0.46),
                (700.0, 0.38),
                (1000.0, 0.3),
                (1466.0, 0.18),
            ],
            ms % 1466.0,
        ),
        ShieldPhase::Death => {
            bench_keys(
                &[
                    (0.0, 0.44),
                    (66.0, 0.1),
                    (166.0, 0.45),
                    (800.0, 0.0),
                    (1100.0, 0.0),
                ],
                ms,
            ) * (1.0 + 0.3 * ms / 1100.0)
        }
    }
}

/// A quad's normal (its local +Z) lies along the camera's own.
fn faces_camera(g: &Transform, camera: &Transform) -> bool {
    (g.rotation * Vec3::Z).dot(camera.rotation * Vec3::Z) > 0.999
}

/// A quad's normal points straight up.
fn lies_flat(g: &Transform) -> bool {
    (g.rotation * Vec3::Z).dot(Vec3::Y) > 0.999
}

/// Where `p` lands on a 1280 x 720 viewport through `camera` and Bevy's
/// default perspective (45° field of view), in pixels.
fn project(camera: &Transform, p: Vec3) -> Vec2 {
    let size = Vec2::new(1280.0, 720.0);
    let projection = PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..default()
    };
    let clip = projection.get_clip_from_view() * camera.compute_matrix().inverse() * p.extend(1.0);
    assert!(clip.w > 0.0, "in front of the camera");
    let ndc = clip.truncate() / clip.w;
    Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
}

/// The match camera (`CameraController`'s placement) at its default zoom
/// and pitch, at `yaw`, looking at `target`.
fn match_camera(target: Vec3, yaw: f32) -> Transform {
    let c = CameraController::default();
    let at = target
        + Vec3::new(
            c.zoom_distance * c.pitch.sin() * yaw.sin(),
            c.zoom_distance * c.pitch.cos(),
            c.zoom_distance * c.pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

// ── routing ─────────────────────────────────────────────────────────────────

/// One name keys all of it: the RON `name`, the arming buff's landing, and
/// the absorb the buff raises (which wears the card; the buff does not).
#[test]
fn one_name_routes_the_cast_the_landing_and_the_card() {
    let defs = AbilityDefinitions::default();
    let def = defs.get(&AbilityType::SacredShield).expect("in the config");
    assert_eq!(def.name, SACRED_SHIELD_AURA);
    assert_eq!(SACRED_SHIELD_AURA_NAME, SACRED_SHIELD_AURA);
    let t = def.applies_aura.as_ref().unwrap().aura_type;
    assert_eq!(t, AuraType::TriggeredAbsorb);
    assert_eq!(
        AuraLandingKind::for_aura(t, &def.name),
        Some(AuraLandingKind::SacredShieldDisc)
    );
    assert_eq!(
        AuraApplyRoute::for_instance(t, &def.name),
        AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding),
        "the landing owns the buff's application, so the band does not double it"
    );
    assert_eq!(
        AuraApplyRoute::for_instance(t, "Another Triggered Absorb"),
        AuraApplyRoute::Band
    );
    assert_eq!(
        PaladinOneShotKind::for_landing(AuraLandingKind::SacredShieldDisc),
        Some(PaladinOneShotKind::SacredShield)
    );
    assert_eq!(
        AuraLandingKind::SacredShieldDisc.cast_ability(),
        Some(AbilityType::SacredShield)
    );
    assert_eq!(
        HealCastKind::for_ability(AbilityType::SacredShield),
        Some(HealCastKind::HolyFlash)
    );
    assert!(AuraLandingKind::flashes_hands_for(
        AbilityType::SacredShield
    ));

    // The absorb wears the card; the buff that arms it does not.
    let arming = arming_aura(None);
    let shield = sacred_config().shield_aura(&def.name, ShieldFrom::Aura(&arming), 40.0);
    let wears = |a: &Aura| -> Vec<ShieldKind> {
        ShieldKind::ALL
            .into_iter()
            .filter(|k| is_live_shield(*k, a))
            .collect()
    };
    assert_eq!(wears(&shield), vec![ShieldKind::SacredShield]);
    assert_eq!(wears(&arming), vec![], "the 30 s buff has no state");
    // Never, whatever its (unread) magnitude: the card keys on the absorb.
    let weighty = Aura {
        magnitude: 40.0,
        ..arming
    };
    assert_eq!(wears(&weighty), vec![], "the buff wears the absorb's card");
    let spent = Aura {
        magnitude: 0.0,
        ..shield
    };
    assert_eq!(wears(&spent), vec![], "a spent absorb wears nothing");
}

// ── the cast and the landing ────────────────────────────────────────────────

/// The cast: both of the Paladin's hands flash the instant Holy flash, and a
/// flat gold glyph disc with six ticks lies at its feet, 1.6 yd across, in
/// by 100 ms and out by 700 ms.
#[test]
fn the_cast_flashes_the_hands_and_lays_a_gold_disc_at_the_paladins_feet() {
    let mut h = Harness::new();
    let at = Vec3::new(-3.0, 1.0, 2.0);
    let paladin = h.unit(CharacterClass::Paladin, at);
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(5.0, 1.0, -1.0));
    h.camera(Vec3::new(0.0, 20.0, 20.0), Vec3::ZERO);
    h.arm(rogue, Some(paladin));
    h.tick(2);
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
    let mut rigs = h.oneshot_rigs();
    rigs.sort_by_key(|(k, _)| format!("{k:?}"));
    assert_eq!(
        rigs,
        vec![
            (PaladinOneShotKind::SacredShield, rogue),
            (PaladinOneShotKind::SacredShieldBase, paladin)
        ]
    );

    h.tick_secs(0.1 - 2.0 * DT);
    let (piece, base, m) = h.piece(OneShotPieceRole::SacredBase);
    assert!(lies_flat(&base), "the base disc lies flat");
    assert!((base.translation - Vec3::new(at.x, 0.04, at.z)).length() < 1e-4);
    assert!((base.scale.x - 1.6).abs() < 1e-4 && (base.scale.y - 1.6).abs() < 1e-4);
    assert_eq!(srgb_u8(m.base_color), [253, 218, 112], "gold");
    // On the bench's track, on its own clock: about 0.8 at 100 ms.
    let age = h.age_of(PaladinOneShotKind::SacredShieldBase);
    let want = piece_pose(OneShotPieceRole::SacredBase, age).alpha;
    assert!((m.base_color.alpha() - want).abs() < 1e-5, "{age}");
    assert!(want > 0.7, "near its 0.8 peak at {age} s: {want}");
    assert_eq!(m.alpha_mode, AlphaMode::Add);
    assert!(m.unlit);
    assert_eq!(
        h.image(m.base_color_texture.as_ref().unwrap()).data,
        oneshot_glyph_texture(6).data,
        "the six-tick glyph"
    );
    assert_eq!(h.mesh_vertices(piece), 4, "a quad");

    h.tick_secs(0.62);
    assert!(
        !h.oneshot_rigs()
            .contains(&(PaladinOneShotKind::SacredShieldBase, paladin)),
        "the base disc is gone by 0.7 s"
    );
    assert!(h.hands().is_empty(), "the flash is 0.7 s");
}

/// The landing on the ally: a flat gold rune disc at its chest (ten ticks,
/// 1.6 yd, turning), two gold rune stars above and below it that face the
/// camera from every bearing and turn opposite ways, and streaks flying out
/// within 1.5 rad of up; all gone by 1.97 s.
#[test]
fn the_ally_wears_the_gold_rune_disc_at_its_chest() {
    for bearing in [0.0_f32, FRAC_PI_2, 2.4] {
        let mut h = Harness::new();
        let paladin = h.unit(CharacterClass::Paladin, Vec3::new(-6.0, 1.0, 0.0));
        let at = Vec3::new(2.0, 1.0, -3.0);
        let rogue = h.unit(CharacterClass::Rogue, at);
        let eye = at + Vec3::new(14.0 * bearing.sin(), 8.0, 14.0 * bearing.cos());
        let cam = h.camera(eye, at);
        h.arm(rogue, Some(paladin));
        h.tick_secs(0.4);

        let (piece, disc, m) = h.piece(OneShotPieceRole::SacredDisc);
        assert!(lies_flat(&disc), "the disc lies flat");
        assert!((disc.translation - Vec3::new(at.x, 1.45, at.z)).length() < 1e-4);
        assert!((disc.scale.x - 1.6).abs() < 1e-4 && (disc.scale.y - 1.6).abs() < 1e-4);
        assert_eq!(srgb_u8(m.base_color), [243, 211, 82]);
        let age = h.age_of(PaladinOneShotKind::SacredShield);
        let want = |role| piece_pose(role, age).alpha;
        assert!((m.base_color.alpha() - want(OneShotPieceRole::SacredDisc)).abs() < 1e-5);
        assert!(m.base_color.alpha() > 0.4, "lit at {age} s");
        assert_eq!(m.alpha_mode, AlphaMode::Add);
        assert_eq!(
            h.image(m.base_color_texture.as_ref().unwrap()).data,
            oneshot_glyph_texture(10).data,
            "the ten-tick glyph"
        );
        assert_eq!(h.mesh_vertices(piece), 4);

        let (_, up, mu) = h.piece(OneShotPieceRole::SacredRune { index: 0 });
        let (_, down, md) = h.piece(OneShotPieceRole::SacredRune { index: 1 });
        for (index, rune, m, y) in [(0, &up, &mu, 1.70), (1, &down, &md, 1.20)] {
            assert!(
                faces_camera(rune, &cam),
                "bearing {bearing}: a rune turned away"
            );
            assert!((rune.translation - Vec3::new(at.x, y, at.z)).length() < 1e-4);
            assert!((rune.scale.x - 0.5).abs() < 1e-4);
            assert_eq!(srgb_u8(m.base_color), [253, 218, 112]);
            let w = want(OneShotPieceRole::SacredRune { index });
            assert!((m.base_color.alpha() - w).abs() < 1e-5);
            assert!(w > 0.7, "near its 0.78 peak at {age} s");
            assert_eq!(m.alpha_mode, AlphaMode::Add);
            assert_eq!(
                h.image(m.base_color_texture.as_ref().unwrap()).data,
                star_flash_texture().data
            );
        }
        // Counter-turning: their up axes lean opposite ways in the view.
        let right = cam.rotation * Vec3::X;
        let lean = |r: &Transform| (r.rotation * Vec3::Y).dot(right);
        assert!(lean(&up) * lean(&down) < 0.0, "the runes turn the same way");

        // The streaks: Streak-facing particles, born at the chest, flying out
        // at 3.3 yd/s within 1.5 rad of up.
        let streaks: Vec<(Vec3, Vec3)> = {
            let mut q = h.app.world_mut().query::<(&ClientParticle, &Transform)>();
            q.iter(h.app.world())
                .filter(|(p, _)| p.facing == ParticleFacing::Streak)
                .map(|(p, t)| (t.translation, p.velocity))
                .collect()
        };
        assert!(streaks.len() >= 8, "{} streaks", streaks.len());
        for (p, v) in &streaks {
            assert!((v.length() - 3.3).abs() < 1e-3, "{v}");
            assert!(
                v.normalize().y >= 1.5_f32.cos() - 1e-4,
                "{v} below the cone"
            );
            assert!((p.xz() - at.xz()).length() < 3.3 * 0.5 + 0.1, "{p}");
        }

        h.tick_secs(1.6);
        assert!(h.oneshot_rigs().is_empty(), "everything retires by 1.97 s");
        // The buff draws nothing after: no rig of any shield on the ally.
        assert!(h.rigs(ShieldKind::SacredShield).is_empty());
    }
}

// ── the absorb's card ───────────────────────────────────────────────────────

/// The card at client scale on our rig: 3.2 x 4.09 client units, from 0.84
/// below the feet to 3.25 above, the two client layers on it in their own
/// golds (`genericglow2b` and `shockwave10d`, PW:S's own textures, added
/// light), and the blended gold outline; standing on the bearer's feet,
/// upright, turned to the camera at every bearing and pitch.
#[test]
fn the_absorb_wears_a_tall_gold_card_at_the_feet() {
    let r = PWS_CLIENT_TO_RIG;
    for (bearing, pitch) in [(0.0_f32, 0.4_f32), (1.3, 0.9), (-2.6, 1.2), (3.0, 0.2)] {
        let mut h = Harness::new();
        let at = Vec3::new(4.0, 1.0, -2.0);
        let rogue = h.unit(CharacterClass::Rogue, at);
        let priest = h.unit(CharacterClass::Priest, Vec3::new(-8.0, 1.0, 0.0));
        let eye = at
            + 30.0
                * Vec3::new(
                    pitch.cos() * bearing.sin(),
                    pitch.sin(),
                    pitch.cos() * bearing.cos(),
                );
        let cam = h.camera(eye, at);
        h.give_absorb(rogue, 40.0);
        // A Priest's shield, to read PW:S's own textures off.
        h.auras(priest).auras.push(Aura {
            effect_type: AuraType::Absorb,
            duration: 30.0,
            magnitude: 70.0,
            ability_name: PW_SHIELD_AURA_NAME.to_string(),
            ..Default::default()
        });
        h.run_to(rogue, ShieldPhase::Loop, 0.2);

        let drawn = h.drawn(rogue, ShieldKind::SacredShield);
        let layers: Vec<ShieldLayer> = drawn.iter().map(|d| d.layer).collect();
        let mut want = vec![ShieldLayer::Outline, ShieldLayer::Rim, ShieldLayer::StreakA];
        want.extend((0..8).map(ShieldLayer::Flare));
        want.extend((0..6).map(ShieldLayer::Spark));
        assert_eq!(ShieldKind::SacredShield.layers(), want.as_slice());
        let (mut got, mut want) = (layers.clone(), want);
        got.sort();
        want.sort();
        assert_eq!(got, want, "the card's pieces");

        let feet = at - Vec3::Y * HALF_HEIGHT;
        let pws = h.drawn(priest, ShieldKind::PowerWordShield);
        let pws_texture = |layer| {
            pws.iter()
                .find(|d| d.layer == layer)
                .and_then(|d| d.material.base_color_texture.clone())
                .unwrap()
        };
        for layer in [ShieldLayer::Outline, ShieldLayer::Rim, ShieldLayer::StreakA] {
            let card = drawn.iter().find(|d| d.layer == layer).unwrap();
            // At the client's size, on the client's span above the feet.
            let half = card.g.scale.truncate();
            assert!(
                (half.x - 1.6 * r).abs() < 1e-4,
                "{layer:?} width {}",
                2.0 * half.x
            );
            assert!(
                (half.y - 2.045 * r).abs() < 1e-4,
                "{layer:?} height {}",
                2.0 * half.y
            );
            let (bottom, top) = (card.g.translation.y - half.y, card.g.translation.y + half.y);
            assert!(
                (bottom - (feet.y - 0.84 * r)).abs() < 1e-3,
                "{layer:?} bottom {bottom}"
            );
            assert!(
                (top - (feet.y + 3.25 * r)).abs() < 1e-3,
                "{layer:?} top {top}"
            );
            assert!(
                (card.g.translation.xz() - at.xz()).length() < 1e-4,
                "on the bearer"
            );
            // Upright, its face turned to the camera about world up.
            assert!(
                (card.g.rotation * Vec3::Y).dot(Vec3::Y) > 0.9999,
                "{layer:?} tipped"
            );
            let normal = (card.g.rotation * Vec3::Z).xz().normalize();
            let to_cam = (eye - card.g.translation).xz().normalize();
            assert!(
                normal.dot(to_cam) > 0.9999,
                "{layer:?} not turned to the camera"
            );
            assert!(card.material.unlit);
        }
        let rim = drawn.iter().find(|d| d.layer == ShieldLayer::Rim).unwrap();
        let streak = drawn
            .iter()
            .find(|d| d.layer == ShieldLayer::StreakA)
            .unwrap();
        let outline = drawn
            .iter()
            .find(|d| d.layer == ShieldLayer::Outline)
            .unwrap();
        assert!(
            close(srgb(rim.material.base_color), SS_RIM_RGB),
            "genericglow2b gold"
        );
        assert!(
            close(srgb(streak.material.base_color), SS_STREAK_RGB),
            "shockwave10d gold"
        );
        assert_eq!(SS_RIM_RGB, [1.0, 0.91, 0.32]);
        assert_eq!(SS_STREAK_RGB, [0.97, 0.99, 0.32]);
        assert_eq!(rim.material.alpha_mode, AlphaMode::Add);
        assert_eq!(streak.material.alpha_mode, AlphaMode::Add);
        assert_eq!(outline.material.alpha_mode, AlphaMode::Blend);
        assert_eq!(
            rim.material.base_color_texture,
            Some(pws_texture(ShieldLayer::Rim)),
            "PW:S's genericglow2b"
        );
        assert_eq!(
            streak.material.base_color_texture,
            Some(pws_texture(ShieldLayer::StreakA)),
            "PW:S's shockwave10d"
        );
        // The outline: the rim gold at PW:S's level, in linear light.
        let o = shield_tuning(ShieldKind::SacredShield).outline;
        let pws_o = shield_tuning(ShieldKind::PowerWordShield).outline;
        assert_eq!(o.rgb, SS_RIM_RGB);
        assert_eq!(
            (o.alpha, o.width, o.glow_width, o.level, o.pulse),
            (
                pws_o.alpha,
                pws_o.width,
                pws_o.glow_width,
                pws_o.level,
                pws_o.pulse
            )
        );
        let want = Color::srgb(o.rgb[0], o.rgb[1], o.rgb[2]).to_linear() * o.level;
        let got = outline.material.base_color.to_linear();
        assert!(
            (got.red - want.red).abs() < 1e-5
                && (got.green - want.green).abs() < 1e-5
                && (got.blue - want.blue).abs() < 1e-5,
            "outline {got:?}"
        );

        // The flares are sprites that face the camera outright.
        for d in drawn
            .iter()
            .filter(|d| matches!(d.layer, ShieldLayer::Flare(_)))
        {
            assert!(faces_camera(&d.g, &cam), "{:?} turned away", d.layer);
            assert_eq!(d.material.alpha_mode, AlphaMode::Add);
            assert_eq!(
                h.image(d.material.base_color_texture.as_ref().unwrap())
                    .data,
                star_flash_texture().data,
                "{:?} is a star",
                d.layer
            );
        }
    }
}

/// The card's life: no pop and no wobble (it stands at its size from the
/// first frame to the death), bright at once (0.26 at birth, 1.0 at the
/// 300 ms spike), then flickering between 0.16 and 0.49 through its 1.466 s
/// loop, reaching both, for as long as the absorb holds — its whole 6 s here
/// (0.7 s of birth, then 5 s of loop).
#[test]
fn the_card_flickers_bright_for_as_long_as_the_absorb_holds() {
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    h.tick(1);
    assert_eq!(h.rig(rogue).unwrap().phase, ShieldPhase::Birth);
    h.tick(1);
    let first = h.card(rogue, ShieldLayer::Rim);
    assert!(
        (first.material.base_color.alpha() - 0.26).abs() < 1e-3,
        "lit at once: {}",
        first.material.base_color.alpha()
    );
    let size = first.g.scale;
    h.run_to(rogue, ShieldPhase::Birth, 0.296);
    let spike = h
        .card(rogue, ShieldLayer::StreakA)
        .material
        .base_color
        .alpha();
    assert!(spike > 0.95, "the 300 ms spike: {spike}");
    assert_eq!(h.card(rogue, ShieldLayer::Rim).g.scale, size, "no pop");

    // Every birth frame on the bench's track.
    h.app.update();
    let mut checked = 0;
    while h.rig(rogue).unwrap().phase == ShieldPhase::Birth {
        let t = h.rig(rogue).unwrap().t;
        let w = h.card(rogue, ShieldLayer::Rim).material.base_color.alpha();
        assert!(
            (w - bench_card(ShieldPhase::Birth, t * 1000.0)).abs() < 1e-4,
            "birth {t}: {w}"
        );
        checked += 1;
        h.tick(1);
    }
    assert!(checked > 20, "{checked} birth frames");

    h.run_to(rogue, ShieldPhase::Loop, 0.0);
    let (mut lo, mut hi) = (f32::MAX, 0.0_f32);
    let frames = (5.0 / DT) as u32;
    for _ in 0..frames {
        h.tick(1);
        let rig = h.rig(rogue).unwrap();
        assert_eq!(rig.phase, ShieldPhase::Loop, "held");
        let rim = h.card(rogue, ShieldLayer::Rim);
        let streak = h.card(rogue, ShieldLayer::StreakA);
        let w = rim.material.base_color.alpha();
        assert_eq!(w, streak.material.base_color.alpha(), "one track");
        // On the bench's 1.466 s loop, frame for frame, three times round.
        let want = bench_card(ShieldPhase::Loop, rig.t * 1000.0);
        assert!((w - want).abs() < 1e-4, "loop {}: {w} vs {want}", rig.t);
        assert!((0.16 - 1e-4..=0.49 + 1e-4).contains(&w), "{w}");
        assert_eq!(rim.g.scale, size, "no wobble");
        lo = lo.min(w);
        hi = hi.max(w);
    }
    assert!(lo < 0.17 && hi > 0.48, "the flicker spans {lo}..{hi}");
}

/// The flares: eight stars over the card from 300 ms into its birth, rising
/// as they cycle, white -> yellow -> white, inside their box over the card's
/// centre; none in the death.
#[test]
fn eight_flares_rise_over_the_card_while_it_holds() {
    let r = PWS_CLIENT_TO_RIG;
    let mut h = Harness::new();
    let at = Vec3::new(1.0, 1.0, 1.0);
    let rogue = h.unit(CharacterClass::Rogue, at);
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Birth, 0.2);
    let lit = |h: &mut Harness| -> Vec<Drawn> {
        h.drawn(rogue, ShieldKind::SacredShield)
            .into_iter()
            .filter(|d| matches!(d.layer, ShieldLayer::Flare(_)))
            .filter(|d| d.material.base_color.alpha() > 0.0)
            .collect()
    };
    assert!(lit(&mut h).is_empty(), "flares before 300 ms");

    let centre =
        at - Vec3::Y * HALF_HEIGHT + Vec3::Y * (SS_CARD_QUAD[2] + SS_CARD_QUAD[3]) / 2.0 * r;
    h.run_to(rogue, ShieldPhase::Loop, 0.5);
    let mut seen_yellow = false;
    let mut rises = 0;
    let mut before: Option<Vec<(ShieldLayer, f32)>> = None;
    for _ in 0..30 {
        let flares = lit(&mut h);
        assert!(flares.len() >= 7, "{} of 8 flares lit", flares.len());
        for d in &flares {
            let off = d.g.translation - centre;
            assert!(off.x.abs() <= 0.7 * r + 1e-3, "{:?} x {off}", d.layer);
            assert!(off.z.abs() <= 0.2 * r + 1e-3, "{:?} z {off}", d.layer);
            assert!(
                off.y >= -0.8 * r - 1e-3 && off.y <= (0.8 + 0.3) * r + 1e-3,
                "{off}"
            );
            assert!((d.g.scale.x * 2.0 - 0.3 * r).abs() < 1e-4, "0.3 across");
            assert!(d.material.base_color.alpha() <= 0.61 + 1e-4);
            let c = srgb_u8(d.material.base_color);
            // White fading toward (241, 234, 44) and back: red and green stay
            // high, blue carries the yellow.
            assert!(c[0] >= 240 && c[1] >= 233, "{c:?}");
            seen_yellow |= c[2] < 80;
        }
        let now: Vec<(ShieldLayer, f32)> = flares
            .iter()
            .map(|d| (d.layer, d.g.translation.y))
            .collect();
        if let Some(prev) = &before {
            rises += now
                .iter()
                .filter(|(l, y)| prev.iter().any(|(pl, py)| pl == l && y > py))
                .count();
        }
        before = Some(now);
        h.tick(1);
    }
    assert!(seen_yellow, "no flare reached its yellow");
    assert!(rises > 100, "the flares do not rise: {rises}");

    h.auras(rogue).auras.clear();
    h.tick(2);
    // None through the whole death.
    let mut frames = 0;
    while h.rig(rogue).is_some() {
        assert_eq!(h.rig(rogue).unwrap().phase, ShieldPhase::Death);
        assert!(lit(&mut h).is_empty(), "flares in the death");
        h.tick(1);
        frames += 1;
    }
    assert!(frames > 60, "{frames} frames of death");
}

/// The death: the card widens by a quarter over 800 ms with its height
/// held, flickers and fades out, and six gold sparks fly out in a ring from
/// its centre for 330 ms; the rig is gone after 1.1 s.
#[test]
fn the_death_swells_the_card_and_throws_six_gold_sparks() {
    let r = PWS_CLIENT_TO_RIG;
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, 1.0, 3.0);
    let rogue = h.unit(CharacterClass::Rogue, at);
    let cam = h.camera(Vec3::new(5.0, 25.0, 30.0), at);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Loop, 0.3);
    let held = h.card(rogue, ShieldLayer::Rim).g.scale;
    let sparks = |h: &mut Harness| -> Vec<Drawn> {
        h.drawn(rogue, ShieldKind::SacredShield)
            .into_iter()
            .filter(|d| matches!(d.layer, ShieldLayer::Spark(_)))
            .filter(|d| d.material.base_color.alpha() > 0.0)
            .collect()
    };
    assert!(sparks(&mut h).is_empty(), "sparks before the death");

    h.auras(rogue).auras.clear();
    h.tick(1);
    assert_eq!(h.rig(rogue).unwrap().phase, ShieldPhase::Death);
    // The death's weight on the bench's track, raised as it goes (its first
    // 160 ms: the dip at 66 ms and the flash at 166 ms).
    for _ in 0..10 {
        let t = h.rig(rogue).unwrap().t;
        let w = h
            .card(rogue, ShieldLayer::StreakA)
            .material
            .base_color
            .alpha();
        let want = bench_card(ShieldPhase::Death, t * 1000.0);
        assert!((w - want).abs() < 1e-4, "death {t}: {w} vs {want}");
        h.tick(1);
    }
    let centre = at - Vec3::Y * HALF_HEIGHT
        + Vec3::Y * ((SS_CARD_QUAD[2] + SS_CARD_QUAD[3]) / 2.0 + 0.15) * r;
    h.run_to(rogue, ShieldPhase::Death, 0.2);
    let ring = sparks(&mut h);
    assert_eq!(ring.len(), 6, "six sparks");
    let t = h.rig(rogue).unwrap().t;
    let mut angles: Vec<f32> = Vec::new();
    for d in &ring {
        let off = d.g.translation - centre;
        assert!(
            off.y.abs() < 1e-3,
            "{:?} off the ring's plane: {off}",
            d.layer
        );
        let want = 1.667 * 1.2 * t * r;
        assert!(
            (off.xz().length() - want).abs() < 1e-3,
            "{} vs {want}",
            off.xz().length()
        );
        assert_eq!(srgb_u8(d.material.base_color), [240, 191, 59], "gold");
        assert!(faces_camera(&d.g, &cam));
        assert!((d.g.scale.x * 2.0 - 0.25 * r).abs() < 1e-4);
        angles.push(off.z.atan2(off.x).rem_euclid(TAU));
    }
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for pair in angles.windows(2) {
        assert!(
            (pair[1] - pair[0] - TAU / 6.0).abs() < 1e-3,
            "evenly round: {angles:?}"
        );
    }

    h.run_to(rogue, ShieldPhase::Death, 0.34);
    assert!(sparks(&mut h).is_empty(), "the sparks outlive 330 ms");
    h.run_to(rogue, ShieldPhase::Death, 0.8);
    let rim = h.card(rogue, ShieldLayer::Rim);
    assert!(
        (rim.g.scale.x / held.x - 1.25).abs() < 0.01,
        "widened a quarter"
    );
    assert!((rim.g.scale.y - held.y).abs() < 1e-5, "its height held");
    assert!(
        rim.material.base_color.alpha() < 0.01,
        "faded out by 800 ms"
    );
    h.tick_secs(0.32);
    assert!(h.rig(rogue).is_none(), "the rig outlived its 1.1 s death");
}

/// What the death reads as, on one bearer: the frames from the card's last
/// held frame until its rig is gone.
fn ending(h: &mut Harness, unit: Entity) -> usize {
    let mut frames = 0;
    while h.rig(unit).is_some() {
        assert_eq!(h.rig(unit).unwrap().phase, ShieldPhase::Death);
        h.tick(1);
        frames += 1;
        assert!(frames < 200, "the death never ended");
    }
    frames
}

/// Every way the absorb ends plays the one 1.1 s death: used up (its points
/// spent, and the funnel's sweep of a spent absorb), purged, run out (the
/// sim's own aura tick, at its 6 s), and its bearer dying (the aura stays on
/// the corpse). While the absorb holds, the 30 s buff going does not end it.
#[test]
fn every_ending_of_the_absorb_plays_the_death() {
    let death = |frames: usize| (68..=71).contains(&frames);

    // Used up: the points go to zero, then the funnel sweeps the aura.
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Loop, 0.4);
    h.auras(rogue).auras[0].magnitude = 0.0;
    h.tick(1);
    let frames = ending(&mut h, rogue);
    assert!(death(frames), "spent: {frames} frames");
    assert!(h.rigs(ShieldKind::SacredShield).is_empty());

    // Purged: the aura is taken off the bearer.
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Loop, 0.4);
    h.auras(rogue)
        .auras
        .retain(|a| a.effect_type != AuraType::Absorb);
    h.tick(1);
    assert!(death(ending(&mut h, rogue)), "purged");

    // Run out: the sim's own aura tick takes it at its 6 s, and the card is
    // held to that frame.
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    let mut held = 0;
    while !h.absorbs(rogue).is_empty() {
        h.tick(1);
        if let Some(rig) = h.rig(rogue) {
            if rig.phase != ShieldPhase::Death {
                held += 1;
            }
        }
        assert!(held < 1000, "the absorb never ran out");
    }
    assert!(
        (held as f32 * DT - 6.0).abs() < 0.05,
        "the card held {:.2} s of a 6 s absorb",
        held as f32 * DT
    );
    h.tick(1);
    assert!(death(ending(&mut h, rogue) + 1), "run out");

    // The bearer dies: its auras stay on the corpse, its card goes.
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Loop, 0.4);
    h.app
        .world_mut()
        .get_mut::<Combatant>(rogue)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(death(ending(&mut h, rogue)), "the bearer died");
    assert!(
        !h.absorbs(rogue).is_empty(),
        "the aura is still on the corpse"
    );

    // The buff goes while the absorb holds (it moved to another ally): the
    // card is the absorb's, so it stays.
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.arm(rogue, None);
    h.give_absorb(rogue, 40.0);
    h.run_to(rogue, ShieldPhase::Loop, 0.2);
    h.auras(rogue)
        .auras
        .retain(|a| a.effect_type != AuraType::TriggeredAbsorb);
    h.tick(60);
    assert_eq!(
        h.rig(rogue).unwrap().phase,
        ShieldPhase::Loop,
        "the card went with the buff"
    );
}

/// The trigger to the card, through the sim's own `fire_triggered_absorbs`:
/// the armed buff alone draws no card; the first hit raises the absorb and
/// the card is born; a second absorb raised over a live one (6 s on) keeps
/// the one card, unbroken.
#[test]
fn the_card_is_born_when_a_hit_raises_the_absorb() {
    let mut h = Harness::new();
    let rogue = h.unit(CharacterClass::Rogue, Vec3::new(0.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 30.0, 25.0), Vec3::ZERO);
    h.arm(rogue, None);
    h.tick(30);
    assert!(h.rig(rogue).is_none(), "the 30 s buff wears a card");
    assert!(h.absorbs(rogue).is_empty());

    let hit = |h: &mut Harness| {
        h.app
            .world_mut()
            .get_mut::<Combatant>(rogue)
            .unwrap()
            .damage_taken += 25.0;
    };
    hit(&mut h);
    h.tick(1);
    assert_eq!(h.absorbs(rogue).len(), 1, "the hit raised the absorb");
    let rig = h.rig(rogue).expect("no card on the raised absorb");
    assert_eq!(rig.phase, ShieldPhase::Birth);

    // The absorb runs 6 s and the slot rearms at 6 s: hit again on the frame
    // it rearms, and the new absorb replaces the old one with no gap.
    let mut phases = Vec::new();
    for _ in 0..((6.0 / DT) as u32 + 20) {
        hit(&mut h);
        h.tick(1);
        phases.extend(h.rigs(ShieldKind::SacredShield).iter().map(|r| r.phase));
    }
    assert!(
        !phases.contains(&ShieldPhase::Death),
        "a replaced absorb broke the card"
    );
    assert_eq!(h.rigs(ShieldKind::SacredShield).len(), 1);
}

/// Through the default match camera (60 yd, pitch 38.7°, 45° field of view,
/// 1280 x 720): the card stands over its bearer's feet on screen at every
/// yaw, its visible gold ring (the rim texture's band, about half the card)
/// spanning a body's height and more; the disc and runes sit on the body; the
/// flares and the death's spark ring stay within the card. Sized for the
/// match, not the bench's close view.
#[test]
fn everything_reads_at_the_match_camera() {
    let r = PWS_CLIENT_TO_RIG;
    let at = Vec3::new(3.0, 1.0, -4.0);
    for step in 0..8 {
        let yaw = step as f32 * TAU / 8.0;
        let cam = match_camera(at, yaw);
        let feet = at - Vec3::Y * HALF_HEIGHT;
        let head = at + Vec3::Y * HALF_HEIGHT;
        let body_px = (project(&cam, feet) - project(&cam, head)).length();
        assert!(
            body_px > 20.0,
            "a body is {body_px} px tall at the match camera"
        );

        let mut h = Harness::new();
        let rogue = h.unit(CharacterClass::Rogue, at);
        h.app.world_mut().spawn((Camera3d::default(), cam));
        h.give_absorb(rogue, 40.0);
        h.run_to(rogue, ShieldPhase::Loop, 0.6);
        let rim = h.card(rogue, ShieldLayer::Rim);
        // The ring's band sits at about half the card's half-size.
        let up = rim.g.rotation * Vec3::Y;
        let side = rim.g.rotation * Vec3::X;
        let ring_top = rim.g.translation + up * rim.g.scale.y * 0.5;
        let ring_bottom = rim.g.translation - up * rim.g.scale.y * 0.5;
        let ring_left = rim.g.translation - side * rim.g.scale.x * 0.5;
        let ring_right = rim.g.translation + side * rim.g.scale.x * 0.5;
        let tall = (project(&cam, ring_top) - project(&cam, ring_bottom)).length();
        let wide = (project(&cam, ring_left) - project(&cam, ring_right)).length();
        assert!(
            tall > 0.9 * body_px,
            "yaw {yaw}: the ring is {tall} px against a {body_px} px body"
        );
        assert!(
            wide > 0.5 * body_px,
            "yaw {yaw}: the ring is {wide} px wide"
        );
        // Centred on the bearer.
        let on_screen = (project(&cam, rim.g.translation) - project(&cam, at)).length();
        assert!(
            on_screen < 0.35 * body_px,
            "yaw {yaw}: {on_screen} px off the body"
        );

        // The card's box on screen, padded a little: every lit flare in it.
        let corners = [
            rim.g.translation + up * rim.g.scale.y + side * rim.g.scale.x,
            rim.g.translation + up * rim.g.scale.y - side * rim.g.scale.x,
            rim.g.translation - up * rim.g.scale.y + side * rim.g.scale.x,
            rim.g.translation - up * rim.g.scale.y - side * rim.g.scale.x,
        ]
        .map(|p| project(&cam, p));
        let (lo, hi) = corners.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
        let mut flares = 0;
        for d in h.drawn(rogue, ShieldKind::SacredShield) {
            if matches!(d.layer, ShieldLayer::Flare(_)) && d.material.base_color.alpha() > 0.0 {
                let p = project(&cam, d.g.translation);
                assert!(
                    p.cmpge(lo).all() && p.cmple(hi).all(),
                    "a flare off the card"
                );
                let px = (project(&cam, d.g.translation + side * d.g.scale.x)
                    - project(&cam, d.g.translation - side * d.g.scale.x))
                .length();
                assert!(px > 3.0, "yaw {yaw}: a flare is {px} px across");
                flares += 1;
            }
        }
        assert!(flares >= 7, "{flares} flares");

        h.auras(rogue).auras.clear();
        h.tick(1);
        h.run_to(rogue, ShieldPhase::Death, 0.3);
        for d in h.drawn(rogue, ShieldKind::SacredShield) {
            if matches!(d.layer, ShieldLayer::Spark(_)) {
                let p = project(&cam, d.g.translation);
                assert!(
                    p.cmpge(lo).all() && p.cmple(hi).all(),
                    "a spark off the card"
                );
            }
        }

        // The landing's disc and runes on the body at the chest.
        let paladin = h.unit(CharacterClass::Paladin, at + Vec3::new(6.0, 0.0, 0.0));
        h.arm(rogue, Some(paladin));
        h.tick_secs(0.4);
        let (_, disc, _) = h.piece(OneShotPieceRole::SacredDisc);
        let chest = project(&cam, disc.translation);
        let (top, bottom) = (project(&cam, head), project(&cam, feet));
        assert!(
            chest.y > top.y && chest.y < bottom.y,
            "yaw {yaw}: the disc is off the body"
        );
        let disc_px = (project(&cam, disc.translation + Vec3::X * 0.8)
            - project(&cam, disc.translation - Vec3::X * 0.8))
        .length()
        .max(
            (project(&cam, disc.translation + Vec3::Z * 0.8)
                - project(&cam, disc.translation - Vec3::Z * 0.8))
            .length(),
        );
        assert!(disc_px > 15.0, "yaw {yaw}: the disc is {disc_px} px across");
        assert!(r > 1.0);
    }
}
