//! Probes for the Destruction casts' landings (AS-348): Shadowburn plays Mind
//! Blast's head landing, and Conflagrate detonates
//! `fire_impactdd_high_chest.m2` at the victim's chest
//! (`rendering/effects/conflagrate.rs`), flinches it, and the Immolate it
//! consumes stops burning on the frame it lands.
//!
//! Everything here asserts what is DRAWN — world positions under propagated
//! `GlobalTransform`s, the materials' colours, alphas and blend modes, the
//! sprites' scales — against the signed-off AS-334 Warlock Talent Visuals
//! Bench (version 1792203924-603a, "Conflagrate" tab), transcribed below
//! straight from `docs/design/benches/2026-10-17-warlock-talent-bench.html`
//! and NOT from the module's tables, so a wrong value in the module cannot
//! agree with itself here.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the
//! chains `states/mod.rs` registers, and the real `process_casting` where the
//! wiring from the sim is the claim.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, CastingState, ClientParticle, Combatant,
    ConflagrateLanding, GameRng, HitFlinch, ImpactAnchor, MatchCountdown, Pet, PetType,
    PlayMatchEntity, PolymorphedVisual, SchoolImpact,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{
    animate_client_particles, animate_conflagrate_bursts, animate_conflagrate_particles,
    animate_immolate_burns, animate_school_impacts, billboard_conflagrate_particles,
    cleanup_immolate_burns, cleanup_play_match, client_landing_style, landing_style,
    pose_conflagrate_rings, spawn_conflagrate_landings, spawn_immolate_burns, spawn_school_impacts,
    ConflagrateBurst, ConflagrateParticle, ConflagrateParticlePart, ConflagrateRingPart,
    ConflagrateShockPiece, ImmolateBurnRig, CONFLAGRATE_EMITTERS, FLINCH_CRIT_MULT, FLINCH_DIP,
    IMPACT_HEAD_Y, IMPACT_PET_STATURE,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;
/// The bench's chest anchor above the floor (`ANCH.chest`).
const BENCH_CHEST: f32 = 1.45;

// ── The bench, transcribed ───────────────────────────────────────────────────

/// One of the bench's Conflagrate particle layers, as its `layers` entry
/// writes it (`win` shifted by the bench's 1000 ms landing).
struct BenchLayer {
    /// The module's emitter it must be (identification only).
    name: &'static str,
    /// `Emitter.draw`: `'lighter'` unless the layer says `blend:'alpha'`.
    additive: bool,
    /// `vRange`: `None` for `>= 3.1` (the whole sphere).
    cone: Option<f32>,
    speed: f32,
    /// The bench's `gravity`: negative rises.
    gravity: f32,
    life: f32,
    rate: f32,
    win: (f32, f32),
    area: f32,
    color: &'static [(f32, [f32; 3])],
    alpha: &'static [(f32, f32)],
    scale: &'static [(f32, f32)],
    tail: f32,
    spin: f32,
}

const BENCH: [BenchLayer; 5] = [
    // {id:'p3', ... speed:2.78, spin:-0.3, life:1.0, rate:125, win:[[0,0],[1000,1],[1333,0]], kind:'star'}
    BenchLayer {
        name: "fire_impactdd_high_chest/P3 spark spray",
        additive: true,
        cone: None,
        speed: 2.78,
        gravity: 0.0,
        life: 1.0,
        rate: 125.0,
        win: (0.0, 333.0),
        area: 0.0,
        color: &[
            (0.0, [255.0, 209.0, 0.0]),
            (0.5, [255.0, 159.0, 35.0]),
            (1.0, [101.0, 0.0, 0.0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.07), (1.0, 0.03)],
        tail: 0.0,
        spin: -0.3,
    },
    // {id:'p45', ... speed:4.1, life:0.7, rate:200, win:[[0,0],[1000,1],[1167,0]], tail:3, kind:'glow'}
    BenchLayer {
        name: "fire_impactdd_high_chest/P4+P5 plume jets",
        additive: true,
        cone: None,
        speed: 4.1,
        gravity: 0.0,
        life: 0.7,
        rate: 200.0,
        win: (0.0, 167.0),
        area: 0.0,
        color: &[
            (0.0, [78.0, 0.0, 0.0]),
            (0.5, [221.0, 180.0, 120.0]),
            (1.0, [213.0, 95.0, 0.0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.1), (1.0, 0.03)],
        tail: 3.0,
        spin: 0.0,
    },
    // {id:'p0', ... speed:0.33, gravity:-0.56, life:1.2, rate:50, win:[[0,0],[1000,1],[1833,0]], kind:'cloud'}
    BenchLayer {
        name: "fire_impactdd_high_chest/P0 glow puffs",
        additive: true,
        cone: None,
        speed: 0.33,
        gravity: -0.56,
        life: 1.2,
        rate: 50.0,
        win: (0.0, 833.0),
        area: 0.0,
        color: &[
            (0.0, [255.0, 222.0, 0.0]),
            (0.5, [206.0, 129.0, 47.0]),
            (1.0, [91.0, 91.0, 91.0]),
        ],
        alpha: &[(0.0, 0.8), (1.0, 0.0)],
        scale: &[(0.0, 0.22), (0.5, 0.22), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
    // {id:'p2', ... speed:0.28, gravity:-0.56, life:1.25, rate:30, win:[[0,0],[1000,1],[1667,0]], blend:'alpha', kind:'cloud'}
    BenchLayer {
        name: "fire_impactdd_high_chest/P2 flame puffs",
        additive: false,
        cone: None,
        speed: 0.28,
        gravity: -0.56,
        life: 1.25,
        rate: 30.0,
        win: (0.0, 667.0),
        area: 0.0,
        color: &[
            (0.0, [255.0, 222.0, 0.0]),
            (0.5, [236.0, 148.0, 54.0]),
            (1.0, [120.0, 120.0, 120.0]),
        ],
        alpha: &[(0.0, 0.9), (1.0, 0.0)],
        scale: &[(0.0, 0.28), (0.5, 0.28), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
    // {id:'p7', vRange:1.2, speed:0.56, life:2.0, rate:20, area:0.56, win:[[0,0],[1000,1],[1667,0]], blend:'alpha', kind:'smoke'}
    BenchLayer {
        name: "fire_impactdd_high_chest/P7 pale smoke",
        additive: false,
        cone: Some(1.2),
        speed: 0.56,
        gravity: 0.0,
        life: 2.0,
        rate: 20.0,
        win: (0.0, 667.0),
        area: 0.56,
        color: &[(0.0, [235.0, 220.0, 174.0]), (1.0, [245.0, 245.0, 240.0])],
        alpha: &[(0.0, 0.6), (1.0, 0.0)],
        scale: &[(0.0, 0.33), (0.5, 0.44), (1.0, 0.03)],
        tail: 0.0,
        spin: 0.0,
    },
];

/// The rings' `customDraw`: drawn for 120 <= t <= 1300 ms.
const RINGS_FROM: f32 = 120.0;
const RINGS_TO: f32 = 1300.0;
const FIRE_R: [(f32, f32); 4] = [(120.0, 0.0), (167.0, 0.03), (400.0, 0.83), (900.0, 0.97)];
const FIRE_A: [(f32, f32); 4] = [(120.0, 0.0), (167.0, 1.0), (600.0, 0.6), (900.0, 0.0)];
const SHOCK_R: [(f32, f32); 4] = [(120.0, 0.0), (133.0, 0.03), (600.0, 1.39), (1200.0, 2.22)];
const SHOCK_A: [(f32, f32); 4] = [(120.0, 0.0), (133.0, 0.9), (700.0, 0.5), (1200.0, 0.0)];
/// `groundDisc(c, 1.45, fire, [245,150,0], fa*0.6)`.
const FIRE_RGB: [f32; 3] = [245.0, 150.0, 0.0];
/// `runeRing(c, 1.45, shock, [255,120,0], sa)`.
const SHOCK_RGB: [f32; 3] = [255.0, 120.0, 0.0];
/// The bench's 2 px stroke at the match camera (60 yd, FOV 45°, 1080p).
const SHOCK_LINE: f32 = 0.092;

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

fn bench_layer(name: &str) -> &'static BenchLayer {
    BENCH
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no bench layer {name}"))
}

/// The bench layer a drawn particle belongs to.
fn layer_of(p: &ConflagrateParticle) -> &'static BenchLayer {
    bench_layer(CONFLAGRATE_EMITTERS[p.emitter].name)
}

fn srgb255(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
}

fn assert_rgb(c: Color, want: [f32; 3], what: &str) {
    let got = srgb255(c);
    for k in 0..3 {
        assert!(
            (got[k] - want[k]).abs() < 0.01,
            "{what}: drawn {got:?}, bench {want:?}"
        );
    }
}

/// A drawn material against a bench colour, alpha and blend.
fn assert_drawn(m: &StandardMaterial, additive: bool, rgb: [f32; 3], alpha: f32, what: &str) {
    assert_rgb(m.base_color, rgb, what);
    assert!(
        (m.base_color.alpha() - alpha).abs() < 1e-4,
        "{what}: alpha {} against the bench's {alpha}",
        m.base_color.alpha()
    );
    if additive {
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
    } else {
        assert_eq!(
            m.alpha_mode,
            AlphaMode::Blend,
            "{what}: source-over is blended"
        );
        assert!(m.unlit, "{what}: a blended piece is a flat colour");
    }
}

// ── The harness ──────────────────────────────────────────────────────────────

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
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
            .insert_resource(MatchCountdown {
                time_remaining: 0.0,
                gates_opened: true,
            })
            .insert_resource(CombatLog::default())
            .insert_resource(GameRng::from_seed(0))
            .insert_resource(AbilityDefinitions::default())
            .insert_resource(ArenaDampening::default())
            .insert_resource(ActiveMapGeometry {
                bounds: Default::default(),
                volumes: Vec::new(),
                cover_anchors: Vec::new(),
            });
        // The sim's landing, then the chains `states/mod.rs` registers.
        app.add_systems(Update, process_casting);
        app.add_systems(
            Update,
            (
                spawn_conflagrate_landings,
                animate_conflagrate_bursts,
                pose_conflagrate_rings,
                animate_conflagrate_particles,
                billboard_conflagrate_particles,
            )
                .chain()
                .after(process_casting),
        );
        app.add_systems(
            Update,
            (
                spawn_immolate_burns,
                animate_immolate_burns,
                cleanup_immolate_burns,
            )
                .chain()
                .after(process_casting),
        );
        app.add_systems(
            Update,
            (
                spawn_school_impacts,
                animate_school_impacts,
                animate_client_particles,
            )
                .chain()
                .after(process_casting),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn victim(&mut self, at: Vec3) -> Entity {
        self.world()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn pet_victim(&mut self, at: Vec3) -> Entity {
        let owner = self.victim(at + Vec3::X * 5.0);
        self.world()
            .spawn((
                Combatant::new(2, 1, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Spider,
                },
                Transform::from_translation(at),
            ))
            .id()
    }

    fn land(&mut self, target: Entity, is_crit: bool) {
        self.world()
            .spawn((ConflagrateLanding { target, is_crit }, PlayMatchEntity));
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, f32, Vec3)> {
        let mut q = self
            .world()
            .query::<(Entity, &ConflagrateBurst, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, r, g)| (e, r.target, r.age, g.translation()))
            .collect()
    }

    fn rig_age(&mut self) -> f32 {
        let rigs = self.rigs();
        assert_eq!(rigs.len(), 1, "one landing, one rig");
        rigs[0].2
    }

    fn particles(&mut self) -> Vec<(Entity, ParticleView)> {
        let mut q = self
            .world()
            .query::<(Entity, &ConflagrateParticle, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, p, g)| {
                (
                    e,
                    ParticleView {
                        layer: layer_of(p),
                        age: p.age,
                        life: p.life,
                        velocity: p.velocity,
                        at: g.translation(),
                    },
                )
            })
            .collect()
    }

    /// `(part, world transform, local scale, material)` of `particle`'s pieces.
    fn parts_of(
        &mut self,
        particle: Entity,
    ) -> Vec<(
        ConflagrateParticlePart,
        GlobalTransform,
        Vec3,
        StandardMaterial,
    )> {
        let children: Vec<Entity> = self
            .app
            .world()
            .get::<Children>(particle)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        children
            .into_iter()
            .map(|c| {
                let part = *world.get::<ConflagrateParticlePart>(c).expect("a part");
                let g = *world.get::<GlobalTransform>(c).unwrap();
                let s = world.get::<Transform>(c).unwrap().scale;
                let m = materials
                    .get(&world.get::<MeshMaterial3d<StandardMaterial>>(c).unwrap().0)
                    .unwrap()
                    .clone();
                (part, g, s, m)
            })
            .collect()
    }

    fn tagged(&self, e: Entity) -> bool {
        self.app.world().get::<PlayMatchEntity>(e).is_some()
    }
}

#[derive(Clone, Copy)]
struct ParticleView {
    layer: &'static BenchLayer,
    age: f32,
    life: f32,
    velocity: Vec3,
    at: Vec3,
}

fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * (BENCH_CHEST - COMBATANT_Y)
}

/// One particle's birth, read the frame it first appears (aged one frame).
struct Birth {
    layer: &'static BenchLayer,
    /// The rig's age on its birth frame, ms after the landing.
    ms: f32,
    /// Where it was born and how it launched, undone from one frame's flight.
    born: Vec3,
    launch: Vec3,
}

/// Land one burst on a still victim and record every birth until `until_ms`.
fn births(until_ms: f32) -> (Vec<Birth>, Vec3) {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, COMBATANT_Y, -2.0);
    let victim = h.victim(at);
    h.land(victim, false);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    loop {
        h.tick(1);
        let Some(&(_, _, age, _)) = h.rigs().first() else {
            break;
        };
        let ms = age * 1000.0;
        if ms > until_ms {
            break;
        }
        for (e, p) in h.particles() {
            if !seen.insert(e) {
                continue;
            }
            assert!(h.tagged(e), "a particle is a PlayMatchEntity");
            assert!((p.age - DT).abs() < 1e-5, "first seen one frame old");
            assert!(
                (p.life - p.layer.life).abs() < 1e-6,
                "{}: lives {} s",
                p.layer.name,
                p.life
            );
            let g = p.layer.gravity;
            // One frame of flight: v1 = v0 - g*dt (y), x1 = x0 + v1*dt.
            let launch = p.velocity + Vec3::Y * g * DT;
            out.push(Birth {
                layer: p.layer,
                ms,
                born: p.at - p.velocity * DT,
                launch,
            });
        }
    }
    (out, at)
}

// ── Shadowburn ───────────────────────────────────────────────────────────────

/// Shadowburn's visual is Mind Blast's (3057): its landing routes to the very
/// table Mind Blast plays, on the head, and never to its Shadow school row.
#[test]
fn shadowburn_routes_to_mind_blasts_head_landing() {
    assert_eq!(
        SchoolImpact::anchor_for(AbilityType::Shadowburn),
        Some(ImpactAnchor::Head)
    );
    assert_eq!(
        SchoolImpact::anchor_for(AbilityType::Shadowburn),
        SchoolImpact::anchor_for(AbilityType::MindBlast)
    );
    let burn = client_landing_style(AbilityType::Shadowburn).expect("a client landing");
    let blast = client_landing_style(AbilityType::MindBlast).expect("Mind Blast's");
    assert!(std::ptr::eq(burn.emitters, blast.emitters));
    assert_eq!(
        landing_style(AbilityType::Shadowburn, SpellSchool::Shadow),
        landing_style(AbilityType::MindBlast, SpellSchool::Shadow)
    );
    assert!(!burn.emitters.is_empty());
}

/// What a landing of `ability` draws on a victim, frame by frame: every
/// particle's world position and colour.
fn school_landing_draws(ability: AbilityType) -> Vec<Vec<([i64; 3], [i64; 4])>> {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(-4.0, COMBATANT_Y, 6.0));
    h.world().spawn((
        SchoolImpact {
            target: victim,
            ability,
            school: SpellSchool::Shadow,
            anchor: SchoolImpact::anchor_for(ability).expect("an anchor"),
            from: Vec3::X,
            magnitude: 0.1,
            is_crit: false,
            age: 0.0,
        },
        PlayMatchEntity,
    ));
    let mut frames = Vec::new();
    for _ in 0..90 {
        h.tick(1);
        let world = h.app.world_mut();
        let mut q = world.query::<(
            &ClientParticle,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let materials = world.resource::<Assets<StandardMaterial>>();
        let mut seen: Vec<([i64; 3], [i64; 4])> = q
            .iter(world)
            .filter(|(p, _, _)| p.age < p.life)
            .map(|(_, g, m)| {
                let t = g.translation();
                let c = materials.get(&m.0).unwrap().base_color.to_srgba();
                let q = |v: f32| (v * 1e4).round() as i64;
                (
                    [q(t.x), q(t.y), q(t.z)],
                    [q(c.red), q(c.green), q(c.blue), q(c.alpha)],
                )
            })
            .collect();
        seen.sort();
        frames.push(seen);
    }
    frames
}

/// Drawn, Shadowburn's landing IS Mind Blast's: the same particles in the same
/// places in the same colours, every frame for 1.4 s — and it is not empty,
/// and it plays at the head (violet flame licks rising from above the chest).
#[test]
fn shadowburn_draws_exactly_what_mind_blast_draws_at_the_head() {
    let burn = school_landing_draws(AbilityType::Shadowburn);
    let blast = school_landing_draws(AbilityType::MindBlast);
    assert_eq!(burn, blast);
    let drawn: usize = burn.iter().map(|f| f.len()).sum();
    assert!(drawn > 500, "{drawn} particle-frames");
    // Born at the head anchor: the first flame licks stand above the chest.
    let first = burn.iter().find(|f| !f.is_empty()).expect("a particle");
    for (at, _) in first {
        let y = at[1] as f32 / 1e4;
        assert!(
            y > COMBATANT_Y + IMPACT_HEAD_Y - 0.2,
            "{y}: a Shadowburn landing is on the head"
        );
    }
}

/// The real `process_casting` lands Shadowburn through the shared impact at
/// the head, and Conflagrate through its own burst only (never ALSO its Fire
/// school row).
#[test]
fn the_sim_lands_shadowburn_on_the_head_and_conflagrate_as_its_own_burst() {
    for (ability, impact, burst) in [
        (AbilityType::Shadowburn, Some(ImpactAnchor::Head), 0),
        (AbilityType::Conflagrate, None, 1),
    ] {
        let mut h = Harness::new();
        let warlock = warlock(&mut h, Vec3::new(-8.0, COMBATANT_Y, 0.0), ability);
        let victim = h.victim(Vec3::new(4.0, COMBATANT_Y, 0.0));
        cast(&mut h, warlock, victim, ability);
        let mut q = h.world().query::<&SchoolImpact>();
        let impacts: Vec<(Entity, AbilityType, ImpactAnchor)> = q
            .iter(h.app.world())
            .map(|i| (i.target, i.ability, i.anchor))
            .collect();
        assert_eq!(
            impacts,
            impact
                .map(|a| vec![(victim, ability, a)])
                .unwrap_or_default(),
            "{ability:?}"
        );
        let rigs = h.rigs();
        assert_eq!(rigs.len(), burst, "{ability:?}");
        for (_, target, _, at) in rigs {
            assert_eq!(target, victim, "the burst is the victim's");
            assert!(
                (at - chest(Vec3::new(4.0, COMBATANT_Y, 0.0))).length() < 1e-5,
                "at the victim's chest: {at}"
            );
        }
    }
}

/// A Conflagrate that crits through the real `process_casting` flinches its
/// victim at the crit depth; its control, one that cannot crit, at the plain
/// depth. The hit's crit reaches the landing from the sim.
#[test]
fn a_critting_conflagrate_flinches_deeper_through_the_sim() {
    for (crit_chance, crits, depth) in [
        (0.0, false, FLINCH_DIP),
        (1.0, true, FLINCH_DIP * FLINCH_CRIT_MULT),
    ] {
        let mut h = Harness::new();
        let caster = warlock(
            &mut h,
            Vec3::new(-8.0, COMBATANT_Y, 0.0),
            AbilityType::Conflagrate,
        );
        h.world().get_mut::<Combatant>(caster).unwrap().crit_chance = crit_chance;
        let victim = h.victim(Vec3::new(4.0, COMBATANT_Y, 0.0));
        cast(&mut h, caster, victim, AbilityType::Conflagrate);
        let log = h.app.world().resource::<CombatLog>();
        assert_eq!(
            log.entries
                .iter()
                .any(|e| e.message.contains("Conflagrate CRITS")),
            crits,
            "crit chance {crit_chance}"
        );
        let flinch = h.app.world().get::<HitFlinch>(victim).expect("a flinch");
        assert!(
            (flinch.depth - depth).abs() < 1e-6,
            "crit chance {crit_chance}: dipped {}",
            flinch.depth
        );
        assert!(h.app.world().get::<HitFlinch>(caster).is_none());
    }
}

/// Every piece is drawn on the unit quad its scale is measured against: a
/// particle's sprite and streak, the fire disc and a rune-ring piece each
/// span ±0.5 in their own x and y, flat in z.
#[test]
fn every_piece_is_drawn_on_a_unit_quad() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::ZERO.with_y(COMBATANT_Y));
    h.land(victim, false);
    h.tick(16);
    let world = h.app.world_mut();
    let mut q = world.query_filtered::<(
        &Mesh3d,
        Has<ConflagrateParticlePart>,
        Option<&ConflagrateRingPart>,
        Has<ConflagrateShockPiece>,
    ), With<Mesh3d>>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut kinds = HashSet::new();
    for (mesh, particle, ring, piece) in q.iter(world) {
        let kind = match (particle, ring, piece) {
            (true, _, _) => "particle part",
            (_, Some(_), _) => "fire disc",
            (_, _, true) => "ring piece",
            _ => continue,
        };
        kinds.insert(kind);
        let mesh = meshes.get(&mesh.0).expect("the mesh");
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(ps)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("{kind}: no positions");
        };
        assert!(!ps.is_empty());
        for p in ps {
            assert!(
                (p[0].abs() - 0.5).abs() < 1e-6 && (p[1].abs() - 0.5).abs() < 1e-6 && p[2] == 0.0,
                "{kind}: a vertex at {p:?}"
            );
        }
    }
    assert_eq!(kinds.len(), 3, "{kinds:?}");
}

