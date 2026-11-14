//! Probes for Divine Protection's globe
//! (`rendering/effects/divine_protection.rs`, AS-376): a red-gold globe at
//! the Paladin's chest for the buff's life, after the instant Holy hand flash.
//!
//! These drive the graphical chain off `ActiveAuras`, as the sim leaves it,
//! and assert what is DRAWN, in world space through `GlobalTransform` and the
//! spawned materials: every piece against an independent transcription of
//! the signed-off bench's `customDraw` across the whole buff, the sprites read
//! back, camera facing under a turned camera, the draw order against the body
//! and Divine Shield's cards at the match camera, and the globe going when the
//! buff runs out, is purged, or its Paladin dies.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. The systems run in the order `states/mod.rs` registers them.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::render::mesh::MeshAabb;
use bevy::render::render_phase::ViewRangefinder3d;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CameraController, CastEnding, CastEndingKind,
    Combatant, HealCastHand, HealCastKind, LandedCast, ShieldCard, ShieldKind, ShieldVisual,
    VisualBody, DIVINE_PROTECTION_AURA,
};
use arenasim::states::play_match::{
    consume_cast_ending_signals, globe_flame_texture, globe_pieces, oneshot_glow_texture,
    oneshot_ring_texture, place_divine_protection_globes, place_shield_cards,
    spawn_instant_cast_hands, star_flash_texture, update_divine_protection_globes,
    update_heal_cast_glows, update_shield_visuals, DivineProtectionGlobe,
    DivineProtectionGlobePiece, GlobePiece, GlobeSprite, DIVINE_SHIELD_AURA_NAME,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;
/// A combatant's body centre stands a yard above its feet; the chest
/// attachment is 1.45 above them.
const CHEST_ABOVE_CENTRE: f32 = 0.45;

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
            (
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                update_divine_protection_globes,
                update_shield_visuals,
                update_heal_cast_glows,
            )
                .chain(),
        );
        app.add_systems(
            PostUpdate,
            (place_divine_protection_globes, place_shield_cards)
                .before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it, with a `VisualBody` child.
    fn unit(&mut self, class: CharacterClass, at: Vec3) -> (Entity, Entity) {
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
        (unit, body)
    }

    fn camera(&mut self, transform: Transform) -> Entity {
        self.app
            .world_mut()
            .spawn((Camera3d::default(), transform))
            .id()
    }

    /// Divine Protection as the real config lands it.
    fn protect(&mut self, unit: Entity) {
        let defs = AbilityDefinitions::default();
        let def = defs.get(&AbilityType::DivineProtection).unwrap();
        let aura = AuraPending::from_ability(unit, unit, def).unwrap().aura;
        assert!((aura.duration - 8.0).abs() < 1e-6, "an 8 s buff");
        self.auras(unit).push(aura);
    }

    fn give(&mut self, unit: Entity, aura: Aura) {
        self.auras(unit).push(aura);
    }

    fn auras(&mut self, unit: Entity) -> Mut<'_, Vec<Aura>> {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .map_unchanged(|a| &mut a.auras)
    }

    /// One frame, the buffs running down as the sim runs them.
    fn step(&mut self) {
        let mut q = self.app.world_mut().query::<&mut ActiveAuras>();
        for mut auras in q.iter_mut(self.app.world_mut()) {
            for aura in &mut auras.auras {
                aura.duration -= DT;
            }
        }
        self.app.update();
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.step();
        }
    }

    fn globes(&mut self) -> Vec<(Entity, f32, f32, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&DivineProtectionGlobe, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(g, t)| (g.bearer, g.age, g.remaining, t.translation()))
            .collect()
    }

    fn pieces(&mut self) -> Vec<Drawn> {
        let mut q = self.app.world_mut().query::<(
            &DivineProtectionGlobePiece,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(p, g, m, mesh)| (p.piece, *g, m.0.clone(), mesh.0.clone()))
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(piece, g, m, mesh)| Drawn {
                piece,
                at: g.translation(),
                rotation: g.rotation(),
                scale: g.scale(),
                material: materials.get(&m).unwrap().clone(),
                mesh,
            })
            .collect()
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity)> {
        let mut q = self.app.world_mut().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster))
            .collect()
    }

    fn count<T: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&T>();
        q.iter(self.app.world()).count()
    }
}

struct Drawn {
    piece: GlobePiece,
    at: Vec3,
    rotation: Quat,
    scale: Vec3,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
}

