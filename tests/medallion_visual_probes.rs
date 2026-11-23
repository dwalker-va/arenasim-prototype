//! Probes for the Medallion of the Alliance's break (AS-432,
//! `rendering/effects/medallion.rs`): the Holy launch on the wearer's hands,
//! the gold dispel ribbon on its drawn body, and the broken stun's whirl
//! scattering instead of retracting.
//!
//! Everything runs through the real graphical systems, fed by the real
//! `process_on_use_items`, and is asserted on WORLD positions, the drawn
//! materials and the rigs' lifetimes. Three probes play the break through the
//! real headless sim (`run_headless_match_staged`'s app with the visual systems
//! riding on it).
//!
//! The bench's numbers are transcribed here from the signed-off Trinket Effects
//! Bench (artifact https://claude.ai/artifact/1FRTwgEeJZWYK1iFcHPkY2, version
//! 1793333359-b46c, "Signed off" preset, Medallion tab), independently of the
//! module's constants, so a wrong constant fails a probe rather than moving it.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`: no window, no
//! GPU.

use std::collections::{BTreeSet, HashMap};
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::headless::runner::{build_headless_app_staged, HeadlessMatchState};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CcBead, CcBeadDisc, CcKind, CcRig, Combatant, CompoundDebuff,
    DispelRibbon, DispelSpark, HealCastHand, HealCastKind, HealCastPhase, MedallionBreak,
    PlayMatchEntity, RibbonSource, VisualBody,
};
use arenasim::states::play_match::equipment::ItemId;
use arenasim::states::play_match::on_use_items::{
    OnUseConfig, OnUseCooldown, OnUseEffect, OnUsePending,
};
use arenasim::states::play_match::{
    apply_cyclone_lift, billboard_cc_beads, break_end, break_scatters, cc_envelope,
    cleanup_cc_flares, cleanup_cc_rigs, cleanup_expired_dispel_ribbons, cleanup_heal_cast_glows,
    cleanup_play_match, place_medallion_ribbons, process_on_use_items, ribbon_climb,
    scatter_cc_beads, spawn_dispel_ribbon_visuals, spawn_medallion_breaks, update_cc_flares,
    update_cc_rigs, update_cyclone_visuals, update_dispel_ribbons, update_dispel_sparks,
    update_hard_cc_visuals, update_heal_cast_glows, BreakEnd, CycloneLift,
};
use arenasim::CharacterClass;

// ── The bench, transcribed ──────────────────────────────────────────────────

/// `drawMedallion`'s scatter: `t < 0.4`, `k = t / 0.4`.
const SCATTER_SECS: f32 = 0.4;
/// `dir = norm([cos θ, 0.35, sin θ])`.
const SCATTER_RISE: f32 = 0.35;
/// `add(b.p, mul(dir, k * 1.6))`.
const SCATTER_TRAVEL: f32 = 1.6;
/// `b.rad * (1 + k)`: twice its radius at the end.
const SCATTER_GROWTH: f32 = 1.0;
/// The whirl's beads (`STUN`): 2 arms of 5, spinning once a second, 1.55 over
/// the body centre.
const BEADS: usize = 10;
const WHIRL_HZ: f32 = 1.0;
/// The sparkle quad spans five times a bead's core (`STUN_SPARKLE_SPAN`).
const SPARKLE_SPAN: f32 = 5.0;
/// The whirl's own retract, for the control.
const RETRACT_SECS: f32 = 0.18;
/// The bead material the scatter fades: `STUN_BEAD_COLOR` alpha and emissive.
const BEAD_ALPHA: f32 = 0.90;
const BEAD_EMISSIVE: [f32; 3] = [3.0, 3.3, 4.2];

/// `RIBBON_COL.gold`: the client's `ribbonblur1bea_gold`.
const GOLD: [f32; 3] = [1.0, 0.82, 0.0];
const GOLD_EMISSIVE: [f32; 3] = [2.5, 2.05, 0.6];
/// The shared dispel-ribbon level (AS-435's ruling over the bench's 0.3).
const RIBBON_LEVEL: f32 = 0.1;
/// `RIB`: life 1.15, from -1.25 under the body centre, rising 1.6, the
/// emissive trimmed to 0.6, igniting ×(1 + 3e^(-t/0.28)).
const RIBBON_LIFE: f32 = 1.15;
const RIBBON_START: f32 = -1.25;
const RIBBON_RISE: f32 = 1.6;
const RIBBON_TRIM: f32 = 0.6;
/// The engine's sparks: the source's full colour, emissive ×1.3.
const SPARK_GAIN: f32 = 1.3;
/// `drawHands(..., 0.45)`: the Holy launch.
const HANDS_SECS: f32 = 0.45;

/// A Priest's ribbon, unchanged: `(0.85, 0.85, 1.0)`, emissive `(2, 2, 2.8)`.
const SILVER: [f32; 3] = [0.85, 0.85, 1.0];
const SILVER_EMISSIVE: [f32; 3] = [2.0, 2.0, 2.8];

const TICK: Duration = Duration::from_millis(16);
const WEARER_AT: Vec3 = Vec3::new(2.0, 1.0, -3.0);

// ── Helpers ─────────────────────────────────────────────────────────────────

fn kidney_shot() -> Aura {
    Aura {
        effect_type: AuraType::Stun,
        duration: 6.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Kidney Shot".to_string(),
        ..Default::default()
    }
}

