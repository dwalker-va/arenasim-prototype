//! Probes for Inner Focus's and Blessed Recovery's visuals (AS-352):
//! `rendering/effects/inner_focus.rs` — the arcane hand flash and four blue
//! comets converging on the Priest's chest as the buff lands, then the
//! violet-cyan glow on both palms until a cast spends it — and
//! `rendering/effects/blessed_recovery.rs` — Renew's amber rings at the chest,
//! once per proc.
//!
//! Everything here asserts what is DRAWN — world positions under propagated
//! `GlobalTransform`s, rotations, scales, the materials' colours, alphas and
//! blend modes — against the signed-off AS-335 Priest Talent Visuals Bench
//! (version 1792207490-53c9, the "Inner Focus" and "Blessed Recovery" tabs),
//! transcribed below straight from
//! `docs/design/benches/2026-10-17-priest-talent-bench.html` and NOT from the
//! modules' tables, so a wrong value in a module cannot agree with itself here.
//!
//! The buffs come through the sim: Inner Focus's aura from the AI's own
//! constructor (`AuraPending::from_ability`), landed by the real
//! `apply_pending_auras` (which spawns the `AuraLanding`); spent by the real
//! `talent_procs::after_turn` (as `decide_abilities` runs it after a turn that
//! chose a cast), its Flash Heal resolved — landed, interrupted or fizzled —
//! by the real `process_casting`, which gives a spent buff back
//! (`next_cast::restore`); Blessed Recovery's heal over time rolled by the
//! real `resolve_talent_procs` off a `CritTaken` event, from the shipped
//! talent's own proc, and ticked by the real `process_hot_ticks`. Runs on
//! `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the chains
//! `states/mod.rs` registers.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::{apply_pending_auras, process_hot_ticks};
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, AuraApplyOwner, AuraApplyRoute, AuraLanding, AuraLandingKind,
    AuraPending, AuraType, CastEnding, CastEndingKind, CastingState, Combatant, GameRng,
    HealCastHand, HealCastKind, HealImpact, HealImpactKind, HotVisual, LandedCast, MatchCountdown,
    PlayMatchEntity, PolymorphedVisual, VisualBody, BLESSED_RECOVERY_AURA, INNER_FOCUS_AURA,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::talent_procs::{
    after_turn, resolve_talent_procs, ProcEvent, TalentProcEvents, TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::{
    cleanup_heal_cast_glows, cleanup_play_match, consume_cast_ending_signals,
    consume_heal_cast_endings, flash_piece_pose, palm_piece_pose, place_blessed_recovery_bursts,
    place_inner_focus, spawn_blessed_recovery_bursts, spawn_heal_cast_glows,
    spawn_inner_focus_flashes, spawn_instant_cast_hands, update_blessed_recovery_bursts,
    update_heal_cast_glows, update_inner_focus_flashes, update_inner_focus_palms,
    BlessedRecoveryBurst, BlessedRecoveryParticle, FlashPiece, InnerFocusFlash,
    InnerFocusFlashPiece, InnerFocusPalm, InnerFocusPalmPiece, InnerFocusPalms, PalmPiece,
    RecoveryLayer,
};
use arenasim::CharacterClass;

/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;
/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;
/// The shipped spell-hand socket on the `VisualBody` (`heal_cast.rs`).
const HAND: Vec3 = Vec3::new(0.72, 0.05, 0.30);

// ── The bench, transcribed ───────────────────────────────────────────────────
//
// `docs/design/benches/2026-10-17-priest-talent-bench.html`, version
// 1792207490-53c9, at its default knobs (every multiplier 1). Milliseconds as
// the bench keys them where it does.

/// `ANCH.chest`.
const BENCH_CHEST: f32 = 1.45;

/// The bench's `keys` over scalars.
fn keys(k: &[(f32, f32)], t: f32) -> f32 {
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

/// The bench's `keys` over colours, truncated as `rgb()` does.
fn keys3(k: &[(f32, [f32; 3])], t: f32) -> [u8; 3] {
    let c = |i: usize| {
        let kk: Vec<(f32, f32)> = k.iter().map(|(t, v)| (*t, v[i])).collect();
        keys(&kk, t).floor() as u8
    };
    [c(0), c(1), c(2)]
}

/// Inner Focus's `customDraw`, comets: at `ms`, comet `(sx, sy)`'s head and
/// trail glow `k` (0 the head): its point in the bench's plane, its full
/// width, colour and alpha. `None` from 900 ms on.
fn bench_comet(sx: f32, sy: f32, k: u8, ms: f32) -> Option<(Vec2, f32, [u8; 3], f32)> {
    if ms >= 900.0 {
        return None;
    }
    let u = (ms / 400.0).min(1.0);
    let a = if ms < 400.0 {
        1.0
    } else {
        (1.0 - (ms - 400.0) / 500.0).max(0.0)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    if k == 0 {
        let p = Vec2::new(
            lerp(0.74 * sx, -0.37 * sx, u),
            lerp(0.74 * sy, -0.37 * sy, u),
        );
        // `sprite('star', p, 0.33*(1-u*0.6)*2, ...)`: its `size` is the width.
        Some((p, 0.33 * (1.0 - u * 0.6) * 2.0, [44, 155, 225], a))
    } else {
        let uu = (u - k as f32 * 0.06).max(0.0);
        let p = Vec2::new(
            lerp(0.74 * sx, -0.37 * sx, uu),
            lerp(0.74 * sy, -0.37 * sy, uu),
        );
        Some((p, 0.17, [0, 121, 198], a * (1.0 - k as f32 / 6.0)))
    }
}

/// The corners in the bench's loop order.
const BENCH_CORNERS: [(f32, f32); 4] = [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];

/// The pop emitter: `rate:[[0,0],[200,60],[400,60],[401,0]]`, speed 2.6, life
/// 0.6, `vRange` 3.14, star, `lighter`.
mod pop {
    pub const RATE: [(f32, f32); 4] = [(0.0, 0.0), (0.2, 60.0), (0.4, 60.0), (0.401, 0.0)];
    pub const SPEED: f32 = 2.6;
    pub const LIFE: f32 = 0.6;
    pub const COLOR: [(f32, [f32; 3]); 3] = [
        (0.0, [88.0, 0.0, 248.0]),
        (0.5, [255.0, 60.0, 60.0]),
        (1.0, [98.0, 181.0, 239.0]),
    ];
    pub const ALPHA: [(f32, f32); 2] = [(0.0, 1.0), (1.0, 0.0)];
    pub const SCALE: [(f32, f32); 3] = [(0.0, 0.02), (0.5, 0.12), (1.0, 0.03)];
}

/// The palm glows: `bbGlow(h, 0.74*pulse, ..., [150,90,240], 0.7)` and
/// `bbGlow(h, 0.4*pulse, ..., [120,220,255], 0.6)`, `pulse =
/// keys([[0,1.15],[266,0.95],[533,1.15]], ms % 533)`.
fn bench_palm(outer: bool, ms: f32) -> (f32, [u8; 3], f32) {
    let pulse = keys(&[(0.0, 1.15), (266.0, 0.95), (533.0, 1.15)], ms % 533.0);
    if outer {
        (0.74 * pulse, [150, 90, 240], 0.7)
    } else {
        (0.4 * pulse, [120, 220, 255], 0.6)
    }
}

/// The palm motes: `rate:21` (10.5 a palm here), life 0.6, speed 0.19,
/// `scale` (radius) 0.03 -> 0.14 -> 0.3, white -> lilac -> violet, alpha 0.25
/// -> 1 -> 0.04.
mod motes {
    pub const RATE_PER_PALM: f32 = 21.0 / 2.0;
    pub const LIFE: f32 = 0.6;
    pub const SPEED: f32 = 0.19;
    pub const COLOR: [(f32, [f32; 3]); 3] = [
        (0.0, [255.0, 255.0, 255.0]),
        (0.5, [200.0, 160.0, 255.0]),
        (1.0, [162.0, 42.0, 231.0]),
    ];
    pub const ALPHA: [(f32, f32); 3] = [(0.0, 0.25), (0.5, 1.0), (1.0, 0.04)];
    pub const SCALE: [(f32, f32); 3] = [(0.0, 0.03), (0.5, 0.14), (1.0, 0.3)];
}

/// Blessed Recovery's two emitters: 9.2/s until 700 ms, speed 0.18, drag 0.3,
/// life 0.76, `vRange` 3.14, glow, `lighter`.
mod renew {
    pub const RATE: f32 = 9.2;
    pub const WINDOW: f32 = 0.7;
    pub const SPEED: f32 = 0.18;
    pub const DRAG: f32 = 0.3;
    pub const LIFE: f32 = 0.76;
    pub const RING_COLOR: [(f32, [f32; 3]); 3] = [
        (0.0, [255.0, 157.0, 3.0]),
        (0.5, [250.0, 219.0, 8.0]),
        (1.0, [253.0, 179.0, 20.0]),
    ];
    pub const RING_ALPHA: [(f32, f32); 3] = [(0.0, 0.15), (0.5, 0.57), (1.0, 0.0)];
    pub const RING_SCALE: [(f32, f32); 3] = [(0.0, 0.28), (0.5, 0.49), (1.0, 1.09)];
    pub const GLOW_COLOR: [(f32, [f32; 3]); 2] =
        [(0.0, [255.0, 150.0, 40.0]), (1.0, [255.0, 250.0, 240.0])];
    pub const GLOW_ALPHA: [(f32, f32); 3] = [(0.0, 0.3), (0.5, 0.45), (1.0, 0.0)];
    pub const GLOW_SCALE: [(f32, f32); 3] = [(0.0, 0.22), (0.5, 0.7), (1.0, 0.56)];
}

/// Births of a piecewise-linear rate track: where its integral reaches 1, 2,
/// ... (the bench's accumulator, continuous). Brute force, independent of
/// the module's solve.
fn bench_births(track: &[(f32, f32)]) -> Vec<f32> {
    let mut births = Vec::new();
    let (mut acc, mut t) = (0.0_f64, 0.0_f64);
    let dt = 1e-6;
    let end = track[track.len() - 1].0 as f64 + 0.01;
    let k64: Vec<(f32, f32)> = track.to_vec();
    while t < end {
        acc += keys(&k64, (t + dt / 2.0) as f32) as f64 * dt;
        t += dt;
        if acc >= births.len() as f64 + 1.0 {
            births.push(t as f32);
        }
    }
    births
}

// ── The harness ──────────────────────────────────────────────────────────────

struct Harness {
    app: App,
    dt: f32,
}

impl Harness {
    fn new() -> Self {
        Harness::with_tick(0.016)
    }

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
        .insert_resource(TalentProcEvents::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        });
        // The sim: casts, procs, auras landing, heal-over-time ticks.
        app.add_systems(
            Update,
            (
                process_casting,
                resolve_talent_procs,
                apply_pending_auras,
                process_hot_ticks,
            )
                .chain(),
        );
        // The cast hands as `states/mod.rs` chains them (their endings read
        // in FixedUpdate there, before the markers are consumed).
        app.add_systems(
            Update,
            (
                consume_heal_cast_endings,
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                spawn_heal_cast_glows,
                update_heal_cast_glows,
                cleanup_heal_cast_glows,
            )
                .chain()
                .after(process_hot_ticks),
        );
        // This card's chains, as `states/mod.rs` registers them.
        app.add_systems(
            Update,
            (
                (
                    spawn_inner_focus_flashes,
                    update_inner_focus_flashes,
                    update_inner_focus_palms,
                )
                    .chain()
                    .after(cleanup_heal_cast_glows),
                (
                    spawn_blessed_recovery_bursts,
                    update_blessed_recovery_bursts,
                )
                    .chain()
                    .after(process_hot_ticks),
            ),
        );
        app.add_systems(
            PostUpdate,
            (place_inner_focus, place_blessed_recovery_bursts)
                .before(TransformSystem::TransformPropagate),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app, dt }
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / self.dt).round() as u32);
    }

    fn camera(&mut self, transform: Transform) -> Entity {
        self.world().spawn((Camera3d::default(), transform)).id()
    }

    /// A unit as `spawn_combatant` builds it, with a `VisualBody` child.
    fn unit(&mut self, team: u8, slot: u8, class: CharacterClass, at: Vec3) -> Entity {
        let world = self.world();
        let unit = world
            .spawn((
                Combatant::new(team, slot, class),
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

    /// A Priest with Inner Focus granted and Blessed Recovery's proc, as a
    /// Discipline-and-Holy build resolves them, plenty of mana, hurt so a
    /// heal lands health.
    fn priest(&mut self, at: Vec3) -> Entity {
        let priest = self.unit(1, 0, CharacterClass::Priest, at);
        let defs = AbilityDefinitions::default();
        let node = TalentDefinitions::shipped()
            .get(TalentId::BlessedRecovery)
            .unwrap()
            .clone();
        let proc = node
            .per_rank
            .iter()
            .find_map(|e| match e {
                TalentEffect::Proc(p) => Some(p.clone()),
                _ => None,
            })
            .expect("Blessed Recovery is a proc");
        let mut c = self.world().get_mut::<Combatant>(priest).unwrap();
        let book = c.book().clone().granting(&defs, AbilityType::InnerFocus);
        *c = c.clone().with_book(book);
        c.talent_procs.slots.push(TalentProcSlot::new(
            TalentId::BlessedRecovery,
            &node.name,
            proc,
        ));
        c.max_mana = 5000.0;
        c.current_mana = 5000.0;
        c.current_health = c.max_health * 0.5;
        priest
    }

    /// The scene: a Priest, its Warrior, and an enemy Rogue.
    fn scene(&mut self) -> (Entity, Entity, Entity) {
        let priest = self.priest(Vec3::new(-3.0, COMBATANT_Y, 0.0));
        let ally = self.unit(
            1,
            1,
            CharacterClass::Warrior,
            Vec3::new(3.0, COMBATANT_Y, 2.0),
        );
        let enemy = self.unit(
            2,
            0,
            CharacterClass::Rogue,
            Vec3::new(0.0, COMBATANT_Y, -4.0),
        );
        (priest, ally, enemy)
    }

    /// The Priest casts Inner Focus: its aura from the AI's own constructor,
    /// landed by the sim this frame.
    fn focus(&mut self, priest: Entity) {
        let defs = AbilityDefinitions::default();
        let pending =
            AuraPending::from_ability(priest, priest, defs.get(&AbilityType::InnerFocus).unwrap())
                .unwrap();
        assert_eq!(pending.aura.ability_name, INNER_FOCUS_AURA);
        self.world().spawn(pending);
        self.tick(1);
    }

    /// A turn that chose `ability` (and spent what covers it), as
    /// `decide_abilities` closes it; a cast with a bar starts its
    /// `CastingState` the same turn.
    fn choose(&mut self, priest: Entity, ability: AbilityType, bar: Option<(Entity, f32)>) {
        self.world()
            .run_system_once(
                move |mut commands: Commands,
                      mut log: ResMut<CombatLog>,
                      mut q: Query<(&mut Combatant, &mut ActiveAuras)>| {
                    let (mut c, mut auras) = q.get_mut(priest).unwrap();
                    after_turn(
                        &mut commands,
                        &mut log,
                        priest,
                        &mut c,
                        Some(&mut auras),
                        &[ability],
                        Some(ability),
                    );
                },
            )
            .unwrap();
        if let Some((target, secs)) = bar {
            self.world()
                .entity_mut(priest)
                .insert(CastingState::new(ability, target, secs));
        }
    }

    /// A crit lands on the Priest: the `CritTaken` event its damage site
    /// pushes for a unit that listens.
    fn crit(&mut self, priest: Entity, by: Entity, amount: f32) {
        self.world().resource_mut::<TalentProcEvents>().0.push(
            ProcEvent::new(priest, TalentTrigger::CritTaken)
                .amount(amount)
                .other(by),
        );
    }

    fn holds_focus(&mut self, priest: Entity) -> bool {
        self.app
            .world()
            .get::<ActiveAuras>(priest)
            .unwrap()
            .auras
            .iter()
            .any(|a| a.ability_name == INNER_FOCUS_AURA)
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.world().query::<&T>();
        q.iter(self.app.world()).count()
    }

    fn flashes(&mut self) -> Vec<(Entity, Entity, f32, Vec3, Visibility)> {
        let mut q = self
            .world()
            .query::<(Entity, &InnerFocusFlash, &GlobalTransform, &Visibility)>();
        q.iter(self.app.world())
            .map(|(e, f, g, v)| (e, f.priest, f.age, g.translation(), *v))
            .collect()
    }

    fn palm_rigs(&mut self) -> Vec<(Entity, Entity, f32, Visibility)> {
        let mut q = self
            .world()
            .query::<(Entity, &InnerFocusPalms, &Visibility)>();
        q.iter(self.app.world())
            .map(|(e, r, v)| (e, r.priest, r.age, *v))
            .collect()
    }

    /// Each palm: its side, whether it is shown (its own and its rig's
    /// visibility), and where it stands.
    fn palms(&mut self) -> Vec<(f32, bool, Vec3)> {
        let mut q = self
            .world()
            .query::<(&InnerFocusPalm, &Visibility, &ChildOf, &GlobalTransform)>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(p, v, c, g)| (p.side, *v, c.parent(), g.translation()))
            .collect();
        rows.into_iter()
            .map(|(side, v, rig, at)| {
                let rig_v = *self.app.world().get::<Visibility>(rig).unwrap();
                (
                    side,
                    v != Visibility::Hidden && rig_v != Visibility::Hidden,
                    at,
                )
            })
            .collect()
    }

    fn shown_sides(&mut self) -> BTreeSet<i32> {
        self.palms()
            .into_iter()
            .filter(|(_, shown, _)| *shown)
            .map(|(side, _, _)| side as i32)
            .collect()
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity, f32)> {
        let mut q = self.world().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster, h.side))
            .collect()
    }

    fn bursts(&mut self) -> Vec<(Entity, Entity, f32, Vec3, Visibility)> {
        let mut q = self
            .world()
            .query::<(Entity, &BlessedRecoveryBurst, &GlobalTransform, &Visibility)>();
        q.iter(self.app.world())
            .map(|(e, b, g, v)| (e, b.priest, b.age, g.translation(), *v))
            .collect()
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

    fn flash_pieces(&mut self) -> Vec<Drawn<FlashPiece>> {
        let mut q = self
            .world()
            .query::<(Entity, &InnerFocusFlashPiece, &GlobalTransform)>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(e, p, g)| (e, p.piece, *g))
            .collect();
        rows.into_iter()
            .map(|(e, piece, g)| Drawn::read(self, e, piece, g))
            .collect()
    }

    fn palm_pieces(&mut self) -> Vec<(f32, Drawn<PalmPiece>)> {
        let mut q = self
            .world()
            .query::<(Entity, &InnerFocusPalmPiece, &ChildOf, &GlobalTransform)>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(e, p, c, g)| (e, p.piece, c.parent(), *g))
            .collect();
        rows.into_iter()
            .map(|(e, piece, palm, g)| {
                let side = self.app.world().get::<InnerFocusPalm>(palm).unwrap().side;
                (side, Drawn::read(self, e, piece, g))
            })
            .collect()
    }

    fn burst_particles(&mut self) -> Vec<Drawn<(RecoveryLayer, u8)>> {
        let mut q = self
            .world()
            .query::<(Entity, &BlessedRecoveryParticle, &GlobalTransform)>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(e, p, g)| (e, (p.piece.layer, p.piece.j), *g))
            .collect();
        rows.into_iter()
            .map(|(e, piece, g)| Drawn::read(self, e, piece, g))
            .collect()
    }
}