/// Every piece the bench draws, named.
fn every_piece() -> BTreeSet<GlobePiece> {
    let mut all = BTreeSet::from([
        GlobePiece::GoldGlow,
        GlobePiece::GoldRing,
        GlobePiece::GoldRingUpper,
        GlobePiece::GoldRingLower,
        GlobePiece::RedGlow,
        GlobePiece::RedRing,
        GlobePiece::PulseFlash,
    ]);
    all.extend((0..14).map(|index| GlobePiece::PulseFlame { index }));
    all.extend((0..6).map(|index| GlobePiece::Glint { index }));
    all
}

fn srgb_u8(c: Color) -> [u8; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

// ── the bench, transcribed ──────────────────────────────────────────────────
//
// `customDraw` of the "Divine Protection" tab of
// `docs/design/benches/2026-10-18-paladin-talent-bench.html` (version
// 1792379243-e3d6), written out from its JavaScript at its default knobs
// (size 1, opacity 1). Independent of the module: none of its constants are
// read here.

/// The bench's `keys` over scalars, in milliseconds.
fn keys(k: &[(f64, f64)], t: f64) -> f64 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for i in 1..k.len() {
        if t <= k[i].0 {
            let (a, b) = (k[i - 1], k[i]);
            let span = b.0 - a.0;
            let u = if span == 0.0 { 1.0 } else { (t - a.0) / span };
            return a.1 + (b.1 - a.1) * u;
        }
    }
    k[k.len() - 1].1
}

/// The bench's `keys` over colours.
fn keys3(k: &[(f64, [f64; 3])], t: f64) -> [f64; 3] {
    let channel = |c: usize| keys(&k.iter().map(|(t, v)| (*t, v[c])).collect::<Vec<_>>(), t);
    [channel(0), channel(1), channel(2)]
}

/// The bench's `hh`.
fn hh(n: f64) -> f64 {
    let x = (n * 12.9898).sin() * 43758.5453;
    x - x.floor()
}

/// What the bench draws for one piece: colour (as the canvas truncates it),
/// alpha, position from the chest, full size (a ring: its radius), and a
/// sprite's angle.
#[derive(Debug)]
struct BenchDraw {
    rgb: [u8; 3],
    alpha: f64,
    at: [f64; 3],
    size: f64,
    angle: f64,
}