/// With the match camera on it, every sprite faces the camera — its face
/// turned to the camera's back vector — rolled in the view plane by its own
/// angle, which the sparks spin.
#[test]
fn every_sprite_faces_the_match_camera() {
    let target = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let mut h = Harness::new();
    let victim = h.victim(target);
    for yaw in [0.4_f32] {
        let cam = match_camera(target, 60.0, 38.7f32.to_radians(), yaw);
        h.world()
            .spawn((Camera3d::default(), cam, GlobalTransform::from(cam)));
        h.land(victim, false);
        let mut sprites = 0;
        for _ in 0..30 {
            h.tick(1);
            for (e, _) in h.particles() {
                let angle = h.app.world().get::<ConflagrateParticle>(e).unwrap().angle;
                for (part, g, _, _) in h.parts_of(e) {
                    if part != ConflagrateParticlePart::Sprite {
                        continue;
                    }
                    let r = g.rotation();
                    assert!(
                        (r * Vec3::Z - cam.back().as_vec3()).length() < 1e-4,
                        "a sprite faces {} not the camera",
                        r * Vec3::Z
                    );
                    let x = cam.rotation.inverse() * (r * Vec3::X);
                    let roll = x.y.atan2(x.x);
                    let want =
                        (angle + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
                    let mut d = roll - want;
                    if d > std::f32::consts::PI {
                        d -= TAU;
                    } else if d < -std::f32::consts::PI {
                        d += TAU;
                    }
                    assert!(d.abs() < 1e-3, "rolled {roll}, its angle {want}");
                    sprites += 1;
                }
            }
        }
        assert!(sprites > 500, "{sprites}");
    }
}

// ── Conflagrate: the sim's landing and the Immolate it eats ──────────────────

/// A Warlock with `ability` in its book and mana to cast it.
fn warlock(h: &mut Harness, at: Vec3, ability: AbilityType) -> Entity {
    let defs = AbilityDefinitions::default();
    let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
    let book = c.book().clone().granting(&defs, ability);
    c = c.with_book(book);
    c.max_mana = 1000.0;
    c.current_mana = 1000.0;
    h.world().spawn((c, Transform::from_translation(at))).id()
}

fn immolate_by(caster: Entity, victim: Entity) -> Aura {
    let defs = AbilityDefinitions::default();
    AuraPending::from_ability(victim, caster, defs.get_unchecked(&AbilityType::Immolate))
        .expect("Immolate applies an aura")
        .aura
}

/// Start a zero-length cast and run frames until it has landed (at most five).
fn cast(h: &mut Harness, caster: Entity, target: Entity, ability: AbilityType) {
    h.world()
        .entity_mut(caster)
        .insert(CastingState::new(ability, target, 0.0));
    for _ in 0..5 {
        h.tick(1);
        if h.app.world().get::<CastingState>(caster).is_none() {
            return;
        }
    }
    panic!("{ability:?} never landed");
}

fn burns_on(h: &mut Harness, victim: Entity) -> usize {
    let mut q = h.world().query::<&ImmolateBurnRig>();
    q.iter(h.app.world()).filter(|r| r.target == victim).count()
}

fn immolates_on(h: &mut Harness, victim: Entity) -> usize {
    h.app
        .world()
        .get::<ActiveAuras>(victim)
        .map(|a| {
            a.auras
                .iter()
                .filter(|a| a.ability_name == "Immolate")
                .count()
        })
        .unwrap_or(0)
}

/// The Immolate burn switches off on the very frame the Conflagrate burst
/// lands — the one rendered frame both are decided on — and not before. Its
/// control: a teammate's Immolate on the same victim is not eaten, so its burn
/// stays up beside the burst.
#[test]
fn the_immolate_burn_goes_out_on_the_frame_the_burst_lands() {
    for teammate_burns in [false, true] {
        let mut h = Harness::new();
        let caster = warlock(
            &mut h,
            Vec3::new(-8.0, COMBATANT_Y, 0.0),
            AbilityType::Conflagrate,
        );
        let other = h
            .world()
            .spawn((
                Combatant::new(1, 1, CharacterClass::Warlock),
                Transform::from_xyz(-8.0, COMBATANT_Y, 4.0),
            ))
            .id();
        let at = Vec3::new(4.0, COMBATANT_Y, 0.0);
        let victim = h.victim(at);
        let mut auras = vec![immolate_by(caster, victim)];
        if teammate_burns {
            auras.push(immolate_by(other, victim));
        }
        h.world().entity_mut(victim).insert(ActiveAuras { auras });
        h.tick(2);
        assert_eq!(burns_on(&mut h, victim), 1, "the burn is up first");
        assert!(h.rigs().is_empty());

        h.world().entity_mut(caster).insert(CastingState::new(
            AbilityType::Conflagrate,
            victim,
            0.0,
        ));
        let mut landed = false;
        for _ in 0..5 {
            h.tick(1);
            let rigs = h.rigs().len();
            if rigs == 0 {
                // Not landed yet: the burn is still up.
                assert_eq!(burns_on(&mut h, victim), 1, "no burst, no burn change");
                continue;
            }
            assert_eq!(rigs, 1);
            assert_eq!(h.rig_age(), DT, "the burst's first frame");
            let (left, burns) = if teammate_burns { (1, 1) } else { (0, 0) };
            assert_eq!(immolates_on(&mut h, victim), left);
            assert_eq!(
                burns_on(&mut h, victim),
                burns,
                "teammate's Immolate: {teammate_burns}"
            );
            landed = true;
            break;
        }
        assert!(landed, "the Conflagrate landed");
        let log = h.app.world().resource::<CombatLog>();
        assert!(log
            .entries
            .iter()
            .any(|e| e.message.contains("Conflagrate consumes Immolate")));
    }
}

// ── Conflagrate: what the burst draws ────────────────────────────────────────

/// Every layer emits only inside its window, from its first frame to its
/// last, at its rate — births FILL the window — and nothing after.
#[test]
fn each_layer_emits_its_window_at_its_rate() {
    let (births, _) = births(1400.0);
    for layer in &BENCH {
        let ms: Vec<f32> = births
            .iter()
            .filter(|b| std::ptr::eq(b.layer, layer))
            .map(|b| b.ms)
            .collect();
        assert!(!ms.is_empty(), "{}: emits", layer.name);
        let first = ms.iter().cloned().fold(f32::MAX, f32::min);
        let last = ms.iter().cloned().fold(0.0, f32::max);
        let gap = 1000.0 / layer.rate + DT * 1000.0;
        assert!(first >= layer.win.0, "{}: {first}", layer.name);
        assert!(
            first <= layer.win.0 + gap,
            "{}: opens at {first}",
            layer.name
        );
        assert!(
            last < layer.win.1,
            "{}: still emitting at {last}",
            layer.name
        );
        assert!(
            last >= layer.win.1 - gap,
            "{}: stops early, at {last}",
            layer.name
        );
        let want = layer.rate * (layer.win.1 - layer.win.0) / 1000.0;
        let slack = layer.rate * DT + 1.0;
        assert!(
            (ms.len() as f32 - want).abs() <= slack,
            "{}: {} births against {want}",
            layer.name,
            ms.len()
        );
    }
    // Five layers and nothing else.
    let names: HashSet<&str> = births.iter().map(|b| b.layer.name).collect();
    assert_eq!(names.len(), 5);
}

/// Each layer launches at its speed from the chest: the sphere layers every
/// way out of it, the smoke in its 1.2 rad cone up from a 0.56 yd square.
/// Births fill their spreads rather than merely staying inside them.
#[test]
fn each_layer_launches_at_its_speed_in_its_spread() {
    let (births, at) = births(1400.0);
    let c = chest(at);
    for layer in &BENCH {
        let mine: Vec<&Birth> = births
            .iter()
            .filter(|b| std::ptr::eq(b.layer, layer))
            .collect();
        for b in &mine {
            assert!(
                (b.launch.length() - layer.speed).abs() < 1e-3,
                "{}: launched at {} yd/s",
                layer.name,
                b.launch.length()
            );
            let off = b.born - c;
            assert!(
                off.y.abs() < 1e-3,
                "{}: born at the chest's height",
                layer.name
            );
            let half = layer.area / 2.0 + 1e-3;
            assert!(
                off.x.abs() <= half && off.z.abs() <= half,
                "{}: born {off} off the chest",
                layer.name
            );
        }
        let dirs: Vec<Vec3> = mine.iter().map(|b| b.launch.normalize()).collect();
        match layer.cone {
            None => {
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    let hi = dirs.iter().map(|d| d.dot(axis)).fold(-1.0, f32::max);
                    let lo = dirs.iter().map(|d| d.dot(axis)).fold(1.0, f32::min);
                    assert!(
                        hi > 0.6 && lo < -0.6,
                        "{}: {axis} spread {lo}..{hi}",
                        layer.name
                    );
                }
            }
            Some(half_angle) => {
                let polar: Vec<f32> = dirs.iter().map(|d| d.y.clamp(-1.0, 1.0).acos()).collect();
                let hi = polar.iter().cloned().fold(0.0, f32::max);
                let lo = polar.iter().cloned().fold(TAU, f32::min);
                assert!(hi <= half_angle + 1e-4, "{}: {hi} off up", layer.name);
                assert!(hi >= half_angle * 0.75, "{}: cone reaches {hi}", layer.name);
                assert!(lo <= half_angle * 0.35, "{}: cone core {lo}", layer.name);
                for axis in [Vec3::X, Vec3::Z] {
                    let offs: Vec<f32> = mine.iter().map(|b| (b.born - c).dot(axis)).collect();
                    let hi = offs.iter().cloned().fold(-1.0, f32::max);
                    let lo = offs.iter().cloned().fold(1.0, f32::min);
                    let half = layer.area / 2.0;
                    assert!(
                        hi > half * 0.6 && lo < -half * 0.6,
                        "{}: the square is filled {lo}..{hi}",
                        layer.name
                    );
                }
            }
        }
    }
}

