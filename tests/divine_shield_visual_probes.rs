//! Probes for Divine Shield's client model (`rendering/effects/shield.rs`,
//! AS-359): `divineshield_low_base.m2` on our rig, beside Power Word: Shield.
//!
//! What tells the two apart in the client is not the outline, which they
//! share, but the body: Divine Shield FILLS it blue-white, recolours streak A
//! cyan and throws off gold sparkles, where PW:S is a hollow gold outline. So
//! these probes read the world: the texel each card lays over the body's
//! centre, the hue of each live material, where every particle is born and
//! which way it flies — never the rig's stored fields. And one Paladin can
//! wear both at once.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the
//! production systems in production order (the lifecycle in `Update`, the
//! placement in `PostUpdate` before propagation, the particles' animation in
//! `Update`), as `states/mod.rs` registers them.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, ClientParticle, Combatant, DivineShieldPending, HealCastHand,
    HealCastKind, ParticleFacing, ShieldCard, ShieldKind, ShieldLayer, ShieldPhase, ShieldVisual,
    VisualBody,
};
use arenasim::states::play_match::systems::process_divine_shield;
use arenasim::states::play_match::{
    animate_client_particles, ds_halo_on, ds_sparkle_rate, particle_rotation, place_shield_cards,
    shield_card_extent, shield_layer_rgb, spawn_divine_shield_cast_hands, update_shield_visuals,
    HaloMode, DIVINE_SHIELD_AURA_NAME, DS_FILL_GAIN, DS_HALO_MODE, DS_SPARKLE_HEIGHT,
    PWS_CLIENT_TO_RIG, PW_SHIELD_AURA_NAME,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
/// The capsule radius every body has.
const BODY_RADIUS: f32 = 0.5;
/// Where the standard bearer's body centre stands.
const BODY: Vec3 = Vec3::new(0.0, 1.0, 0.0);

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
        app.add_systems(
            Update,
            (update_shield_visuals, animate_client_particles).chain(),
        );
        app.add_systems(
            PostUpdate,
            place_shield_cards.before(TransformSystem::TransformPropagate),
        );
        let mut h = Harness { app };
        h.app.world_mut().spawn((
            Camera3d::default(),
            Transform::from_translation(Vec3::new(0.0, 40.0, 30.0)).looking_at(BODY, Vec3::Y),
        ));
        h
    }

    /// A Paladin with the real rendered hierarchy: the sim entity at capsule
    /// centre height and a `VisualBody` child.
    fn paladin(&mut self, at: Vec3) -> Entity {
        let bearer = self
            .app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Paladin),
                Transform::from_translation(at),
                ActiveAuras::default(),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        self.app.world_mut().entity_mut(bearer).add_child(body);
        bearer
    }

    fn give(&mut self, bearer: Entity, aura: Aura) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .push(aura);
    }

    fn remove(&mut self, bearer: Entity, name: &str) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .retain(|a| a.ability_name != name);
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn rigs(&mut self) -> Vec<ShieldVisual> {
        let mut q = self.app.world_mut().query::<&ShieldVisual>();
        q.iter(self.app.world()).copied().collect()
    }

    fn rig(&mut self, bearer: Entity, kind: ShieldKind) -> Option<ShieldVisual> {
        self.rigs()
            .into_iter()
            .find(|r| r.bearer == bearer && r.kind == kind)
    }

    /// Ticks until the bearer's `kind` rig is `at` seconds into `phase`.
    fn run_to(&mut self, bearer: Entity, kind: ShieldKind, phase: ShieldPhase, at: f32) {
        for _ in 0..2000 {
            if let Some(r) = self.rig(bearer, kind) {
                if r.phase == phase && r.t >= at {
                    return;
                }
            }
            self.tick(1);
        }
        panic!("the {kind:?} never reached {phase:?} at {at}s");
    }

    /// Every card of one rig on screen.
    fn cards(&mut self, bearer: Entity, kind: ShieldKind) -> Vec<Card> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.bearer == bearer && r.kind == kind)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        let images = world.resource::<Assets<Image>>();
        kids.into_iter()
            .filter_map(|k| {
                let card = world.get::<ShieldCard>(k)?;
                let g = world.get::<GlobalTransform>(k)?.compute_transform();
                let m = materials.get(&world.get::<MeshMaterial3d<StandardMaterial>>(k)?.0)?;
                let image = images.get(m.base_color_texture.as_ref()?)?;
                Some(Card {
                    layer: card.layer,
                    at: g.translation,
                    half: g.scale.truncate(),
                    weight: m.base_color.alpha(),
                    rgb: m.base_color.to_srgba(),
                    texture: image.clone(),
                })
            })
            .collect()
    }

    fn card(&mut self, bearer: Entity, kind: ShieldKind, layer: ShieldLayer) -> Card {
        self.cards(bearer, kind)
            .into_iter()
            .find(|c| c.layer == layer)
            .unwrap_or_else(|| panic!("no {layer:?} card on the {kind:?}"))
    }

    /// Every live client particle: (facing, world position, velocity, the
    /// colour its material shows now).
    fn particles(&mut self) -> Vec<(ParticleFacing, Vec3, Vec3, Srgba)> {
        let mut q = self.app.world_mut().query::<(
            &ClientParticle,
            &Transform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        q.iter(world)
            .map(|(p, t, m)| {
                (
                    p.facing,
                    t.translation,
                    p.velocity,
                    materials.get(&m.0).unwrap().base_color.to_srgba(),
                )
            })
            .collect()
    }

    fn sparkles(&mut self) -> Vec<(Vec3, Vec3, Srgba)> {
        self.particles()
            .into_iter()
            .filter(|p| matches!(p.0, ParticleFacing::Spin { .. }))
            .map(|p| (p.1, p.2, p.3))
            .collect()
    }

    fn halos(&mut self) -> Vec<(Vec3, Vec3)> {
        self.particles()
            .into_iter()
            .filter(|p| p.0 == ParticleFacing::Camera)
            .map(|p| (p.1, p.2))
            .collect()
    }
}

