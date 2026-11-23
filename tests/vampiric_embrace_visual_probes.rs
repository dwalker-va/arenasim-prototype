//! Probes for Vampiric Embrace's visuals (AS-357): the dust implosion
//! `soothingkiss_impact_head.m2` plays at the victim's chest as the mark lands
//! (`rendering/effects/vampiric_embrace.rs`), and the violet heal stream
//! `vampiricembrace_state_base.m2` plays on each ally its heals reach — the
//! shipped heal rig (`heal_impact.rs`), at most once every 3 s per ally.
//!
//! Everything here asserts what is DRAWN — world positions under propagated
//! `GlobalTransform`s, the materials' colours, alphas, emissives and blend
//! modes, the sprites' textures and scales — against the signed-off AS-335
//! Priest Talent Visuals Bench (version 1792207490-53c9, "Vampiric Embrace"
//! tab), transcribed below straight from
//! `docs/design/benches/2026-10-17-priest-talent-bench.html` and NOT from the
//! module's tables, so a wrong value in the module cannot agree with itself
//! here.
//!
//! The marks and the heals come through the sim: a zero-length cast resolved
//! by the real `process_casting`, its aura landed by the real
//! `apply_pending_auras` (which spawns the `AuraLanding`), and Mind Blast's
//! damage feeding the real leech (`combat_core::leech::land` spawns each
//! ally's `HealImpact`). Runs on `MinimalPlugins` + `AssetPlugin` +
//! `TransformPlugin` with the chains `states/mod.rs` registers.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraLanding,
    AuraLandingKind, AuraType, CastingState, Combatant, GameRng, HealImpact, HealImpactKind,
    HealImpactRig, HealMote, MatchCountdown, Pet, PetType, PlayMatchEntity, ShadowPainHit,
    VisualBody,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{
    age_vampiric_embrace_motes, animate_heal_impacts, animate_shadow_pain_hits,
    animate_vampiric_embrace_implosions, billboard_heal_impacts, cleanup_play_match,
    place_vampiric_embrace_implosions, spawn_heal_impacts, spawn_shadow_pain_hits,
    spawn_vampiric_embrace_implosions, VampiricEmbraceImplosion, VampiricEmbraceMote,
};
use arenasim::CharacterClass;

const DT: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;
/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;

// ── The bench, transcribed ───────────────────────────────────────────────────

/// `ANCH.chest`.
const BENCH_CHEST: f32 = 1.45;

/// `{id:'implode', ... emitter:{anchor:'chest', inward:1.0, speed:-1.67,
/// life:0.5, rate:80, win:[[0,1],[1967,0]], color:[[0,[0,0,196]],
/// [0.5,[101,22,198]],[1,[212,5,195]]], alpha:[[0,0],[0.5,1],[1,0.78]],
/// scale:[[0,0.83],[0.5,0.42],[1,0.14]], kind:'dust', follow:true}}` — no
/// `blend`, so `Emitter.draw` composites it `'lighter'`: additive.
mod implode {
    pub const SHELL: f32 = 1.0;
    pub const SPEED: f32 = 1.67;
    pub const LIFE: f32 = 0.5;
    pub const RATE: f32 = 80.0;
    pub const WIN_MS: f32 = 1967.0;
    pub const COLOR: [(f32, [f32; 3]); 3] = [
        (0.0, [0.0, 0.0, 196.0]),
        (0.5, [101.0, 22.0, 198.0]),
        (1.0, [212.0, 5.0, 195.0]),
    ];
    pub const ALPHA: [(f32, f32); 3] = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.78)];
    pub const SCALE: [(f32, f32); 3] = [(0.0, 0.83), (0.5, 0.42), (1.0, 0.14)];
    /// `sprite`'s `dust`: the else branch, `soft = [0.85, 0.5, 0]`.
    pub const FALLOFF: [(f32, f32); 3] = [(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)];
}

/// `{id:'heals', ... emitter:{anchor:'feet', at:'ally', trig:'heal',
/// span:633, vRange:0.15, speed:1.4, life:1.1, rate:[[0,20],[100,40],
/// [633,0]], area:0.69, color:[[0,[47,0,179]],[0.5,[74,0,225]],
/// [1,[138,90,238]]], alpha:[[0,0.9],[1,0]], scale:[[0,0.09],[1,0.05]],
/// kind:'glow'}}` — additive; and the `every` knob at its default, 3 s
/// (`ms - lastHeal >= every*1000 - 1`).
mod heals {
    pub const SPAN_MS: f32 = 633.0;
    pub const RATE: [(f32, f32); 3] = [(0.0, 20.0), (100.0, 40.0), (633.0, 0.0)];
    pub const CONE: f32 = 0.15;
    pub const SPEED: f32 = 1.4;
    pub const LIFE: f32 = 1.1;
    pub const AREA: f32 = 0.69;
    pub const COLOR: [(f32, [f32; 3]); 3] = [
        (0.0, [47.0, 0.0, 179.0]),
        (0.5, [74.0, 0.0, 225.0]),
        (1.0, [138.0, 90.0, 238.0]),
    ];
    pub const ALPHA: [(f32, f32); 2] = [(0.0, 0.9), (1.0, 0.0)];
    pub const SCALE: [(f32, f32); 2] = [(0.0, 0.09), (1.0, 0.05)];
    /// `sprite`'s `glow`: `soft = [1, 0.45, 0]`.
    pub const FALLOFF: [(f32, f32); 3] = [(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)];
    pub const EVERY_SECS: f32 = 3.0;
}

/// The bench's `keys`: linear between keys, clamped outside them.
fn bench_keys(k: &[(f32, f32)], t: f32) -> f32 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            let u = if span > 0.0 { (t - w[0].0) / span } else { 1.0 };
            return w[0].1 + (w[1].1 - w[0].1) * u;
        }
    }
    k[k.len() - 1].1
}

fn bench_rgb(k: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let ch = |c: usize| {
        let ks: Vec<(f32, f32)> = k.iter().map(|(t, v)| (*t, v[c])).collect();
        bench_keys(&ks, t)
    };
    [ch(0), ch(1), ch(2)]
}