fn aura(effect_type: AuraType, name: &str) -> Aura {
    Aura {
        effect_type,
        duration: 8.0,
        magnitude: 0.5,
        break_on_damage_threshold: -1.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

fn medallion() -> OnUseConfig {
    OnUseConfig {
        effect: OnUseEffect::CcBreak,
        cooldown: OnUseCooldown::OncePerMatch,
        price: Some(24.0),
        magnitude: 0.0,
        duration: 0.0,
        heal_over_time: None,
    }
}

fn linear(srgb: [f32; 3]) -> [f32; 3] {
    let l = Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear();
    [l.red, l.green, l.blue]
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * (1.0 + b.abs())
}

fn assert_vec(label: &str, got: Vec3, want: Vec3, tol: f32) {
    assert!(
        got.distance(want) <= tol,
        "{label}: got {got}, want {want} (off {})",
        got.distance(want)
    );
}

fn assert_rgb(label: &str, got: [f32; 3], want: [f32; 3]) {
    for i in 0..3 {
        assert!(
            close(got[i], want[i]),
            "{label}: got {got:?}, want {want:?}"
        );
    }
}

/// The bench's match camera: 60 yd off, pitched `pitch` degrees down.
fn camera_at(target: Vec3, distance: f32, pitch_deg: f32, yaw: f32) -> Transform {
    let p = pitch_deg.to_radians();
    let offset = Quat::from_rotation_y(yaw) * Vec3::new(0.0, p.sin(), p.cos()) * distance;
    Transform::from_translation(target + offset).looking_at(target, Vec3::Y)
}

// ── The unit harness ────────────────────────────────────────────────────────

/// A request to stun the wearer again between the break and the visuals.
#[derive(Resource, Default)]
struct Restun(bool);

fn restun(mut flag: ResMut<Restun>, mut auras: Query<&mut ActiveAuras>) {
    if std::mem::take(&mut flag.0) {
        for mut a in auras.iter_mut() {
            a.auras.push(kidney_shot());
        }
    }
}

/// How far the wearer runs per second, and how high its body bobs.
#[derive(Resource, Default)]
struct Motion {
    run: Vec3,
    bob: f32,
    /// How high the body is held off its rest, and how fast that changes.
    lift: f32,
    lift_rate: f32,
}

#[derive(Component)]
struct Runner;

/// The wearer runs and its body bobs, as the interpolation and the gaits move
/// them, before any visual reads them.
fn run_and_bob(
    time: Res<Time>,
    mut motion: ResMut<Motion>,
    mut units: Query<&mut Transform, (With<Runner>, Without<VisualBody>)>,
    mut bodies: Query<&mut Transform, (With<VisualBody>, Without<Runner>)>,
) {
    let dt = time.delta_secs();
    for mut t in units.iter_mut() {
        t.translation += motion.run * dt;
    }
    motion.lift = (motion.lift + motion.lift_rate * dt).max(0.0);
    let y = motion.lift + motion.bob * (time.elapsed_secs() * 9.0).sin();
    for mut t in bodies.iter_mut() {
        t.translation.y = y;
    }
}

struct Harness {
    app: App,
    wearer: Entity,
    body: Option<Entity>,
    dt: f32,
    /// Frames played, so a loop waiting on an effect that never ends fails
    /// instead of hanging.
    frames: u32,
}

impl Harness {
    fn new() -> Self {
        Self::build(TICK, true)
    }

    /// `body`: whether the wearer carries a rendered body (a headless-shaped
    /// unit does not).
    fn build(tick: Duration, body: bool) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(tick));
        app.init_resource::<CombatLog>();
        app.init_resource::<Restun>();
        app.init_resource::<Motion>();
        // The client's order: the break is read just before the use resolves
        // (FixedUpdate, Phase 1), then the hard-CC group, the Cyclone, the
        // ribbon and the hands (Update); the lift and the ribbon's placement
        // (PostUpdate).
        app.add_systems(
            Update,
            (
                run_and_bob,
                spawn_medallion_breaks,
                process_on_use_items,
                restun,
                update_hard_cc_visuals,
                update_cc_rigs,
                billboard_cc_beads,
                scatter_cc_beads,
                update_cc_flares,
                cleanup_cc_rigs,
                cleanup_cc_flares,
                update_cyclone_visuals,
                spawn_dispel_ribbon_visuals,
                update_dispel_ribbons,
                update_dispel_sparks,
                cleanup_expired_dispel_ribbons,
                update_heal_cast_glows,
                cleanup_heal_cast_glows,
            )
                .chain(),
        );
        app.add_systems(
            PostUpdate,
            (apply_cyclone_lift, place_medallion_ribbons)
                .chain()
                .before(TransformSystem::TransformPropagate),
        );
        let wearer = app
            .world_mut()
            .spawn((
                Transform::from_translation(WEARER_AT),
                Combatant::new(1, 0, CharacterClass::Mage),
                ActiveAuras { auras: Vec::new() },
                Runner,
                PlayMatchEntity,
            ))
            .id();
        let body = body.then(|| {
            let b = app
                .world_mut()
                .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
                .id();
            app.world_mut().entity_mut(wearer).add_child(b);
            b
        });
        // Bevy's first frame has no delta: play it before anything is
        // measured, so every frame from here on is one tick long.
        app.update();
        Harness {
            app,
            wearer,
            body,
            dt: tick.as_secs_f32(),
            frames: 0,
        }
    }

    fn tick(&mut self) {
        self.frames += 1;
        assert!(self.frames < 2000, "an effect never ended");
        self.app.update();
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    fn camera(&mut self, cam: Transform) {
        self.app.world_mut().spawn((Camera3d::default(), cam));
    }

    fn put(&mut self, a: Aura) {
        let w = self.wearer;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(w)
            .unwrap()
            .auras
            .push(a);
    }

    fn clear_auras(&mut self) {
        let w = self.wearer;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(w)
            .unwrap()
            .auras
            .clear();
    }

    /// The wearer's AI uses the Medallion, as `try_cc_break` queues it.
    fn use_medallion(&mut self) {
        let w = self.wearer;
        self.app.world_mut().spawn(OnUsePending {
            wearer: w,
            item: ItemId::MedallionOfTheAlliance,
            item_name: "Medallion of the Alliance".to_string(),
            config: medallion(),
        });
    }

    /// Stunned, and played until the whirl is fully grown.
    fn stunned(&mut self) {
        self.put(kidney_shot());
        self.ticks(30);
        assert_eq!(self.rigs().len(), 1, "the whirl is up");
    }

    fn rigs(&mut self) -> Vec<(Entity, CcRigView)> {
        let mut q = self.app.world_mut().query::<(Entity, &CcRig)>();
        q.iter(self.app.world())
            .map(|(e, r)| {
                (
                    e,
                    CcRigView {
                        kind: r.kind,
                        age: r.age,
                        retract: r.retract,
                        scatter: r.scatter,
                    },
                )
            })
            .collect()
    }

    fn rig(&mut self) -> (Entity, CcRigView) {
        let rigs = self.rigs();
        assert_eq!(rigs.len(), 1, "one rig: {rigs:?}");
        rigs[0]
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<(), With<C>>();
        q.iter(self.app.world()).count()
    }

    fn global(&self, e: Entity) -> GlobalTransform {
        *self.app.world().get::<GlobalTransform>(e).unwrap()
    }

    fn material(&self, e: Entity) -> StandardMaterial {
        let h = self
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(e)
            .unwrap()
            .0
            .clone();
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&h)
            .unwrap()
            .clone()
    }

    /// Every bead of the rig: its entity, rest and size.
    fn beads(&mut self, rig: Entity) -> Vec<(Entity, Vec3, f32)> {
        let children: Vec<Entity> = self
            .app
            .world()
            .get::<Children>(rig)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        children
            .into_iter()
            .filter_map(|c| {
                self.app
                    .world()
                    .get::<CcBead>(c)
                    .map(|b| (c, b.rest, b.size))
            })
            .collect()
    }

    fn ribbons(&mut self) -> Vec<(Entity, Entity, RibbonSource, f32)> {
        let mut q = self.app.world_mut().query::<(Entity, &DispelRibbon)>();
        q.iter(self.app.world())
            .map(|(e, r)| (e, r.target, r.source, r.lifetime))
            .collect()
    }

    fn medallion_ribbon(&mut self) -> Entity {
        let r: Vec<_> = self
            .ribbons()
            .into_iter()
            .filter(|r| r.2 == RibbonSource::Medallion)
            .collect();
        assert_eq!(r.len(), 1, "one Medallion ribbon");
        r[0].0
    }

    fn hands(&mut self) -> Vec<(Entity, HealCastKind, HealCastPhase, Entity, Entity)> {
        let mut q = self.app.world_mut().query::<(Entity, &HealCastHand)>();
        q.iter(self.app.world())
            .map(|(e, h)| (e, h.kind, h.phase, h.caster, h.body))
            .collect()
    }

    fn kill(&mut self) {
        let w = self.wearer;
        let mut c = self.app.world_mut().get_mut::<Combatant>(w).unwrap();
        c.current_health = 0.0;
        c.is_dead = true;
    }
}