/// One drawn piece, read back from the world.
#[derive(Clone, Copy, Debug)]
struct Drawn<P> {
    piece: P,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    rgb: [u8; 3],
    alpha: f32,
    alpha_mode: AlphaMode,
}

impl<P: Copy> Drawn<P> {
    fn read(h: &Harness, entity: Entity, piece: P, g: GlobalTransform) -> Self {
        let m = h.material(entity);
        let c = m.base_color.to_srgba();
        let (scale, rotation, at) = g.to_scale_rotation_translation();
        Drawn {
            piece,
            at,
            rotation,
            scale,
            rgb: [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8),
            alpha: c.alpha,
            alpha_mode: m.alpha_mode,
        }
    }

    fn shown(&self) -> bool {
        self.alpha > 0.0 && self.scale.x > 2e-3
    }
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

/// The depth of a combatant body's nearest point along `forward`, its capsule
/// centred at `centre`.
fn body_nearest_depth(centre: Vec3, eye: Vec3, forward: Vec3) -> f32 {
    [
        centre - Vec3::Y * BODY_HALF_SEGMENT,
        centre + Vec3::Y * BODY_HALF_SEGMENT,
    ]
    .iter()
    .map(|e| (*e - eye).dot(forward))
    .fold(f32::MAX, f32::min)
        - BODY_RADIUS
}

fn level(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z).normalize()
}