/// Watch one burst for 2.8 s: every live particle, every frame, against the
/// bench — colour, alpha, blend and sprite size along its life, and its streak
/// (the plumes' alone) trailing back along its flight.
#[test]
fn every_particle_is_drawn_as_the_bench_draws_it() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(victim, false);
    let mut checked = [0usize; 5];
    for _ in 0..(2.8 / DT) as u32 {
        h.tick(1);
        for (e, p) in h.particles() {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let l = p.layer;
            let rgb = bench_rgb(l.color, t);
            let alpha = bench_keys(l.alpha, t);
            let radius = bench_keys(l.scale, t);
            let parts = h.parts_of(e);
            let mut streaks = 0;
            for (part, g, scale, m) in &parts {
                match part {
                    ConflagrateParticlePart::Sprite => {
                        assert_drawn(m, l.additive, rgb, alpha, l.name);
                        assert!(
                            (scale.x - radius * 2.0).abs() < 1e-5,
                            "{}: drawn {} across, bench radius {radius}",
                            l.name,
                            scale.x
                        );
                        assert!((g.translation() - p.at).length() < 1e-5);
                    }
                    ConflagrateParticlePart::Streak => {
                        streaks += 1;
                        let speed = p.velocity.length();
                        let len = (l.tail * 0.12).min(speed * 0.06);
                        assert_drawn(m, l.additive, rgb, alpha * 0.85, l.name);
                        assert!((scale.x - len).abs() < 1e-5, "{}: tail {}", l.name, scale.x);
                        assert!((scale.y - radius * 0.7).abs() < 1e-5);
                        let back = (g.translation() - p.at).normalize();
                        assert!(
                            (back + p.velocity.normalize()).length() < 1e-3,
                            "{}: the tail trails the flight",
                            l.name
                        );
                        assert!(((g.translation() - p.at).length() - len / 2.0).abs() < 1e-4);
                    }
                }
            }
            assert_eq!(streaks, usize::from(l.tail > 0.0), "{}", l.name);
            let i = BENCH.iter().position(|b| std::ptr::eq(b, l)).unwrap();
            checked[i] += 1;
        }
    }
    assert!(checked.iter().all(|&n| n > 50), "{checked:?}");
}

