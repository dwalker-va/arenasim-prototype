//! Probes for the shields' blended outline and Divine Shield's tint (AS-401,
//! `rendering/effects/shield.rs`).
//!
//! The client's three shields are additive light, which the bright arena sand
//! swallows; each now also wears an alpha-BLENDED outline that can darken the
//! sand, sharp on its inner edge and glowing outward, and Divine Shield a
//! blended wash of its blue fill. These read the live cards — their
//! materials, their textures and their weights — through the production
//! systems, never the rig's stored fields.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the two
//! shield systems in production order.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, ShieldCard, ShieldKind, ShieldLayer, ShieldPhase,
    ShieldVisual, VisualBody,
};
use arenasim::states::play_match::{
    place_shield_cards, shield_outline_alpha, shield_tuning, update_shield_visuals,
    AEGIS_AURA_NAME, AEGIS_OUTLINE_RGB, DIVINE_SHIELD_AURA_NAME, DS_FILL_RGB, DS_FILL_TINT,
    ICE_BARRIER_AURA_NAME, OUTLINE_INNER, PW_SHIELD_AURA_NAME,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const BODY: Vec3 = Vec3::new(0.0, 1.0, 0.0);

struct Harness {
    app: App,
    bearer: Entity,
}

/// One card as drawn: its material and its texture's alpha along a radius.
struct Card {
    layer: ShieldLayer,
    weight: f32,
    material: StandardMaterial,
    texture: Option<Image>,
}

impl Card {
    /// The texture's alpha at radius `r` (fraction of the half-size), read
    /// along the row through the centre.
    fn texel(&self, r: f32) -> f32 {
        let image = self.texture.as_ref().expect("a textured card");
        let size = image.width() as usize;
        let col = ((size as f32 / 2.0) * (1.0 + r)).min(size as f32 - 1.0) as usize;
        let row = size / 2;
        image.data.as_ref().unwrap()[(row * size + col) * 4 + 3] as f32 / 255.0
    }
}

impl Harness {
    fn new(class: CharacterClass) -> Self {
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
        app.add_systems(Update, update_shield_visuals);
        app.add_systems(
            PostUpdate,
            place_shield_cards.before(TransformSystem::TransformPropagate),
        );
        app.world_mut().spawn((
            Camera3d::default(),
            Transform::from_translation(Vec3::new(0.0, 40.0, 30.0)).looking_at(BODY, Vec3::Y),
        ));
        let bearer = app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, class),
                Transform::from_translation(BODY),
                ActiveAuras::default(),
            ))
            .id();
        let body = app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        app.world_mut().entity_mut(bearer).add_child(body);
        Harness { app, bearer }
    }

    fn give(&mut self, kind: ShieldKind) {
        let aura = match kind {
            ShieldKind::DivineShield => Aura {
                effect_type: AuraType::DamageImmunity,
                duration: 12.0,
                magnitude: 1.0,
                ability_name: DIVINE_SHIELD_AURA_NAME.to_string(),
                ..Default::default()
            },
            ShieldKind::PowerWordShield | ShieldKind::IceBarrier | ShieldKind::Aegis => Aura {
                effect_type: AuraType::Absorb,
                duration: 30.0,
                magnitude: 70.0,
                ability_name: match kind {
                    ShieldKind::IceBarrier => ICE_BARRIER_AURA_NAME,
                    ShieldKind::Aegis => AEGIS_AURA_NAME,
                    _ => PW_SHIELD_AURA_NAME,
                }
                .to_string(),
                ..Default::default()
            },
        };
        let bearer = self.bearer;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .push(aura);
    }

    fn take_all(&mut self) {
        let bearer = self.bearer;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(bearer)
            .unwrap()
            .auras
            .clear();
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn rig(&mut self, kind: ShieldKind) -> Option<ShieldVisual> {
        let mut q = self.app.world_mut().query::<&ShieldVisual>();
        q.iter(self.app.world()).copied().find(|r| r.kind == kind)
    }

    fn run_to(&mut self, kind: ShieldKind, phase: ShieldPhase, at: f32) {
        for _ in 0..2000 {
            if let Some(r) = self.rig(kind) {
                if r.phase == phase && r.t >= at {
                    return;
                }
            }
            self.tick(1);
        }
        panic!("the {kind:?} never reached {phase:?} at {at}s");
    }

    fn cards(&mut self, kind: ShieldKind) -> Vec<Card> {
        let mut rigs = self.app.world_mut().query::<(&ShieldVisual, &Children)>();
        let kids: Vec<Entity> = rigs
            .iter(self.app.world())
            .filter(|(r, _)| r.kind == kind)
            .flat_map(|(_, c)| c.iter().collect::<Vec<_>>())
            .collect();
        let world = self.app.world();
        let materials = world.resource::<Assets<StandardMaterial>>();
        let images = world.resource::<Assets<Image>>();
        kids.into_iter()
            .filter_map(|k| {
                let card = world.get::<ShieldCard>(k)?;
                let m = materials
                    .get(&world.get::<MeshMaterial3d<StandardMaterial>>(k)?.0)?
                    .clone();
                Some(Card {
                    layer: card.layer,
                    weight: m.base_color.alpha(),
                    texture: m
                        .base_color_texture
                        .as_ref()
                        .and_then(|t| images.get(t))
                        .cloned(),
                    material: m,
                })
            })
            .collect()
    }

    fn card(&mut self, kind: ShieldKind, layer: ShieldLayer) -> Option<Card> {
        self.cards(kind).into_iter().find(|c| c.layer == layer)
    }
}