#[derive(Clone)]
struct Card {
    layer: ShieldLayer,
    at: Vec3,
    half: Vec2,
    weight: f32,
    rgb: Srgba,
    texture: Image,
}

impl Card {
    /// The texel (its alpha: the texture's shape) this card lays over a world
    /// point on its own vertical. The card is upright and centred on the body's
    /// vertical, so the point's place in it is its height off the card centre.
    fn texel_at(&self, point: Vec3) -> f32 {
        let r = ((point.y - self.at.y) / self.half.y).abs();
        let size = self.texture.width() as usize;
        let row = ((size as f32 / 2.0) * (1.0 + r)).min(size as f32 - 1.0) as usize;
        let col = size / 2;
        let data = self.texture.data.as_ref().expect("card texels");
        data[(row * size + col) * 4 + 3] as f32 / 255.0
    }

    /// What this card adds over the body's centre right now: texel × weight.
    fn light_over(&self, point: Vec3) -> f32 {
        self.texel_at(point) * self.weight
    }
}

fn divine_shield() -> Aura {
    Aura {
        effect_type: AuraType::DamageImmunity,
        duration: 12.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
        ability_name: DIVINE_SHIELD_AURA_NAME.to_string(),
        ..Default::default()
    }
}

fn pw_shield() -> Aura {
    Aura {
        effect_type: AuraType::Absorb,
        duration: 30.0,
        magnitude: 70.0,
        ability_name: PW_SHIELD_AURA_NAME.to_string(),
        ..Default::default()
    }
}

fn is_blue(c: Srgba) -> bool {
    c.blue > 0.9 && c.red < 0.5
}

fn is_gold(c: Srgba) -> bool {
    c.red > 0.9 && c.blue < 0.85 && c.red > c.blue + 0.1
}

// --- identity -------------------------------------------------------------------

/// The visual is keyed on the aura's name; the aura is named by
/// `process_divine_shield`, and the name must be the ability's RON name — a
/// rename on either side must fail here, not silently draw nothing.
#[test]
fn the_divine_shield_aura_carries_the_ability_name() {
    let defs = AbilityDefinitions::default();
    assert_eq!(
        defs.get_unchecked(&AbilityType::DivineShield).name,
        DIVINE_SHIELD_AURA_NAME
    );
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(defs);
    let paladin = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Paladin),
            Transform::default(),
            ActiveAuras::default(),
        ))
        .id();
    world.spawn(DivineShieldPending {
        caster: paladin,
        caster_team: 1,
        caster_slot: 0,
        caster_class: CharacterClass::Paladin,
    });
    world.run_system_once(process_divine_shield).unwrap();
    let auras = &world.get::<ActiveAuras>(paladin).unwrap().auras;
    assert!(
        auras
            .iter()
            .any(|a| a.effect_type == AuraType::DamageImmunity
                && a.ability_name == DIVINE_SHIELD_AURA_NAME),
        "the immunity is not named {DIVINE_SHIELD_AURA_NAME:?}: {:?}",
        auras.iter().map(|a| &a.ability_name).collect::<Vec<_>>()
    );
}