#[derive(Clone, Copy, Debug)]
struct CcRigView {
    kind: CcKind,
    age: f32,
    retract: Option<f32>,
    scatter: Option<f32>,
}

/// `n` frames of `dt` summed as the systems sum them.
fn summed(dt: f32, n: u32) -> f32 {
    let mut s = 0.0f32;
    for _ in 0..n {
        s += dt;
    }
    s
}

/// The first frame count whose summed `dt` reaches `secs`.
fn frames_to(dt: f32, secs: f32) -> u32 {
    let mut s = 0.0f32;
    let mut n = 0;
    while s < secs {
        s += dt;
        n += 1;
    }
    n
}

// ── The route ───────────────────────────────────────────────────────────────

/// The stun scatters; every other crowd control the Medallion breaks keeps
/// its end, named with the card it waits on; nothing else is broken.
#[test]
fn only_the_stun_scatters_and_every_other_break_is_named() {
    let silent: BTreeSet<String> = AuraType::ALL
        .into_iter()
        .filter(|a| matches!(break_end(*a), BreakEnd::KnownSilent { card: "AS-432", .. }))
        .map(|a| format!("{a:?}"))
        .collect();
    let want: BTreeSet<String> = [
        "Cyclone",
        "Fear",
        "Incapacitate",
        "MovementSpeedSlow",
        "Polymorph",
        "Root",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(silent, want);
    let scatters: Vec<AuraType> = AuraType::ALL
        .into_iter()
        .filter(|a| matches!(break_end(*a), BreakEnd::Scatters(_)))
        .collect();
    assert_eq!(scatters, vec![AuraType::Stun]);
    assert_eq!(break_end(AuraType::Stun), BreakEnd::Scatters(CcKind::Stun));
    assert_eq!(break_end(AuraType::Silence), BreakEnd::NotBroken);

    // What a break takes off decides what scatters: a stun among a root, a
    // fear and a silence scatters the stun alone, once.
    let auras = ActiveAuras {
        auras: vec![
            aura(AuraType::Root, "Frost Nova"),
            kidney_shot(),
            aura(AuraType::Fear, "Fear"),
            aura(AuraType::Silence, "Silence"),
            kidney_shot(),
        ],
    };
    assert_eq!(break_scatters(Some(&auras)), vec![CcKind::Stun]);
    let no_stun = ActiveAuras {
        auras: vec![
            aura(AuraType::Root, "Frost Nova"),
            aura(AuraType::Silence, "Silence"),
        ],
    };
    assert_eq!(break_scatters(Some(&no_stun)), Vec::<CcKind>::new());
    assert_eq!(break_scatters(None), Vec::<CcKind>::new());
    // The break takes a compound's riders with its face, so a stun riding a
    // broken slow scatters too; a stun riding nothing broken does not.
    let rider = Aura {
        compound: Some(CompoundDebuff::FrostArmorChill),
        ..kidney_shot()
    };
    let face = Aura {
        compound: Some(CompoundDebuff::FrostArmorChill),
        ..aura(AuraType::MovementSpeedSlow, "Frost Armor")
    };
    let ridden = ActiveAuras {
        auras: vec![face, rider.clone()],
    };
    assert_eq!(break_scatters(Some(&ridden)), vec![CcKind::Stun]);
    let alone = ActiveAuras { auras: vec![rider] };
    assert_eq!(break_scatters(Some(&alone)), Vec::<CcKind>::new());
}

/// A stun broken, then a new one landing while the old whirl still flies
/// apart: the new stun gets a whirl of its own, and that whirl's end retracts
/// it alone; the scattering one plays out its scatter untouched.
#[test]
fn a_new_stun_during_the_scatter_gets_its_own_whirl() {
    let mut h = Harness::new();
    h.stunned();
    let (old, _) = h.rig();
    h.use_medallion();
    h.tick();
    h.ticks(2);
    h.put(kidney_shot());
    h.tick();
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "a fresh whirl beside the scatter: {rigs:?}");
    h.ticks(2);
    h.clear_auras();
    h.tick();
    let mut frames = 7;
    let mut new_retracted = false;
    while h.rigs().iter().any(|(e, _)| *e == old) {
        for (e, v) in h.rigs() {
            if e == old {
                assert_eq!(v.retract, None, "the scattering whirl never retracts");
                assert!(v.scatter.is_some());
            } else {
                new_retracted |= v.retract.is_some();
                assert_eq!(v.scatter, None);
            }
        }
        h.tick();
        frames += 1;
    }
    assert!(new_retracted);
    assert_eq!(frames, frames_to(h.dt, SCATTER_SECS));
}

// ── The scatter ─────────────────────────────────────────────────────────────