fn bench(piece: GlobePiece, u: f64, dur: f64) -> BenchDraw {
    let env = keys(
        &[(0.0, 0.0), (200.0, 1.0), (dur - 300.0, 1.0), (dur, 0.0)],
        u,
    );
    let cyc = u % 2634.0;
    let r = 0.75;
    let w_g = keys(
        &[
            (0.0, 0.0),
            (167.0, 0.0),
            (267.0, 0.8),
            (734.0, 0.8),
            (834.0, 0.0),
        ],
        cyc,
    )
    .max(keys(
        &[
            (0.0, 0.0),
            (600.0, 0.0),
            (700.0, 0.8),
            (1100.0, 0.8),
            (1200.0, 0.0),
        ],
        cyc,
    ));
    let w_r = keys(
        &[
            (0.0, 0.0),
            (1034.0, 0.0),
            (1134.0, 0.8),
            (1534.0, 0.8),
            (1634.0, 0.0),
        ],
        cyc,
    )
    .max(keys(
        &[(0.0, 0.0), (1267.0, 0.0), (1500.0, 1.0), (1834.0, 0.0)],
        cyc,
    ));
    let red = keys3(
        &[
            (0.0, [246.0, 35.0, 35.0]),
            (1267.0, [255.0, 255.0, 255.0]),
            (1500.0, [249.0, 58.0, 44.0]),
            (1800.0, [246.0, 35.0, 35.0]),
        ],
        cyc,
    );
    let trunc = |c: [f64; 3]| c.map(|v| v as u8);
    let draw = |rgb: [f64; 3], alpha: f64, at: [f64; 3], size: f64| BenchDraw {
        rgb: trunc(rgb),
        alpha,
        at,
        size,
        angle: 0.0,
    };
    let gold_glow = [252.0, 236.0, 190.0];
    let gold_ring = [252.0, 240.0, 200.0];
    let pu = cyc - 1267.0;
    match piece {
        GlobePiece::GoldGlow => draw(gold_glow, w_g * env * 0.5, [0.0; 3], 2.0 * r * 1.15),
        GlobePiece::GoldRing => draw(gold_ring, w_g * env * 0.8, [0.0; 3], r),
        GlobePiece::GoldRingUpper => draw(gold_ring, w_g * env * 0.5, [0.0, 0.35, 0.0], r * 0.8),
        GlobePiece::GoldRingLower => draw(gold_ring, w_g * env * 0.5, [0.0, -0.35, 0.0], r * 0.8),
        GlobePiece::RedGlow => draw(red, w_r * env * 0.55, [0.0; 3], 2.0 * r),
        GlobePiece::RedRing => draw(red, w_r * env * 0.8, [0.0; 3], r * 1.02),
        GlobePiece::PulseFlash => {
            let alpha = if (0.0..700.0).contains(&pu) {
                keys(&[(0.0, 0.0), (33.0, 1.0), (200.0, 0.6), (400.0, 0.0)], pu) * env
            } else {
                0.0
            };
            draw([255.0, 200.0, 150.0], alpha, [0.0; 3], 1.4)
        }
        GlobePiece::PulseFlame { index } => {
            let k = index as f64;
            let life = 500.0;
            let a = pu - (index % 3) as f64 * 12.0;
            if !(0.0..700.0).contains(&pu) || !(0.0..=life).contains(&a) {
                return draw([255.0; 3], 0.0, [0.0; 3], 0.3);
            }
            let ph = a / life;
            let d = [hh(k) - 0.5, hh(k + 7.0) - 0.5, hh(k + 13.0) - 0.5];
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let reach = 2.8 * ph * 0.5;
            draw(
                keys3(
                    &[
                        (0.0, [255.0; 3]),
                        (0.5, [233.0, 98.0, 32.0]),
                        (1.0, [255.0; 3]),
                    ],
                    ph,
                ),
                keys(&[(0.0, 0.51), (0.5, 0.29), (1.0, 0.0)], ph) * env,
                [d[0] / l * reach, d[1] / l * reach, d[2] / l * reach],
                0.3,
            )
        }
        GlobePiece::Glint { index } => {
            let k = index as f64;
            let lit = cyc > 200.0 && cyc < 1900.0;
            // JavaScript's `%` keeps the dividend's sign, as Rust's does.
            let ph = ((cyc - 200.0) / 500.0 + k / 6.0) % 1.0;
            let a = k * 1.7 + ((cyc - 200.0) / 500.0).floor();
            BenchDraw {
                rgb: trunc(keys3(
                    &[
                        (0.0, [255.0; 3]),
                        (0.5, [223.0, 32.0, 32.0]),
                        (1.0, [255.0; 3]),
                    ],
                    ph,
                )),
                alpha: if lit {
                    keys(&[(0.0, 0.0), (0.5, 0.5), (1.0, 0.0)], ph) * env
                } else {
                    0.0
                },
                at: [
                    r * 0.9 * a.cos(),
                    r * 0.8 * (a * 1.3).sin(),
                    r * 0.9 * a.sin(),
                ],
                size: 0.2,
                angle: a,
            }
        }
    }
}

/// The ring sprite's stroke runs at 0.965 of the quad's half-width.
const RING_CENTRE: f32 = 0.965;

// ── the globe on a Paladin ──────────────────────────────────────────────────

/// A Paladin holding Divine Protection wears one globe at its chest, of
/// every piece the bench draws, and its hands flash the Holy flash once.
#[test]
fn the_globe_rises_on_the_paladin_with_one_holy_hand_flash() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(2.0, 1.0, -3.0));
    let (bystander, _) = h.unit(CharacterClass::Warrior, Vec3::new(-4.0, 1.0, 0.0));
    h.protect(paladin);
    h.tick(1);
    let globes = h.globes();
    assert_eq!(globes.len(), 1, "one globe");
    assert_eq!(globes[0].0, paladin);
    let drawn: BTreeSet<GlobePiece> = h.pieces().iter().map(|d| d.piece).collect();
    assert_eq!(drawn, every_piece(), "every layer the bench signed off");
    assert_eq!(h.pieces().len(), 27, "each piece once");
    assert_eq!(globe_pieces().len(), 27);
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
    // Held: no second globe, no second flash.
    h.tick(30);
    assert_eq!(h.globes().len(), 1);
    assert_eq!(h.hands().len(), 2, "the one flash, still playing");
    h.tick(30);
    assert!(h.hands().is_empty(), "the flash is over by 0.7 s");
    assert_eq!(h.globes().len(), 1, "the globe holds");
    assert!(h.globes().iter().all(|g| g.0 != bystander));
}