/// Flight and spin, per layer, on fresh particles: the second difference of
/// the drawn height is the bench's gravity (rising for the two puff layers),
/// the drawn horizontal motion holds its launch, and the sparks' sprite turns
/// at -0.3 rev/s while nothing else turns.
#[test]
fn each_layer_flies_under_its_gravity_and_spins_at_its_rate() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(1.0, COMBATANT_Y, 1.0));
    h.land(victim, false);
    h.tick(8);
    // Three consecutive frames of every particle alive through them.
    let mut frames: Vec<HashMap<Entity, (Vec3, f32, &'static BenchLayer)>> = Vec::new();
    for _ in 0..3 {
        h.tick(1);
        let mut f = HashMap::new();
        for (e, p) in h.particles() {
            let sprite = h
                .parts_of(e)
                .into_iter()
                .find(|(part, ..)| *part == ConflagrateParticlePart::Sprite)
                .expect("a sprite");
            let x = sprite.1.rotation() * Vec3::X;
            f.insert(e, (p.at, x.y.atan2(x.x), p.layer));
        }
        frames.push(f);
    }
    let mut seen = [0usize; 5];
    for (e, (p0, a0, layer)) in &frames[0] {
        let (Some((p1, a1, _)), Some((p2, _, _))) = (frames[1].get(e), frames[2].get(e)) else {
            continue;
        };
        let accel = (*p2 - *p1 * 2.0 + *p0) / (DT * DT);
        assert!(
            (accel - Vec3::Y * -layer.gravity).length() < 2e-2,
            "{}: drawn acceleration {accel}",
            layer.name
        );
        let mut turn = a1 - a0;
        while turn > std::f32::consts::PI {
            turn -= TAU;
        }
        while turn < -std::f32::consts::PI {
            turn += TAU;
        }
        assert!(
            (turn - layer.spin * TAU * DT).abs() < 1e-4,
            "{}: turned {turn}",
            layer.name
        );
        let i = BENCH.iter().position(|b| std::ptr::eq(b, *layer)).unwrap();
        seen[i] += 1;
    }
    assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
}