/// Two rotations within a few millionths of a radian of each other (by their
/// dot product: `angle_between`'s `acos` is ill-conditioned there).
fn same_turn(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 1.0 - 1e-6
}

/// A drawn colour the bench's within a byte a channel (its keys and the
/// module's interpolate in different float orders before truncating).
fn rgb_near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (*x as i32 - y as i32).abs() <= 1)
}

fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Inner Focus's buff lands its flash and Blessed Recovery's heal over time
/// its rings, each by name: both are owned off the aura band, the flash names
/// Inner Focus as its cast (whose hands are the arcane ones, flashed off the
/// landing and never off a `Landed` marker), and the heal over time draws
/// nothing on its ticks. Siblings by type keep their own: Divine Favor's
/// primed heal, Shadow Trance, an unnamed next-cast modifier and an unnamed
/// heal over time.
#[test]
fn the_routes_name_the_flash_and_the_rings_and_nothing_else() {
    use AuraLandingKind as L;
    assert_eq!(
        L::for_aura(AuraType::NextCast, INNER_FOCUS_AURA),
        Some(L::InnerFocusFlash)
    );
    assert_eq!(
        L::for_aura(AuraType::HealingOverTime, BLESSED_RECOVERY_AURA),
        Some(L::BlessedRecoveryRings)
    );
    assert_eq!(
        HotVisual::for_hot(BLESSED_RECOVERY_AURA),
        Some(HotVisual::LandingRings)
    );
    assert_eq!(
        HealImpact::kind_for_hot_tick(AuraType::HealingOverTime, BLESSED_RECOVERY_AURA),
        None,
        "its ticks draw nothing"
    );
    for (t, name) in [
        (AuraType::NextCast, INNER_FOCUS_AURA),
        (AuraType::HealingOverTime, BLESSED_RECOVERY_AURA),
    ] {
        assert_eq!(
            AuraApplyRoute::for_instance(t, name),
            AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding),
            "{name}"
        );
    }
    assert_eq!(
        L::InnerFocusFlash.cast_ability(),
        Some(AbilityType::InnerFocus)
    );
    assert_eq!(L::BlessedRecoveryRings.cast_ability(), None);
    assert!(L::flashes_hands_for(AbilityType::InnerFocus));
    assert_eq!(
        HealCastKind::for_ability(AbilityType::InnerFocus),
        Some(HealCastKind::Arcane)
    );
    // The siblings.
    assert_eq!(
        L::for_aura(AuraType::NextCast, "Divine Favor"),
        Some(L::DivineFavorBurst)
    );
    assert_eq!(
        L::for_aura(AuraType::NextCast, "Shadow Trance"),
        Some(L::ShadowTrance)
    );
    assert_eq!(L::for_aura(AuraType::NextCast, "Some Other Focus"), None);
    assert_eq!(L::for_aura(AuraType::HealingOverTime, "Renew"), None);
    assert_eq!(
        L::for_aura(AuraType::HealingOverTime, "Some Other Heal"),
        None
    );
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::NextCast, "Some Other Focus"),
        AuraApplyRoute::Band
    );
    // `ALL` names every landing once, these two among them.
    let all: Vec<L> = L::ALL.to_vec();
    for kind in [L::InnerFocusFlash, L::BlessedRecoveryRings] {
        assert_eq!(all.iter().filter(|k| **k == kind).count(), 1, "{kind:?}");
    }
    for (i, a) in all.iter().enumerate() {
        assert!(!all[i + 1..].contains(a), "{a:?} twice in ALL");
    }
}

// ── Inner Focus: the real sim path ───────────────────────────────────────────

/// Inner Focus lands: one flash at the Priest's chest, the arcane hands on
/// both of its spell hands (and on no one else), and the palm state rising
/// the same frame — hidden on each hand while the arcane hands play there,
/// shown once they have gone, both palms at the shipped spell-hand sockets.
#[test]
fn the_cast_flashes_the_arcane_hands_raises_the_comets_and_the_palms_yield() {
    let mut h = Harness::new();
    let (priest, ally, _) = h.scene();
    h.focus(priest);
    let flashes = h.flashes();
    assert_eq!(flashes.len(), 1, "one flash");
    assert_eq!(flashes[0].1, priest);
    let mut hands = h.hands();
    hands.sort_by(|a, b| a.2.total_cmp(&b.2));
    assert_eq!(
        hands,
        vec![
            (HealCastKind::Arcane, priest, -1.0),
            (HealCastKind::Arcane, priest, 1.0)
        ],
        "the arcane hands on both of the Priest's hands"
    );
    let rigs = h.palm_rigs();
    assert_eq!(rigs.len(), 1, "one palm state");
    assert_eq!(rigs[0].1, priest);
    assert_eq!(h.palms().len(), 2);
    assert!(
        h.shown_sides().is_empty(),
        "the palms yield to the arcane hands"
    );
    // Through the hands' kit: still yielding, every frame of it.
    let mut frames = 0;
    while !h.hands().is_empty() {
        assert!(h.shown_sides().is_empty(), "yielding while a hand plays");
        h.tick(1);
        frames += 1;
        assert!(frames < 200, "the arcane kit ends");
    }
    assert!(
        close(frames as f32 * h.dt, 1.0, 2.0 * h.dt),
        "the arcane kit runs its 1.0 s ({frames} frames)"
    );
    h.tick(1);
    assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]), "both palms shown");
    // Each palm on its rendered spell hand.
    let unit = *h.app.world().get::<Transform>(priest).unwrap();
    for (side, _, at) in h.palms() {
        let want = unit.transform_point(Vec3::new(HAND.x * side, HAND.y, HAND.z));
        assert!((at - want).length() < 1e-4, "palm {side}: {at} vs {want}");
    }
    // Nothing on the ally.
    assert!(h.flashes().iter().all(|f| f.1 != ally));
    assert!(h.hands().is_empty());
    // Every rig is torn down with the match.
    for (rig, ..) in h.flashes().into_iter().chain(
        h.palm_rigs()
            .into_iter()
            .map(|(e, p, a, v)| (e, p, a, Vec3::ZERO, v)),
    ) {
        assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());
    }
}