/// The integral of the heal layer's rate track over its span: the motes one
/// heal emits.
fn heal_rate_integral() -> f32 {
    heals::RATE
        .windows(2)
        .map(|w| 0.5 * (w[0].1 + w[1].1) * (w[1].0 - w[0].0) / 1000.0)
        .sum()
}

/// A drawn additive material against a bench colour and alpha: exact where
/// the colour channel is not near an integer boundary of the ramp (the bench
/// keys are integers, and the sprite colour is the ramp's own value).
fn assert_additive(m: &StandardMaterial, rgb: [f32; 3], alpha: f32, what: &str) {
    let s = m.base_color.to_srgba();
    let got = [s.red * 255.0, s.green * 255.0, s.blue * 255.0];
    for k in 0..3 {
        assert!(
            (got[k] - rgb[k]).abs() < 0.01,
            "{what}: drawn {got:?}, bench {rgb:?}"
        );
    }
    assert!(
        (m.base_color.alpha() - alpha).abs() < 1e-4,
        "{what}: alpha {} against the bench's {alpha}",
        m.base_color.alpha()
    );
    assert_eq!(
        m.alpha_mode,
        AlphaMode::Add,
        "{what}: 'lighter' is additive"
    );
    let lin = Color::srgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0).to_linear();
    let e = m.emissive;
    for (got, want) in [
        (e.red, lin.red * 2.5),
        (e.green, lin.green * 2.5),
        (e.blue, lin.blue * 2.5),
    ] {
        assert!((got - want).abs() < 1e-4, "{what}: emissive {e:?}");
    }
}

// ── The harness ──────────────────────────────────────────────────────────────

/// Every heal stream rig built, `(recipient, frame)`, in order.
#[derive(Resource, Default)]
struct StreamsBuilt(Vec<(Entity, u32)>);

#[derive(Resource, Default)]
struct Frame(u32);

fn count_streams(
    mut built: ResMut<StreamsBuilt>,
    mut frame: ResMut<Frame>,
    rigs: Query<&HealImpact, Added<HealImpactRig>>,
) {
    frame.0 += 1;
    for impact in rigs.iter() {
        if impact.kind == HealImpactKind::VampiricEmbrace {
            built.0.push((impact.target, frame.0));
        }
    }
}

struct Harness {
    app: App,
    /// The frame's length, seconds.
    dt: f32,
}

impl Harness {
    fn new() -> Self {
        Harness::with_tick(DT)
    }

    /// A harness whose frames are `dt` seconds long.
    fn with_tick(dt: f32) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            dt,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(0))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ArenaDampening::default())
        .insert_resource(StreamsBuilt::default())
        .insert_resource(Frame::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        });
        // The sim's cast and its aura's landing, then the chains
        // `states/mod.rs` registers.
        app.add_systems(Update, (process_casting, apply_pending_auras).chain());
        app.add_systems(
            Update,
            (
                spawn_vampiric_embrace_implosions,
                animate_vampiric_embrace_implosions,
                age_vampiric_embrace_motes,
            )
                .chain()
                .after(apply_pending_auras),
        );
        app.add_systems(
            Update,
            (
                spawn_heal_impacts,
                // Counted as the rig is built, before a long frame can age it
                // out the same frame.
                count_streams,
                animate_heal_impacts,
                billboard_heal_impacts,
            )
                .chain()
                .after(apply_pending_auras),
        );
        app.add_systems(
            Update,
            (spawn_shadow_pain_hits, animate_shadow_pain_hits)
                .chain()
                .after(apply_pending_auras),
        );
        // Posed on the rendered body in PostUpdate, before propagation, as
        // `states/mod.rs` registers it.
        app.add_systems(
            PostUpdate,
            place_vampiric_embrace_implosions
                .before(bevy::transform::TransformSystem::TransformPropagate),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app, dt }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / self.dt).round() as u32);
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn camera(&mut self, transform: Transform) {
        self.world().spawn((Camera3d::default(), transform));
    }

    fn priest(&mut self, at: Vec3) -> Entity {
        let defs = AbilityDefinitions::default();
        let mut c = Combatant::new(1, 0, CharacterClass::Priest);
        let book = c
            .book()
            .clone()
            .granting(&defs, AbilityType::VampiricEmbrace);
        c = c.with_book(book);
        c.max_mana = 5000.0;
        c.current_mana = 5000.0;
        self.world()
            .spawn((c, Transform::from_translation(at)))
            .id()
    }

    fn unit(&mut self, team: u8, slot: u8, class: CharacterClass, at: Vec3) -> Entity {
        self.world()
            .spawn((
                Combatant::new(team, slot, class),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn pet(&mut self, owner: Entity, at: Vec3) -> Entity {
        self.world()
            .spawn((
                Combatant::new(1, 3, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Spider,
                },
                Transform::from_translation(at),
            ))
            .id()
    }

    /// Start a zero-length cast and run frames until it has landed.
    fn cast(&mut self, caster: Entity, target: Entity, ability: AbilityType) {
        self.world()
            .entity_mut(caster)
            .insert(CastingState::new(ability, target, 0.0));
        for _ in 0..5 {
            self.tick(1);
            if self.app.world().get::<CastingState>(caster).is_none() {
                return;
            }
        }
        panic!("{ability:?} never landed");
    }

    fn implosions(&mut self) -> Vec<(Entity, Entity, f32, Vec3)> {
        let mut q = self
            .world()
            .query::<(Entity, &VampiricEmbraceImplosion, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, r, g)| (e, r.target, r.age, g.translation()))
            .collect()
    }

    fn streams(&self) -> Vec<(Entity, u32)> {
        self.app.world().resource::<StreamsBuilt>().0.clone()
    }

    fn frame(&self) -> u32 {
        self.app.world().resource::<Frame>().0
    }

    fn material(&self, entity: Entity) -> StandardMaterial {
        let world = self.app.world();
        world
            .resource::<Assets<StandardMaterial>>()
            .get(
                &world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .unwrap()
                    .0,
            )
            .unwrap()
            .clone()
    }

    fn mesh_corners(&self, entity: Entity) -> BTreeSet<(i32, i32, i32)> {
        let world = self.app.world();
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&world.get::<Mesh3d>(entity).unwrap().0)
            .unwrap();
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap()
            .iter()
            .map(|p| {
                (
                    (p[0] * 10.0).round() as i32,
                    (p[1] * 10.0).round() as i32,
                    (p[2] * 10.0).round() as i32,
                )
            })
            .collect()
    }

    /// The sprite texture a material wears, as alpha against the distance
    /// from its centre (in half-widths), along its middle row.
    fn falloff(&self, m: &StandardMaterial) -> Vec<(f32, f32)> {
        let images = self.app.world().resource::<Assets<Image>>();
        let image = images
            .get(&m.base_color_texture.clone().expect("a sprite texture"))
            .unwrap();
        let w = image.width();
        let data = image.data.as_ref().unwrap();
        let y = w / 2;
        (0..w)
            .map(|x| {
                let dx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let a = data[((y * w + x) * 4 + 3) as usize] as f32 / 255.0;
                ((dx * dx + dy * dy).sqrt(), a)
            })
            .collect()
    }
}