/// The bubble no longer draws the sphere; it wears the client's five cards
/// and our blended outline and tint (AS-401).
#[test]
fn divine_shield_wears_the_client_cards_not_the_sphere() {
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.1);
    let mut layers: Vec<ShieldLayer> = h
        .cards(paladin, ShieldKind::DivineShield)
        .iter()
        .map(|c| c.layer)
        .collect();
    layers.sort_by_key(|l| *l as u8);
    assert_eq!(
        layers,
        vec![
            ShieldLayer::Rim,
            ShieldLayer::StreakA,
            ShieldLayer::StreakB,
            ShieldLayer::FillOuter,
            ShieldLayer::FillInner,
            ShieldLayer::Outline,
            ShieldLayer::Tint,
        ]
    );
}

/// The arrival is PW:S's — nothing for 200 ms, then a tall full-strength pop
/// at 300 ms — and in it the body itself flashes blue-white: the fill lays a
/// bright texel over the body's centre at full weight.
#[test]
fn the_bubble_arrives_late_as_a_blue_white_flash_over_the_body() {
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Birth, 0.10);
    for c in h.cards(paladin, ShieldKind::DivineShield) {
        assert!(
            c.weight < 1e-3 && c.half.max_element() < 1e-3,
            "{:?} drawn before 200 ms",
            c.layer
        );
    }
    assert!(h.sparkles().is_empty(), "sparkles before the pop");
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Birth, 0.296);
    let flash: f32 = ShieldLayer::FILL
        .iter()
        .map(|&l| {
            h.card(paladin, ShieldKind::DivineShield, l)
                .light_over(BODY)
        })
        .sum();
    assert!(flash > 1.0, "the body did not flash at the pop: {flash:.2}");
    let rim = h.card(paladin, ShieldKind::DivineShield, ShieldLayer::Rim);
    assert!(
        rim.half.y / rim.half.x > 2.0,
        "the pop is not tall and narrow"
    );
}

/// The held bubble: the body sits in a blue glow the whole time (the fill over
/// the body centre, on the rim's 0.06–0.15 track × `DS_FILL_GAIN`), streak A is cyan,
/// streak B, the rim and the outline are gold — where PW:S, at the same moment, lays
/// NOTHING over the body: every one of its cards is hollow there.
#[test]
fn the_bubble_fills_the_body_blue_where_pws_is_hollow() {
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    let priest_target = h.paladin(Vec3::new(5.0, 1.0, 0.0));
    let target_body = Vec3::new(5.0, 1.0, 0.0);
    h.give(paladin, divine_shield());
    h.give(priest_target, pw_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.0);
    for _ in 0..80 {
        h.tick(1);
        let ds = h.cards(paladin, ShieldKind::DivineShield);
        let glow: f32 = ds
            .iter()
            .filter(|c| ShieldLayer::FILL.contains(&c.layer))
            .map(|c| c.light_over(BODY))
            .sum();
        assert!(
            (0.06 * DS_FILL_GAIN..=0.35 * DS_FILL_GAIN).contains(&glow),
            "the body's blue glow {glow:.3} is outside the sustained band"
        );
        for c in &ds {
            match c.layer {
                ShieldLayer::FillOuter | ShieldLayer::FillInner | ShieldLayer::Tint => {
                    assert!(is_blue(c.rgb), "{:?} {:?}", c.layer, c.rgb)
                }
                // The rim's gold, darkened to PW:S's level to read on sand (AS-405).
                ShieldLayer::Outline => assert!(
                    c.rgb.red > c.rgb.blue + 0.4 && c.rgb.green > c.rgb.blue + 0.4,
                    "the outline is not gold: {:?}",
                    c.rgb
                ),
                ShieldLayer::StreakA => assert!(
                    c.rgb.blue > 0.9 && c.rgb.green > 0.8 && c.rgb.red < 0.5,
                    "streak A is not cyan: {:?}",
                    c.rgb
                ),
                ShieldLayer::Rim | ShieldLayer::StreakB => assert!(is_gold(c.rgb), "{:?}", c.rgb),
                ShieldLayer::Icicles | ShieldLayer::Glint => {
                    panic!("Divine Shield wears no icicles")
                }
            }
        }
        // The rim keeps PW:S's hollow and clears the body.
        let rim = ds.iter().find(|c| c.layer == ShieldLayer::Rim).unwrap();
        assert!(0.5 * rim.half.x >= BODY_RADIUS + 0.1);
        assert_eq!(rim.texel_at(BODY), 0.0, "the rim is not hollow");

        let pws = h.cards(priest_target, ShieldKind::PowerWordShield);
        assert_eq!(pws.len(), 4);
        for c in &pws {
            assert_eq!(
                c.texel_at(target_body),
                0.0,
                "PW:S's {:?} covers the body",
                c.layer
            );
            assert!(!is_blue(c.rgb), "PW:S's {:?} is blue", c.layer);
        }
    }
}