/// The break scatters the whirl: every bead flies from where the whirl held
/// it, out along `(cos θ, 0.35, sin θ)`, 1.6 yd over 0.4 s, growing to twice
/// its size and fading to nothing, while the hub stays frozen at the break.
/// It never retracts.
#[test]
fn the_broken_stun_scatters_as_the_bench_draws_it() {
    let mut h = Harness::new();
    let cam = camera_at(WEARER_AT, 60.0, 38.7, 0.4);
    h.camera(cam);
    h.stunned();
    let (rig, before) = h.rig();
    h.use_medallion();
    h.tick();

    let after = h.rig().1;
    assert_eq!(after.retract, None, "the whirl does not retract");
    assert_eq!(
        after.scatter,
        Some(h.dt),
        "armed and aged on the break's frame"
    );
    assert_eq!(h.count::<MedallionBreak>(), 0, "the break is read once");

    // The whirl as it stood at the break: spin frozen at the age it had.
    let age_at_break = before.age + h.dt - after.scatter.unwrap();
    assert!(
        close(age_at_break, before.age),
        "{age_at_break} vs {}",
        before.age
    );
    let spin = Quat::from_rotation_y(-age_at_break * TAU * WHIRL_HZ);
    let beads = h.beads(rig);
    assert_eq!(beads.len(), BEADS);

    let mut frames = 1;
    let mut saw_mid = false;
    while let Some((_, view)) = h.rigs().into_iter().find(|(e, _)| *e == rig) {
        let scatter = view.scatter.expect("scattering");
        assert_eq!(view.retract, None);
        assert!(close(view.scatter.unwrap(), summed(h.dt, frames)));
        let k = scatter / SCATTER_SECS;
        saw_mid |= (0.4..0.6).contains(&k);
        let hub = h.global(rig);
        assert!(
            hub.rotation().angle_between(spin) < 1e-3,
            "the hub holds the break's spin"
        );
        for (bead, rest, size) in &beads {
            let theta = rest.z.atan2(rest.x);
            let dir = Vec3::new(theta.cos(), SCATTER_RISE, theta.sin()).normalize();
            let want = hub.translation() + spin * (*rest + dir * SCATTER_TRAVEL * k);
            let g = h.global(*bead);
            assert_vec("bead", g.translation(), want, 1e-3);
            let span = size * SPARKLE_SPAN * (1.0 + SCATTER_GROWTH * k);
            assert!(close(g.scale().x, span), "{} vs {span}", g.scale().x);
            // No disc under a scattering bead: the bench's scatter is the
            // sparkles alone.
            for disc in h.app.world().get::<Children>(*bead).unwrap().iter() {
                assert!(h.app.world().get::<CcBeadDisc>(disc).is_some());
                assert_eq!(
                    h.app.world().get::<Visibility>(disc),
                    Some(&Visibility::Hidden)
                );
            }
        }
        let m = h.material(beads[0].0);
        assert_eq!(m.alpha_mode, AlphaMode::Add, "drawn additive");
        assert!(close(m.base_color.alpha(), BEAD_ALPHA * (1.0 - k)));
        assert_rgb(
            "bead emissive",
            [m.emissive.red, m.emissive.green, m.emissive.blue],
            BEAD_EMISSIVE.map(|c| c * (1.0 - k)),
        );
        h.tick();
        frames += 1;
    }
    assert!(saw_mid, "the probe saw the scatter's middle");
    // Gone on the frame the scatter's 0.4 s is reached.
    assert_eq!(frames, frames_to(h.dt, SCATTER_SECS));
    assert_eq!(h.count::<CcRig>(), 0);
    assert_eq!(h.count::<CcBead>(), 0);
}

/// A stun broken while its whirl is still growing scatters from the size the
/// whirl had reached, and holds that size through the scatter.
#[test]
fn a_stun_broken_mid_grow_scatters_from_where_it_reached() {
    let mut h = Harness::new();
    h.camera(camera_at(WEARER_AT, 60.0, 38.7, 0.0));
    h.put(kidney_shot());
    h.ticks(4);
    let (rig, before) = h.rig();
    h.use_medallion();
    h.tick();
    let grown = cc_envelope(before.age, None, 0.14, RETRACT_SECS);
    assert!(grown < 0.9, "still growing: {grown}");
    let spin = Quat::from_rotation_y(-before.age * TAU * WHIRL_HZ);
    let beads = h.beads(rig);
    let mut frames = 1;
    while let Some((_, view)) = h.rigs().into_iter().find(|(e, _)| *e == rig) {
        let k = view.scatter.unwrap() / SCATTER_SECS;
        let hub = h.global(rig);
        for (bead, rest, size) in &beads {
            let theta = rest.z.atan2(rest.x);
            let dir = Vec3::new(theta.cos(), SCATTER_RISE, theta.sin()).normalize();
            let want = hub.translation() + spin * (*rest * grown + dir * SCATTER_TRAVEL * k);
            let g = h.global(*bead);
            assert_vec(&format!("frame {frames}"), g.translation(), want, 1e-3);
            let span = size * SPARKLE_SPAN * grown * (1.0 + SCATTER_GROWTH * k);
            assert!(close(g.scale().x, span), "{} vs {span}", g.scale().x);
        }
        h.tick();
        frames += 1;
    }
    assert_eq!(frames, frames_to(h.dt, SCATTER_SECS));
}

/// Control: a stun that ends on its own (here, any removal that is not the
/// Medallion's) retracts, and never scatters.
#[test]
fn a_stun_that_ends_on_its_own_retracts() {
    let mut h = Harness::new();
    h.stunned();
    h.clear_auras();
    h.tick();
    let view = h.rig().1;
    assert_eq!(view.scatter, None);
    assert_eq!(view.retract, Some(h.dt));
    h.ticks(frames_to(h.dt, RETRACT_SECS) - 1);
    assert_eq!(h.count::<CcRig>(), 0, "retracted in 0.18 s");
}

/// A unit stunned again before the visuals see the break keeps its whirl; the
/// break is spent, and the new stun's own end retracts.
#[test]
fn a_restun_keeps_the_whirl_and_its_end_retracts() {
    let mut h = Harness::new();
    h.stunned();
    let (rig, _) = h.rig();
    h.use_medallion();
    h.app.world_mut().resource_mut::<Restun>().0 = true;
    h.tick();
    let view = h.rig();
    assert_eq!(view.0, rig, "the same whirl");
    assert_eq!((view.1.retract, view.1.scatter), (None, None));
    assert_eq!(h.count::<MedallionBreak>(), 0, "the break was read");
    h.ticks(5);
    h.clear_auras();
    h.tick();
    assert_eq!(h.rig().1.scatter, None);
    assert!(h.rig().1.retract.is_some());
}

/// A break that takes off a root and no stun scatters nothing: the root
/// retracts as ever, and no break is handed over.
#[test]
fn a_broken_root_keeps_its_retract() {
    let mut h = Harness::new();
    h.put(aura(AuraType::Root, "Frost Nova"));
    h.ticks(20);
    assert_eq!(h.rig().1.kind, CcKind::Root);
    h.use_medallion();
    h.tick();
    let view = h.rig().1;
    assert_eq!(view.scatter, None);
    assert!(view.retract.is_some());
    assert_eq!(h.count::<MedallionBreak>(), 0);
    assert_eq!(h.ribbons().len(), 1, "the ribbon still plays");
}

// ── The ribbon and the hands ────────────────────────────────────────────────