/// The rings, every frame from the landing to 1.4 s: the fire disc and the
/// rune ring are drawn exactly when the bench draws them, at its radii and
/// alphas, flat at the chest, in their colours, additive; the ring's line is
/// a 48-gon through its radius plus eight ticks from 0.82 of it.
#[test]
fn the_fire_disc_and_the_rune_ring_are_drawn_as_the_bench_draws_them() {
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, COMBATANT_Y, 5.0);
    let victim = h.victim(at);
    h.land(victim, false);
    let c = chest(at);
    let (mut fire_frames, mut shock_frames) = (0, 0);
    for _ in 0..(1.4 / DT) as u32 {
        h.tick(1);
        let Some(&(rig, _, age, rig_at)) = h.rigs().first() else {
            break;
        };
        assert!((rig_at - c).length() < 1e-5, "the rig stands at the chest");
        let ms = age * 1000.0;
        let in_time = (RINGS_FROM..=RINGS_TO).contains(&ms);
        let (fr, fa) = (bench_keys(&FIRE_R, ms), bench_keys(&FIRE_A, ms) * 0.6);
        let (sr, sa) = (bench_keys(&SHOCK_R, ms), bench_keys(&SHOCK_A, ms));
        let fire_drawn = in_time && fa > 0.003 && fr > 0.001;
        let shock_drawn = in_time && sa > 0.003;

        let world = h.app.world_mut();
        let children: Vec<Entity> = world.get::<Children>(rig).unwrap().iter().collect();
        let mut parts = 0;
        for child in children {
            let Some(&part) = world.get::<ConflagrateRingPart>(child) else {
                continue;
            };
            parts += 1;
            let shown = *world.get::<Visibility>(child).unwrap() != Visibility::Hidden;
            match part {
                ConflagrateRingPart::FireDisc => {
                    assert_eq!(shown, fire_drawn, "fire disc at {ms} ms");
                    if !fire_drawn {
                        continue;
                    }
                    fire_frames += 1;
                    let g = world.get::<GlobalTransform>(child).unwrap();
                    let (scale, rot, t) = g.to_scale_rotation_translation();
                    assert!((t - c).length() < 1e-5);
                    assert!((rot * Vec3::Z - Vec3::Y).length() < 1e-5, "flat, facing up");
                    assert!((scale.x - fr * 2.0).abs() < 1e-5 && (scale.y - fr * 2.0).abs() < 1e-5);
                    let m = world.resource::<Assets<StandardMaterial>>();
                    let handle = &world
                        .get::<MeshMaterial3d<StandardMaterial>>(child)
                        .unwrap()
                        .0;
                    assert_drawn(m.get(handle).unwrap(), true, FIRE_RGB, fa, "fire disc");
                }
                ConflagrateRingPart::ShockRing => {
                    assert_eq!(shown, shock_drawn, "rune ring at {ms} ms");
                    if !shock_drawn {
                        continue;
                    }
                    shock_frames += 1;
                    let pieces: Vec<Entity> =
                        world.get::<Children>(child).unwrap().iter().collect();
                    assert_eq!(pieces.len(), 56, "48 sides and 8 ticks");
                    let corner = |a: f32, r: f32| c + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                    let mut segments = HashSet::new();
                    let mut ticks = HashSet::new();
                    for piece in pieces {
                        let g = world.get::<GlobalTransform>(piece).unwrap();
                        let (scale, rot, t) = g.to_scale_rotation_translation();
                        assert!((rot * Vec3::Z - Vec3::Y).length() < 1e-4, "flat");
                        assert!((scale.y - SHOCK_LINE).abs() < 1e-6, "line width");
                        let along = rot * Vec3::X;
                        let ends = [t - along * scale.x / 2.0, t + along * scale.x / 2.0];
                        let near = |p: Vec3, q: Vec3| (p - q).length() < 2e-4;
                        let joins = |a: Vec3, b: Vec3| {
                            (near(ends[0], a) && near(ends[1], b))
                                || (near(ends[0], b) && near(ends[1], a))
                        };
                        if let Some(i) = (0..48).find(|&i| {
                            joins(
                                corner(i as f32 / 48.0 * TAU, sr),
                                corner((i + 1) as f32 / 48.0 * TAU, sr),
                            )
                        }) {
                            segments.insert(i);
                        } else if let Some(k) = (0..8).find(|&k| {
                            let a = k as f32 / 8.0 * TAU;
                            joins(corner(a, sr * 0.82), corner(a, sr))
                        }) {
                            ticks.insert(k);
                        } else {
                            panic!("a piece off the ring at {ms} ms: {ends:?}");
                        }
                        let m = world.resource::<Assets<StandardMaterial>>();
                        let handle = &world
                            .get::<MeshMaterial3d<StandardMaterial>>(piece)
                            .unwrap()
                            .0;
                        assert_drawn(m.get(handle).unwrap(), true, SHOCK_RGB, sa, "rune ring");
                    }
                    assert_eq!(segments.len(), 48, "every side at {ms} ms");
                    assert_eq!(ticks.len(), 8, "every tick at {ms} ms");
                }
            }
        }
        assert_eq!(parts, 2, "a fire disc and a rune ring");
    }
    // 120-900 ms and 120-1200 ms of frames, give or take one at each end.
    assert!((45..=50).contains(&fire_frames), "{fire_frames}");
    assert!((63..=69).contains(&shock_frames), "{shock_frames}");
}