// --- the sparkles ---------------------------------------------------------------

/// Through the held bubble gold glints pour off the Paladin: born near the
/// waist (inside the emitter's ~1 yd sphere), flying outward, spinning, about
/// 24 alive at a time (60/s for 0.4 s). PW:S throws none.
#[test]
fn gold_sparkles_pour_off_the_bubble_and_none_off_pws() {
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.0);
    h.tick(30);
    let waist = BODY + Vec3::Y * (DS_SPARKLE_HEIGHT * PWS_CLIENT_TO_RIG - 1.25);
    let mut counts = Vec::new();
    for _ in 0..60 {
        h.tick(1);
        let sparkles = h.sparkles();
        counts.push(sparkles.len());
        for (at, velocity, colour) in sparkles {
            let out = at - waist;
            assert!(
                out.length() < 2.5,
                "a sparkle {:.2} yd from the waist",
                out.length()
            );
            assert!(
                out.length() < 0.2 || out.normalize().dot(velocity.normalize()) > 0.99,
                "a sparkle is not flying outward"
            );
            assert!(
                colour.red > 0.9 && colour.blue < 0.85,
                "a sparkle is not gold: {colour:?}"
            );
        }
    }
    let mean = counts.iter().sum::<usize>() as f32 / counts.len() as f32;
    assert!(
        (16.0..=32.0).contains(&mean),
        "{mean:.1} sparkles alive, not ~24"
    );

    // The glints spin: the same particle turns in the view plane as it ages.
    let mut q = h.app.world_mut().query::<&ClientParticle>();
    let spinning = q
        .iter(h.app.world())
        .find(|p| matches!(p.facing, ParticleFacing::Spin { .. }))
        .expect("a sparkle");
    let later = ClientParticle {
        age: spinning.age + 0.2,
        life: spinning.life,
        velocity: spinning.velocity,
        gravity: spinning.gravity,
        size: spinning.size,
        mid: spinning.mid,
        palette: spinning.palette.clone(),
        step: spinning.step,
        facing: spinning.facing,
        owned: spinning.owned,
    };
    let now = particle_rotation(spinning, Quat::IDENTITY, Quat::IDENTITY);
    let then = particle_rotation(&later, Quat::IDENTITY, Quat::IDENTITY);
    let turned = now.angle_between(then);
    assert!(
        (turned - 0.6).abs() < 0.01,
        "a glint turned {turned:.3} rad in 0.2 s, not 0.6"
    );

    let mut control = Harness::new();
    let shielded = control.paladin(BODY);
    control.give(shielded, pw_shield());
    control.run_to(
        shielded,
        ShieldKind::PowerWordShield,
        ShieldPhase::Loop,
        0.3,
    );
    assert!(control.particles().is_empty(), "PW:S threw particles");
}