fn assert_falloff(row: &[(f32, f32)], stops: &[(f32, f32)], what: &str) {
    assert!(row.len() >= 32, "{what}: a texture");
    for (r, a) in row {
        let want = bench_keys(stops, *r);
        assert!(
            (a - want).abs() < 1.0 / 255.0 + 1e-3,
            "{what}: alpha {a} at {r} of the radius, bench {want}"
        );
    }
}

fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * (BENCH_CHEST - COMBATANT_Y)
}

/// The match camera at `zoom` and `pitch` from the vertical, and `yaw`,
/// looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// The depth of the body's nearest point along `forward`, for a unit standing
/// at `at`.
fn body_nearest_depth(at: Vec3, eye: Vec3, forward: Vec3) -> f32 {
    [
        at - Vec3::Y * BODY_HALF_SEGMENT,
        at + Vec3::Y * BODY_HALF_SEGMENT,
    ]
    .iter()
    .map(|e| (*e - eye).dot(forward))
    .fold(f32::MAX, f32::min)
        - BODY_RADIUS
}

/// A Priest with a Warrior beside it, both on team 1, and an enemy Warrior on
/// team 2 for them to embrace.
struct Scene {
    priest: Entity,
    ally: Entity,
    enemy: Entity,
    enemy_at: Vec3,
}

fn scene(h: &mut Harness) -> Scene {
    let priest = h.priest(Vec3::new(-8.0, COMBATANT_Y, 0.0));
    let ally = h.unit(
        1,
        1,
        CharacterClass::Warrior,
        Vec3::new(-4.0, COMBATANT_Y, 3.0),
    );
    // Hurt, so every heal lands health and is logged.
    for unit in [priest, ally] {
        let mut c = h.world().get_mut::<Combatant>(unit).unwrap();
        c.current_health = c.max_health * 0.3;
    }
    let enemy_at = Vec3::new(4.0, COMBATANT_Y, -1.0);
    let enemy = h.unit(2, 0, CharacterClass::Warrior, enemy_at);
    Scene {
        priest,
        ally,
        enemy,
        enemy_at,
    }
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Vampiric Embrace's mark lands its implosion (and so raises no band), its
/// leech heals play the violet stream, and nothing else is caught: a leech
/// mark by another name keeps the band, and the Warlock's leeches draw no
/// heal landing.
#[test]
fn the_routes_name_the_implosion_and_the_stream_and_nothing_else() {
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::LeechMark, "Vampiric Embrace"),
        Some(AuraLandingKind::VampiricEmbraceImplosion)
    );
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::LeechMark, "Some Other Leech"),
        None
    );
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::DamageOverTime, "Vampiric Embrace"),
        None
    );
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::LeechMark, "Vampiric Embrace"),
        AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding)
    );
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::LeechMark, "Some Other Leech"),
        AuraApplyRoute::Band
    );
    assert_eq!(
        HealImpact::kind_for_leech("Vampiric Embrace"),
        Some(HealImpactKind::VampiricEmbrace)
    );
    for warlock in ["Death Coil", "Siphon Life", "Drain Life"] {
        assert_eq!(HealImpact::kind_for_leech(warlock), None, "{warlock}");
    }
    // The ability's own aura carries the routed name.
    let defs = AbilityDefinitions::default();
    let ve = defs.get_unchecked(&AbilityType::VampiricEmbrace);
    let aura = ve.applies_aura.as_ref().unwrap();
    assert_eq!(aura.aura_type, AuraType::LeechMark);
    assert_eq!(ve.name, "Vampiric Embrace");
}

// ── Through the sim ──────────────────────────────────────────────────────────

/// The real cast lands one implosion, on its target, at its chest — beside a
/// Shadow Word: Pain already on the victim, whose own hit is its own. Its
/// control: Shadow Word: Pain alone lands no implosion.
#[test]
fn the_cast_lands_one_implosion_on_its_target_at_its_chest() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::ShadowWordPain);
    assert!(
        h.implosions().is_empty(),
        "Shadow Word: Pain lands no implosion"
    );
    let mut q = h.world().query::<&ShadowPainHit>();
    assert_eq!(q.iter(h.app.world()).count(), 1, "the control's own hit");

    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    let auras = h.app.world().get::<ActiveAuras>(s.enemy).unwrap();
    assert_eq!(
        auras
            .auras
            .iter()
            .filter(|a| a.effect_type == AuraType::LeechMark)
            .count(),
        1
    );
    assert_eq!(auras.auras.len(), 2, "beside the Shadow Word: Pain");
    let rigs = h.implosions();
    assert_eq!(rigs.len(), 1, "one implosion");
    let (rig, target, _, at) = rigs[0];
    assert_eq!(target, s.enemy, "on the mark's bearer");
    assert!(
        (at - chest(s.enemy_at)).length() < 1e-5,
        "at the chest: {at}"
    );
    assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());
    let mut q = h.world().query::<&AuraLanding>();
    assert_eq!(q.iter(h.app.world()).count(), 0, "the marker is consumed");

    // It rides its victim (the bench's `follow`): the victim runs, and the
    // implosion stands at its chest.
    let moved = s.enemy_at + Vec3::new(3.0, 0.0, -2.0);
    h.world().get_mut::<Transform>(s.enemy).unwrap().translation = moved;
    h.tick(2);
    let (_, _, _, at) = h.implosions()[0];
    assert!((at - chest(moved)).length() < 1e-5, "followed: {at}");
}