/// The ribbon: on the wearer, from its feet, gold at the shared level,
/// opaque, igniting; its sparks the gold at full level, additive.
#[test]
fn the_ribbon_is_the_gold_dispel_ribbon_on_the_wearer() {
    let mut h = Harness::new();
    h.camera(camera_at(WEARER_AT, 60.0, 38.7, 0.0));
    h.stunned();
    h.use_medallion();
    h.tick();
    let ribbons = h.ribbons();
    assert_eq!(ribbons.len(), 1);
    let (ribbon, target, source, _) = ribbons[0];
    assert_eq!((target, source), (h.wearer, RibbonSource::Medallion));
    assert!(h.app.world().get::<PlayMatchEntity>(ribbon).is_some());
    // On the frame it is drawn first, its base ring is at the feet, one
    // frame into its climb.
    let at = h.global(ribbon).translation();
    let progress = (RIBBON_LIFE - h.dt) / RIBBON_LIFE;
    assert_vec(
        "ribbon at the feet",
        at,
        WEARER_AT + Vec3::Y * (RIBBON_START + ribbon_climb(progress) * RIBBON_RISE),
        1e-4,
    );

    let m = h.material(ribbon);
    assert_eq!(m.alpha_mode, AlphaMode::Opaque);
    let base = m.base_color.to_linear();
    assert_rgb(
        "ribbon base",
        [base.red, base.green, base.blue],
        linear(GOLD).map(|c| c * RIBBON_LEVEL),
    );
    let ignite = 1.0 + 3.0 * (-h.dt / 0.28f32).exp();
    assert_rgb(
        "ribbon emissive",
        [m.emissive.red, m.emissive.green, m.emissive.blue],
        GOLD_EMISSIVE.map(|c| c * RIBBON_TRIM * RIBBON_LEVEL * ignite),
    );

    // The sparks, once the strip plays out.
    h.ticks(frames_to(h.dt, RIBBON_LIFE * 0.6));
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<Entity, With<DispelSpark>>();
    let sparks: Vec<Entity> = q.iter(h.app.world()).collect();
    assert!(!sparks.is_empty(), "the strip sheds sparks");
    for s in &sparks {
        assert!(h.app.world().get::<PlayMatchEntity>(*s).is_some());
        let m = h.material(*s);
        assert_eq!(m.alpha_mode, AlphaMode::Add);
        let b = m.base_color.to_srgba();
        assert_rgb("spark", [b.red, b.green, b.blue], GOLD);
        assert!(close(b.alpha, 1.0));
        assert_rgb(
            "spark emissive",
            [m.emissive.red, m.emissive.green, m.emissive.blue],
            GOLD_EMISSIVE.map(|c| c * SPARK_GAIN),
        );
    }
}

/// A dispeller's ribbon is unchanged: its class colour at the shared level,
/// and anchored on the unit's root, not its drawn body.
#[test]
fn a_priest_dispel_ribbon_is_unchanged() {
    let mut h = Harness::new();
    h.app.world_mut().resource_mut::<Motion>().bob = 0.3;
    let w = h.wearer;
    let ribbon = h
        .app
        .world_mut()
        .spawn((
            DispelRibbon {
                target: w,
                source: RibbonSource::Dispel(CharacterClass::Priest),
                lifetime: RIBBON_LIFE,
                initial_lifetime: RIBBON_LIFE,
                spin: 0.0,
            },
            PlayMatchEntity,
        ))
        .id();
    let mut bobbed = false;
    for _ in 0..30 {
        h.tick();
        let Some(r) = h.app.world().get::<DispelRibbon>(ribbon) else {
            break;
        };
        let progress = r.lifetime / r.initial_lifetime;
        let root = h.app.world().get::<Transform>(w).unwrap().translation;
        let body_y = h.global(h.body.unwrap()).translation().y;
        bobbed |= (body_y - root.y).abs() > 0.1;
        let want = root + Vec3::Y * (RIBBON_START + ribbon_climb(progress) * RIBBON_RISE);
        assert_vec(
            "priest ribbon on the root",
            h.global(ribbon).translation(),
            want,
            1e-4,
        );
    }
    assert!(bobbed, "the body moved off the root");
    let m = h.material(ribbon);
    let base = m.base_color.to_linear();
    assert_rgb(
        "priest base",
        [base.red, base.green, base.blue],
        linear(SILVER).map(|c| c * RIBBON_LEVEL),
    );
    let mut q = h.app.world_mut().query::<&DispelRibbon>();
    let r = q.iter(h.app.world()).next().unwrap();
    let age = RIBBON_LIFE - r.lifetime;
    let ignite = 1.0 + 3.0 * (-age / 0.28f32).exp();
    assert_rgb(
        "priest emissive",
        [m.emissive.red, m.emissive.green, m.emissive.blue],
        SILVER_EMISSIVE.map(|c| c * RIBBON_TRIM * RIBBON_LEVEL * ignite),
    );
}

/// The hands: the Holy launch on both spell hands of the wearer's body, one
/// flare of 0.45 s; none on a unit with no rendered body, whose ribbon still
/// plays.
#[test]
fn the_hands_are_the_holy_launch_on_the_wearer() {
    let mut h = Harness::new();
    h.stunned();
    h.use_medallion();
    h.tick();
    let hands = h.hands();
    assert_eq!(hands.len(), 2, "both hands");
    for (rig, kind, phase, caster, body) in &hands {
        assert_eq!(*kind, HealCastKind::Holy);
        assert_eq!((*caster, *body), (h.wearer, h.body.unwrap()));
        let parent = h.app.world().get::<ChildOf>(*rig).unwrap().parent();
        assert_eq!(parent, h.body.unwrap(), "rides the drawn body");
        let HealCastPhase::Flare { remaining } = phase else {
            panic!("a launch, not a loop: {phase:?}");
        };
        assert!(close(*remaining, HANDS_SECS - h.dt), "{remaining}");
    }
    let mut seen = 1;
    while !h.hands().is_empty() {
        h.tick();
        seen += 1;
    }
    assert_eq!(seen, frames_to(h.dt, HANDS_SECS));

    let mut bare = Harness::build(TICK, false);
    bare.stunned();
    bare.use_medallion();
    bare.tick();
    assert!(bare.hands().is_empty());
    assert_eq!(bare.ribbons().len(), 1);
    assert_eq!(bare.rig().1.scatter, Some(bare.dt));
}

/// A dead wearer's use resolves nothing, so nothing is drawn.
#[test]
fn a_dead_wearers_use_draws_nothing() {
    let mut h = Harness::new();
    h.stunned();
    h.kill();
    h.use_medallion();
    h.tick();
    assert!(h.ribbons().is_empty());
    assert!(h.hands().is_empty());
    assert_eq!(h.count::<MedallionBreak>(), 0);
}

// ── The drawn body ──────────────────────────────────────────────────────────