/// A `Landed` marker naming Inner Focus (the animation sandbox lands every
/// entry through a zero-length cast) flashes no hands of its own: they come
/// off the aura's landing alone, so they flash once. The same marker naming
/// Moonfire, whose arcane hands DO come off it, flashes them: the path is
/// live.
#[test]
fn a_landed_marker_flashes_no_second_pair_of_hands() {
    for (ability, flashes) in [
        (AbilityType::InnerFocus, false),
        (AbilityType::Moonfire, true),
    ] {
        let mut h = Harness::new();
        let (priest, _, _) = h.scene();
        h.world().spawn((
            CastEnding {
                caster: priest,
                kind: CastEndingKind::Landed,
            },
            LandedCast { ability },
        ));
        h.tick(1);
        let hands = h.hands();
        assert_eq!(
            hands.len(),
            if flashes { 2 } else { 0 },
            "{ability:?}: {hands:?}"
        );
        assert!(hands
            .iter()
            .all(|(k, c, _)| *k == HealCastKind::Arcane && *c == priest));
    }
}

/// The yield is per hand and per Priest: take the arcane rig off one hand and
/// only that palm shows; a cast-hand rig of ANOTHER Priest hides nothing.
#[test]
fn a_palm_yields_only_to_its_own_hand_of_its_own_priest() {
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    let other = h.unit(
        1,
        2,
        CharacterClass::Priest,
        Vec3::new(6.0, COMBATANT_Y, -2.0),
    );
    h.focus(priest);
    let mut q = h.world().query::<(Entity, &HealCastHand)>();
    let main: Vec<Entity> = q
        .iter(h.app.world())
        .filter(|(_, r)| r.side > 0.0)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(main.len(), 1);
    h.world().entity_mut(main[0]).despawn();
    h.tick(1);
    assert_eq!(
        h.shown_sides(),
        BTreeSet::from([1]),
        "the main palm shows; the off one still yields"
    );
    // The hands run out; then another Priest flashes its own.
    h.tick_secs(1.1);
    assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]));
    // A Flash Heal begun by the other Priest: its Holy loop on its hands.
    h.world()
        .entity_mut(other)
        .insert(CastingState::new(AbilityType::FlashHeal, priest, 1.5));
    {
        let mut c = h.world().get_mut::<Combatant>(other).unwrap();
        c.current_mana = 5000.0;
    }
    h.tick(1);
    assert!(
        h.hands().iter().any(|(_, caster, _)| *caster == other),
        "the other Priest's hands play"
    );
    assert_eq!(
        h.shown_sides(),
        BTreeSet::from([-1, 1]),
        "another Priest's hands hide nothing"
    );
}

/// Spent by an instant: the palms go the frame the cast is chosen, and no
/// flash or hand replays.
#[test]
fn an_instant_spends_it_and_takes_the_palms() {
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick_secs(1.2);
    assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]));
    h.choose(priest, AbilityType::PowerWordShield, None);
    assert!(!h.holds_focus(priest), "spent");
    h.tick(1);
    assert!(h.palm_rigs().is_empty(), "the palms go with the buff");
    assert_eq!(h.count::<InnerFocusPalmPiece>(), 0);
    h.tick_secs(1.0);
    assert!(h.palm_rigs().is_empty());
    assert!(h.flashes().is_empty(), "nothing replays");
}

/// Spent by a cast with a bar, in each of its three endings. A Flash Heal
/// that lands keeps it spent: the palms stay down through the cast and after
/// it. One interrupted, or one that fizzles on a target dead at completion,
/// gives it back: the palms rise again (once the Holy hands have stopped) and
/// the comets and arcane hands do not replay, a restore being no landing.
#[test]
fn a_cast_spends_it_and_one_that_does_not_land_gives_the_palms_back() {
    for ending in ["lands", "interrupted", "fizzles"] {
        let mut h = Harness::new();
        let (priest, ally, _) = h.scene();
        h.focus(priest);
        h.tick_secs(1.2);
        assert!(h.flashes().is_empty(), "{ending}: the flash has played out");
        assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]), "{ending}");
        h.choose(priest, AbilityType::FlashHeal, Some((ally, 1.5)));
        assert!(!h.holds_focus(priest), "{ending}: spent as it is chosen");
        h.tick(1);
        assert!(h.palm_rigs().is_empty(), "{ending}: down through the cast");
        assert!(
            h.hands()
                .iter()
                .all(|(k, c, _)| *k == HealCastKind::Holy && *c == priest),
            "{ending}: the Holy loop, nothing arcane"
        );
        h.tick_secs(0.5);
        assert!(h.palm_rigs().is_empty(), "{ending}");
        match ending {
            "interrupted" => {
                let mut cast = h.world().get_mut::<CastingState>(priest).unwrap();
                cast.interrupted = true;
                cast.interrupted_display_time = 0.5;
            }
            "fizzles" => {
                h.world().get_mut::<Combatant>(ally).unwrap().current_health = 0.0;
            }
            _ => {}
        }
        h.tick_secs(1.2);
        assert!(
            h.world().get::<CastingState>(priest).is_none(),
            "{ending}: the cast is over"
        );
        h.tick(2);
        let back = ending != "lands";
        assert_eq!(h.holds_focus(priest), back, "{ending}: the buff");
        assert_eq!(h.palm_rigs().len(), back as usize, "{ending}: the palms");
        if back {
            // Shown once no hand rig is left on the Priest.
            h.tick_secs(0.6);
            assert!(h.hands().is_empty(), "{ending}: the hands are done");
            assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]), "{ending}");
        }
        assert!(h.flashes().is_empty(), "{ending}: no comets replay");
        assert!(
            h.hands().iter().all(|(k, ..)| *k != HealCastKind::Arcane),
            "{ending}: no arcane hands replay"
        );
    }
}