/// The burst flinches its victim — the shipped wound dip, deeper on a crit —
/// on the frame it lands.
#[test]
fn the_burst_flinches_its_victim_deeper_on_a_crit() {
    for (is_crit, depth) in [(false, FLINCH_DIP), (true, FLINCH_DIP * FLINCH_CRIT_MULT)] {
        let mut h = Harness::new();
        let victim = h.victim(Vec3::ZERO.with_y(COMBATANT_Y));
        h.tick(1);
        assert!(h.app.world().get::<HitFlinch>(victim).is_none());
        h.land(victim, is_crit);
        h.tick(1);
        let flinch = h.app.world().get::<HitFlinch>(victim).expect("a flinch");
        assert!((flinch.depth - depth).abs() < 1e-6, "crit {is_crit}");
        assert_eq!(flinch.elapsed, 0.0);
    }
}

/// The rings ride a running victim's chest; a particle, once born, does not.
#[test]
fn the_rings_ride_the_victim_and_the_particles_stay_where_they_were_born() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(victim, false);
    h.tick(10);
    let before: HashMap<Entity, ParticleView> = h.particles().into_iter().collect();
    let step = Vec3::new(1.5, 0.0, -0.75);
    h.world().get_mut::<Transform>(victim).unwrap().translation += step;
    h.tick(1);
    let rig_at = h.rigs()[0].3;
    assert!((rig_at - chest(Vec3::new(0.0, COMBATANT_Y, 0.0) + step)).length() < 1e-5);
    let mut moved = 0;
    for (e, p) in h.particles() {
        if let Some(was) = before.get(&e) {
            moved += 1;
            let flown = (p.at - was.at).length();
            assert!(
                flown < p.velocity.length() * DT + 1e-3,
                "{}: dragged {flown}",
                p.layer.name
            );
        }
    }
    assert!(moved > 20);
}

/// A pet victim's burst is at its smaller chest and scaled to its stature.
#[test]
fn a_pet_victims_burst_is_at_its_stature() {
    let mut h = Harness::new();
    let pet = h.pet_victim(Vec3::new(2.0, 0.75, 2.0));
    h.land(pet, false);
    h.tick(14);
    let (_, _, _, at) = h.rigs()[0];
    assert!(at.y < 0.75 + 0.2, "a pet's chest is low: {at}");
    for (e, p) in h.particles() {
        let sprite = h
            .parts_of(e)
            .into_iter()
            .find(|(part, ..)| *part == ConflagrateParticlePart::Sprite)
            .unwrap();
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let want = bench_keys(p.layer.scale, t) * 2.0 * IMPACT_PET_STATURE;
        assert!((sprite.2.x - want).abs() < 1e-5, "{}", p.layer.name);
        let launch = p.velocity + Vec3::Y * p.layer.gravity * IMPACT_PET_STATURE * p.age;
        assert!((launch.length() - p.layer.speed * IMPACT_PET_STATURE).abs() < 1e-3);
    }
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

/// The burst is a landing: on a victim that dies under it, it plays on (the
/// killing blow is the cast it matters most for) — the sparks keep coming to
/// their window's end and the rig retires on time, at 1.3 s.
#[test]
fn a_victim_dying_mid_burst_still_gets_its_whole_burst() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    // Other auras on it, and its Immolate burn up beside the burst.
    let immolate = immolate_by(victim, victim);
    h.world().entity_mut(victim).insert(ActiveAuras {
        auras: vec![immolate],
    });
    h.land(victim, false);
    h.tick(6);
    h.world()
        .get_mut::<Combatant>(victim)
        .unwrap()
        .current_health = 0.0;
    assert!(!h.app.world().get::<Combatant>(victim).unwrap().is_alive());
    let born_dead: usize = {
        let mut seen: HashSet<Entity> = h.particles().into_iter().map(|(e, _)| e).collect();
        let mut n = 0;
        for _ in 0..12 {
            h.tick(1);
            for (e, _) in h.particles() {
                if seen.insert(e) {
                    n += 1;
                }
            }
        }
        n
    };
    assert!(born_dead > 20, "{born_dead} born after the death");
    // Its burn goes with the death; the burst does not.
    assert_eq!(burns_on(&mut h, victim), 0);
    assert_eq!(h.rigs().len(), 1);
    h.tick(((1.3 - 18.0 * DT) / DT) as u32 + 2);
    assert!(h.rigs().is_empty(), "retired at 1.3 s");
}

/// A victim that is GONE takes the rig, rings and all, at once; the particles
/// already in the air live out their own lives and then are gone too.
#[test]
fn a_victim_gone_mid_burst_takes_the_rig_and_leaves_the_particles_to_finish() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(victim, false);
    h.tick(10);
    let alive = h.particles().len();
    assert!(alive > 20);
    h.world().despawn(victim);
    h.tick(1);
    assert!(h.rigs().is_empty());
    let mut q = h.world().query::<&ConflagrateRingPart>();
    assert_eq!(q.iter(h.app.world()).count(), 0, "the rings went with it");
    let mut q = h.world().query::<&ConflagrateShockPiece>();
    assert_eq!(q.iter(h.app.world()).count(), 0);
    assert!(h.particles().len() >= alive / 2, "the air keeps its fire");
    h.tick((2.1 / DT) as u32);
    assert!(h.particles().is_empty(), "and then it is gone");
}

/// Polymorphed mid-burst, the victim keeps its burst: a landing plays out on
/// the sheep (the empowered flash's precedent).
#[test]
fn a_polymorph_mid_burst_does_not_stop_it() {
    for sheep in [false, true] {
        let mut h = Harness::new();
        let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
        h.land(victim, false);
        h.tick(5);
        if sheep {
            h.world().entity_mut(victim).insert(PolymorphedVisual);
        }
        let seen: HashSet<Entity> = h.particles().into_iter().map(|(e, _)| e).collect();
        h.tick(10);
        let new = h
            .particles()
            .into_iter()
            .filter(|(e, _)| !seen.contains(e))
            .count();
        assert!(new > 30, "sheep {sheep}: {new}");
        assert_eq!(h.rigs().len(), 1);
        let mut q = h.world().query::<(&ConflagrateRingPart, &Visibility)>();
        assert_eq!(q.iter(h.app.world()).count(), 2);
    }
}