/// The globe stands on the RENDERED body: 0.45 yd above the body's centre,
/// riding its bob, and following the unit as it walks.
#[test]
fn the_globe_rides_the_rendered_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, 1.0, -3.0);
    let (paladin, body) = h.unit(CharacterClass::Paladin, at);
    let bob = Vec3::new(0.1, 0.22, -0.05);
    h.app
        .world_mut()
        .get_mut::<Transform>(body)
        .unwrap()
        .translation = bob;
    h.protect(paladin);
    h.tick(2);
    let chest = at + bob + Vec3::Y * CHEST_ABOVE_CENTRE;
    assert!(
        (h.globes()[0].3 - chest).length() < 1e-5,
        "{} vs {chest}",
        h.globes()[0].3
    );
    let moved = Vec3::new(-6.0, 1.0, 4.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(paladin)
        .unwrap()
        .translation = moved;
    h.tick(1);
    assert!((h.globes()[0].3 - (moved + bob + Vec3::Y * CHEST_ABOVE_CENTRE)).length() < 1e-5);
}

/// Every piece, every few frames across a whole buff, against the bench:
/// its colour, alpha, place on the globe and size. Returns the pieces lit
/// somewhere in it and the frames the globe stood.
fn compare_across_a_buff(duration: f32) -> (BTreeSet<GlobePiece>, u32) {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, 1.0, 2.0);
    let (paladin, _) = h.unit(CharacterClass::Paladin, at);
    h.camera(Transform::from_xyz(0.0, 30.0, 40.0).looking_at(at, Vec3::Y));
    h.protect(paladin);
    h.auras(paladin)[0].duration = duration;
    let chest = at + Vec3::Y * CHEST_ABOVE_CENTRE;
    let mut lit = BTreeSet::new();
    let mut compared = 0;
    let mut frames = 0;
    loop {
        h.step();
        let Some(&(_, age, remaining, centre)) = h.globes().first() else {
            break;
        };
        frames += 1;
        assert!((centre - chest).length() < 1e-5);
        assert!(frames < 600, "the globe outlived its buff");
        // The sim removes a spent buff.
        if remaining <= DT {
            h.auras(paladin).clear();
        }
        if frames % 3 != 0 {
            continue;
        }
        let (u, dur) = (age as f64 * 1000.0, (age + remaining) as f64 * 1000.0);
        for d in h.pieces() {
            let b = bench(d.piece, u, dur);
            let what = format!("{:?} at {u:.0} of {dur:.0} ms", d.piece);
            let alpha = d.material.base_color.alpha() as f64;
            assert!(
                (alpha - b.alpha.min(1.0)).abs() < 2e-3,
                "{what}: alpha {alpha} vs the bench's {}",
                b.alpha
            );
            let size = match d.piece.sprite() {
                GlobeSprite::Ring => d.scale.x * RING_CENTRE / 2.0,
                _ => d.scale.x,
            };
            assert!(
                (size - b.size as f32).abs() < 1e-4,
                "{what}: size {size} vs the bench's {}",
                b.size
            );
            assert!((d.scale.x - d.scale.y).abs() < 1e-5, "{what}: square");
            compared += 1;
            // Where and in what colour only matter while it shows.
            if b.alpha <= 0.0 {
                continue;
            }
            let rgb = srgb_u8(d.material.base_color);
            for c in 0..3 {
                assert!(
                    (rgb[c] as i32 - b.rgb[c] as i32).abs() <= 1,
                    "{what}: colour {rgb:?} vs the bench's {:?}",
                    b.rgb
                );
            }
            let off = d.at - chest;
            let want = Vec3::new(b.at[0] as f32, b.at[1] as f32, b.at[2] as f32);
            assert!(
                (off - want).length() < 2e-3,
                "{what}: at {off} vs the bench's {want}"
            );
            if b.alpha > 0.05 {
                lit.insert(d.piece);
            }
        }
    }
    assert!(compared > 27 * (frames / 3 - 1), "{compared}");
    (lit, frames)
}

/// The real 8 s buff: every piece is lit somewhere in it, so the comparison
/// is not over darkness, and the globe stands the whole buff.
#[test]
fn every_piece_draws_what_the_bench_draws_across_the_buff() {
    let (lit, frames) = compare_across_a_buff(8.0);
    assert_eq!(
        lit,
        every_piece(),
        "every piece is lit somewhere in the buff"
    );
    assert!((495..=502).contains(&frames), "{frames} frames");
}