/// The sparkles thin out across the 1.1 s death, as the client's rate track
/// ramps to zero, and the halo — read literally, the bench's signed-off
/// reading — drops a few faint rings from the crown in its first 300 ms only.
/// The body's blue glow fades with the rim. Within the life of the last
/// particle after the bubble is gone, nothing is left.
#[test]
fn the_death_thins_the_sparkles_and_drops_the_halo() {
    assert_eq!(DS_HALO_MODE, HaloMode::DeathOnly);
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.0);
    for _ in 0..60 {
        h.tick(1);
        assert!(h.halos().is_empty(), "a halo fell while the bubble held");
    }
    let held = h.sparkles().len();
    let held_glow = body_glow(&mut h, paladin);
    h.remove(paladin, DIVINE_SHIELD_AURA_NAME);

    let crown = BODY.y - 1.25 + 2.641 * PWS_CLIENT_TO_RIG;
    let mut halos_seen = 0;
    let mut late = usize::MAX;
    for frame in 0..75 {
        h.tick(1);
        let halos = h.halos();
        halos_seen = halos_seen.max(halos.len());
        for (at, velocity) in &halos {
            assert!(
                at.y <= crown + 1e-3 && velocity.y < 0.0,
                "a halo is not falling"
            );
            assert!(
                (at.xz() - BODY.xz()).length() < 1e-3,
                "a halo is off the body's axis"
            );
        }
        // The body's blue glow leaves with the rim: on from where it was,
        // never flaring, faded out by the end of the rim's 1 s fade.
        let glow = body_glow(&mut h, paladin);
        if frame == 0 {
            assert!(
                (glow - held_glow).abs() < 0.03,
                "the glow jumped from {held_glow:.3} to {glow:.3} as the bubble went"
            );
        }
        if frame < 62 {
            assert!(
                glow <= held_glow + 0.03,
                "the glow flared in the death: {glow:.3}"
            );
        }
        if frame == 60 {
            late = h.sparkles().len();
            assert!(
                glow < 0.15 * held_glow,
                "the glow had not faded near the end of the death: {glow:.3} of {held_glow:.3}"
            );
        }
    }
    assert!(
        (3..=6).contains(&halos_seen),
        "{halos_seen} halo rings, not the client's ~4-5"
    );
    assert!(
        late * 2 < held,
        "the sparkles did not thin across the death: {held} held, {late} at 0.97 s"
    );
    assert!(h.rigs().is_empty(), "the rig outlived its death");
    h.tick(90);
    assert!(h.particles().is_empty(), "particles outlived the bubble");
}

/// What the bubble's fill lays over the body's centre now.
fn body_glow(h: &mut Harness, paladin: Entity) -> f32 {
    h.cards(paladin, ShieldKind::DivineShield)
        .iter()
        .filter(|c| ShieldLayer::FILL.contains(&c.layer))
        .map(|c| c.light_over(BODY))
        .sum()
}

/// The emitters' readings, from the functions the system calls: no sparkles
/// before 300 ms of the birth, 60/s held, none at the end of the death; the
/// halo under each reading.
#[test]
fn the_emitter_tracks_are_the_clients() {
    assert_eq!(ds_sparkle_rate(ShieldPhase::Birth, 250.0), 0.0);
    assert_eq!(ds_sparkle_rate(ShieldPhase::Birth, 600.0), 60.0);
    assert_eq!(ds_sparkle_rate(ShieldPhase::Loop, 100.0), 60.0);
    assert!((ds_sparkle_rate(ShieldPhase::Death, 550.0) - 30.0).abs() < 1e-3);
    assert_eq!(ds_sparkle_rate(ShieldPhase::Death, 1100.0), 0.0);
    for (mode, held, early_death, late_death) in [
        (HaloMode::Off, false, false, false),
        (HaloMode::DeathOnly, false, true, false),
        (HaloMode::WholeShield, true, true, false),
    ] {
        assert_eq!(ds_halo_on(mode, ShieldPhase::Loop, 100.0), held, "{mode:?}");
        assert_eq!(
            ds_halo_on(mode, ShieldPhase::Death, 200.0),
            early_death,
            "{mode:?}"
        );
        assert_eq!(
            ds_halo_on(mode, ShieldPhase::Death, 400.0),
            late_death,
            "{mode:?}"
        );
    }
}

// --- both shields on one Paladin ------------------------------------------------