/// A match that ends mid-burst takes every piece of it: everything it spawns
/// at the top is a `PlayMatchEntity`, and the rings and sprites are their
/// children.
#[test]
fn a_match_ending_mid_burst_leaves_nothing() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0));
    // A Shadowburn landing in the air at the same time.
    h.world().spawn((
        SchoolImpact {
            target: victim,
            ability: AbilityType::Shadowburn,
            school: SpellSchool::Shadow,
            anchor: ImpactAnchor::Head,
            from: Vec3::X,
            magnitude: 0.2,
            is_crit: false,
            age: 0.0,
        },
        PlayMatchEntity,
    ));
    h.land(victim, false);
    h.tick(12);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert!(h.tagged(rigs[0].0), "the rig is a PlayMatchEntity");
    let particles = h.particles();
    assert!(particles.len() > 30);
    for (e, _) in &particles {
        assert!(h.tagged(*e), "a particle is a PlayMatchEntity");
    }
    let mut q = h.world().query::<&SchoolImpact>();
    assert_eq!(
        q.iter(h.app.world()).count(),
        1,
        "the Shadowburn landing too"
    );
    h.world().run_system_cached(cleanup_play_match).unwrap();
    h.world().flush();
    let w = h.world();
    assert_eq!(w.query::<&ConflagrateBurst>().iter(w).count(), 0);
    assert_eq!(w.query::<&ConflagrateParticle>().iter(w).count(), 0);
    assert_eq!(w.query::<&ConflagrateParticlePart>().iter(w).count(), 0);
    assert_eq!(w.query::<&ConflagrateRingPart>().iter(w).count(), 0);
    assert_eq!(w.query::<&ConflagrateShockPiece>().iter(w).count(), 0);
    assert_eq!(w.query::<&ConflagrateLanding>().iter(w).count(), 0);
    assert_eq!(w.query::<&SchoolImpact>().iter(w).count(), 0);
}

/// A landing on a victim already gone draws nothing, and its marker is spent.
#[test]
fn a_landing_on_a_victim_already_gone_draws_nothing() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::ZERO);
    h.world().despawn(victim);
    h.land(victim, false);
    h.tick(3);
    assert!(h.rigs().is_empty());
    assert!(h.particles().is_empty());
    let mut q = h.world().query::<&ConflagrateLanding>();
    assert_eq!(q.iter(h.app.world()).count(), 0);
}

// ── The sprites' shapes, read back from the textures ─────────────────────────

/// The alpha a texture holds at `(x, y)` in -1..1 across it, read off the
/// nearest pixel's centre, and that centre's radius and bearing.
fn texel(image: &Image, x: f32, y: f32) -> (f32, f32, f32) {
    let w = image.texture_descriptor.size.width as usize;
    let px = (((x + 1.0) / 2.0 * w as f32) as usize).min(w - 1);
    let py = (((y + 1.0) / 2.0 * w as f32) as usize).min(w - 1);
    let data = image.data.as_ref().expect("texture data");
    let a = data[(py * w + px) * 4 + 3] as f32 / 255.0;
    let (cx, cy) = (
        (px as f32 + 0.5) / w as f32 * 2.0 - 1.0,
        (py as f32 + 0.5) / w as f32 * 2.0 - 1.0,
    );
    (a, (cx * cx + cy * cy).sqrt(), cy.atan2(cx))
}

fn texture_of(world: &World, m: &StandardMaterial) -> Image {
    world
        .resource::<Assets<Image>>()
        .get(m.base_color_texture.as_ref().expect("a textured sprite"))
        .expect("the texture")
        .clone()
}

/// Every sprite is the shape the bench's `sprite()` paints: `glow`, `cloud`
/// and `smoke` the radial gradients with their stops (1 / 0.45, 0.9 / 0.6,
/// 0.85 / 0.5 at the centre and half-radius, 0 at the rim), `star` a plus of
/// rays 0.12 of its radius thick over a core gone by 0.45 of it; and the fire
/// disc `groundDisc`'s gradient (1, 0.55 at 0.55, 0). The streak has no
/// texture: a flat stroke.
#[test]
fn every_sprite_is_the_shape_the_bench_paints() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::ZERO.with_y(COMBATANT_Y));
    h.land(victim, false);
    h.tick(12);
    let stops = |name: &str| -> Option<[(f32, f32); 3]> {
        match name {
            "fire_impactdd_high_chest/P4+P5 plume jets" => {
                Some([(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)])
            }
            "fire_impactdd_high_chest/P0 glow puffs"
            | "fire_impactdd_high_chest/P2 flame puffs" => {
                Some([(0.0, 0.9), (0.45, 0.6), (1.0, 0.0)])
            }
            "fire_impactdd_high_chest/P7 pale smoke" => {
                Some([(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)])
            }
            _ => None,
        }
    };
    let mut seen = HashSet::new();
    for (e, p) in h.particles() {
        if !seen.insert(p.layer.name) {
            continue;
        }
        for (part, _, _, m) in h.parts_of(e) {
            if part == ConflagrateParticlePart::Streak {
                assert!(m.base_color_texture.is_none(), "a streak is a flat stroke");
                continue;
            }
            let image = texture_of(h.app.world(), &m);
            match stops(p.layer.name) {
                Some(stops) => {
                    for r in [0.05, 0.2, 0.45, 0.6, 0.8, 0.97] {
                        let (a, at, _) = texel(&image, r, 0.0);
                        let want = bench_keys(&stops, at);
                        assert!(
                            (a - want).abs() < 1.5 / 255.0,
                            "{}: {a} at {at}, bench {want}",
                            p.layer.name
                        );
                    }
                }
                None => {
                    // The star: solid along an axis (a ray), clear between
                    // rays beyond the core, the core fading to 0.45.
                    for (x, y, solid) in [
                        (0.9, 0.0, true),
                        (0.0, -0.9, true),
                        (0.6, 0.06, true),
                        (0.6, 0.6, false),
                        (0.5, 0.2, false),
                    ] {
                        let (a, ..) = texel(&image, x, y);
                        assert_eq!(a > 0.99, solid, "star at ({x}, {y}): {a}");
                        assert!(solid || a < 0.01, "star at ({x}, {y}): {a}");
                    }
                    let (a, at, _) = texel(&image, 0.25, 0.25);
                    let want = (1.0 - at / 0.45).max(0.0);
                    assert!((a - want).abs() < 1.5 / 255.0, "core {a} at {at}");
                }
            }
        }
    }
    assert_eq!(seen.len(), 5, "{seen:?}");
    // The fire disc, once it is drawn.
    h.tick(4);
    let rig = h.rigs()[0].0;
    let world = h.app.world();
    let disc = world
        .get::<Children>(rig)
        .unwrap()
        .iter()
        .find(|c| world.get::<ConflagrateRingPart>(*c) == Some(&ConflagrateRingPart::FireDisc))
        .unwrap();
    let handle = &world
        .get::<MeshMaterial3d<StandardMaterial>>(disc)
        .unwrap()
        .0;
    let m = world
        .resource::<Assets<StandardMaterial>>()
        .get(handle)
        .unwrap();
    let image = texture_of(world, m);
    assert_eq!(
        m.emissive_texture, m.base_color_texture,
        "its glow is shaped too"
    );
    for r in [0.1, 0.3, 0.55, 0.7, 0.9] {
        let (a, at, _) = texel(&image, r, 0.0);
        let want = bench_keys(&[(0.0, 1.0), (0.55, 0.55), (1.0, 0.0)], at);
        assert!(
            (a - want).abs() < 1.5 / 255.0,
            "disc {a} at {at}, bench {want}"
        );
    }
}

// ── Held in front of the body, at the match camera ───────────────────────────