/// The ribbon stands on the body as drawn THIS frame: a wearer that broke a
/// Cyclone is set down by the lift over the ribbon's life, and runs and bobs
/// as it does. A ribbon placed off the root, or off last frame's body, misses
/// it. Played with the real Cyclone lift (0.35 yd), and with the body held
/// 1.5 yd up and set down at 2 yd/s beneath it.
#[test]
fn the_ribbon_follows_the_drawn_body_down_out_of_a_cyclone() {
    for held in [0.0, 1.5] {
        follows_the_drawn_body(held);
    }
}

fn follows_the_drawn_body(held: f32) {
    let mut h = Harness::new();
    h.put(aura(AuraType::Cyclone, "Cyclone"));
    h.app.world_mut().resource_mut::<Motion>().lift = held;
    h.ticks(90);
    let body = h.body.unwrap();
    assert!(h.app.world().get::<CycloneLift>(body).is_some(), "lifted");
    let lifted = h.global(body).translation().y - h.global(h.wearer).translation().y;
    assert!(lifted > held + 0.25, "the body hangs {lifted} yd up");
    {
        let mut m = h.app.world_mut().resource_mut::<Motion>();
        m.run = Vec3::new(6.0, 0.0, 2.0);
        m.bob = 0.08;
        m.lift_rate = -2.0;
    }
    h.use_medallion();
    let mut drops = Vec::new();
    let mut frames = 0;
    loop {
        h.tick();
        frames += 1;
        let Some(ribbon) = h
            .ribbons()
            .into_iter()
            .find(|r| r.2 == RibbonSource::Medallion)
            .map(|r| (r.0, r.3))
        else {
            break;
        };
        let progress = (ribbon.1 / RIBBON_LIFE).max(0.0);
        let drawn = h.global(body).translation();
        drops.push(drawn.y);
        let want = drawn + Vec3::Y * (RIBBON_START + ribbon_climb(progress) * RIBBON_RISE);
        assert_vec(
            &format!("frame {frames}"),
            h.global(ribbon.0).translation(),
            want,
            1e-3,
        );
    }
    assert_eq!(frames, frames_to(h.dt, RIBBON_LIFE));
    // Non-vacuity: the body came down a long way while the ribbon played,
    // and moved every frame.
    let fell = drops.first().unwrap() - drops.iter().cloned().fold(f32::INFINITY, f32::min);
    assert!(fell > held + 0.2, "the body fell {fell}");
    assert!(drops.windows(2).filter(|w| w[0] != w[1]).count() > drops.len() / 2);
}

/// The scatter's beads never sink behind the body they burst from: a bead
/// whose translation is deeper than the body centre (so the blended body
/// paints over it) never overlaps the body on screen. At the match camera, a
/// low one and a near one, from four sides, with the wearer running.
#[test]
fn no_scattering_bead_is_buried_by_the_body() {
    // Non-vacuity: beads do sort behind the body, from every camera.
    let mut behind = 0;
    for (distance, pitch) in [(60.0, 38.7), (60.0, 12.0), (14.0, 38.7), (14.0, 8.0)] {
        for yaw in [0.0, 1.6, 3.2, 4.7] {
            let mut h = Harness::new();
            let cam = camera_at(WEARER_AT, distance, pitch, yaw);
            h.camera(cam);
            h.stunned();
            h.app.world_mut().resource_mut::<Motion>().run = Vec3::new(3.0, 0.0, -2.0);
            let (rig, _) = h.rig();
            h.use_medallion();
            while h.rigs().iter().any(|(e, _)| *e == rig) {
                let centre = h.global(h.body.unwrap()).translation();
                let eye = cam.translation;
                let forward = cam.forward().as_vec3();
                let centre_depth = (centre - eye).dot(forward);
                for (bead, _, _) in h.beads(rig) {
                    let g = h.global(bead);
                    let p = g.translation();
                    let core = g.scale().x / SPARKLE_SPAN;
                    if (p - eye).dot(forward) <= centre_depth {
                        continue;
                    }
                    // The bead is drawn under the body: its core must clear
                    // the body's silhouette along its view ray.
                    let ray = (p - eye).normalize();
                    let clearance = (0..=40)
                        .map(|i| {
                            let s = centre + Vec3::Y * (-0.75 + 1.5 * i as f32 / 40.0);
                            let v = s - eye;
                            (v - ray * v.dot(ray)).length()
                        })
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        clearance > 0.5 + core,
                        "cam {distance}/{pitch}/{yaw}: a bead {clearance} off the body axis is buried"
                    );
                    behind += 1;
                }
                h.tick();
            }
        }
    }
    assert!(behind > 100, "{behind} bead-frames sorted behind the body");
}

// ── Facing ──────────────────────────────────────────────────────────────────

/// The scattering beads and the ribbon's sparks face the camera: each quad's
/// Z is the camera's back and its X is square to the screen.
#[test]
fn the_beads_and_sparks_face_the_camera() {
    let mut h = Harness::new();
    let cam = camera_at(WEARER_AT, 60.0, 38.7, 0.9);
    h.camera(cam);
    h.stunned();
    let (rig, _) = h.rig();
    h.use_medallion();
    let back = cam.back().as_vec3();
    let right = cam.right().as_vec3();
    let (mut beads, mut sparks) = (0, 0);
    for _ in 0..frames_to(h.dt, RIBBON_LIFE) {
        h.tick();
        if h.rigs().iter().any(|(e, _)| *e == rig) {
            for (bead, _, _) in h.beads(rig) {
                let r = h.global(bead).rotation();
                assert_vec("bead Z", r * Vec3::Z, back, 1e-3);
                assert_vec("bead X", r * Vec3::X, right, 1e-3);
                beads += 1;
            }
        }
        let mut q = h
            .app
            .world_mut()
            .query_filtered::<&GlobalTransform, With<DispelSpark>>();
        for g in q.iter(h.app.world()) {
            assert_vec("spark Z", g.rotation() * Vec3::Z, back, 1e-3);
            assert_vec("spark X", g.rotation() * Vec3::X, right, 1e-3);
            sparks += 1;
        }
    }
    assert!(beads > 0 && sparks > 0, "{beads} beads, {sparks} sparks");
}

// ── Frame-rate independence ─────────────────────────────────────────────────