/// The pet body (`Capsule3d::new(0.35, 0.6)`, centred 0.45 below a pet's
/// transform) at 0.55 of a combatant's stature.
const PET_BODY_Y: f32 = -0.45;
const PET_STATURE: f32 = 0.55;

/// A Vampiric Embrace on a pet implodes at the pet's chest, on a shell and at
/// a speed scaled to its smaller body.
#[test]
fn a_pets_implosion_is_scaled_to_its_body() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    let owner = h.unit(
        2,
        1,
        CharacterClass::Hunter,
        Vec3::new(6.0, COMBATANT_Y, 2.0),
    );
    let pet_at = Vec3::new(5.0, COMBATANT_Y, -3.0);
    let pet = h
        .world()
        .spawn((
            Combatant::new(2, 2, CharacterClass::Hunter),
            Pet {
                owner,
                pet_type: PetType::Spider,
            },
            Transform::from_translation(pet_at),
        ))
        .id();
    h.cast(s.priest, pet, AbilityType::VampiricEmbrace);
    let rigs = h.implosions();
    assert_eq!(rigs.len(), 1);
    let (_, target, _, at) = rigs[0];
    assert_eq!(target, pet);
    let pet_chest = pet_at + Vec3::Y * (PET_BODY_Y + (BENCH_CHEST - COMBATANT_Y) * PET_STATURE);
    assert!((at - pet_chest).length() < 1e-5, "at the pet's chest: {at}");
    h.tick(10);
    let mut q = h.world().query::<&VampiricEmbraceMote>();
    let motes: Vec<(Vec3, Vec3, f32)> = q
        .iter(h.app.world())
        .map(|m| (m.offset - m.velocity * m.age, m.velocity, m.age))
        .collect();
    assert!(motes.len() > 5);
    for (born, v, _) in motes {
        let r = born.length() / PET_STATURE;
        assert!((0.6 - 1e-3..=1.0 + 1e-3).contains(&r), "shell {r}");
        assert!((v.length() - implode::SPEED * PET_STATURE).abs() < 1e-3);
    }
}

/// A Mind Blast into the mark heals the Priest and its ally through the real
/// leech, and BOTH play the stream on the frame of the heal — at each one's
/// feet — while the team's pet (no party member) and the enemy get none.
/// Its control: the same Mind Blast without the mark plays no stream.
#[test]
fn a_heal_plays_the_stream_on_every_ally_it_reaches_and_on_no_one_else() {
    for marked in [false, true] {
        let mut h = Harness::new();
        let s = scene(&mut h);
        let pet = h.pet(s.ally, Vec3::new(-3.0, COMBATANT_Y, 5.0));
        if marked {
            h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
        }
        h.cast(s.priest, s.enemy, AbilityType::MindBlast);
        let log = h.app.world().resource::<CombatLog>();
        let heals = log
            .entries
            .iter()
            .filter(|e| e.message.contains("Vampiric Embrace heals"))
            .count();
        let streams = h.streams();
        if !marked {
            assert_eq!(heals, 0);
            assert!(streams.is_empty(), "no mark, no stream");
            continue;
        }
        assert_eq!(heals, 2, "the Priest and its ally");
        let got: BTreeSet<Entity> = streams.iter().map(|(e, _)| *e).collect();
        assert_eq!(got, BTreeSet::from([s.priest, s.ally]));
        assert_eq!(streams[0].1, streams[1].1, "both on the heal's frame");
        assert!(!got.contains(&pet) && !got.contains(&s.enemy));
        // Each stream stands at its own ally's feet.
        let mut q = h
            .world()
            .query::<(&HealImpact, &HealImpactRig, &GlobalTransform)>();
        let world = h.app.world();
        let at: Vec<(Entity, Vec3)> = q
            .iter(world)
            .map(|(i, _, g)| (i.target, g.translation()))
            .collect();
        for (target, rig) in at {
            let feet = world.get::<Transform>(target).unwrap().translation;
            assert!(
                (rig.xz() - feet.xz()).length() < 1e-5,
                "a stream on its ally"
            );
            assert!((rig.y - (feet.y - 1.15)).abs() < 1e-5, "at the Base attach");
        }
    }
}

/// The ruling's throttle, per ally, through the sim's heals: an ally healed
/// twice within 3 s plays once; healed again 3 s after it last played, it
/// plays again; and the clock is each ally's own — an ally whose first heal
/// lands while the others are still throttled plays at once. (That ally is
/// cycloned for the first heals: a cycloned unit takes no healing, so it is
/// given no stream either.)
#[test]
fn the_stream_plays_on_each_ally_at_most_once_every_three_seconds() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    let late = h.unit(
        1,
        2,
        CharacterClass::Mage,
        Vec3::new(-6.0, COMBATANT_Y, -3.0),
    );
    h.world().entity_mut(late).insert(ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::Cyclone,
            duration: 100.0,
            ..Default::default()
        }],
    });
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    let blast = |h: &mut Harness| -> u32 {
        h.cast(s.priest, s.enemy, AbilityType::MindBlast);
        h.frame()
    };
    let per = |h: &Harness, who: Entity| -> Vec<u32> {
        h.streams()
            .into_iter()
            .filter(|(e, _)| *e == who)
            .map(|(_, f)| f)
            .collect()
    };

    let first = blast(&mut h);
    assert_eq!(per(&h, s.ally), vec![first]);
    assert_eq!(per(&h, s.priest), vec![first]);
    assert!(per(&h, late).is_empty(), "the cycloned ally is not healed");

    // 1 s on: throttled for the two, and the freed ally's first heal plays.
    h.tick_secs(1.0);
    h.world()
        .entity_mut(late)
        .insert(ActiveAuras { auras: vec![] });
    let second = blast(&mut h);
    assert_eq!(per(&h, s.ally), vec![first], "healed again within 3 s");
    assert_eq!(per(&h, late), vec![second], "its own clock");

    // Just short of 3 s after the first: still throttled.
    let short = (heals::EVERY_SECS / DT) as u32 - (second - first) - 4;
    h.tick(short);
    let third = blast(&mut h);
    assert!((third - first) as f32 * DT < heals::EVERY_SECS);
    assert_eq!(per(&h, s.ally), vec![first]);
    assert_eq!(per(&h, s.priest), vec![first]);

    // 3 s after the first: it plays again, and the late ally (1 s into its
    // own clock) does not.
    h.tick(4);
    let fourth = blast(&mut h);
    assert!((fourth - first) as f32 * DT >= heals::EVERY_SECS - 0.001);
    assert!(((fourth - first) as f32 * DT) < heals::EVERY_SECS + 0.1);
    assert_eq!(per(&h, s.ally), vec![first, fourth]);
    assert_eq!(per(&h, s.priest), vec![first, fourth]);
    assert_eq!(per(&h, late), vec![second]);

    // Every heal landed and was logged, throttled or not.
    let log = h.app.world().resource::<CombatLog>();
    let heals_to = |name: &str| {
        log.entries
            .iter()
            .filter(|e| e.message.contains("Vampiric Embrace heals") && e.message.contains(name))
            .count()
    };
    assert_eq!(heals_to("Warrior"), 4);
}