/// Lifecycle with other buffs on the Priest: a purge of a sibling buff leaves
/// the palms; a purge of Inner Focus takes them; a sheep hides palms and flash
/// (their clocks running on) and its end shows them again; death takes the
/// palms but lets a flash play out; a Priest that is gone takes everything;
/// the match ending takes everything. A next-cast buff by another name raises
/// no palms.
#[test]
fn the_palms_and_the_flash_live_and_die_with_the_priest() {
    let defs = AbilityDefinitions::default();
    let dress = |h: &mut Harness, priest: Entity| {
        for ability in [
            AbilityType::PowerWordFortitude,
            AbilityType::PowerWordShield,
            AbilityType::DivineFavor,
        ] {
            let aura = AuraPending::from_ability(priest, priest, defs.get(&ability).unwrap())
                .unwrap()
                .aura;
            h.world()
                .get_mut::<ActiveAuras>(priest)
                .unwrap()
                .auras
                .push(aura);
        }
    };
    let remove = |h: &mut Harness, priest: Entity, name: &str| {
        h.world()
            .get_mut::<ActiveAuras>(priest)
            .unwrap()
            .auras
            .retain(|a| a.ability_name != name);
    };

    // Another next-cast buff raises nothing; a sibling's purge leaves them.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    dress(&mut h, priest);
    h.tick(3);
    assert!(h.palm_rigs().is_empty(), "Divine Favor is not Inner Focus");
    h.focus(priest);
    h.tick_secs(1.2);
    assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]));
    remove(&mut h, priest, "Power Word: Fortitude");
    remove(&mut h, priest, "Divine Favor");
    h.tick(2);
    assert_eq!(h.palm_rigs().len(), 1, "a sibling's purge leaves them");
    remove(&mut h, priest, INNER_FOCUS_AURA);
    h.tick(1);
    assert!(
        h.palm_rigs().is_empty(),
        "a purge of Inner Focus takes them"
    );

    // A sheep hides both, clocks running on; its end shows them.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick(2);
    h.world().entity_mut(priest).insert(PolymorphedVisual);
    h.tick(1);
    assert_eq!(h.flashes()[0].4, Visibility::Hidden, "the flash on a sheep");
    let age0 = h.palm_rigs()[0].2;
    h.tick_secs(1.2);
    assert!(h.shown_sides().is_empty(), "no palms on a sheep");
    assert!(h.palm_rigs()[0].2 > age0 + 1.1, "the clock runs on");
    h.world().entity_mut(priest).remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(h.shown_sides(), BTreeSet::from([-1, 1]));

    // A Priest focused while a sheep is hidden from its first frame.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.world().entity_mut(priest).insert(PolymorphedVisual);
    h.focus(priest);
    assert_eq!(h.flashes()[0].4, Visibility::Hidden);
    assert_eq!(h.palm_rigs()[0].3, Visibility::Hidden);

    // Death: the palms go (the aura stays on the corpse), the flash plays out.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick(2);
    h.world()
        .get_mut::<Combatant>(priest)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert!(h.holds_focus(priest), "the aura stays on the corpse");
    assert!(h.palm_rigs().is_empty(), "the palms go with the Priest");
    assert_eq!(h.flashes().len(), 1, "the flash plays out");
    h.tick_secs(1.1);
    assert!(h.flashes().is_empty(), "and ends");

    // Gone: everything goes at once.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick(2);
    h.world().entity_mut(priest).despawn();
    h.tick(1);
    assert!(h.flashes().is_empty());
    assert!(h.palm_rigs().is_empty());
    assert_eq!(h.count::<InnerFocusFlashPiece>(), 0);
    assert_eq!(h.count::<InnerFocusPalmPiece>(), 0);

    // The match ends mid-flash.
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick(2);
    let rigs: Vec<Entity> = h
        .flashes()
        .into_iter()
        .map(|f| f.0)
        .chain(h.palm_rigs().into_iter().map(|r| r.0))
        .collect();
    assert_eq!(rigs.len(), 2);
    for rig in rigs {
        assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());
    }
    h.world().run_system_cached(cleanup_play_match).unwrap();
    assert!(h.flashes().is_empty());
    assert!(h.palm_rigs().is_empty());
    assert_eq!(h.count::<InnerFocusFlashPiece>(), 0);
    assert_eq!(h.count::<InnerFocusPalmPiece>(), 0);
    assert_eq!(h.count::<InnerFocusPalm>(), 0);
}

// ── Inner Focus: what is drawn ───────────────────────────────────────────────

/// The flash's pieces against the bench, with no camera (nothing held): four
/// heads and their twenty trail glows on the comets' paths in the Priest's
/// own upright plane, 0.3 yd in front of it — turned with a Priest facing
/// 0.7 rad off +Z — each the bench's width, colour and alpha at every frame;
/// the heads and trails BLENDED (`customDraw`'s `source-over`), unturned
/// (roll 0, X square to the view); nothing drawn from 900 ms on.
#[test]
fn the_comets_fly_the_benchs_paths_in_the_priests_plane() {
    for dt in [0.008, 0.016, 0.033] {
        comets_at(dt);
    }
}

/// The comets at `dt`-second frames: the flash's clock is the harness's own
/// (frames × dt since the landing's frame), and the flash lives its second.
fn comets_at(dt: f32) {
    let mut h = Harness::with_tick(dt);
    let (priest, _, _) = h.scene();
    let facing = Quat::from_rotation_y(0.7);
    h.world().get_mut::<Transform>(priest).unwrap().rotation = facing;
    h.focus(priest);
    let right = level(facing * Vec3::X);
    let forward = level(facing * Vec3::Z);
    let (mut seen, mut comet_frames, mut frames) = (0, 0, 0);
    loop {
        let flashes = h.flashes();
        if flashes.is_empty() {
            break;
        }
        // The landing's frame ages the flash once: its age is the harness's
        // time since then, never a step of its own.
        frames += 1;
        assert!(
            close(flashes[0].2, frames as f32 * dt, 1e-4),
            "at {dt} s frames, frame {frames}: the flash is {} s old",
            flashes[0].2
        );
        let ms = flashes[0].2 * 1000.0;
        if ms < 900.0 {
            comet_frames += 1;
        }
        let unit = h.app.world().get::<Transform>(priest).unwrap().translation;
        let chest = unit + Vec3::Y * (BENCH_CHEST - COMBATANT_Y);
        assert!(
            (flashes[0].3 - chest).length() < 1e-4,
            "the rig at the chest"
        );
        let pieces = h.flash_pieces();
        assert_eq!(pieces.len(), 4 + 20 + 18, "every piece the bench draws");
        for d in pieces {
            let (comet, k) = match d.piece {
                FlashPiece::CometHead { comet } => (comet, 0),
                FlashPiece::CometTrail { comet, dot } => (comet, dot),
                FlashPiece::Pop { .. } => {
                    assert_eq!(d.alpha_mode, AlphaMode::Add, "the pop is `lighter`");
                    continue;
                }
            };
            assert_eq!(
                d.alpha_mode,
                AlphaMode::Blend,
                "customDraw's sprites are source-over"
            );
            let (sx, sy) = BENCH_CORNERS[comet as usize];
            match bench_comet(sx, sy, k, ms) {
                Some((p, width, rgb, alpha)) => {
                    let want = chest + right * p.x + Vec3::Y * p.y + forward * 0.3;
                    assert!(
                        (d.at - want).length() < 1e-4,
                        "comet {comet} glow {k} at {ms} ms: {} vs {want}",
                        d.at
                    );
                    assert!(close(d.scale.x, width, 1e-5), "{comet}/{k} width {ms}");
                    assert!(close(d.scale.y, width, 1e-5));
                    assert_eq!(d.rgb, rgb, "{comet}/{k} colour");
                    assert!(close(d.alpha, alpha, 1e-5), "{comet}/{k} alpha {ms}");
                    // Unturned: no camera, so its local axes are the world's.
                    assert!(same_turn(d.rotation, Quat::IDENTITY), "roll 0");
                    seen += 1;
                }
                None => assert!(!d.shown(), "nothing from 900 ms ({ms})"),
            }
        }
        h.tick(1);
    }
    assert_eq!(seen, comet_frames * 24, "every comet piece, every frame");
    assert!(
        comet_frames as f32 * dt > 0.9 - dt,
        "{comet_frames} frames of comets at {dt}"
    );
    // The flash lives its 1.0 s: the last of its stars dies at 1.0 s.
    assert!(
        close(frames as f32 * dt, 1.0, dt),
        "the flash lives 1.0 s ({frames} frames at {dt})"
    );
}

/// The spark pop: 18 stars, born where the bench's rate track owes them,
/// each flying straight out of the chest at 2.6 yd/s for its 0.6 s life,
/// sized, coloured and faded on the bench's keys, its roll fixed through its
/// life and spread round the circle — and the same 18 at 8, 16 and 33 ms
/// frames.
#[test]
fn the_pop_bears_the_benchs_eighteen_stars_at_any_frame_rate() {
    let births = bench_births(&pop::RATE);
    assert_eq!(births.len(), 18, "the bench's track owes 18");
    let mut flights: BTreeMap<u8, Vec<f32>> = BTreeMap::new();
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let (priest, _, _) = h.scene();
        h.focus(priest);
        let mut stars: BTreeMap<u8, (f32, Vec3)> = BTreeMap::new();
        let mut rolls: BTreeMap<u8, Quat> = BTreeMap::new();
        let mut frames = 0;
        while let Some(flash) = h.flashes().first().copied() {
            frames += 1;
            let age = flash.2;
            assert!(
                close(age, frames as f32 * dt, 1e-4),
                "at {dt} s frames: the flash is {age} s old at frame {frames}"
            );
            let chest = flash.3;
            for d in h.flash_pieces() {
                let FlashPiece::Pop { star } = d.piece else {
                    continue;
                };
                let born = births[star as usize];
                let page = age - born;
                // The bench's births are found by brute force, to a
                // microsecond: a frame that close to a birth or a death is
                // left alone.
                if page.abs() < 1e-4 || (page - pop::LIFE).abs() < 1e-4 {
                    continue;
                }
                if !(0.0..pop::LIFE).contains(&page) {
                    assert!(!d.shown(), "star {star} outside its life at {age}");
                    continue;
                }
                // Flight: a straight line out at the bench's speed.
                let off = d.at - chest;
                assert!(
                    close(off.length(), pop::SPEED * page, 2e-3),
                    "star {star} at {age} (dt {dt}): {} vs {}",
                    off.length(),
                    pop::SPEED * page
                );
                if page > 0.05 {
                    if let Some((_, dir)) = stars.get(&star) {
                        assert!(off.normalize().dot(*dir) > 0.9999, "straight");
                    }
                    stars.entry(star).or_insert((page, off.normalize()));
                }
                let t = page / pop::LIFE;
                assert!(close(d.scale.x, 2.0 * keys(&pop::SCALE, t), 2e-4));
                assert!(close(d.alpha, keys(&pop::ALPHA, t), 2e-4));
                let want = keys3(&pop::COLOR, t);
                assert!(
                    rgb_near(d.rgb, want),
                    "star {star} colour {:?} vs {want:?}",
                    d.rgb
                );
                // Spin: none; its roll is the one it was born with.
                match rolls.get(&star) {
                    Some(r) => assert!(same_turn(*r, d.rotation), "star {star} spun"),
                    None => {
                        rolls.insert(star, d.rotation);
                    }
                }
            }
            h.tick(1);
        }
        assert_eq!(
            stars.keys().copied().collect::<BTreeSet<u8>>(),
            (0..18).collect(),
            "all 18 stars fly at {dt} s frames"
        );
        assert!(
            close(frames as f32 * dt, 1.0, dt),
            "the pop's flash lives 1.0 s ({frames} frames at {dt})"
        );
        flights.entry(0).or_default().push(stars.len() as f32);
        // The rolls (no camera: the roll is the rotation about Z) spread
        // round the circle.
        let angles: Vec<f32> = rolls
            .values()
            .map(|q| {
                let (axis, angle) = q.to_axis_angle();
                if angle < 1e-6 {
                    0.0
                } else {
                    (angle * axis.z.signum()).rem_euclid(TAU)
                }
            })
            .collect();
        let spread = angles.iter().cloned().fold(f32::MIN, f32::max)
            - angles.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread > PI, "rolls spread {spread}");
        let distinct: BTreeSet<i32> = angles.iter().map(|a| (a * 1000.0) as i32).collect();
        assert_eq!(distinct.len(), 18, "each star its own roll");
    }
}