/// At 8, 16 and 33 ms a frame, the scatter, the ribbon and the hands each
/// last exactly the frames their seconds take, and the rig's age is the
/// frames times the tick.
#[test]
fn every_layer_lasts_its_seconds_at_any_frame_rate() {
    for ms in [8u64, 16, 33] {
        let mut h = Harness::build(Duration::from_millis(ms), true);
        h.put(kidney_shot());
        let up = 20;
        h.ticks(up);
        let (rig, view) = h.rig();
        assert!(
            close(view.age, summed(h.dt, up)),
            "{ms} ms: age {}",
            view.age
        );
        h.use_medallion();
        let (mut scatter_frames, mut ribbon_frames, mut hand_frames) = (None, None, None);
        for frame in 1..=200u32 {
            h.tick();
            let rig_now = h.rigs().into_iter().find(|(e, _)| *e == rig);
            if let Some((_, v)) = rig_now {
                assert!(close(v.age, summed(h.dt, up + frame)), "{ms} ms age");
            } else if scatter_frames.is_none() {
                scatter_frames = Some(frame);
            }
            if h.ribbons().is_empty() && ribbon_frames.is_none() {
                ribbon_frames = Some(frame);
            }
            if h.hands().is_empty() && hand_frames.is_none() {
                hand_frames = Some(frame);
            }
        }
        assert_eq!(
            scatter_frames,
            Some(frames_to(h.dt, SCATTER_SECS)),
            "{ms} ms"
        );
        assert_eq!(ribbon_frames, Some(frames_to(h.dt, RIBBON_LIFE)), "{ms} ms");
        assert_eq!(hand_frames, Some(frames_to(h.dt, HANDS_SECS)), "{ms} ms");
    }
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

/// Beside other auras, a wearer that dies mid-scatter still sees the scatter
/// out, and the ribbon plays out on its body; one despawned mid-scatter takes
/// the rig with it and leaves the ribbon to finish where it stood; and the
/// match's end clears every rig and marker, each of which carries
/// `PlayMatchEntity`.
#[test]
fn the_break_survives_death_and_despawn_and_ends_with_the_match() {
    let others = || {
        [
            aura(AuraType::Root, "Frost Nova"),
            aura(AuraType::DamageOverTime, "Corruption"),
            aura(AuraType::MaxHealthIncrease, "Power Word: Fortitude"),
        ]
    };
    // Death.
    let mut h = Harness::new();
    h.camera(camera_at(WEARER_AT, 60.0, 38.7, 0.0));
    for a in others() {
        h.put(a);
    }
    h.stunned_with_root();
    let stun = h.stun_rig();
    h.use_medallion();
    h.tick();
    assert!(h.app.world().get::<PlayMatchEntity>(stun).is_some());
    h.kill();
    let mut frames = 1;
    while h.rigs().iter().any(|(e, _)| *e == stun) {
        h.tick();
        frames += 1;
    }
    assert_eq!(frames, frames_to(h.dt, SCATTER_SECS), "the scatter ran out");
    h.ticks(frames_to(h.dt, RIBBON_LIFE));
    assert!(h.ribbons().is_empty());

    // Despawn.
    let mut h = Harness::new();
    for a in others() {
        h.put(a);
    }
    h.stunned_with_root();
    h.use_medallion();
    h.tick();
    let ribbon = h.medallion_ribbon();
    for (rig, _) in h.rigs() {
        assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());
    }
    h.ticks(3);
    let stood = h.global(ribbon).translation();
    let w = h.wearer;
    h.app.world_mut().entity_mut(w).despawn();
    h.tick();
    assert_eq!(h.count::<CcRig>(), 0, "the rigs go with their owner");
    h.tick();
    assert_vec("ribbon holds", h.global(ribbon).translation(), stood, 0.2);
    h.ticks(frames_to(h.dt, RIBBON_LIFE));
    assert!(h.ribbons().is_empty());

    // Match end, mid-everything.
    let mut h = Harness::new();
    h.camera(camera_at(WEARER_AT, 60.0, 38.7, 0.0));
    for a in others() {
        h.put(a);
    }
    h.stunned_with_root();
    h.use_medallion();
    h.ticks(frames_to(h.dt, RIBBON_LIFE * 0.7));
    assert!(h.count::<DispelSpark>() > 0 && h.count::<DispelRibbon>() > 0);
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    assert_eq!(h.count::<CcRig>(), 0);
    assert_eq!(h.count::<CcBead>(), 0);
    assert_eq!(h.count::<DispelRibbon>(), 0);
    assert_eq!(h.count::<DispelSpark>(), 0);
    assert_eq!(h.count::<MedallionBreak>(), 0);
    assert_eq!(h.count::<HealCastHand>(), 0);
}

/// The break the sim path hands the hard-CC group, and the ribbon, carry the
/// match's tag as they are spawned (the hard-CC group despawns the break on
/// the frame after, so it is read here before that frame runs).
#[test]
fn the_break_and_its_ribbon_are_tagged_for_the_match() {
    let mut h = Harness::new();
    h.stunned();
    h.use_medallion();
    h.app
        .world_mut()
        .run_system_once(spawn_medallion_breaks)
        .unwrap();
    let mut q = h
        .app
        .world_mut()
        .query::<(&MedallionBreak, Option<&PlayMatchEntity>)>();
    let breaks: Vec<(MedallionBreak, bool)> = q
        .iter(h.app.world())
        .map(|(b, t)| (b.clone(), t.is_some()))
        .collect();
    let w = h.wearer;
    assert_eq!(
        breaks,
        vec![(
            MedallionBreak {
                wearer: w,
                scatters: vec![CcKind::Stun],
            },
            true
        )]
    );
    let mut q = h
        .app
        .world_mut()
        .query::<(&DispelRibbon, Option<&PlayMatchEntity>)>();
    let tagged: Vec<bool> = q.iter(h.app.world()).map(|(_, t)| t.is_some()).collect();
    assert_eq!(tagged, vec![true]);
}

impl Harness {
    /// Stunned and rooted, both rigs grown.
    fn stunned_with_root(&mut self) {
        self.put(kidney_shot());
        self.ticks(30);
        assert_eq!(self.rigs().len(), 2, "a whirl and a root");
    }

    fn stun_rig(&mut self) -> Entity {
        self.rigs()
            .into_iter()
            .find(|(_, v)| v.kind == CcKind::Stun)
            .unwrap()
            .0
    }
}

// ── Through the real sim ────────────────────────────────────────────────────

const WEARER: Unit = Unit::new(1, 0);
const MATE: Unit = Unit::new(1, 1);
const PALADIN: Unit = Unit::new(2, 0);
const MAGE: Unit = Unit::new(2, 1);