// ── The implosion, as drawn ──────────────────────────────────────────────────

/// One dust mote's birth, read the frame it first appears (one frame old).
struct Birth {
    ms: f32,
    born: Vec3,
    launch: Vec3,
}

/// Land an implosion on a still enemy, on frames `dt` long, and record every
/// birth.
fn implosion_births(dt: f32) -> (Vec<Birth>, Vec3) {
    let mut h = Harness::with_tick(dt);
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(&(_, _, age, _)) = h.implosions().first() {
        let mut q = h.world().query::<(Entity, &VampiricEmbraceMote)>();
        let motes: Vec<(Entity, Vec3, Vec3, f32, f32)> = q
            .iter(h.app.world())
            .map(|(e, m)| (e, m.offset, m.velocity, m.age, m.life))
            .collect();
        for (e, offset, velocity, mote_age, life) in motes {
            if !seen.insert(e) {
                continue;
            }
            assert!((mote_age - dt).abs() < 1e-5, "first seen one frame old");
            assert!((life - implode::LIFE).abs() < 1e-6, "lives {life}");
            out.push(Birth {
                ms: age * 1000.0,
                born: offset - velocity * dt,
                launch: velocity,
            });
        }
        h.tick(1);
    }
    (out, s.enemy_at)
}

/// Every mote is born on the bench's shell — 0.6 to 1.0 of it out from the
/// chest, filling that band in every direction — and travels straight IN at
/// 1.67 yd/s; motes are born only inside the 1967 ms window, at 80 a second.
/// The same at 8 ms and 33 ms frames as at 16: the rate, the ageing and the
/// flight are per second, never per frame.
#[test]
fn the_dust_is_born_on_the_shell_and_falls_in_for_the_window() {
    for dt in [DT, 0.008, 0.033] {
        dust_is_born_on_the_shell(dt);
    }
}

fn dust_is_born_on_the_shell(dt: f32) {
    let (births, _) = implosion_births(dt);
    let expected = implode::RATE * implode::WIN_MS / 1000.0;
    assert!(
        (births.len() as f32 - expected).abs() <= implode::RATE * dt + 1.0,
        "{} motes against the bench's {expected} at {dt} s frames",
        births.len()
    );
    let (mut near, mut far) = (f32::MAX, 0.0_f32);
    let mut octants = BTreeSet::new();
    for b in &births {
        assert!(b.ms < implode::WIN_MS, "born at {} ms", b.ms);
        let r = b.born.length();
        assert!(
            (0.6 * implode::SHELL - 1e-4..=implode::SHELL + 1e-4).contains(&r),
            "born {r} out"
        );
        near = near.min(r);
        far = far.max(r);
        assert!((b.launch.length() - implode::SPEED).abs() < 1e-3);
        assert!(
            b.launch.normalize().dot(-b.born.normalize()) > 0.9999,
            "travels straight in"
        );
        octants.insert((b.born.x > 0.0, b.born.y > 0.0, b.born.z > 0.0));
    }
    assert!(
        near < 0.65 && far > 0.95,
        "the shell is filled: {near}..{far}"
    );
    assert_eq!(octants.len(), 8, "from every side");
    let last = births.iter().map(|b| b.ms).fold(0.0, f32::max);
    assert!(
        last > implode::WIN_MS - 1000.0 * dt - 25.0,
        "emits to the window's end: {last}"
    );
}

/// Every mote is drawn as the bench draws it at its age: its colour, alpha and
/// emissive off the tracks, additive, on the `dust` falloff, on a unit quad,
/// as wide as twice the keyed radius (no camera: nothing held). The rig retires
/// once its last mote has lived out its life.
#[test]
fn the_dust_is_drawn_as_the_bench_keys_it() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    let mut checked = 0;
    let mut mid_life = 0;
    for _ in 0..40 {
        h.tick(1);
        let mut q = h
            .world()
            .query::<(Entity, &VampiricEmbraceMote, &Transform)>();
        let motes: Vec<(Entity, f32, f32, Vec3)> = q
            .iter(h.app.world())
            .map(|(e, m, t)| (e, m.age, m.life, t.scale))
            .collect();
        for (e, age, life, scale) in motes {
            let t = age / life;
            let m = h.material(e);
            assert_additive(
                &m,
                bench_rgb(&implode::COLOR, t),
                bench_keys(&implode::ALPHA, t),
                "dust",
            );
            let want = 2.0 * bench_keys(&implode::SCALE, t);
            assert!((scale.x - want).abs() < 1e-4, "{} vs {want}", scale.x);
            if (0.3..0.7).contains(&t) {
                mid_life += 1;
            }
            checked += 1;
        }
    }
    assert!(checked > 500 && mid_life > 100, "{checked} / {mid_life}");
    let mut q = h.world().query::<(Entity, &VampiricEmbraceMote)>();
    let one = q.iter(h.app.world()).next().unwrap().0;
    let m = h.material(one);
    assert_falloff(&h.falloff(&m), &implode::FALLOFF, "dust");
    assert_eq!(
        h.mesh_corners(one),
        BTreeSet::from([(-5, -5, 0), (-5, 5, 0), (5, -5, 0), (5, 5, 0)]),
        "a unit quad"
    );
    // The rig lives until its last mote dies: 1967 + 500 ms.
    h.tick_secs(2.467 - 40.0 * DT - 0.05);
    assert_eq!(h.implosions().len(), 1);
    h.tick_secs(0.1);
    assert!(h.implosions().is_empty(), "retired with its last mote");
    let mut q = h.world().query::<&VampiricEmbraceMote>();
    assert_eq!(q.iter(h.app.world()).count(), 0);
}