fn class_of(kind: ShieldKind) -> CharacterClass {
    match kind {
        ShieldKind::PowerWordShield => CharacterClass::Priest,
        ShieldKind::DivineShield => CharacterClass::Paladin,
        ShieldKind::IceBarrier => CharacterClass::Mage,
        // A trinket: anyone can wear it.
        ShieldKind::Aegis => CharacterClass::Warrior,
    }
}

// --- the profile ------------------------------------------------------------------

/// The user's ruling: sharp on the inner edge (facing the combatant), a glow
/// fading outward. The edge rises from nothing to full within 1% of the
/// half-size; the glow takes its whole `glow_width` (at least ten times as
/// long) to fall, monotonically, to nothing, as (1 - u)^falloff.
#[test]
fn the_outline_is_sharp_inside_and_glows_outward() {
    for kind in ShieldKind::ALL {
        let o = shield_tuning(kind).outline;
        assert_eq!(shield_outline_alpha(kind, OUTLINE_INNER - 0.007), 0.0);
        assert!(
            shield_outline_alpha(kind, 0.0) == 0.0,
            "{kind:?} is not hollow"
        );
        assert!(
            shield_outline_alpha(kind, OUTLINE_INNER + 0.003) > 0.99,
            "{kind:?}'s inner edge is not sharp"
        );
        // The core holds full.
        for i in 0..=10 {
            let r = OUTLINE_INNER + 0.003 + (o.width - 0.003) * i as f32 / 10.0;
            assert!(shield_outline_alpha(kind, r) > 0.99, "{kind:?} core at {r}");
        }
        // The glow falls the long way, monotonically, on its curve.
        let start = OUTLINE_INNER + o.width;
        let mut last = 1.0;
        for i in 1..=20 {
            let u = i as f32 / 20.0;
            let a = shield_outline_alpha(kind, start + o.glow_width * u);
            assert!(a <= last, "{kind:?}'s glow rises at u = {u}");
            assert!(
                (a - (1.0 - u).powf(o.glow_falloff)).abs() < 1e-4,
                "{kind:?}'s glow is off its curve at u = {u}: {a}"
            );
            last = a;
        }
        assert_eq!(shield_outline_alpha(kind, start + o.glow_width), 0.0);
        assert!(
            o.glow_width >= 10.0 * 0.01,
            "{kind:?}'s glow is no softer than its edge"
        );
        // Half way out the glow is still lit, unlike a hard band.
        assert!(shield_outline_alpha(kind, start + o.glow_width * 0.5) > 0.2);
    }
}

/// The outline card each shield wears carries exactly that profile in its
/// texture, so the card the player sees is the profile above.
#[test]
fn each_outline_card_wears_its_profile() {
    for kind in ShieldKind::ALL {
        let mut h = Harness::new(class_of(kind));
        h.give(kind);
        h.run_to(kind, ShieldPhase::Loop, 0.1);
        let card = h.card(kind, ShieldLayer::Outline).expect("no outline card");
        let size = card.texture.as_ref().unwrap().width() as f32;
        let mut checked = 0;
        for i in 0..200 {
            let r = i as f32 / 200.0;
            // The texel centre this sample lands in, and the profile there.
            let col = ((size / 2.0) * (1.0 + r)).floor();
            let centre = ((col + 0.5) / size) * 2.0 - 1.0;
            let want = shield_outline_alpha(kind, centre);
            assert!(
                (card.texel(r) - want).abs() < 1.0 / 255.0 + 1e-3,
                "{kind:?} outline texel at {r}: {} vs {want}",
                card.texel(r)
            );
            checked += 1;
        }
        assert_eq!(checked, 200);
    }
}