/// A buff ending inside the pale-gold layer (0.95 s), so the 0.3 s fade-out
/// is drawn over lit pieces and compared too.
#[test]
fn a_buff_ending_while_lit_fades_as_the_bench_fades() {
    let (lit, frames) = compare_across_a_buff(0.95);
    assert!(lit.contains(&GlobePiece::GoldGlow), "{lit:?}");
    assert!((57..=61).contains(&frames), "{frames} frames");
}

/// The fade, read off the aura's own time: a buff with 0.1 s left draws the
/// pale-gold glow at a third of its held 0.4.
#[test]
fn the_globe_fades_out_over_the_buffs_last_0_3_seconds() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.protect(paladin);
    h.auras(paladin)[0].duration = 0.6;
    h.tick(32);
    let (_, age, remaining, _) = h.globes()[0];
    assert!((0.4..0.6).contains(&age), "in the gold window: {age}");
    assert!((0.05..0.15).contains(&remaining), "{remaining}");
    let gold = h
        .pieces()
        .into_iter()
        .find(|d| d.piece == GlobePiece::GoldGlow)
        .unwrap()
        .material
        .base_color
        .alpha();
    let want = 0.4 * remaining / 0.3;
    assert!((gold - want).abs() < 2e-3, "{gold} vs {want}");
}

// ── the globe goes ──────────────────────────────────────────────────────────

/// The globe goes, pieces and all, on the frame the buff runs out, is
/// purged, or its Paladin dies (auras stay on a corpse) or is gone.
#[test]
fn the_globe_goes_when_the_buff_ends_or_its_paladin_dies() {
    for ending in ["runs out", "purged", "dies", "gone"] {
        let mut h = Harness::new();
        let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
        h.protect(paladin);
        h.tick(20);
        assert_eq!(h.globes().len(), 1, "{ending}: up first");
        assert_eq!(h.count::<DivineProtectionGlobePiece>(), 27);
        match ending {
            "runs out" => {
                for aura in h.auras(paladin).iter_mut() {
                    aura.duration = DT / 2.0;
                }
                h.tick(1);
                assert_eq!(h.globes().len(), 1, "still up on its last frame");
                h.auras(paladin).clear();
            }
            "purged" => {
                let mut auras = h.auras(paladin);
                auras.retain(|a| a.ability_name != DIVINE_PROTECTION_AURA);
                assert!(auras.is_empty());
            }
            "dies" => {
                h.app
                    .world_mut()
                    .get_mut::<Combatant>(paladin)
                    .unwrap()
                    .current_health = 0.0;
                assert_eq!(h.auras(paladin).len(), 1, "the buff stays on the corpse");
            }
            _ => {
                h.app.world_mut().entity_mut(paladin).despawn();
            }
        }
        h.tick(1);
        assert!(h.globes().is_empty(), "{ending}: the globe is gone");
        assert_eq!(
            h.count::<DivineProtectionGlobePiece>(),
            0,
            "{ending}: and every piece with it"
        );
    }
}

/// Nothing rises on a corpse holding the buff, nor for any other
/// damage-taken reduction, nor for the name on another type.
#[test]
fn a_corpse_and_another_ward_raise_no_globe() {
    let mut h = Harness::new();
    let (corpse, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Combatant>(corpse)
        .unwrap()
        .current_health = 0.0;
    h.protect(corpse);
    let (warded, _) = h.unit(CharacterClass::Paladin, Vec3::new(5.0, 1.0, 0.0));
    let mut ward = Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 8.0,
        magnitude: 0.3,
        ability_name: "Some Other Ward".into(),
        ..Default::default()
    };
    h.give(warded, ward.clone());
    let (named, _) = h.unit(CharacterClass::Paladin, Vec3::new(-5.0, 1.0, 0.0));
    ward.effect_type = AuraType::DamageReduction;
    ward.ability_name = DIVINE_PROTECTION_AURA.into();
    h.give(named, ward);
    h.tick(10);
    assert!(h.globes().is_empty());
    assert!(h.hands().is_empty(), "and no hands flash");
    // The control: the same Paladin, alive, raises one.
    h.app
        .world_mut()
        .get_mut::<Combatant>(corpse)
        .unwrap()
        .current_health = 100.0;
    h.tick(1);
    assert_eq!(h.globes().len(), 1);
}