// ── The stream, as drawn ─────────────────────────────────────────────────────

/// One heal's motes: born at the ally's FEET (the floor), filling the bench's
/// 0.69 yd square, rising at 1.4 yd/s within 0.15 rad of straight up, for
/// 633 ms at the bench's keyed rate — whose integral is 13.7 motes; a peak at
/// the window's middle instead of 100 ms in would make it 15.8.
#[test]
fn the_stream_rises_from_the_feet_for_the_benches_span_and_count() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    h.cast(s.priest, s.enemy, AbilityType::MindBlast);
    let ally_at = h.app.world().get::<Transform>(s.ally).unwrap().translation;
    let mut seen = BTreeSet::new();
    let mut births: Vec<(f32, Vec3, Vec3)> = Vec::new();
    for _ in 0..60 {
        let world = h.world();
        let mut rigs = world.query::<(Entity, &HealImpact)>();
        let rig: Vec<(Entity, f32)> = rigs
            .iter(world)
            .filter(|(_, i)| i.target == s.ally && i.kind == HealImpactKind::VampiricEmbrace)
            .map(|(e, i)| (e, i.age))
            .collect();
        let mut motes = world.query::<(Entity, &HealMote, &ChildOf, &GlobalTransform)>();
        let found: Vec<(Entity, f32, f32, Vec3, Vec3)> = motes
            .iter(world)
            .filter(|(_, _, p, _)| rig.iter().any(|(r, _)| *r == p.parent()))
            .map(|(e, m, _, g)| (e, m.age, m.life, m.velocity, g.translation()))
            .collect();
        for (e, age, life, velocity, at) in found {
            if seen.insert(e) {
                assert_eq!(age, 0.0, "first seen as it is born");
                assert!((life - heals::LIFE).abs() < 1e-6, "lives {life}");
                births.push((rig[0].1 * 1000.0, at, velocity));
            }
        }
        h.tick(1);
    }
    let n = births.len() as f32;
    let integral = heal_rate_integral();
    assert!((integral - 13.66).abs() < 0.01);
    assert!(
        (n - integral).abs() <= 1.0,
        "{n} motes against the bench's {integral}"
    );
    let (mut xs, mut zs) = (0.0_f32, 0.0_f32);
    for (ms, at, v) in &births {
        assert!(*ms <= heals::SPAN_MS + 17.0, "born {ms} ms after the heal");
        let off = *at - ally_at;
        // Born on the floor (no camera: nothing held), in the square.
        assert!(
            (at.y - 0.0).abs() < 1e-4,
            "born at {} above the floor",
            at.y
        );
        assert!(off.x.abs() <= heals::AREA / 2.0 + 1e-4);
        assert!(off.z.abs() <= heals::AREA / 2.0 + 1e-4);
        xs = xs.max(off.x.abs());
        zs = zs.max(off.z.abs());
        assert!((v.y - heals::SPEED).abs() < 1e-4, "rises at {}", v.y);
        let tilt = v.xz().length().atan2(v.y);
        assert!(tilt <= heals::CONE + 1e-4, "{tilt} rad off straight up");
    }
    assert!(xs > 0.25 && zs > 0.25, "the square is filled: {xs}, {zs}");
}

/// Every stream mote wears the bench's look at its age: violet over its life,
/// alpha 0.9 -> 0, additive, on the `glow` falloff, as wide as twice its
/// keyed radius; on a unit quad.
#[test]
fn the_stream_is_drawn_as_the_bench_keys_it() {
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    h.cast(s.priest, s.enemy, AbilityType::MindBlast);
    let mut checked = 0;
    let mut late = 0;
    let mut sample = None;
    for _ in 0..80 {
        h.tick(1);
        let mut q = h.world().query::<(Entity, &HealMote, &Transform)>();
        let motes: Vec<(Entity, f32, f32, Vec3)> = q
            .iter(h.app.world())
            .map(|(e, m, t)| (e, m.age, m.life, t.scale))
            .collect();
        for (e, age, life, scale) in motes {
            // A mote is drawn from its first frame on (the rig spawns it
            // collapsed), until its life runs out.
            if age == 0.0 || age > life {
                continue;
            }
            let t = age / life;
            assert_additive(
                &h.material(e),
                bench_rgb(&heals::COLOR, t),
                bench_keys(&heals::ALPHA, t),
                "stream",
            );
            let want = 2.0 * bench_keys(&heals::SCALE, t);
            assert!((scale.x - want).abs() < 1e-4, "{} vs {want}", scale.x);
            assert!((scale.y - want).abs() < 1e-4, "round, not a streak");
            if t > 0.6 {
                late += 1;
            }
            checked += 1;
            sample = Some(e);
        }
    }
    assert!(checked > 300 && late > 50, "{checked} / {late}");
    let e = sample.unwrap();
    assert_falloff(&h.falloff(&h.material(e)), &heals::FALLOFF, "stream");
    assert_eq!(
        h.mesh_corners(e),
        BTreeSet::from([(-5, -5, 0), (-5, 5, 0), (5, -5, 0), (5, 5, 0)]),
        "a unit quad"
    );
}

// ── Burial and the camera ────────────────────────────────────────────────────