// --- the materials ------------------------------------------------------------------

/// The outline and tint are BLENDED, so they can darken the sand; every client
/// card stays additive. The outline is its shield's client colour at its
/// level in linear light; the tint is the client's fill blue, on Divine Shield
/// alone.
#[test]
fn the_outline_and_tint_blend_and_the_client_cards_stay_additive() {
    for kind in ShieldKind::ALL {
        let mut h = Harness::new(class_of(kind));
        h.give(kind);
        h.run_to(kind, ShieldPhase::Loop, 0.1);
        let cards = h.cards(kind);
        assert_eq!(cards.len(), kind.layers().len());
        let o = shield_tuning(kind).outline;
        for c in &cards {
            match c.layer {
                ShieldLayer::Outline => {
                    assert_eq!(c.material.alpha_mode, AlphaMode::Blend, "{kind:?}");
                    assert!(c.material.unlit);
                    let want = Color::srgb(o.rgb[0], o.rgb[1], o.rgb[2]).to_linear() * o.level;
                    let got = c.material.base_color.to_linear();
                    for (g, w) in [
                        (got.red, want.red),
                        (got.green, want.green),
                        (got.blue, want.blue),
                    ] {
                        assert!((g - w).abs() < 1e-5, "{kind:?} outline colour {got:?}");
                    }
                }
                ShieldLayer::Tint => {
                    assert_eq!(kind, ShieldKind::DivineShield, "only the bubble is tinted");
                    assert_eq!(c.material.alpha_mode, AlphaMode::Blend);
                    let s = c.material.base_color.to_srgba();
                    for (g, w) in [
                        (s.red, DS_FILL_RGB[0]),
                        (s.green, DS_FILL_RGB[1]),
                        (s.blue, DS_FILL_RGB[2]),
                    ] {
                        assert!((g - w).abs() < 1e-4, "tint colour {s:?}");
                    }
                }
                _ => assert_eq!(
                    c.material.alpha_mode,
                    AlphaMode::Add,
                    "{kind:?} {:?}",
                    c.layer
                ),
            }
        }
        assert_eq!(
            cards.iter().any(|c| c.layer == ShieldLayer::Tint),
            kind == ShieldKind::DivineShield
        );
    }
}

/// Every outline is drawn in its own rim's colour, read off the live rim card;
/// and Divine Shield's is PW:S's gold line at PW:S's level, because the client
/// draws the same gold rim on both (AS-405). Its blue stays inside, in the
/// tint and the fill. Aegis is the one exception, by ruling (AS-338): PW:S's
/// gold rim, outlined in silver so the two can be told apart.
#[test]
fn every_outline_wears_its_rim_colour_and_the_bubble_wears_the_shields_gold() {
    let mut outlines = Vec::new();
    for kind in ShieldKind::ALL {
        let mut h = Harness::new(class_of(kind));
        h.give(kind);
        h.run_to(kind, ShieldPhase::Loop, 0.1);
        let outline = h.card(kind, ShieldLayer::Outline).unwrap();
        let rim = h.card(kind, ShieldLayer::Rim).unwrap();
        let level = shield_tuning(kind).outline.level;
        let got = outline.material.base_color.to_linear();
        let want = if kind == ShieldKind::Aegis {
            let [r, g, b] = AEGIS_OUTLINE_RGB;
            Color::srgb(r, g, b).to_linear() * level
        } else {
            rim.material.base_color.to_linear() * level
        };
        for (g, w) in [
            (got.red, want.red),
            (got.green, want.green),
            (got.blue, want.blue),
        ] {
            assert!(
                (g - w).abs() < 1e-5,
                "{kind:?} outline {got:?} is not its rim's colour {want:?}"
            );
        }
        outlines.push((kind, got));
    }
    let of = |k: ShieldKind| outlines.iter().find(|(o, _)| *o == k).unwrap().1;
    let (ds, pws) = (
        of(ShieldKind::DivineShield),
        of(ShieldKind::PowerWordShield),
    );
    for (d, p) in [
        (ds.red, pws.red),
        (ds.green, pws.green),
        (ds.blue, pws.blue),
    ] {
        assert!(
            (d - p).abs() < 1e-6,
            "Divine Shield's outline {ds:?} vs PW:S's {pws:?}"
        );
    }
    assert!(
        ds.red > 2.0 * ds.blue,
        "Divine Shield's outline is not gold: {ds:?}"
    );
    // Aegis: silver, so it reads apart from the gold PW:S line on its own rim.
    let aegis = of(ShieldKind::Aegis);
    assert!(
        aegis.blue > aegis.red && aegis.red > 0.5 * aegis.blue,
        "Aegis's outline is not silver: {aegis:?}"
    );
    assert!(
        (aegis.blue - pws.blue).abs() > 0.1,
        "Aegis's outline {aegis:?} reads as PW:S's gold {pws:?}"
    );
}