/// The palms against the bench across a whole pulse and more: both glows on
/// every palm at the bench's widths (pulsing on its 533 ms loop), colours and
/// alphas, added; each palm's motes born at 10.5/s — exactly
/// `floor(10.5 t)` by `t` at 8, 16 and 33 ms frames — drawn on the bench's
/// keys, drifting straight out at 0.19 yd/s from a 0.45 x 0.1 yd box round
/// the palm.
#[test]
fn the_palms_glow_pulse_and_shed_as_the_bench_draws_them() {
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let (priest, _, _) = h.scene();
        h.focus(priest);
        // Let the arcane hands go; the palm clock runs from the landing.
        let mut births: BTreeMap<(i32, u32), (f32, Vec3, Vec3)> = BTreeMap::new();
        let mut elapsed = 0.0;
        for frame in 0..((2.2 / dt) as u32) {
            h.tick(1);
            let rig = h.palm_rigs()[0];
            let age = rig.2;
            // The palm clock is the harness's: it rises at 0 on the
            // landing's frame and runs `dt` a frame since.
            elapsed = (frame + 1) as f32 * dt;
            assert!(
                close(age, elapsed, 1e-4),
                "at {dt} s frames: the palms are {age} s old, {elapsed} s in"
            );
            let ms = age * 1000.0;
            let palms: BTreeMap<i32, Vec3> = h
                .palms()
                .into_iter()
                .map(|(s, _, at)| (s as i32, at))
                .collect();
            for (side, d) in h.palm_pieces() {
                let palm = palms[&(side as i32)];
                assert_eq!(d.alpha_mode, AlphaMode::Add, "every palm call is lighter");
                match d.piece {
                    PalmPiece::Outer | PalmPiece::Inner => {
                        let (w, rgb, a) = bench_palm(d.piece == PalmPiece::Outer, ms);
                        assert!((d.at - palm).length() < 1e-4, "on the palm");
                        assert!(close(d.scale.x, w, 1e-4), "width at {ms}");
                        assert_eq!(d.rgb, rgb);
                        assert!(close(d.alpha, a, 1e-5));
                    }
                    PalmPiece::Mote { slot } => {
                        if !d.shown() {
                            continue;
                        }
                        // Which birth: the latest into this slot.
                        let n = (age * motes::RATE_PER_PALM).floor() as u32;
                        let j = (n - 1) - ((n - 1 - slot as u32) % 7);
                        let page = age - (j + 1) as f32 / motes::RATE_PER_PALM;
                        assert!((0.0..motes::LIFE).contains(&page));
                        let t = page / motes::LIFE;
                        assert!(close(d.scale.x, 2.0 * keys(&motes::SCALE, t), 2e-4));
                        assert!(close(d.alpha, keys(&motes::ALPHA, t), 2e-4));
                        let want = keys3(&motes::COLOR, t);
                        assert!(rgb_near(d.rgb, want), "{:?} vs {want:?}", d.rgb);
                        let off = d.at - palm;
                        let e = births.entry((side as i32, j)).or_insert((page, off, off));
                        e.2 = off;
                        // Born in the box round the palm, drifting out.
                        if page < dt * 1.5 {
                            assert!(off.x.abs() <= 0.225 + 0.19 * page + 1e-4);
                            assert!(off.y.abs() <= 0.05 + 0.19 * page + 1e-4);
                            assert!(off.z.abs() <= 0.225 + 0.19 * page + 1e-4);
                        }
                        let (p0, o0, _) = births[&(side as i32, j)];
                        if page - p0 > 0.1 {
                            let speed = (off - o0).length() / (page - p0);
                            assert!(close(speed, motes::SPEED, 1e-3), "drift {speed}");
                        }
                    }
                }
            }
        }
        // Every mote owed by the harness's own time, on each palm.
        let want = (elapsed * motes::RATE_PER_PALM).floor() as usize;
        for side in [-1, 1] {
            let seen = births.keys().filter(|(s, _)| *s == side).count();
            assert_eq!(seen, want, "palm {side} at {dt}: {seen} of {want} motes");
        }
        // The two palms shed their own motes: the same birth stands
        // elsewhere on each.
        let apart = (0..want as u32)
            .filter(|j| {
                let (a, b) = (births[&(-1, *j)], births[&(1, *j)]);
                (a.1 - b.1).length() > 1e-3
            })
            .count();
        assert_eq!(apart, want, "each palm's motes are its own at {dt}");
    }
}