/// The animation sandbox lands Divine Protection through a zero-length cast,
/// so a `Landed` marker names it: the hands still flash once, at the globe.
#[test]
fn a_landed_marker_does_not_flash_the_hands_twice() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.app.world_mut().spawn((
        CastEnding {
            caster: paladin,
            kind: CastEndingKind::Landed,
        },
        LandedCast {
            ability: AbilityType::DivineProtection,
        },
    ));
    h.tick(2);
    assert!(h.hands().is_empty(), "nothing off the marker");
    h.protect(paladin);
    h.tick(2);
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
}

// ── what is drawn ───────────────────────────────────────────────────────────

/// Every piece's material and mesh: additive, unlit, both faces, the bench's
/// sprite (read back), on a unit quad.
#[test]
fn every_piece_is_an_additive_sprite_on_a_unit_quad() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.protect(paladin);
    h.tick(2);
    let drawn = h.pieces();
    let world = h.app.world();
    let (images, meshes) = (
        world.resource::<Assets<Image>>(),
        world.resource::<Assets<Mesh>>(),
    );
    let mut sprites = BTreeSet::new();
    for d in &drawn {
        let m = &d.material;
        assert!(matches!(m.alpha_mode, AlphaMode::Add), "{:?}", d.piece);
        assert!(m.unlit && m.double_sided && m.cull_mode.is_none());
        assert_eq!(m.depth_bias, 0.1, "{:?} sorts over Divine Shield", d.piece);
        let expected = match d.piece.sprite() {
            GlobeSprite::Glow => oneshot_glow_texture(),
            GlobeSprite::Ring => oneshot_ring_texture(),
            GlobeSprite::Flame => globe_flame_texture(),
            GlobeSprite::Star => star_flash_texture(),
        };
        let texture = m.base_color_texture.as_ref().expect("a sprite");
        assert_eq!(
            images.get(texture).unwrap().data,
            expected.data,
            "{:?} is drawn with the bench's {:?}",
            d.piece,
            d.piece.sprite()
        );
        sprites.insert(format!("{:?}", d.piece.sprite()));
        let mesh = meshes.get(&d.mesh).unwrap();
        assert_eq!(mesh.count_vertices(), 4, "a quad");
        let aabb = mesh.compute_aabb().unwrap();
        assert_eq!(Vec3::from(aabb.half_extents), Vec3::new(0.5, 0.5, 0.0));
    }
    assert_eq!(sprites.len(), 4, "glow, ring, flame and star: {sprites:?}");
    // The sprites a role draws with are the bench's.
    use GlobePiece as P;
    for (piece, sprite) in [
        (P::GoldGlow, GlobeSprite::Glow),
        (P::RedGlow, GlobeSprite::Glow),
        (P::PulseFlash, GlobeSprite::Glow),
        (P::GoldRing, GlobeSprite::Ring),
        (P::GoldRingUpper, GlobeSprite::Ring),
        (P::GoldRingLower, GlobeSprite::Ring),
        (P::RedRing, GlobeSprite::Ring),
        (P::PulseFlame { index: 3 }, GlobeSprite::Flame),
        (P::Glint { index: 2 }, GlobeSprite::Star),
    ] {
        assert_eq!(piece.sprite(), sprite, "{piece:?}");
    }
}

/// The flame sprite is the bench's `sprite('flame')`: a soft radial glow at
/// 0.8 / 0.55 / 0 of its radius, cut to an upright ellipse 0.6 across and
/// 1.0 tall, centred a quarter of the radius up.
#[test]
fn the_flame_sprite_reads_back_as_the_benchs() {
    let image = globe_flame_texture();
    let px = image.width() as usize;
    let data = image.data.as_ref().unwrap();
    let alpha = |x: f32, y_up: f32| {
        let col = ((x + 1.0) / 2.0 * px as f32 - 0.5).round() as usize;
        let row = ((1.0 - y_up) / 2.0 * px as f32 - 0.5).round() as usize;
        data[(row * px + col) * 4 + 3] as f32 / 255.0
    };
    let stop = |r: f32| {
        if r <= 0.45 {
            0.8 + (0.55 - 0.8) * r / 0.45
        } else {
            0.55 * (1.0 - (r - 0.45) / 0.55)
        }
    };
    for (x, y) in [
        (0.0_f32, 0.0_f32),
        (0.0, 0.5),
        (0.3, 0.3),
        (0.0, -0.6),
        (0.4, 0.0),
    ] {
        let r = (x * x + y * y).sqrt();
        assert!(
            (alpha(x, y) - stop(r)).abs() < 0.03,
            "({x}, {y}): {} vs {}",
            alpha(x, y),
            stop(r)
        );
    }
    // Cut by the ellipse: across past 0.6, below past 0.75.
    assert_eq!(alpha(0.65, 0.0), 0.0);
    assert_eq!(alpha(0.0, -0.8), 0.0);
    assert!(alpha(0.0, 0.8) > 0.0, "it reaches higher than it hangs");
    assert!(data.chunks(4).all(|p| p[..3] == [255, 255, 255]), "white");
}