// --- the life ------------------------------------------------------------------------

/// The outline follows the rim's life: dark for the client's 200 ms delay,
/// full at the pop, held through the wobble between `alpha` (the rim at its
/// peak) and `alpha × (1 - pulse × 0.6)` (the rim's trough is 0.4 of its
/// peak on both client tracks), reaching both, then faded out with the death
/// — not cut.
#[test]
fn the_outline_pops_holds_pulses_and_fades_with_the_rim() {
    for kind in ShieldKind::ALL {
        let o = shield_tuning(kind).outline;
        let mut h = Harness::new(class_of(kind));
        h.give(kind);
        h.run_to(kind, ShieldPhase::Birth, 0.10);
        let w = h.card(kind, ShieldLayer::Outline).unwrap().weight;
        assert!(w < 1e-3, "{kind:?} outline lit in the delay: {w}");
        h.run_to(kind, ShieldPhase::Birth, 0.296);
        let w = h.card(kind, ShieldLayer::Outline).unwrap().weight;
        assert!(w > 0.9 * o.alpha, "{kind:?} outline did not pop: {w}");

        h.run_to(kind, ShieldPhase::Loop, 0.0);
        let (mut lo, mut hi) = (f32::MAX, 0.0_f32);
        for _ in 0..80 {
            h.tick(1);
            let w = h.card(kind, ShieldLayer::Outline).unwrap().weight;
            assert!(
                w >= o.alpha * (1.0 - o.pulse * 0.6) - 1e-3 && w <= o.alpha + 1e-3,
                "{kind:?} held outline {w}"
            );
            lo = lo.min(w);
            hi = hi.max(w);
        }
        assert!(hi > o.alpha * 0.97, "{kind:?} never reached full: {hi}");
        assert!(
            lo < o.alpha * (1.0 - 0.5 * o.pulse),
            "{kind:?} never pulsed: {lo}"
        );

        let before = h.card(kind, ShieldLayer::Outline).unwrap().weight;
        h.take_all();
        h.tick(2);
        assert_eq!(h.rig(kind).unwrap().phase, ShieldPhase::Death);
        let early = h.card(kind, ShieldLayer::Outline).unwrap().weight;
        assert!(
            early > 0.8 * before,
            "{kind:?} outline cut at the death, not faded: {before} -> {early}"
        );
        h.run_to(kind, ShieldPhase::Death, 1.0);
        let w = h.card(kind, ShieldLayer::Outline).map_or(0.0, |c| c.weight);
        assert!(w < 0.02, "{kind:?} outline outlived the rim's fade: {w}");
    }
}

/// Divine Shield's tint lies over the body at its own opacity, through the
/// same life as the outline.
#[test]
fn the_bubble_is_tinted_blue_while_it_holds() {
    let mut h = Harness::new(CharacterClass::Paladin);
    h.give(ShieldKind::DivineShield);
    h.run_to(ShieldKind::DivineShield, ShieldPhase::Loop, 0.0);
    let pulse = shield_tuning(ShieldKind::DivineShield).outline.pulse;
    for _ in 0..40 {
        h.tick(1);
        let tint = h.card(ShieldKind::DivineShield, ShieldLayer::Tint).unwrap();
        assert!(
            tint.weight >= DS_FILL_TINT * (1.0 - pulse * 0.6) - 1e-3
                && tint.weight <= DS_FILL_TINT + 1e-3,
            "tint opacity {}",
            tint.weight
        );
        // It is the fill's soft disc: densest over the body's centre.
        assert!(tint.texel(0.0) > 0.85, "the tint is hollow");
    }
}