/// A Priest shields a bubbled Paladin: both models are worn at once, on the
/// same body, and each keeps its own life — the PW:S cards stay hollow gold
/// and the bubble's fill and sparkles stay; when the bubble runs out the
/// shield holds on, and when the shield breaks the bubble holds on.
#[test]
fn a_bubbled_paladin_wears_a_priests_shield_too() {
    let mut h = Harness::new();
    let paladin = h.paladin(BODY);
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.2);
    h.give(paladin, pw_shield());
    h.run_to(paladin, ShieldKind::PowerWordShield, ShieldPhase::Loop, 0.1);

    let ds = h.cards(paladin, ShieldKind::DivineShield);
    let pws = h.cards(paladin, ShieldKind::PowerWordShield);
    assert_eq!(
        (ds.len(), pws.len()),
        (
            ShieldKind::DivineShield.layers().len(),
            ShieldKind::PowerWordShield.layers().len()
        )
    );
    for c in ds.iter().chain(&pws) {
        assert!(
            (c.at.xz() - BODY.xz()).length() < 1e-4,
            "{:?} is not on the body",
            c.layer
        );
    }
    let ds_rim = ds.iter().find(|c| c.layer == ShieldLayer::Rim).unwrap();
    let pws_rim = pws.iter().find(|c| c.layer == ShieldLayer::Rim).unwrap();
    assert!(
        (ds_rim.at.y - pws_rim.at.y).abs() < 0.2,
        "the two outlines do not coincide"
    );
    assert!(
        !h.sparkles().is_empty(),
        "the bubble lost its sparkles under the shield"
    );

    // The bubble runs out; the shield holds.
    h.remove(paladin, DIVINE_SHIELD_AURA_NAME);
    h.tick(75);
    assert!(h.rig(paladin, ShieldKind::DivineShield).is_none());
    assert_eq!(
        h.rig(paladin, ShieldKind::PowerWordShield).unwrap().phase,
        ShieldPhase::Loop
    );

    // Reverse: a fresh bubble under a shield that then breaks.
    h.give(paladin, divine_shield());
    h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.1);
    h.remove(paladin, PW_SHIELD_AURA_NAME);
    h.tick(75);
    assert!(h.rig(paladin, ShieldKind::PowerWordShield).is_none());
    assert_eq!(
        h.rig(paladin, ShieldKind::DivineShield).unwrap().phase,
        ShieldPhase::Loop
    );
}