/// `medallion_staged.rs`'s match: a Warrior wearing the Medallion and `mate`,
/// against a Paladin and a Mage, everyone close.
fn staged_app(mate: &str, stage: Stage) -> App {
    let mut equipment = vec![HashMap::new(), HashMap::new()];
    equipment[0].insert("Trinket2".to_string(), "MedallionOfTheAlliance".to_string());
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into(), mate.into()],
        team2: vec!["Paladin".into(), "Mage".into()],
        team1_equipment: equipment,
        random_seed: Some(7),
        max_duration_secs: 6.0,
        ..Default::default()
    };
    let stage = stage
        .place(WEARER, 0.0, 0.0)
        .place(MATE, 0.0, 12.0)
        .place(PALADIN, 3.0, 0.0)
        .place(MAGE, 0.0, -25.0);
    let mut app = build_headless_app_staged(&config, true, None, stage);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
    // The client's registration: the break in Phase 1, just before the use
    // resolves; the hard-CC group and the ribbon in Update; the placement in
    // PostUpdate.
    app.add_systems(
        FixedUpdate,
        spawn_medallion_breaks
            .after(arenasim::states::play_match::process_travel_form)
            .before(process_on_use_items),
    );
    app.add_systems(
        Update,
        (
            update_hard_cc_visuals,
            update_cc_rigs,
            billboard_cc_beads,
            scatter_cc_beads,
            update_cc_flares,
            cleanup_cc_rigs,
            cleanup_cc_flares,
            spawn_dispel_ribbon_visuals,
            update_dispel_ribbons,
            update_dispel_sparks,
            cleanup_expired_dispel_ribbons,
        )
            .chain(),
    );
    app.add_systems(PostUpdate, place_medallion_ribbons);
    app
}

/// One stun whirl on the wearer in a played match: the frames it stood, and
/// the frame each end was armed.
#[derive(Default, Debug, Clone, Copy)]
struct Whirl {
    first: u32,
    last: u32,
    scattered: Option<u32>,
    retracted: Option<u32>,
}

/// What a played match drew on the wearer: its first stun whirl, and every
/// ribbon on it, by the frame it appeared and its source.
#[derive(Default, Debug)]
struct Drawn {
    whirl: Whirl,
    ribbons: Vec<(u32, RibbonSource)>,
}

impl Drawn {
    fn ribbons_by(&self, source: RibbonSource) -> Vec<u32> {
        self.ribbons
            .iter()
            .filter(|(_, s)| *s == source)
            .map(|(f, _)| *f)
            .collect()
    }
}

fn play_drawn(mut app: App) -> Drawn {
    let mut wearer = None;
    let mut whirls: Vec<(Entity, Whirl)> = Vec::new();
    let mut ribbons_seen = BTreeSet::new();
    let mut ribbons = Vec::new();
    for frame in 0..(60 * 20) {
        app.update();
        let world = app.world_mut();
        if wearer.is_none() {
            let mut q = world.query::<(Entity, &Combatant)>();
            wearer = q
                .iter(world)
                .find(|(_, c)| c.team == 1 && c.slot == 0)
                .map(|(e, _)| e);
        }
        let Some(w) = wearer else { continue };
        let mut q = world.query::<(Entity, &CcRig)>();
        for (e, r) in q.iter(world) {
            if r.owner != w || r.kind != CcKind::Stun {
                continue;
            }
            let i = match whirls.iter().position(|(x, _)| *x == e) {
                Some(i) => i,
                None => {
                    whirls.push((
                        e,
                        Whirl {
                            first: frame,
                            ..Default::default()
                        },
                    ));
                    whirls.len() - 1
                }
            };
            let whirl = &mut whirls[i].1;
            whirl.last = frame;
            if r.scatter.is_some() {
                whirl.scattered.get_or_insert(frame);
            }
            if r.retract.is_some() {
                whirl.retracted.get_or_insert(frame);
            }
        }
        let mut q = world.query::<(Entity, &DispelRibbon)>();
        for (e, r) in q.iter(world) {
            if r.target == w && ribbons_seen.insert(e) {
                ribbons.push((frame, r.source));
            }
        }
        if world.resource::<HeadlessMatchState>().match_complete {
            break;
        }
    }
    Drawn {
        whirl: whirls.first().expect("the wearer was stunned").1,
        ribbons,
    }
}

/// A stunned Medallion wearer breaks the Hammer of Justice in a played match:
/// on the frame its whirl scatters, the gold ribbon starts on it, and the
/// whirl never retracts.
#[test]
fn a_played_break_scatters_the_whirl_and_plays_the_ribbon() {
    let stage = Stage::new().land(WEARER, AbilityType::HammerOfJustice, PALADIN);
    let d = play_drawn(staged_app("Mage", stage));
    let at = d.whirl.scattered.expect("the whirl scattered");
    assert!(d.whirl.first < at, "{d:?}");
    assert_eq!(d.whirl.retracted, None, "{d:?}");
    assert_eq!(
        d.ribbons_by(RibbonSource::Medallion),
        vec![at],
        "the ribbon starts with the scatter: {d:?}"
    );
    // The scatter runs its 0.4 s at the sim's 60 Hz.
    assert_eq!(
        d.whirl.last - at + 1,
        frames_to(1.0 / 60.0, SCATTER_SECS) - 1,
        "{d:?}"
    );
}

/// Control: a stun too short to break runs out with the Medallion kept: its
/// whirl retracts, and no gold ribbon plays while it stood.
#[test]
fn a_played_stun_that_runs_out_retracts() {
    let stun = Aura {
        effect_type: AuraType::Stun,
        duration: 1.5,
        break_on_damage_threshold: -1.0,
        ability_name: "Hammer of Justice".to_string(),
        ..Default::default()
    };
    let d = play_drawn(staged_app("Mage", Stage::new().aura(WEARER, stun)));
    let at = d.whirl.retracted.expect("the whirl retracted");
    assert_eq!(d.whirl.scattered, None, "{d:?}");
    // It retracts when the stun's 1.5 s run out (the stage lands at the
    // gates, frame 600 at 60 Hz).
    assert_eq!(d.whirl.first, 600, "{d:?}");
    assert_eq!(at - d.whirl.first, 90, "{d:?}");
    assert!(
        d.ribbons.iter().all(|(f, _)| *f > at),
        "nothing on the wearer while the stun held: {d:?}"
    );
}

/// A Medallion that waits for its Priest's Dispel Magic draws nothing: the
/// Priest's own ribbon starts as the dispelled stun retracts, and no gold
/// ribbon plays while it stood.
#[test]
fn a_medallion_waiting_on_a_dispel_draws_nothing() {
    let stage = Stage::new().land(WEARER, AbilityType::HammerOfJustice, PALADIN);
    let d = play_drawn(staged_app("Priest", stage));
    let at = d.whirl.retracted.expect("the whirl retracted");
    assert_eq!(d.whirl.scattered, None, "{d:?}");
    let priest = RibbonSource::Dispel(CharacterClass::Priest);
    assert_eq!(d.ribbons.first(), Some(&(at, priest)), "{d:?}");
    assert!(
        d.ribbons_by(RibbonSource::Medallion)
            .iter()
            .all(|f| *f > d.whirl.last),
        "{d:?}"
    );
}