/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`, centred on its
/// transform.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;
/// A pet's sim height (`play_match/mod.rs` spawns pets at 0.75), its body
/// mesh's drop below that, and the radius of the sphere bounding its
/// `Capsule3d::new(0.35, 0.6)` whichever way it faces.
const PET_SIM_Y: f32 = 0.75;
const PET_BODY_DROP: f32 = -0.45;
const PET_BODY_BOUND: f32 = 0.35 + 0.3;

/// The match camera (`CameraController`): `zoom` yards out, `pitch` from the
/// vertical, at `yaw`, looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// Every drawn piece of one burst, per frame: `(world position, world
/// scale)` of each particle's sprite and of each ring piece, keyed by entity.
type Frame = HashMap<Entity, (Vec3, Vec3)>;

/// Play one burst on a still victim for `frames`, with the match camera on
/// it or — the control, the same world with no camera, which draws every
/// piece where it truly is — a plain entity in the camera's place, so the
/// two worlds allocate the same entities and play the same scatter.
fn drawn_frames(camera: Option<Transform>, frames: u32, pet: bool) -> (Vec<Frame>, Vec3) {
    let mut h = Harness::new();
    let at = victim_spot(pet);
    let victim = if pet { h.pet_victim(at) } else { h.victim(at) };
    match camera {
        Some(t) => {
            h.world()
                .spawn((Camera3d::default(), t, GlobalTransform::from(t)));
        }
        None => {
            h.world()
                .spawn((Transform::default(), GlobalTransform::default()));
        }
    }
    h.land(victim, false);
    let mut out = Vec::new();
    for _ in 0..frames {
        h.tick(1);
        let world = h.app.world_mut();
        let mut q = world
            .query_filtered::<(Entity, &GlobalTransform, Option<&ConflagrateParticlePart>), (
                With<Mesh3d>,
                Or<(
                    With<ConflagrateParticlePart>,
                    With<ConflagrateShockPiece>,
                    With<ConflagrateRingPart>,
                )>,
            )>();
        let frame: Frame = q
            .iter(world)
            // A streak is a screen line, not a held quad: its own test.
            .filter(|(_, _, part)| *part != Some(&ConflagrateParticlePart::Streak))
            .map(|(e, g, _)| {
                let (s, _, t) = g.to_scale_rotation_translation();
                (e, (t, s))
            })
            .collect();
        out.push(frame);
    }
    (out, at)
}

/// The bench paints the whole burst over its victim. Here the body writes no
/// depth and the transparent pass sorts by translation, so every piece is
/// drawn on its own view ray in front of the body's nearest point, scaled by
/// the same ratio — it projects exactly where the true piece does — and a
/// piece already in front is left alone. Checked frame by frame against the
/// same burst drawn with no camera, at two yaws of the match camera.
#[test]
fn every_piece_is_held_in_front_of_the_body_where_the_bench_draws_it() {
    for yaw in [0.0_f32, 2.3] {
        assert_held(false, yaw);
    }
}

/// A pet's body is its own: a capsule half a combatant's, hanging below its
/// sim transform, held against the sphere bounding it. Its burst is held in
/// front of THAT body — not a combatant's — rings and particles alike.
#[test]
fn a_pet_victims_burst_is_held_in_front_of_the_pets_body() {
    for yaw in [0.0_f32, 2.3] {
        assert_held(true, yaw);
    }
}

/// Where the hold probes stand their victim.
fn victim_spot(pet: bool) -> Vec3 {
    Vec3::new(2.0, if pet { PET_SIM_Y } else { COMBATANT_Y }, -3.0)
}

fn assert_held(pet: bool, yaw: f32) {
    {
        let target = victim_spot(pet);
        let cam = match_camera(target, 60.0, 38.7f32.to_radians(), yaw);
        let (seen, at) = drawn_frames(Some(cam), 60, pet);
        let (truth, _) = drawn_frames(None, 60, pet);
        let (eye, forward) = (cam.translation, cam.forward().as_vec3());
        let depth = |p: Vec3| (p - eye).dot(forward);
        // The body the bench's burst is painted over: a combatant's upright
        // capsule, or the sphere bounding a pet's (`Capsule3d::new(0.35,
        // 0.6)`, centred 0.45 below its sim transform).
        let (top, bottom, radius) = if pet {
            let c = at + Vec3::Y * PET_BODY_DROP;
            (c, c, PET_BODY_BOUND)
        } else {
            (
                at + Vec3::Y * BODY_HALF_SEGMENT,
                at - Vec3::Y * BODY_HALF_SEGMENT,
                BODY_RADIUS,
            )
        };
        let nearest = depth(top).min(depth(bottom)) - radius;
        let (mut held, mut kept) = (0, 0);
        for (drawn, real) in seen.iter().zip(&truth) {
            assert_eq!(
                drawn.keys().collect::<HashSet<_>>(),
                real.keys().collect::<HashSet<_>>()
            );
            for (e, (d_at, d_scale)) in drawn {
                let (r_at, r_scale) = real[e];
                // Sprites, streaks and ring pieces that are hidden or empty
                // carry no footprint worth checking.
                if r_scale.max_element() < 1e-3 {
                    continue;
                }
                let k = depth(*d_at) / depth(r_at);
                assert!(
                    (*d_at - (eye + (r_at - eye) * k)).length() < 2e-3,
                    "yaw {yaw}: drawn off its view ray: {d_at} for {r_at}"
                );
                assert!(
                    depth(*d_at) < nearest,
                    "yaw {yaw}: drawn behind the body's front at {d_at}"
                );
                if depth(r_at) < nearest - 0.06 {
                    assert!((*d_at - r_at).length() < 1e-4, "in front already, moved");
                    kept += 1;
                } else if depth(r_at) >= nearest {
                    // Behind the body's front: it would sort behind the body.
                    assert!(k < 1.0, "yaw {yaw}: not held at {r_at}");
                    held += 1;
                }
                assert!(
                    (*d_scale - r_scale * k).length() < 2e-4 * r_scale.length().max(1.0),
                    "yaw {yaw}: drawn {d_scale}, keeps screen size {}",
                    r_scale * k
                );
            }
        }
        assert!(
            held > 500 && kept > 100,
            "yaw {yaw}: {held} held, {kept} kept"
        );
    }
}

/// A streak is the bench's screen line: laid in the camera's plane at its
/// particle's depth, its far end projecting where the true segment's does.
#[test]
fn a_plumes_streak_is_its_flight_as_the_camera_sees_it() {
    let target = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let cam = match_camera(target, 60.0, 38.7f32.to_radians(), 0.9);
    let mut h = Harness::new();
    let victim = h.victim(target);
    h.world()
        .spawn((Camera3d::default(), cam, GlobalTransform::from(cam)));
    h.land(victim, false);
    let (eye, f) = (cam.translation, cam.forward().as_vec3());
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let screen = |p: Vec3| {
        let d = p - eye;
        Vec2::new(d.dot(right), d.dot(up)) / d.dot(f)
    };
    let mut streaks = 0;
    for _ in 0..12 {
        h.tick(1);
        for (e, p) in h.particles() {
            if p.layer.tail == 0.0 {
                continue;
            }
            let true_at = h
                .app
                .world()
                .get::<ConflagrateParticle>(e)
                .map(|c| c.at)
                .unwrap();
            let len = (p.layer.tail * 0.12).min(p.velocity.length() * 0.06);
            let far = true_at - p.velocity.normalize() * len;
            for (part, g, _, _) in h.parts_of(e) {
                if part != ConflagrateParticlePart::Streak {
                    continue;
                }
                let (s, r, t) = g.to_scale_rotation_translation();
                assert!(((t - p.at).dot(f)).abs() < 1e-4, "in the camera's plane");
                let tip = t + r * Vec3::X * (s.x / 2.0) * (t - p.at).dot(r * Vec3::X).signum();
                assert!(
                    (screen(tip) - screen(far)).length() < 1e-4,
                    "the tail ends where the flight projects"
                );
                assert!((screen(p.at) - screen(true_at)).length() < 1e-5);
                streaks += 1;
            }
        }
    }
    assert!(streaks > 100, "{streaks}");
}