/// Every piece turns to the camera (its Z the camera's back, its X square to
/// the view) and, under the match camera and at low pitch and near zoom,
/// stands in front of the Priest's RENDERED body on its true point's view
/// ray, scaled to hold its screen size — with the Priest running across the
/// view at 7 yd/s and its body bobbing, so a hold or an anchor a frame
/// behind fails.
#[test]
fn every_piece_turns_to_the_camera_and_holds_in_front_of_the_moving_body() {
    let cams = [
        ("match", 60.0_f32, 38.7_f32),
        ("low pitch", 30.0, 75.0),
        ("near zoom", 12.0, 38.7),
    ];
    for (label, zoom, pitch) in cams {
        for yaw in [0.4_f32, 3.3] {
            let mut h = Harness::new();
            let (priest, _, _) = h.scene();
            let start = h.app.world().get::<Transform>(priest).unwrap().translation;
            // Facing away from the camera: the comets' plane stands behind
            // the body.
            h.world().get_mut::<Transform>(priest).unwrap().rotation =
                Quat::from_rotation_y(yaw + PI);
            let cam = match_camera(start, zoom, pitch.to_radians(), yaw);
            h.camera(cam);
            let eye = cam.translation;
            let fwd = cam.forward().as_vec3();
            let back = cam.back().as_vec3();
            let across = cam.right().as_vec3().with_y(0.0).normalize();
            h.focus(priest);
            let (mut held, mut checked, mut palms_checked) = (0, 0, 0);
            // Whether a piece drawn at `at` turned `rot` and sized `scale`
            // stands for one truly at `truly` `size` across: turned to the
            // camera, in front of the body, on its true view ray, holding its
            // screen size. Returns whether it had to be held.
            let check = |at: Vec3, rot: Quat, scale: Vec3, truly: Vec3, size: f32, nearest: f32| {
                assert!(
                    (rot * Vec3::Z).dot(back) > 0.99999,
                    "{label}: Z to the camera"
                );
                assert!((rot * Vec3::X).dot(fwd).abs() < 1e-4, "{label}: X square");
                let depth = (at - eye).dot(fwd);
                assert!(
                    depth < nearest,
                    "{label} yaw {yaw}: buried, {depth} behind {nearest}"
                );
                assert!(
                    (at - eye).normalize().dot((truly - eye).normalize()) > 0.999_999,
                    "{label}: off its true view ray"
                );
                let k = depth / (truly - eye).dot(fwd);
                assert!(close(scale.x, size * k, 1e-4), "{label}: screen size");
                k < 0.999
            };
            for frame in 0..90 {
                // Run and bob, this frame.
                let at = start + across * 7.0 * h.dt * frame as f32;
                h.world().get_mut::<Transform>(priest).unwrap().translation = at;
                let body = h.app.world().get::<Children>(priest).unwrap()[0];
                let bob = Vec3::new(0.0, 0.12 * (frame as f32 * 0.7).sin(), 0.0);
                h.world().get_mut::<Transform>(body).unwrap().translation = bob;
                h.tick(1);
                let centre = at + bob;
                let nearest = body_nearest_depth(centre, eye, fwd);
                let chest = centre + Vec3::Y * (BENCH_CHEST - COMBATANT_Y);
                let unit = *h.app.world().get::<Transform>(priest).unwrap();
                let facing = level(unit.rotation * Vec3::Z);
                let right = level(unit.rotation * Vec3::X);
                if let Some(f) = h.flashes().first().copied() {
                    assert!(
                        (f.3 - chest).length() < 1e-4,
                        "{label}: the flash at this frame's rendered chest"
                    );
                    for d in h.flash_pieces() {
                        if !d.shown() {
                            continue;
                        }
                        let (comet, k) = match d.piece {
                            FlashPiece::CometHead { comet } => (comet, 0),
                            FlashPiece::CometTrail { comet, dot } => (comet, dot),
                            FlashPiece::Pop { .. } => {
                                // A star: its true point and width are its
                                // pose (pinned against the bench, camera-free,
                                // in the pop's own probe); the hold must keep
                                // it on that point's ray, at that size.
                                let pose = flash_piece_pose(d.piece, f.2, right, facing).unwrap();
                                held += check(
                                    d.at,
                                    d.rotation,
                                    d.scale,
                                    chest + pose.at,
                                    pose.size,
                                    nearest,
                                ) as u32;
                                checked += 1;
                                continue;
                            }
                        };
                        let (sx, sy) = BENCH_CORNERS[comet as usize];
                        let (p, w, ..) = bench_comet(sx, sy, k, f.2 * 1000.0).unwrap();
                        let truly = chest + right * p.x + Vec3::Y * p.y + facing * 0.3;
                        held += check(d.at, d.rotation, d.scale, truly, w, nearest) as u32;
                        checked += 1;
                    }
                }
                let palm_at = |side: f32| {
                    unit.transform_point(bob + Vec3::new(HAND.x * side, HAND.y, HAND.z))
                };
                for (side, _, drawn) in h.palms() {
                    assert!(
                        (drawn - palm_at(side)).length() < 1e-4,
                        "{label}: palm {side} on this frame's hand"
                    );
                }
                let age = h.palm_rigs()[0].2;
                for (side, d) in h.palm_pieces() {
                    if !d.shown() {
                        continue;
                    }
                    if matches!(d.piece, PalmPiece::Mote { .. }) {
                        // A mote: on its pose's ray, at its pose's size (the
                        // pose is pinned against the bench in the palms'
                        // own probe).
                        let hand = u32::from(side < 0.0);
                        let pose = palm_piece_pose(d.piece, age, hand).unwrap();
                        held += check(
                            d.at,
                            d.rotation,
                            d.scale,
                            palm_at(side) + pose.at,
                            pose.size,
                            nearest,
                        ) as u32;
                        checked += 1;
                        continue;
                    }
                    let (w, ..) = bench_palm(d.piece == PalmPiece::Outer, age * 1000.0);
                    held += check(d.at, d.rotation, d.scale, palm_at(side), w, nearest) as u32;
                    palms_checked += 1;
                }
            }
            assert!(checked > 500, "{label}: {checked} pieces checked");
            assert!(palms_checked > 300, "{label}: {palms_checked} palm glows");
            assert!(held > 20, "{label} yaw {yaw}: {held} needed holding");
        }
    }
}

// ── Blessed Recovery ─────────────────────────────────────────────────────────

/// A crit taken procs Blessed Recovery through the sim's own resolver and
/// lands its heal over time: one burst at the Priest's chest, once per proc —
/// a second crit while the heal is up refreshes it and plays a second — and
/// none on the heal's ticks, which heal and draw nothing.
#[test]
fn each_proc_plays_one_burst_and_the_ticks_draw_nothing() {
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.crit(priest, enemy, 120.0);
    h.tick(1);
    let bursts = h.bursts();
    assert_eq!(bursts.len(), 1, "one burst on the proc");
    assert_eq!(bursts[0].1, priest);
    assert!(h.app.world().get::<PlayMatchEntity>(bursts[0].0).is_some());
    let hot = h
        .app
        .world()
        .get::<ActiveAuras>(priest)
        .unwrap()
        .auras
        .iter()
        .find(|a| a.ability_name == BLESSED_RECOVERY_AURA)
        .cloned()
        .expect("the heal over time landed");
    assert_eq!(hot.effect_type, AuraType::HealingOverTime);
    // Its ticks: it heals, and draws nothing.
    let mut q = h.world().query::<&HealImpact>();
    let before = q.iter(h.app.world()).count();
    let hp0 = h
        .app
        .world()
        .get::<Combatant>(priest)
        .unwrap()
        .current_health;
    h.tick_secs(2.5);
    assert!(
        h.app
            .world()
            .get::<Combatant>(priest)
            .unwrap()
            .current_health
            > hp0,
        "a tick healed"
    );
    let mut q = h.world().query::<&HealImpact>();
    assert_eq!(
        q.iter(h.app.world())
            .filter(|i| i.target == priest && i.kind != HealImpactKind::VampiricEmbrace)
            .count(),
        before,
        "a tick draws no heal landing"
    );
    assert!(h.bursts().is_empty(), "the burst ended; ticks raise none");
    // A second crit while the heal is up: a refresh, and a second burst.
    h.crit(priest, enemy, 90.0);
    h.tick(1);
    assert_eq!(h.bursts().len(), 1, "a refresh plays it again");
    let n = h
        .app
        .world()
        .get::<ActiveAuras>(priest)
        .unwrap()
        .auras
        .iter()
        .filter(|a| a.ability_name == BLESSED_RECOVERY_AURA)
        .count();
    assert_eq!(n, 1, "refreshed, not stacked beside");
    // No landing marker is left unconsumed.
    assert_eq!(h.count::<AuraLanding>(), 0);
}

/// A second crit 0.3 s into the first burst refreshes the heal over time and
/// plays a second burst BESIDE the first (procs that close are common in
/// play): two live at once, each a full burst of twelve on its own clock
/// (the harness's frames × dt since its own proc), each living its whole
/// 1.412 s — at 8, 16 and 33 ms frames.
#[test]
fn a_proc_mid_burst_plays_a_second_burst_beside_the_first() {
    let life = 6.0 / renew::RATE + renew::LIFE;
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let (priest, _, enemy) = h.scene();
        h.crit(priest, enemy, 120.0);
        h.tick(1);
        let first = h.bursts();
        assert_eq!(first.len(), 1);
        let first = first[0].0;
        let gap = (0.3 / dt).round() as u32;
        h.tick(gap - 1);
        h.crit(priest, enemy, 90.0);
        h.tick(1);
        let bursts = h.bursts();
        assert_eq!(bursts.len(), 2, "two bursts at once at {dt}");
        let hots = h
            .app
            .world()
            .get::<ActiveAuras>(priest)
            .unwrap()
            .auras
            .iter()
            .filter(|a| a.ability_name == BLESSED_RECOVERY_AURA)
            .count();
        assert_eq!(hots, 1, "the second proc refreshed the heal over time");
        let second = bursts.iter().find(|b| b.0 != first).unwrap().0;
        // Each burst's frames seen, and its own clock.
        let mut seen: BTreeMap<Entity, u32> = BTreeMap::from([(first, gap), (second, 0)]);
        while !h.bursts().is_empty() {
            for (entity, priest_of, age, ..) in h.bursts() {
                assert_eq!(priest_of, priest);
                let n = seen.get_mut(&entity).expect("only the two bursts");
                *n += 1;
                assert!(
                    close(age, *n as f32 * dt, 1e-4),
                    "at {dt}: a burst {age} s old at its frame {n}"
                );
                let mut q = h.world().query::<(&BlessedRecoveryParticle, &ChildOf)>();
                let mine = q
                    .iter(h.app.world())
                    .filter(|(_, c)| c.parent() == entity)
                    .count();
                assert_eq!(mine, 12, "each burst its own twelve");
            }
            h.tick(1);
        }
        for (entity, n) in seen {
            assert!(
                close(n as f32 * dt, life, dt),
                "at {dt}: {entity:?} lived {n} frames, not {life} s"
            );
        }
    }
}