/// Bevy's transparent pass draws in ascending order of view depth plus the
/// material's `depth_bias`; this reads both off the live cards. At every pitch
/// the camera allows (0.1–1.5 rad from vertical), every yaw and every body
/// offset the animation layers write, every bubble card sorts in front of the
/// blended capsule, and the blended pair sorts behind the light (AS-401): the
/// outline first, then the blue tint, then every additive card, so neither
/// blended card dims the client's light.
#[test]
fn the_cards_sort_in_front_of_the_body_at_every_pitch() {
    let offsets = [
        Vec3::new(0.0, 0.22, 0.0),
        Vec3::new(0.0, 0.28, 0.0),
        Vec3::new(0.18, 0.10, 0.0),
        Vec3::new(-0.18, 0.10, 0.0),
        Vec3::new(0.0, 0.10, 0.18),
        Vec3::new(0.0, 0.10, -0.18),
        Vec3::new(0.0, 1.2, 0.0),
        Vec3::new(0.0, -0.10, 0.0),
    ];
    let additive = [
        ShieldLayer::FillOuter,
        ShieldLayer::FillInner,
        ShieldLayer::Rim,
        ShieldLayer::StreakA,
        ShieldLayer::StreakB,
    ];
    let mut checked = 0;
    for offset in offsets {
        let mut h = Harness::new();
        let paladin = h.paladin(BODY);
        let body = h
            .app
            .world()
            .get::<Children>(paladin)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        h.app
            .world_mut()
            .get_mut::<Transform>(body)
            .unwrap()
            .translation = offset;
        h.give(paladin, divine_shield());
        h.run_to(paladin, ShieldKind::DivineShield, ShieldPhase::Loop, 0.0);
        let cam = {
            let mut q = h.app.world_mut().query_filtered::<Entity, With<Camera3d>>();
            q.single(h.app.world()).unwrap()
        };
        for pitch in [0.1_f32, 0.4, 0.675, 1.0, 1.3, 1.5] {
            for yaw in [0.0_f32, 1.3, 2.9, 4.4] {
                let d = 40.0;
                let eye = BODY
                    + Vec3::new(
                        d * pitch.sin() * yaw.sin(),
                        d * pitch.cos(),
                        d * pitch.sin() * yaw.cos(),
                    );
                *h.app.world_mut().get_mut::<Transform>(cam).unwrap() =
                    Transform::from_translation(eye).looking_at(BODY, Vec3::Y);
                h.tick(1);
                let mut rigs = h.app.world_mut().query::<(&ShieldVisual, &Children)>();
                let kids: Vec<Entity> = rigs
                    .iter(h.app.world())
                    .filter(|(r, _)| r.kind == ShieldKind::DivineShield)
                    .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
                    .collect();
                let world = h.app.world();
                let view = world.get::<GlobalTransform>(cam).unwrap().compute_matrix();
                let range = ViewRangefinder3d::from_world_from_view(&view);
                let body_depth = range.distance_translation(
                    &world.get::<GlobalTransform>(body).unwrap().translation(),
                );
                // (layer, Bevy's sort key) per card.
                let materials = world.resource::<Assets<StandardMaterial>>();
                let keys: Vec<(ShieldLayer, f32)> = kids
                    .into_iter()
                    .map(|k| {
                        let layer = world.get::<ShieldCard>(k).unwrap().layer;
                        let at = world.get::<GlobalTransform>(k).unwrap().translation();
                        let bias = materials
                            .get(&world.get::<MeshMaterial3d<StandardMaterial>>(k).unwrap().0)
                            .unwrap()
                            .depth_bias;
                        (layer, range.distance_translation(&at) + bias)
                    })
                    .collect();
                assert_eq!(keys.len(), ShieldKind::DivineShield.layers().len());
                let key = |l: ShieldLayer| keys.iter().find(|k| k.0 == l).unwrap().1;
                for &(layer, k) in &keys {
                    assert!(
                        k > body_depth,
                        "{layer:?} sorts behind the body at pitch {pitch}, yaw {yaw}, offset {offset}"
                    );
                    checked += 1;
                }
                assert!(
                    key(ShieldLayer::Outline) < key(ShieldLayer::Tint),
                    "the outline sorts over the tint at pitch {pitch}, yaw {yaw}"
                );
                for layer in additive {
                    assert!(
                        key(ShieldLayer::Tint) < key(layer),
                        "the tint sorts over {layer:?} at pitch {pitch}, yaw {yaw}, offset {offset}"
                    );
                }
            }
        }
    }
    assert_eq!(checked, 8 * 6 * 4 * ShieldKind::DivineShield.layers().len());
}

/// The bubble's five cards ride the rendered body as PW:S's three do, with
/// the client's 0 push, so what keeps PW:S's cards sorting in front of the
/// blended capsule at every pitch holds for these too: every card centre,
/// the two fill cards included, sits above the body centre at every frame of
/// the bubble's life (`the_cards_sort_in_front_of_the_body_at_every_pitch`
/// above rests on the same).
#[test]
fn every_bubble_card_centre_sits_above_the_body_centre() {
    for &layer in ShieldKind::DivineShield.layers() {
        for (w, hgt) in [
            (0.0, 0.0),
            (0.522, 1.11),
            (0.901, 1.123),
            (1.04, 1.0),
            (1.271, 1.158),
        ] {
            let (centre, _) =
                shield_card_extent(ShieldKind::DivineShield, layer, Vec2::new(w, hgt));
            assert!(
                centre - 1.25 > 0.1,
                "{layer:?} centre {centre:.3} is not above the body centre at scale ({w}, {hgt})"
            );
        }
    }
}