/// At the match camera — from its pitch and yaws all round, and a low and a
/// near one — every implosion mote and every stream mote faces the camera and
/// stands nearer than every point of its unit's body (so none is buried in
/// it), on its true point's view ray and scaled to hold its screen size.
#[test]
fn every_mote_stands_in_front_of_its_body_where_the_bench_puts_it() {
    let mut held = 0;
    let mut checked = 0;
    let mut stream_held = 0;
    for (zoom, pitch, yaw) in [
        (60.0, 38.7f32.to_radians(), 0.0),
        (60.0, 38.7f32.to_radians(), 2.3),
        (60.0, 38.7f32.to_radians(), 4.4),
        (20.0, 1.5, 4.0),
        (60.0, 0.1, 1.0),
    ] {
        let mut h = Harness::new();
        let s = scene(&mut h);
        let cam = match_camera(Vec3::new(-2.0, COMBATANT_Y, 1.0), zoom, pitch, yaw);
        h.camera(cam);
        h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
        h.cast(s.priest, s.enemy, AbilityType::MindBlast);
        h.tick(20);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let world = h.app.world_mut();
        // `(drawn, true point, keyed radius, the unit's feet, its quad,
        // whether it is a stream mote)`.
        let mut pieces: Vec<(Vec3, Vec3, f32, Vec3, GlobalTransform, bool)> = Vec::new();
        let mut q = world.query::<(&VampiricEmbraceMote, &ChildOf, &GlobalTransform)>();
        for (m, parent, g) in q.iter(world) {
            let rig = world
                .get::<VampiricEmbraceImplosion>(parent.parent())
                .unwrap();
            let victim = world.get::<Transform>(rig.target).unwrap().translation;
            let truly = chest(victim) + m.offset;
            let r = bench_keys(&implode::SCALE, m.age / m.life);
            pieces.push((g.translation(), truly, r, victim, *g, false));
        }
        let mut q = world.query::<(&HealMote, &ChildOf, &GlobalTransform)>();
        for (m, parent, g) in q.iter(world) {
            if m.age == 0.0 || m.age > m.life {
                continue;
            }
            let rig = world.get::<GlobalTransform>(parent.parent()).unwrap();
            let target = world.get::<HealImpact>(parent.parent()).unwrap().target;
            let at = world.get::<Transform>(target).unwrap().translation;
            let truly = rig.translation() + m.offset;
            let r = bench_keys(&heals::SCALE, m.age / m.life);
            pieces.push((g.translation(), truly, r, at, *g, true));
        }
        for (drawn, truly, r, unit, g, stream) in pieces {
            let g = g.compute_transform();
            let normal = g.rotation * Vec3::Z;
            assert!(
                normal.dot(-forward) > 0.9999,
                "a quad turned off the camera"
            );
            let depth = (drawn - eye).dot(forward);
            let nearest = body_nearest_depth(unit, eye, forward);
            assert!(depth < nearest, "buried: {depth} behind {nearest}");
            let ray = (truly - eye).normalize();
            assert!(
                (drawn - eye).normalize().dot(ray) > 0.999_999,
                "a held mote left its screen position"
            );
            let k = depth / (truly - eye).dot(forward);
            assert!(
                (g.scale.x - 2.0 * r * k).abs() < 1e-4,
                "screen size {} vs {}",
                g.scale.x,
                2.0 * r * k
            );
            if k < 0.999 {
                assert!(depth > nearest - 0.1, "held only as far as it must be");
                held += 1;
                stream_held += stream as usize;
            }
            checked += 1;
        }
    }
    assert!(checked > 150, "only {checked} quads checked");
    assert!(held > 60, "only {held} needed holding — vacuous");
    assert!(stream_held > 10, "only {stream_held} stream motes held");
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

/// The implosion plays out on a victim that dies under it (it is a landing),
/// goes at once with a victim that is gone, and a match that ends mid-
/// implosion — or mid-stream — takes every piece of both.
#[test]
fn the_implosion_outlives_a_death_but_not_a_despawn_or_the_match() {
    // Dies under it: plays on.
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    h.tick(10);
    h.world()
        .get_mut::<Combatant>(s.enemy)
        .unwrap()
        .current_health = 0.0;
    h.tick(30);
    assert_eq!(h.implosions().len(), 1, "a landing plays out on a corpse");

    // Gone: retired at once, motes and all.
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    h.tick(10);
    let mut q = h.world().query::<&VampiricEmbraceMote>();
    assert!(q.iter(h.app.world()).count() > 5);
    h.world().entity_mut(s.enemy).despawn();
    h.tick(2);
    assert!(h.implosions().is_empty());
    let mut q = h.world().query::<&VampiricEmbraceMote>();
    assert_eq!(q.iter(h.app.world()).count(), 0);

    // The match ends mid-implosion and mid-stream.
    let mut h = Harness::new();
    let s = scene(&mut h);
    h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
    h.cast(s.priest, s.enemy, AbilityType::MindBlast);
    h.tick(5);
    let mut q = h.world().query::<&HealMote>();
    assert!(q.iter(h.app.world()).count() > 0, "a stream is playing");
    let rigs = h.implosions();
    assert!(!rigs.is_empty());
    for (rig, ..) in &rigs {
        assert!(h.app.world().get::<PlayMatchEntity>(*rig).is_some());
    }
    let mut q = h.world().query_filtered::<Entity, With<HealImpact>>();
    let streams: Vec<Entity> = q.iter(h.app.world()).collect();
    assert!(!streams.is_empty());
    for e in streams {
        assert!(h.app.world().get::<PlayMatchEntity>(e).is_some());
    }
    h.world().run_system_cached(cleanup_play_match).unwrap();
    let mut q = h.world().query::<&VampiricEmbraceMote>();
    assert_eq!(q.iter(h.app.world()).count(), 0);
    let mut q = h.world().query::<&HealMote>();
    assert_eq!(q.iter(h.app.world()).count(), 0);
    assert!(h.implosions().is_empty());
}

// ── The rendered body ────────────────────────────────────────────────────────

/// How far a unit's rendered body (its `VisualBody` child) stands from its
/// rest pose: `local` is the child's translation, `rest_y` its rest height.
/// The point the body's anchors are measured from, as a unit at rest there.
fn rendered_root(unit: &Transform, local: Vec3, rest_y: f32) -> Vec3 {
    unit.translation + unit.rotation * (local - Vec3::Y * rest_y)
}

/// A unit's chest above a rendered root, a combatant's or a pet's.
fn chest_above(root: Vec3, pet: bool) -> Vec3 {
    if pet {
        root + Vec3::Y * (PET_BODY_Y + (BENCH_CHEST - COMBATANT_Y) * PET_STATURE)
    } else {
        root + Vec3::Y * (BENCH_CHEST - COMBATANT_Y)
    }
}

/// The depth along `forward` of the nearest point of the body standing at a
/// rendered `root`: a combatant's upright capsule, or the sphere bounding a
/// pet's (`Capsule3d::new(0.35, 0.6)` centred 0.45 below its transform, laid
/// along its facing).
fn nearest_depth(root: Vec3, pet: bool, eye: Vec3, forward: Vec3) -> f32 {
    if pet {
        (root + Vec3::Y * PET_BODY_Y - eye).dot(forward) - (0.35 + 0.3)
    } else {
        body_nearest_depth(root, eye, forward)
    }
}

/// The implosion stands at the chest of its victim's RENDERED body — the
/// `VisualBody` child, wherever the walk bob, fear bob or swing step has
/// carried it — and every mote is held in front of that body, on its true
/// point's view ray. Checked on a combatant and on a pet (whose body rests
/// 0.45 below its transform), still, then after the victim runs four yards
/// across the view and its body bobs, so the anchor and the body the motes
/// are held against are both read afresh each frame.
#[test]
fn the_implosion_rides_the_rendered_body_and_is_held_in_front_of_it() {
    let mut held = 0;
    let mut checked = 0;
    for pet in [false, true] {
        let mut h = Harness::new();
        let s = scene(&mut h);
        let rest_y = if pet { PET_BODY_Y } else { 0.0 };
        let at = Vec3::new(2.0, COMBATANT_Y, -3.0);
        let facing = Quat::from_rotation_y(0.7);
        let mut unit = h.world().spawn((
            Combatant::new(2, 2, CharacterClass::Hunter),
            Transform::from_translation(at).with_rotation(facing),
        ));
        if pet {
            unit.insert(Pet {
                owner: s.enemy,
                pet_type: PetType::Spider,
            });
        }
        let victim = unit
            .with_child((
                VisualBody { rest_y },
                Transform::from_translation(Vec3::new(0.3, rest_y + 0.25, -0.2)),
            ))
            .id();
        let cam = match_camera(at, 60.0, 38.7f32.to_radians(), 0.4);
        h.camera(cam);
        h.cast(s.priest, victim, AbilityType::VampiricEmbrace);
        h.tick(12);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        for step in 0..2 {
            if step == 1 {
                // Run four yards across the view, and bob the body.
                let right = cam.right().as_vec3().with_y(0.0).normalize();
                h.world().get_mut::<Transform>(victim).unwrap().translation = at + right * 4.0;
                let body = h.app.world().get::<Children>(victim).unwrap()[0];
                h.world().get_mut::<Transform>(body).unwrap().translation =
                    Vec3::new(-0.25, rest_y + 0.1, 0.35);
                h.tick(1);
            }
            let world = h.app.world();
            let unit = *world.get::<Transform>(victim).unwrap();
            let body = world.get::<Children>(victim).unwrap()[0];
            let local = world.get::<Transform>(body).unwrap().translation;
            let root = rendered_root(&unit, local, rest_y);
            let chest = chest_above(root, pet);
            let rigs = h.implosions();
            assert_eq!(rigs.len(), 1);
            assert!(
                (rigs[0].3 - chest).length() < 1e-4,
                "pet {pet}, step {step}: the rig at {} is not the rendered chest {chest}",
                rigs[0].3
            );
            let stature = if pet { PET_STATURE } else { 1.0 };
            let nearest = nearest_depth(root, pet, eye, forward);
            let world = h.app.world_mut();
            let mut q = world.query::<(&VampiricEmbraceMote, &GlobalTransform)>();
            let motes: Vec<(Vec3, f32, f32, GlobalTransform)> = q
                .iter(world)
                .map(|(m, g)| (m.offset, m.age, m.life, *g))
                .collect();
            assert!(motes.len() > 5, "{} motes", motes.len());
            for (offset, age, life, g) in motes {
                let g = g.compute_transform();
                let truly = chest + offset;
                let depth = (g.translation - eye).dot(forward);
                assert!(
                    depth < nearest,
                    "pet {pet}, step {step}: buried, {depth} behind {nearest}"
                );
                assert!(
                    (g.translation - eye)
                        .normalize()
                        .dot((truly - eye).normalize())
                        > 0.999_999,
                    "pet {pet}, step {step}: off its true view ray"
                );
                let k = depth / (truly - eye).dot(forward);
                let r = bench_keys(&implode::SCALE, age / life) * stature;
                assert!((g.scale.x - 2.0 * r * k).abs() < 1e-4, "screen size");
                if k < 0.999 {
                    assert!(
                        depth > nearest - 0.1,
                        "pet {pet}: held {} in front of the body, farther than it must be",
                        nearest - depth
                    );
                    held += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 60, "{checked} motes checked");
    assert!(held > 20, "{held} needed holding");
}

/// The throttle's edge, to the millisecond: a heal 2.990 s or 2.998 s after
/// the stream last played on an ally is still throttled, and one 2.9995 s
/// after — past 2.999, the bench's `ms - lastHeal >= every*1000 - 1` — plays.
#[test]
fn the_stream_throttle_turns_over_at_three_seconds_to_the_millisecond() {
    for (gap, plays) in [(2.990_f32, false), (2.998, false), (2.9995, true)] {
        let mut h = Harness::new();
        h.world()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(10));
        let s = scene(&mut h);
        h.cast(s.priest, s.enemy, AbilityType::VampiricEmbrace);
        h.cast(s.priest, s.enemy, AbilityType::MindBlast);
        assert_eq!(
            h.streams().iter().filter(|(e, _)| *e == s.ally).count(),
            1,
            "the first heal plays"
        );
        // One frame, `gap` long, in which the second heal lands.
        h.world()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                gap,
            )));
        h.world().entity_mut(s.priest).insert(CastingState::new(
            AbilityType::MindBlast,
            s.enemy,
            0.0,
        ));
        h.tick(1);
        assert!(
            h.app.world().get::<CastingState>(s.priest).is_none(),
            "landed"
        );
        let delta = h.app.world().resource::<Time<Virtual>>().delta_secs();
        assert!((delta - gap).abs() < 1e-6, "a {delta} s frame");
        assert_eq!(
            h.streams().iter().filter(|(e, _)| *e == s.ally).count(),
            1 + plays as usize,
            "a heal {gap} s after the last stream"
        );
    }
}