/// Under a turned camera, every glow and flame faces it, each glint faces it
/// rolled by its angle as the bench turns its star, and every ring lies level.
#[test]
fn the_glows_face_a_turned_camera_and_the_rings_lie_level() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, 1.0, -1.0);
    let (paladin, _) = h.unit(CharacterClass::Paladin, at);
    // Turned, pitched and rolled: no axis of it is the world's.
    let looking =
        Transform::from_xyz(-9.0, 14.0, 6.0).looking_at(Vec3::new(1.0, 0.0, 2.0), Vec3::Y);
    let cam = looking.with_rotation(looking.rotation * Quat::from_rotation_z(0.3));
    h.camera(cam);
    h.protect(paladin);
    // 0.9 s in: the glints are out and the cycle is between their beats.
    h.tick(57);
    let age = h.globes()[0].1 as f64;
    let (mut checked, mut rolled) = (0, 0);
    for d in h.pieces() {
        match d.piece.sprite() {
            GlobeSprite::Ring => {
                assert!(
                    (d.rotation * Vec3::Z).dot(Vec3::Y) > 0.9999,
                    "{:?} lies level",
                    d.piece
                );
            }
            GlobeSprite::Star => {
                let a = bench(d.piece, age * 1000.0, 8000.0).angle as f32;
                let want = cam.rotation * Quat::from_rotation_z(-a);
                assert!(
                    d.rotation.angle_between(want) < 1e-3,
                    "{:?} faces the camera rolled by {}",
                    d.piece,
                    -a
                );
                rolled += ((-a).rem_euclid(std::f32::consts::FRAC_PI_2) > 0.05) as u32;
            }
            _ => {
                assert!(
                    d.rotation.angle_between(cam.rotation) < 1e-4,
                    "{:?} faces the camera",
                    d.piece
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 27);
    assert!(rolled >= 4, "most glints are visibly rolled: {rolled}");
}

/// The match camera (`CameraController`'s placement) at `zoom` and `pitch`
/// from the vertical, and `yaw`, looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// The draw order at the match camera's distance, over every pitch it allows
/// (its own 38.7° among them), the body offsets the animation layers write,
/// and Divine Shield's whole life beside it (its tall birth pop, its loop and
/// its death swell): every piece on the globe's axis (its glows and rings)
/// stands on the body's axis and sorts in front of the blended body, as the
/// bench draws it over the body; and Divine Shield's blended outline and tint
/// sort behind every one of them, so they never dim the globe's light. Both
/// rigs draw throughout.
#[test]
fn the_globe_sorts_over_the_body_and_divine_shield_at_the_match_camera() {
    let controller = CameraController::default();
    assert_eq!(controller.zoom_distance, 60.0);
    let offsets = [
        Vec3::ZERO,
        Vec3::new(0.0, 0.22, 0.0),
        Vec3::new(0.18, 0.10, 0.0),
        Vec3::new(0.0, 0.10, -0.18),
        Vec3::new(0.0, 1.2, 0.0),
        Vec3::new(0.0, -0.10, 0.0),
    ];
    let views: Vec<(f32, f32)> = [0.1_f32, 0.4, controller.pitch, 1.0, 1.3, 1.5]
        .into_iter()
        .flat_map(|pitch| [0.0_f32, 1.3, 2.9, 4.4].map(|yaw| (pitch, yaw)))
        .collect();
    let target = Vec3::new(0.0, 1.0, 0.0);
    let mut checked = 0;
    for offset in offsets {
        let mut h = Harness::new();
        let cam = h.camera(Transform::default());
        let (paladin, body) = h.unit(CharacterClass::Paladin, target);
        h.app
            .world_mut()
            .get_mut::<Transform>(body)
            .unwrap()
            .translation = offset;
        h.protect(paladin);
        h.give(
            paladin,
            Aura {
                effect_type: AuraType::DamageImmunity,
                duration: 12.0,
                ability_name: DIVINE_SHIELD_AURA_NAME.into(),
                ..Default::default()
            },
        );
        // Birth and loop for 1.6 s, then the shield ends and swells away for 1 s.
        for frame in 0..160_usize {
            if frame == 100 {
                h.auras(paladin)
                    .retain(|a| a.ability_name != DIVINE_SHIELD_AURA_NAME);
            }
            let (pitch, yaw) = views[frame % views.len()];
            *h.app.world_mut().get_mut::<Transform>(cam).unwrap() =
                match_camera(target, controller.zoom_distance, pitch, yaw);
            h.tick(1);
            let world = h.app.world();
            let view = world.get::<GlobalTransform>(cam).unwrap().compute_matrix();
            let range = ViewRangefinder3d::from_world_from_view(&view);
            let body_at = world.get::<GlobalTransform>(body).unwrap().translation();
            let body_key = range.distance_translation(&body_at);
            // Divine Shield's blended layers, with their sort bias.
            let mut q = h.app.world_mut().query_filtered::<(
                &GlobalTransform,
                &MeshMaterial3d<StandardMaterial>,
            ), With<ShieldCard>>();
            let world = h.app.world();
            let materials = world.resource::<Assets<StandardMaterial>>();
            let blended: Vec<f32> = q
                .iter(world)
                .filter_map(|(g, m)| {
                    let m = materials.get(&m.0)?;
                    matches!(m.alpha_mode, AlphaMode::Blend)
                        .then(|| range.distance_translation(&g.translation()) + m.depth_bias)
                })
                .collect();
            let shielded = {
                let mut shields = h.app.world_mut().query::<&ShieldVisual>();
                shields
                    .iter(h.app.world())
                    .any(|s| s.kind == ShieldKind::DivineShield && s.bearer == paladin)
            };
            assert!(shielded, "Divine Shield draws too (frame {frame})");
            assert_eq!(blended.len(), 2, "Divine Shield's outline and tint");
            let pieces = h.pieces();
            assert_eq!(pieces.len(), 27, "the globe draws too");
            for d in pieces {
                if matches!(
                    d.piece,
                    GlobePiece::PulseFlame { .. } | GlobePiece::Glint { .. }
                ) {
                    continue;
                }
                let what = format!(
                    "{:?} at frame {frame}, pitch {pitch}, yaw {yaw}, offset {offset}",
                    d.piece
                );
                assert!(
                    (d.at.xz() - body_at.xz()).length() < 1e-4,
                    "{what}: off the body's axis"
                );
                let key = range.distance_translation(&d.at) + d.material.depth_bias;
                assert!(key > body_key, "{what}: sorts behind the body");
                for shield in &blended {
                    assert!(
                        key > *shield,
                        "{what}: Divine Shield's blended layer sorts over it by {}",
                        shield - key
                    );
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 6 * 160 * 7);
}

/// At the match camera and a 1080p view, the globe's equator as drawn spans
/// about 33 px across — the size the bench's 0.75 yd radius comes to at
/// 60 yd, the size the Human testing note asks a person to judge.
#[test]
fn the_globe_spans_its_radius_at_the_match_camera() {
    let mut h = Harness::new();
    let (paladin, _) = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.protect(paladin);
    h.tick(2);
    let chest = h.globes()[0].3;
    let ring = h
        .pieces()
        .into_iter()
        .find(|d| d.piece == GlobePiece::GoldRing)
        .unwrap();
    let radius = ring.scale.x * RING_CENTRE / 2.0;
    let controller = CameraController::default();
    let cam = match_camera(chest, controller.zoom_distance, controller.pitch, 0.0);
    let projection = PerspectiveProjection {
        aspect_ratio: 16.0 / 9.0,
        ..default()
    };
    let to_px = |p: Vec3| {
        let clip = projection.get_clip_from_view() * cam.compute_matrix().inverse() * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new((ndc.x + 1.0) / 2.0 * 1920.0, (1.0 - ndc.y) / 2.0 * 1080.0)
    };
    let right = cam.rotation * Vec3::X;
    let across = (to_px(chest + right * radius) - to_px(chest - right * radius)).length();
    assert!((30.0..37.0).contains(&across), "{across} px");
}