/// The two colours of the bubble's own are blue, and the cards both shields
/// wear keep PW:S's gold: the only hues spent are the client's. The outline
/// traces the gold rim, so it is gold too (AS-405): the blue is the faint
/// fill inside it, never the line around it.
#[test]
// 0.318 is a client colour channel, not an approximation of 1/pi.
#[allow(clippy::approx_constant)]
fn the_bubble_spends_only_its_client_blues() {
    for &layer in ShieldKind::DivineShield.layers() {
        let ds = shield_layer_rgb(ShieldKind::DivineShield, layer);
        let pws = shield_layer_rgb(ShieldKind::PowerWordShield, layer);
        match layer {
            ShieldLayer::Rim | ShieldLayer::StreakB => assert_eq!(ds, pws, "{layer:?}"),
            ShieldLayer::Outline => {
                assert_eq!(ds, pws, "the outline is not PW:S's");
                assert_eq!(ds, [1.0, 0.91, 0.318], "the outline is not the rim gold");
            }
            ShieldLayer::StreakA => assert_eq!(ds, [0.318, 0.91, 1.0]),
            ShieldLayer::FillOuter | ShieldLayer::FillInner | ShieldLayer::Tint => {
                assert_eq!(ds, [0.318, 0.396, 1.0], "{layer:?}")
            }
            ShieldLayer::Icicles | ShieldLayer::Glint => unreachable!("not a Divine Shield layer"),
        }
    }
}

// --- the bearer going away --------------------------------------------------------

/// A Paladin that dies inside the bubble (only a corpse keeps the aura) lets
/// it go with the one ending; a despawned one takes it within a frame.
#[test]
fn the_bubble_goes_with_its_bearer() {
    let mut h = Harness::new();
    let dying = h.paladin(BODY);
    let gone = h.paladin(Vec3::new(6.0, 1.0, 0.0));
    h.give(dying, divine_shield());
    h.give(gone, divine_shield());
    h.run_to(dying, ShieldKind::DivineShield, ShieldPhase::Loop, 0.1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(dying)
        .unwrap()
        .current_health = 0.0;
    h.app.world_mut().entity_mut(gone).despawn();
    h.tick(1);
    assert!(h.rig(gone, ShieldKind::DivineShield).is_none());
    assert_eq!(
        h.rig(dying, ShieldKind::DivineShield).unwrap().phase,
        ShieldPhase::Death
    );
    h.tick(70);
    assert!(h.rigs().is_empty());
}

// --- the Paladin's hands ----------------------------------------------------------

fn hands_app() -> (App, Entity) {
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
    app.insert_resource(CombatLog::default());
    // Production order: the hands read the pending at the head of the tick,
    // and Phase 1's resolver consumes it after them.
    app.add_systems(
        Update,
        (spawn_divine_shield_cast_hands, process_divine_shield).chain(),
    );
    let paladin = app
        .world_mut()
        .spawn((
            Combatant::new(0, 0, CharacterClass::Paladin),
            Transform::from_xyz(0.0, 1.0, 0.0),
            ActiveAuras::default(),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(paladin).add_child(body);
    (app, paladin)
}

fn hands(app: &mut App) -> Vec<(HealCastKind, Vec3)> {
    app.update();
    let mut q = app.world_mut().query::<(&HealCastHand, &GlobalTransform)>();
    q.iter(app.world())
        .map(|(h, at)| (h.kind, at.translation()))
        .collect()
}

/// Kit 417: the bubble's cast flashes both of the Paladin's own hands with the
/// Holy launch, off its pending component (Divine Shield has no cast state),
/// read before `process_divine_shield` consumes it. The control: a Paladin
/// that casts nothing lights no hands. The sandbox's own path, where the
/// pending is queued from `Update`, is `animation_sandbox_boot.rs`
/// `divine_shield_previews_its_cards_and_both_hands`.
#[test]
fn the_bubble_cast_flashes_the_paladins_holy_hands() {
    let (mut app, paladin) = hands_app();
    assert!(hands(&mut app).is_empty(), "hands with no cast");
    app.world_mut().spawn(DivineShieldPending {
        caster: paladin,
        caster_team: 0,
        caster_slot: 0,
        caster_class: CharacterClass::Paladin,
    });
    app.update();
    let lit = hands(&mut app);
    assert_eq!(lit.len(), 2, "{lit:?}");
    for (kind, at) in &lit {
        assert_eq!(*kind, HealCastKind::Holy);
        assert!(
            at.distance(Vec3::new(0.0, 1.0, 0.0)) < 1.2,
            "a hand lit off the Paladin"
        );
    }
    assert!((lit[0].1 - lit[1].1).length() > 1.0, "one hand twice");
    // One flash per cast: the pending is read the tick it is queued only.
    app.update();
    assert_eq!(hands(&mut app).len(), 2);
}