/// The burst against the bench at 8, 16 and 33 ms frames: exactly six of
/// each layer born, at `k / 9.2` s, inside the 700 ms window; each flying out
/// of the chest along a straight line, slowed by the drag as the bench's
/// per-frame `1 − 0.9 dt` integrates to; sized, coloured and faded on the
/// bench's keys, added; the rig gone once the last has lived its life.
#[test]
fn the_burst_draws_six_of_each_on_the_benchs_keys_at_any_frame_rate() {
    for dt in [0.008, 0.016, 0.033] {
        let mut h = Harness::with_tick(dt);
        let (priest, _, enemy) = h.scene();
        h.crit(priest, enemy, 120.0);
        h.tick(1);
        let mut seen: BTreeSet<(RecoveryLayer, u8)> = BTreeSet::new();
        let mut frames = 0;
        let mut rolls: BTreeMap<(RecoveryLayer, u8), Quat> = BTreeMap::new();
        while let Some(burst) = h.bursts().first().copied() {
            let age = burst.2;
            let unit = h.app.world().get::<Transform>(priest).unwrap().translation;
            let chest = unit + Vec3::Y * (BENCH_CHEST - COMBATANT_Y);
            assert!((burst.3 - chest).length() < 1e-4);
            let particles = h.burst_particles();
            assert_eq!(particles.len(), 12, "six slots a layer");
            for d in particles {
                assert_eq!(d.alpha_mode, AlphaMode::Add);
                let (layer, j) = d.piece;
                let born = (j as f32 + 1.0) / renew::RATE;
                assert!(born < renew::WINDOW);
                let page = age - born;
                if !(0.0..renew::LIFE).contains(&page) {
                    assert!(!d.shown(), "{layer:?} {j} outside its life");
                    continue;
                }
                seen.insert(d.piece);
                let k = 3.0 * renew::DRAG;
                let flown = renew::SPEED * (1.0 - (-k * page).exp()) / k;
                assert!(
                    close((d.at - chest).length(), flown, 1e-4),
                    "{layer:?} {j} flown {} vs {flown}",
                    (d.at - chest).length()
                );
                let t = page / renew::LIFE;
                let (scale, alpha, color): (&[(f32, f32)], &[(f32, f32)], Vec<(f32, [f32; 3])>) =
                    match layer {
                        RecoveryLayer::Rings => (
                            &renew::RING_SCALE,
                            &renew::RING_ALPHA,
                            renew::RING_COLOR.to_vec(),
                        ),
                        RecoveryLayer::Glows => (
                            &renew::GLOW_SCALE,
                            &renew::GLOW_ALPHA,
                            renew::GLOW_COLOR.to_vec(),
                        ),
                    };
                assert!(
                    close(d.scale.x, 2.0 * keys(scale, t), 2e-4),
                    "{layer:?} size"
                );
                assert!(close(d.alpha, keys(alpha, t), 2e-4), "{layer:?} alpha");
                let want = keys3(&color, t);
                assert!(rgb_near(d.rgb, want), "{layer:?}: {:?} vs {want:?}", d.rgb);
                match rolls.get(&d.piece) {
                    Some(r) => assert!(same_turn(*r, d.rotation), "spun"),
                    None => {
                        rolls.insert(d.piece, d.rotation);
                    }
                }
            }
            h.tick(1);
            frames += 1;
        }
        let want: BTreeSet<(RecoveryLayer, u8)> = [RecoveryLayer::Rings, RecoveryLayer::Glows]
            .into_iter()
            .flat_map(|l| (0..6).map(move |j| (l, j)))
            .collect();
        assert_eq!(seen, want, "six of each drawn at {dt}");
        let life = 6.0 / 9.2 + 0.76;
        assert!(
            close(frames as f32 * dt, life, 2.0 * dt),
            "the rig lives {life} s ({frames} frames at {dt})"
        );
    }
}

/// The burst under the match camera and near it, with the Priest running
/// across the view: every particle turned to the camera and held in front of
/// this frame's rendered body on its true view ray. A sheep hides it, its
/// clock running on; death lets it play out; a Priest gone takes it; the
/// match ending takes it.
#[test]
fn the_burst_holds_in_front_of_the_moving_body_and_lives_with_the_priest() {
    for (zoom, pitch) in [(60.0_f32, 38.7_f32), (12.0, 75.0)] {
        let mut h = Harness::new();
        let (priest, _, enemy) = h.scene();
        let start = h.app.world().get::<Transform>(priest).unwrap().translation;
        let cam = match_camera(start, zoom, pitch.to_radians(), 0.4);
        h.camera(cam);
        let (eye, fwd) = (cam.translation, cam.forward().as_vec3());
        let across = cam.right().as_vec3().with_y(0.0).normalize();
        h.crit(priest, enemy, 120.0);
        let (mut checked, mut held) = (0, 0);
        for frame in 0..80 {
            let at = start + across * 7.0 * h.dt * frame as f32;
            h.world().get_mut::<Transform>(priest).unwrap().translation = at;
            let body = h.app.world().get::<Children>(priest).unwrap()[0];
            let bob = Vec3::Y * 0.1 * (frame as f32 * 0.5).sin();
            h.world().get_mut::<Transform>(body).unwrap().translation = bob;
            h.tick(1);
            let Some(burst) = h.bursts().first().copied() else {
                continue;
            };
            let centre = at + bob;
            let chest = centre + Vec3::Y * (BENCH_CHEST - COMBATANT_Y);
            assert!((burst.3 - chest).length() < 1e-4, "this frame's chest");
            let nearest = body_nearest_depth(centre, eye, fwd);
            for d in h.burst_particles() {
                if !d.shown() {
                    continue;
                }
                assert!((d.rotation * Vec3::Z).dot(cam.back().as_vec3()) > 0.99999);
                assert!((d.rotation * Vec3::X).dot(fwd).abs() < 1e-4);
                let depth = (d.at - eye).dot(fwd);
                assert!(depth < nearest, "buried: {depth} behind {nearest}");
                let (layer, j) = d.piece;
                let page = burst.2 - (j as f32 + 1.0) / renew::RATE;
                let t = page / renew::LIFE;
                let size = 2.0
                    * keys(
                        match layer {
                            RecoveryLayer::Rings => &renew::RING_SCALE,
                            RecoveryLayer::Glows => &renew::GLOW_SCALE,
                        },
                        t,
                    );
                // Its true point is on the line from the eye through it, at
                // the depth the size ratio names.
                let k = d.scale.x / size;
                let truly = eye + (d.at - eye) / k;
                let k3 = 3.0 * renew::DRAG;
                let flown = renew::SPEED * (1.0 - (-k3 * page).exp()) / k3;
                assert!(
                    close((truly - chest).length(), flown, 2e-3),
                    "on its true ray: {} vs {flown}",
                    (truly - chest).length()
                );
                if k < 0.999 {
                    held += 1;
                }
                checked += 1;
            }
        }
        assert!(checked > 300, "{checked} particles checked");
        assert!(held > 30, "{held} needed holding");
    }

    // A sheep hides it, clock running on.
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.world().entity_mut(priest).insert(PolymorphedVisual);
    h.crit(priest, enemy, 120.0);
    h.tick(1);
    assert_eq!(
        h.bursts()[0].4,
        Visibility::Hidden,
        "hidden from its first frame"
    );
    let a0 = h.bursts()[0].2;
    h.tick(10);
    assert!(h.bursts()[0].2 > a0 + 0.15);
    h.world().entity_mut(priest).remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(h.bursts()[0].4, Visibility::Inherited);

    // Death: it plays out.
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.crit(priest, enemy, 120.0);
    h.tick(2);
    h.world()
        .get_mut::<Combatant>(priest)
        .unwrap()
        .current_health = 0.0;
    h.tick(10);
    assert_eq!(h.bursts().len(), 1, "a landing plays out on a corpse");

    // Gone: at once, particles and all.
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.crit(priest, enemy, 120.0);
    h.tick(2);
    h.world().entity_mut(priest).despawn();
    h.tick(1);
    assert!(h.bursts().is_empty());
    assert_eq!(h.count::<BlessedRecoveryParticle>(), 0);

    // The match ends mid-burst.
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.crit(priest, enemy, 120.0);
    h.tick(2);
    assert_eq!(h.bursts().len(), 1);
    h.world().run_system_cached(cleanup_play_match).unwrap();
    assert!(h.bursts().is_empty());
    assert_eq!(h.count::<BlessedRecoveryParticle>(), 0);
}

/// A proc with nobody to read it raises no Inner Focus visual, and Inner
/// Focus raises no burst: the two routes never cross.
#[test]
fn neither_landing_plays_the_others_visual() {
    let mut h = Harness::new();
    let (priest, _, enemy) = h.scene();
    h.crit(priest, enemy, 120.0);
    h.tick(3);
    assert!(h.flashes().is_empty());
    assert!(h.palm_rigs().is_empty());
    let mut h = Harness::new();
    let (priest, _, _) = h.scene();
    h.focus(priest);
    h.tick(3);
    assert!(h.bursts().is_empty());
}
