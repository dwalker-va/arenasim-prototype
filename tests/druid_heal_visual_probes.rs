//! Probes for the Druid's heal-over-time and Mark of the Wild visuals
//! (`rendering/effects/druid_heals.rs`, AS-212): Rejuvenation's ribbon swirl,
//! Lifebloom's head pulse and gold bloom, Mark of the Wild's paw glyph, and the
//! per-name HoT tick route that stops the Druid's HoTs borrowing Healing
//! Stream's pulse.
//!
//! Two halves:
//!
//! - **The sim's markers**, driven through the real systems on a bare `World`
//!   (the `aura_stacking_bloom.rs` idiom): `apply_pending_auras` marks a
//!   landing on a fresh application AND on a refresh; `process_blooms` marks a
//!   burst on every ending that blooms (expiry, purge, a source-pinned purge)
//!   and on none that does not (a refresh, the bearer's death); a Druid HoT's
//!   tick lands no `HealImpact` while Healing Stream's still does.
//! - **The renderer**, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//!   with the chain `states/mod.rs` registers, asserting WORLD geometry: the
//!   ribbons are stretched bands orbiting the body at the bench's radius and
//!   heights; the pulse sits over the head and leaves by every exit path —
//!   expiry/dispel/purge (the aura leaves the list), the last aura (the
//!   component is removed), and death (the aura stays on the corpse) — and
//!   survives a refresh; two Druids' Lifeblooms draw two pulses; the glyph's
//!   crossed plates stand 1.03 yd over the crown.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::{apply_pending_auras, process_hot_ticks, update_auras};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraLanding, AuraLandingKind, AuraPending, AuraType,
    BloomBurst, BloomVisual, Combatant, DispelPending, DispelScope, DruidEffect, DruidEffectRig,
    DruidParticle, GameRng, HealImpact, HealImpactKind, MarkOfTheWildPlate, RejuvenationRibbon,
    LIFEBLOOM_AURA, MARK_OF_THE_WILD_AURA, REJUVENATION_AURA,
};
use arenasim::states::play_match::effects::{process_blooms, process_dispels, BloomPending};
use arenasim::states::play_match::{
    animate_druid_effects, animate_druid_particles, billboard_druid_particles,
    cleanup_druid_effects, glyph_alpha, spawn_druid_effects, AbilityDefinitions, AbilityType,
    BODY_CENTRE_ABOVE_FLOOR, DRUID_HEAD_ABOVE_FLOOR, MOTW_GLYPH_HEIGHT_ABOVE_HEAD, MOTW_GLYPH_SECS,
    MOTW_GLYPH_SIZE, MOTW_GLYPH_TEXTURE, REJUV_LANDING_SECS, REJUV_RIBBON_HEIGHTS,
    REJUV_RIBBON_ORBIT_RADIUS, REJUV_RIBBON_WIDTH,
};
use arenasim::CharacterClass;

// ── the sim's markers ────────────────────────────────────────────────────────

fn world(dt: f32) -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(dt));
    world.insert_resource(time);
    world
}

fn druid_and_ally(world: &mut World) -> (Entity, Entity) {
    let druid = world
        .spawn((
            Combatant::new(1, 1, CharacterClass::Druid),
            Transform::default(),
        ))
        .id();
    let mut hurt = Combatant::new(1, 0, CharacterClass::Warrior);
    hurt.current_health = 100.0;
    let ally = world.spawn((hurt, Transform::default())).id();
    (druid, ally)
}

fn cast(ability: AbilityType, target: Entity, caster: Entity) -> AuraPending {
    let defs = AbilityDefinitions::default();
    AuraPending::from_ability_scaled(target, caster, defs.get_unchecked(&ability), 100.0)
        .expect("the ability applies an aura")
}

fn land(world: &mut World, pending: AuraPending) {
    world.spawn(pending);
    world.run_system_once(apply_pending_auras).unwrap();
}

fn landings(world: &mut World) -> Vec<(Entity, AuraLandingKind)> {
    let mut q = world.query::<&AuraLanding>();
    q.iter(world).map(|l| (l.target, l.kind)).collect()
}

fn bursts(world: &mut World) -> Vec<(Entity, BloomVisual)> {
    let mut q = world.query::<&BloomBurst>();
    q.iter(world).map(|b| (b.target, b.kind)).collect()
}

/// The router's names are the RON names: a rename in `abilities.ron` must not
/// silently drop a Druid HoT off its visual.
#[test]
fn the_routed_names_are_the_config_names() {
    let defs = AbilityDefinitions::default();
    let name = |a: AbilityType| defs.get_unchecked(&a).name.clone();
    assert_eq!(name(AbilityType::Rejuvenation), REJUVENATION_AURA);
    assert_eq!(name(AbilityType::Lifebloom), LIFEBLOOM_AURA);
    assert_eq!(name(AbilityType::MarkOfTheWild), MARK_OF_THE_WILD_AURA);
}

#[test]
fn rejuvenation_marks_its_swirl_on_landing_and_on_a_refresh() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(&mut world, cast(AbilityType::Rejuvenation, ally, druid));
    assert_eq!(
        landings(&mut world),
        vec![(ally, AuraLandingKind::RejuvenationSwirl)]
    );
    // A recast refreshes (max_stacks 1) — and the client plays the landing on
    // every cast, so the swirl plays again.
    land(&mut world, cast(AbilityType::Rejuvenation, ally, druid));
    assert!(world
        .resource::<CombatLog>()
        .entries
        .iter()
        .any(|e| e.message.contains("Rejuvenation refreshed")));
    assert_eq!(landings(&mut world).len(), 2);
}

#[test]
fn mark_of_the_wild_marks_one_glyph_and_lifebloom_marks_no_landing() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(&mut world, cast(AbilityType::MarkOfTheWild, ally, druid));
    // One glyph for the whole compound — its riders land no glyph of their own.
    assert_eq!(
        landings(&mut world),
        vec![(ally, AuraLandingKind::MarkOfTheWildGlyph)]
    );
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid));
    assert_eq!(
        landings(&mut world).len(),
        1,
        "Lifebloom's identity is its state, not a landing"
    );
}

/// A HoT aura due to tick this frame.
fn due(mut aura: Aura) -> Aura {
    aura.time_until_next_tick = 0.001;
    aura
}

#[test]
fn a_druid_hot_tick_heals_and_draws_nothing_while_healing_stream_still_pulses() {
    let mut world = world(0.016);
    let (druid, ally) = druid_and_ally(&mut world);
    let rejuv = due(cast(AbilityType::Rejuvenation, ally, druid).aura);
    let lifebloom = due(cast(AbilityType::Lifebloom, ally, druid).aura);
    world.entity_mut(ally).insert(ActiveAuras {
        auras: vec![rejuv, lifebloom],
    });
    let before = world
        .entity(ally)
        .get::<Combatant>()
        .unwrap()
        .current_health;
    world.run_system_once(process_hot_ticks).unwrap();
    let after = world
        .entity(ally)
        .get::<Combatant>()
        .unwrap()
        .current_health;
    assert!(after > before, "the probe went vacuous: neither HoT ticked");
    let impacts = {
        let mut q = world.query::<&HealImpact>();
        q.iter(&world).count()
    };
    assert_eq!(
        impacts, 0,
        "Rejuvenation and Lifebloom ticks must not borrow the totem pulse"
    );

    // Healing Stream on its own bearer still pulses.
    let mut hurt = Combatant::new(1, 2, CharacterClass::Priest);
    hurt.current_health = 100.0;
    let shaman = world
        .spawn((
            Combatant::new(1, 3, CharacterClass::Shaman),
            Transform::default(),
        ))
        .id();
    let stream = Aura {
        effect_type: AuraType::HealingOverTime,
        duration: 2.0,
        magnitude: 8.0,
        tick_interval: 1.0,
        time_until_next_tick: 0.001,
        caster: Some(shaman),
        ability_name: "Healing Stream Totem".to_string(),
        ..Default::default()
    };
    let bearer = world
        .spawn((
            hurt,
            Transform::default(),
            ActiveAuras {
                auras: vec![stream],
            },
        ))
        .id();
    world.run_system_once(process_hot_ticks).unwrap();
    let impacts: Vec<(Entity, HealImpactKind)> = {
        let mut q = world.query::<&HealImpact>();
        q.iter(&world).map(|i| (i.target, i.kind)).collect()
    };
    assert_eq!(impacts, vec![(bearer, HealImpactKind::TotemPulse)]);
}

/// A Lifebloom from `caster` on its last frame, `stacks` deep.
fn ending_lifebloom(target: Entity, caster: Entity, stacks: u8) -> Aura {
    let mut aura = cast(AbilityType::Lifebloom, target, caster).aura;
    aura.stacks = aura
        .stacks
        .map(|s| arenasim::states::play_match::AuraStacks { count: stacks, ..s });
    aura.duration = 0.5;
    aura.time_until_next_tick = 5.0;
    aura
}

fn run_expiry(world: &mut World) {
    world.run_system_once(process_hot_ticks).unwrap();
    world.run_system_once(update_auras).unwrap();
    world.run_system_once(process_blooms).unwrap();
}

#[test]
fn the_burst_plays_when_lifebloom_runs_out() {
    let mut world = world(1.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let aura = ending_lifebloom(ally, druid, 2);
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });
    run_expiry(&mut world);
    assert_eq!(bursts(&mut world), vec![(ally, BloomVisual::GoldBurst)]);
}

#[test]
fn the_burst_plays_when_lifebloom_is_purged_or_purged_by_source() {
    let scopes = |druid: Entity| {
        [
            DispelScope::Purge(AuraType::HealingOverTime),
            DispelScope::PurgeSource {
                effect: AuraType::HealingOverTime,
                source: LIFEBLOOM_AURA.to_string(),
                owner: Some(druid),
            },
        ]
    };
    for i in 0..2 {
        let mut world = world(0.0);
        let (druid, ally) = druid_and_ally(&mut world);
        let aura = cast(AbilityType::Lifebloom, ally, druid).aura;
        world
            .entity_mut(ally)
            .insert(ActiveAuras { auras: vec![aura] });
        let shaman = world
            .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
            .id();
        world.spawn(DispelPending {
            target: ally,
            dispeller: shaman,
            log_prefix: "[PURGE]",
            caster_class: CharacterClass::Shaman,
            heal_on_success: None,
            scope: scopes(druid)[i].clone(),
        });
        world.run_system_once(process_dispels).unwrap();
        world.run_system_once(process_blooms).unwrap();
        assert_eq!(
            bursts(&mut world),
            vec![(ally, BloomVisual::GoldBurst)],
            "scope {i}"
        );
    }
}

/// The burst site's OWN alive check, pinned directly: a bloom that reaches
/// `process_blooms` for a bearer who is already dead (killed after its aura's
/// last frame queued the bloom) lands no heal and plays no burst. The
/// expiry-path probe below cannot show this — `process_hot_ticks` never
/// queues a bloom for a corpse in the first place.
#[test]
fn a_bloom_queued_for_a_dead_bearer_plays_no_burst() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    world
        .entity_mut(ally)
        .get_mut::<Combatant>()
        .unwrap()
        .current_health = 0.0;
    world.spawn(BloomPending {
        target: ally,
        amount: 40.0,
        ability_name: LIFEBLOOM_AURA.to_string(),
        caster: Some(druid),
    });
    world.run_system_once(process_blooms).unwrap();
    assert!(bursts(&mut world).is_empty(), "a corpse does not bloom");

    // The same pending bloom on a LIVING bearer does burst — the probe is
    // not vacuous.
    let mut world = world_alive();
    let (druid, ally) = druid_and_ally(&mut world);
    world.spawn(BloomPending {
        target: ally,
        amount: 40.0,
        ability_name: LIFEBLOOM_AURA.to_string(),
        caster: Some(druid),
    });
    world.run_system_once(process_blooms).unwrap();
    assert_eq!(bursts(&mut world), vec![(ally, BloomVisual::GoldBurst)]);
}

fn world_alive() -> World {
    world(0.0)
}

#[test]
fn no_burst_on_a_refresh_or_on_the_bearers_death() {
    // A refresh.
    let mut world_a = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world_a);
    land(&mut world_a, cast(AbilityType::Lifebloom, ally, druid));
    land(&mut world_a, cast(AbilityType::Lifebloom, ally, druid));
    world_a.run_system_once(process_blooms).unwrap();
    assert!(bursts(&mut world_a).is_empty(), "a refresh does not bloom");

    // The bearer dies with Lifebloom on its last frame.
    let mut world_b = world(1.0);
    let (druid, ally) = druid_and_ally(&mut world_b);
    let aura = ending_lifebloom(ally, druid, 3);
    world_b
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });
    world_b
        .entity_mut(ally)
        .get_mut::<Combatant>()
        .unwrap()
        .current_health = 0.0;
    run_expiry(&mut world_b);
    assert!(bursts(&mut world_b).is_empty(), "a corpse does not bloom");
}

#[test]
fn two_druids_lifeblooms_burst_separately() {
    let mut world = world(1.0);
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = world
        .spawn((
            Combatant::new(1, 2, CharacterClass::Druid),
            Transform::default(),
        ))
        .id();
    let a = ending_lifebloom(ally, druid_a, 3);
    let b = ending_lifebloom(ally, druid_b, 1);
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![a, b] });
    run_expiry(&mut world);
    assert_eq!(
        bursts(&mut world),
        vec![
            (ally, BloomVisual::GoldBurst),
            (ally, BloomVisual::GoldBurst)
        ]
    );
}

// ── the renderer ─────────────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;

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
        // The chain `states/mod.rs` registers.
        app.add_systems(
            Update,
            (
                spawn_druid_effects,
                animate_druid_effects,
                animate_druid_particles,
                billboard_druid_particles,
                cleanup_druid_effects,
            )
                .chain(),
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

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / TICK_SECS).ceil() as u32);
    }

    fn bearer(&mut self, at: Vec3, auras: Vec<Aura>) -> Entity {
        let mut e = self.app.world_mut().spawn((
            Combatant::new(0, 0, CharacterClass::Warrior),
            Transform::from_translation(at),
        ));
        if !auras.is_empty() {
            e.insert(ActiveAuras { auras });
        }
        e.id()
    }

    fn rigs(&mut self, effect: DruidEffect) -> Vec<(Entity, Entity, Option<Entity>)> {
        let world = self.app.world_mut();
        let mut q = world.query::<(Entity, &DruidEffectRig)>();
        q.iter(world)
            .filter(|(_, r)| r.effect == effect)
            .map(|(e, r)| (e, r.target, r.caster))
            .collect()
    }

    fn particles(&mut self, effect: DruidEffect) -> Vec<(Entity, Vec3)> {
        let world = self.app.world_mut();
        let mut q = world.query::<(&DruidParticle, &GlobalTransform)>();
        q.iter(world)
            .filter(|(p, _)| p.effect == effect)
            .map(|(p, g)| (p.rig, g.translation()))
            .collect()
    }
}

fn lifebloom_from(caster: Entity) -> Aura {
    Aura {
        effect_type: AuraType::HealingOverTime,
        duration: 7.0,
        magnitude: 1.0,
        tick_interval: 1.0,
        time_until_next_tick: 1.0,
        caster: Some(caster),
        ability_name: LIFEBLOOM_AURA.to_string(),
        ..Default::default()
    }
}

/// Each ribbon's vertex positions and its triangle-list index count.
fn ribbon_positions(h: &mut Harness) -> Vec<(usize, Vec<Vec3>, usize)> {
    let world = h.app.world_mut();
    let mut q = world.query::<&RejuvenationRibbon>();
    let ribbons: Vec<(usize, Handle<Mesh>)> =
        q.iter(world).map(|r| (r.index, r.mesh.clone())).collect();
    let meshes = world.resource::<Assets<Mesh>>();
    ribbons
        .into_iter()
        .map(|(i, handle)| {
            let mesh = meshes.get(&handle).expect("the ribbon mesh exists");
            let Some(VertexAttributeValues::Float32x3(ps)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("ribbon positions")
            };
            let indices = mesh.indices().map_or(0, |ix| ix.len());
            (
                i,
                ps.iter().map(|p| Vec3::from_array(*p)).collect(),
                indices,
            )
        })
        .collect()
}

#[test]
fn rejuvenation_swirls_five_banded_ribbons_round_the_body_then_leaves() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, COMBATANT_Y, -2.0);
    let ally = h.bearer(at, vec![]);
    h.app.world_mut().spawn(AuraLanding {
        target: ally,
        kind: AuraLandingKind::RejuvenationSwirl,
    });
    h.tick_secs(1.0);

    assert_eq!(h.rigs(DruidEffect::RejuvenationSwirl).len(), 1);
    let ribbons = ribbon_positions(&mut h);
    assert_eq!(ribbons.len(), 5, "five ribbons");
    for (index, verts, indices) in &ribbons {
        // A full trail: 15 samples, two edge vertices each — a strip, not a
        // sprite.
        assert_eq!(verts.len(), 30, "ribbon {index} is a band of 15 samples");
        // And it is DRAWN: two triangles per segment between the 15 samples.
        assert_eq!(
            *indices,
            (15 - 1) * 6,
            "ribbon {index} must emit its strip's triangles"
        );
        let floor_height = REJUV_RIBBON_HEIGHTS[*index];
        let mut along = 0.0;
        for pair in verts.chunks(2) {
            let centre = (pair[0] + pair[1]) * 0.5;
            let radial = Vec2::new(centre.x - at.x, centre.z - at.z).length();
            assert!(
                (radial - REJUV_RIBBON_ORBIT_RADIUS).abs() < 0.05,
                "ribbon {index} orbits at {radial}, not {REJUV_RIBBON_ORBIT_RADIUS}"
            );
            let world_height = floor_height + (COMBATANT_Y - BODY_CENTRE_ABOVE_FLOOR);
            assert!(
                (centre.y - world_height).abs() <= 0.12 + 1e-3,
                "ribbon {index} at height {} (bench {world_height} ± 0.12)",
                centre.y
            );
            let width = pair[0].distance(pair[1]);
            assert!(
                (width - REJUV_RIBBON_WIDTH).abs() < 1e-3,
                "band width {width}"
            );
        }
        for k in 1..verts.len() / 2 {
            along += ((verts[2 * k] + verts[2 * k + 1]) - (verts[2 * k - 2] + verts[2 * k - 1]))
                .length()
                * 0.5;
        }
        // 0.45 s of a 0.75 yd orbit at 1 rev/s is ~2.1 yd of band.
        assert!(
            along > 5.0 * REJUV_RIBBON_WIDTH,
            "ribbon {index} is {along} yd long — a band, not a dot"
        );
    }
    assert!(
        !h.particles(DruidEffect::RejuvenationSwirl).is_empty(),
        "the flare sparks and starburst emit"
    );

    h.tick_secs(REJUV_LANDING_SECS);
    assert!(
        h.rigs(DruidEffect::RejuvenationSwirl).is_empty(),
        "the swirl retires"
    );
    assert!(ribbon_positions(&mut h).is_empty(), "and takes its ribbons");
    assert!(h.particles(DruidEffect::RejuvenationSwirl).is_empty());
}

#[test]
fn the_lifebloom_pulse_sits_over_the_head() {
    let mut h = Harness::new();
    let druid = h.bearer(Vec3::new(-5.0, COMBATANT_Y, 0.0), vec![]);
    let at = Vec3::new(2.0, COMBATANT_Y, 1.0);
    h.bearer(at, vec![lifebloom_from(druid)]);
    // Into the second loop's streak window.
    h.tick_secs(1.834 + 0.35);
    let particles = h.particles(DruidEffect::LifebloomPulse);
    assert!(particles.len() >= 3, "the pulse emits: {}", particles.len());
    let crown = at.y + DRUID_HEAD_ABOVE_FLOOR - BODY_CENTRE_ABOVE_FLOOR;
    let near_head = particles
        .iter()
        .filter(|(_, p)| p.y > crown - 0.6 && Vec2::new(p.x - at.x, p.z - at.z).length() < 1.5)
        .count();
    assert_eq!(
        near_head,
        particles.len(),
        "every pulse particle is about the head"
    );
}

/// Every exit path of the aura retires the pulse and its particles.
#[test]
fn the_pulse_leaves_by_every_exit_path_and_survives_a_refresh() {
    #[derive(Debug, Clone, Copy)]
    enum Exit {
        /// Expiry, dispel and purge all take the aura out of the list.
        AuraLeavesTheList,
        /// It was the bearer's last aura: `ActiveAuras` itself is removed.
        ComponentRemoved,
        /// Death preserves the aura on the corpse.
        BearerDies,
    }
    for exit in [
        Exit::AuraLeavesTheList,
        Exit::ComponentRemoved,
        Exit::BearerDies,
    ] {
        let mut h = Harness::new();
        let druid = h.bearer(Vec3::new(-5.0, COMBATANT_Y, 0.0), vec![]);
        let other = Aura {
            effect_type: AuraType::MaxHealthIncrease,
            duration: 600.0,
            magnitude: 20.0,
            ability_name: MARK_OF_THE_WILD_AURA.to_string(),
            ..Default::default()
        };
        let auras = match exit {
            Exit::ComponentRemoved => vec![lifebloom_from(druid)],
            _ => vec![lifebloom_from(druid), other],
        };
        let ally = h.bearer(Vec3::new(1.0, COMBATANT_Y, 1.0), auras);
        h.tick_secs(0.4);
        let rigs = h.rigs(DruidEffect::LifebloomPulse);
        assert_eq!(rigs.len(), 1, "{exit:?}: one pulse");
        assert!(
            !h.particles(DruidEffect::LifebloomPulse).is_empty(),
            "{exit:?}: vacuous"
        );

        // A refresh: the same aura, duration and stacks restored. One pulse,
        // the same one.
        {
            let world = h.app.world_mut();
            let mut auras = world.get_mut::<ActiveAuras>(ally).unwrap();
            let lb = auras
                .auras
                .iter_mut()
                .find(|a| a.ability_name == LIFEBLOOM_AURA)
                .unwrap();
            lb.duration = 7.0;
        }
        h.tick(3);
        assert_eq!(
            h.rigs(DruidEffect::LifebloomPulse),
            rigs,
            "{exit:?}: a refresh keeps the pulse"
        );

        {
            let world = h.app.world_mut();
            match exit {
                Exit::AuraLeavesTheList => {
                    world
                        .get_mut::<ActiveAuras>(ally)
                        .unwrap()
                        .auras
                        .retain(|a| a.ability_name != LIFEBLOOM_AURA);
                }
                Exit::ComponentRemoved => {
                    world.entity_mut(ally).remove::<ActiveAuras>();
                }
                Exit::BearerDies => {
                    world.get_mut::<Combatant>(ally).unwrap().current_health = 0.0;
                }
            }
        }
        h.tick(2);
        assert!(
            h.rigs(DruidEffect::LifebloomPulse).is_empty(),
            "{exit:?}: the pulse leaves"
        );
        assert!(
            h.particles(DruidEffect::LifebloomPulse).is_empty(),
            "{exit:?}: and takes its particles"
        );
        // And does not come back while the corpse still carries the aura.
        h.tick_secs(1.0);
        assert!(
            h.rigs(DruidEffect::LifebloomPulse).is_empty(),
            "{exit:?}: no re-spawn"
        );
    }
}

#[test]
fn two_druids_lifeblooms_draw_two_pulses_and_leave_separately() {
    let mut h = Harness::new();
    let druid_a = h.bearer(Vec3::new(-5.0, COMBATANT_Y, 0.0), vec![]);
    let druid_b = h.bearer(Vec3::new(5.0, COMBATANT_Y, 0.0), vec![]);
    let ally = h.bearer(
        Vec3::new(0.0, COMBATANT_Y, 3.0),
        vec![lifebloom_from(druid_a), lifebloom_from(druid_b)],
    );
    h.tick_secs(0.4);
    let mut casters: Vec<Option<Entity>> = h
        .rigs(DruidEffect::LifebloomPulse)
        .iter()
        .map(|(_, target, caster)| {
            assert_eq!(*target, ally);
            *caster
        })
        .collect();
    casters.sort();
    let mut expected = vec![Some(druid_a), Some(druid_b)];
    expected.sort();
    assert_eq!(casters, expected, "one pulse per caster's Lifebloom");

    // A's Lifebloom ends (it bloomed, was purged, ran out): B's pulse stays.
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(ally)
        .unwrap()
        .auras
        .retain(|a| a.caster != Some(druid_a));
    h.tick(2);
    let left: Vec<Option<Entity>> = h
        .rigs(DruidEffect::LifebloomPulse)
        .iter()
        .map(|r| r.2)
        .collect();
    assert_eq!(left, vec![Some(druid_b)]);
}

#[test]
fn two_bloom_bursts_on_one_bearer_play_as_two() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let ally = h.bearer(at, vec![]);
    for _ in 0..2 {
        h.app.world_mut().spawn(BloomBurst {
            target: ally,
            kind: BloomVisual::GoldBurst,
        });
    }
    h.tick_secs(0.3);
    let rigs = h.rigs(DruidEffect::LifebloomBloom);
    assert_eq!(rigs.len(), 2);
    let particles = h.particles(DruidEffect::LifebloomBloom);
    for (rig, _, _) in &rigs {
        assert!(
            particles.iter().any(|(r, _)| r == rig),
            "each burst emits its own gold"
        );
    }
    // The burst sits at the chest: born within a yard of it.
    let chest = at.y + 0.45;
    assert!(particles.iter().all(|(_, p)| (p.y - chest).abs() < 2.0));
    h.tick_secs(2.5);
    assert!(
        h.rigs(DruidEffect::LifebloomBloom).is_empty(),
        "the burst retires"
    );
    assert!(h.particles(DruidEffect::LifebloomBloom).is_empty());
}

#[test]
fn the_glyph_is_two_superimposed_camera_facing_plates_over_the_crown_for_two_thirds_of_a_second() {
    assert!(
        std::path::Path::new("assets")
            .join(MOTW_GLYPH_TEXTURE)
            .exists(),
        "the decoded agility_128 texture is committed"
    );
    let mut h = Harness::new();
    let at = Vec3::new(-1.0, COMBATANT_Y, 4.0);
    let ally = h.bearer(at, vec![]);
    // A bearer turned away from the world axes, so a plate that merely
    // inherited the bearer's facing could not pass for camera-facing.
    h.app
        .world_mut()
        .get_mut::<Transform>(ally)
        .unwrap()
        .rotation = Quat::from_rotation_y(1.1);
    let camera = Transform::from_xyz(8.0, 9.0, 14.0).looking_at(at, Vec3::Y);
    h.app.world_mut().spawn((Camera3d::default(), camera));
    h.app.world_mut().spawn(AuraLanding {
        target: ally,
        kind: AuraLandingKind::MarkOfTheWildGlyph,
    });
    h.tick(3);

    let plates: Vec<(usize, GlobalTransform)> = {
        let world = h.app.world_mut();
        let mut q = world.query::<(&MarkOfTheWildPlate, &GlobalTransform)>();
        q.iter(world).map(|(p, g)| (p.layer, *g)).collect()
    };
    assert_eq!(plates.len(), 2, "red-orange and gold");
    let crown = at.y + DRUID_HEAD_ABOVE_FLOOR - BODY_CENTRE_ABOVE_FLOOR;
    for (layer, g) in &plates {
        let p = g.translation();
        assert!(
            (p.y - (crown + MOTW_GLYPH_HEIGHT_ABOVE_HEAD)).abs() < 1e-3,
            "plate {layer} at {} — 1.03 yd over the crown",
            p.y
        );
        assert!(Vec2::new(p.x - at.x, p.z - at.z).length() < 1e-3);
        let (scale, _, _) = g.to_scale_rotation_translation();
        assert!((scale.x - MOTW_GLYPH_SIZE).abs() < 1e-3);
    }
    // One plane, superimposed — the client's two quads, and the bench's one
    // screen-space glyph — so the gold fades out over the red-orange on the
    // same face. And that face is turned to the camera.
    let normal = |g: &GlobalTransform| g.compute_transform().rotation * Vec3::Z;
    assert!(
        plates[0]
            .1
            .translation()
            .distance(plates[1].1.translation())
            < 1e-4
    );
    assert!(normal(&plates[0].1).dot(normal(&plates[1].1)) > 1.0 - 1e-4);
    let facing = camera.rotation * Vec3::Z;
    for (layer, g) in &plates {
        assert!(
            normal(g).dot(facing) > 1.0 - 1e-3,
            "plate {layer} faces the camera"
        );
    }

    // The client's weight track: peaks at 167 ms, gold gone by 500 ms.
    assert!(glyph_alpha(0, 0.167) > glyph_alpha(0, 0.4));
    assert!(glyph_alpha(1, 0.3) > 0.0);
    assert_eq!(glyph_alpha(1, 0.55), 0.0);
    assert!(
        glyph_alpha(0, 0.55) > 0.0,
        "the red-orange outlasts the gold"
    );

    h.tick_secs(MOTW_GLYPH_SECS + 0.1);
    assert!(
        h.rigs(DruidEffect::MarkOfTheWildGlyph).is_empty(),
        "the glyph retires"
    );
    let left = {
        let world = h.app.world_mut();
        let mut q = world.query::<&MarkOfTheWildPlate>();
        q.iter(world).count()
    };
    assert_eq!(left, 0, "and takes its plates");
}

/// Whether a texel adds anything to the frame under a plate's material. Bevy
/// premultiplies an `AlphaMode::Add` fragment by its alpha and adds it; a LIT
/// fragment carries the PBR specular (F0 = 0.04 for a dielectric, whatever
/// its base colour) and the ambient, so under a lit material any texel with
/// alpha adds light — black included.
fn texel_adds_light(material: &StandardMaterial, texel: [u8; 4]) -> bool {
    let has_alpha = texel[3] > 0;
    match material.alpha_mode {
        AlphaMode::Add if material.unlit => has_alpha && texel[..3].iter().any(|c| *c > 0),
        AlphaMode::Add | AlphaMode::Blend | AlphaMode::Premultiplied => has_alpha,
        AlphaMode::Mask(cutoff) => f32::from(texel[3]) / 255.0 >= cutoff,
        _ => true,
    }
}

/// The paw texture is the decoded `agility_128.blp` — DXT1 with no alpha, a
/// white paw on black — so every texel is opaque and the background is in the
/// colour. Under the plates' own material, the frame round the paw must add
/// nothing, or the plate draws as a lit square (AS-225).
#[test]
fn the_glyph_plates_add_nothing_outside_the_paw() {
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    use bevy::render::render_asset::RenderAssetUsages;

    let bytes = std::fs::read(std::path::Path::new("assets").join(MOTW_GLYPH_TEXTURE))
        .expect("the decoded agility_128 texture is committed");
    let paw = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .expect("the paw decodes");
    let (w, h) = (paw.width() as usize, paw.height() as usize);
    let data = paw.data.as_ref().expect("pixel data");
    assert_eq!(data.len(), w * h * 4, "RGBA8");
    let texel = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    // The frame: every texel within 8 of an edge. The paw's nearest stroke
    // is 11 texels in, so this is all background — and it is the part of the
    // quad that drew as a box.
    const FRAME: usize = 8;
    let frame: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| x.min(y).min(w - 1 - x).min(h - 1 - y) < FRAME)
        .collect();
    // The frame is opaque black: the shape is in the colour.
    assert!(frame.iter().all(|&(x, y)| texel(x, y) == [0, 0, 0, 255]));
    let paw_texels: Vec<[u8; 4]> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| texel(x, y))
        .filter(|t| t[..3].iter().any(|c| *c > 200))
        .collect();
    assert!(paw_texels.len() > 500, "the paw is there to draw");

    let mut harness = Harness::new();
    let ally = harness.bearer(Vec3::new(0.0, COMBATANT_Y, 0.0), vec![]);
    harness.app.world_mut().spawn(AuraLanding {
        target: ally,
        kind: AuraLandingKind::MarkOfTheWildGlyph,
    });
    harness.tick(3);
    let world = harness.app.world_mut();
    let handles: Vec<(usize, Handle<StandardMaterial>)> = {
        let mut q = world.query::<(&MarkOfTheWildPlate, &MeshMaterial3d<StandardMaterial>)>();
        q.iter(world).map(|(p, m)| (p.layer, m.0.clone())).collect()
    };
    assert_eq!(handles.len(), 2, "red-orange and gold");
    let materials = world.resource::<Assets<StandardMaterial>>();
    for (layer, handle) in handles {
        let material = materials.get(&handle).expect("plate material");
        let wears = material
            .base_color_texture
            .as_ref()
            .and_then(|t| t.path())
            .map(|p| p.path().to_path_buf());
        assert_eq!(
            wears.as_deref(),
            Some(std::path::Path::new(MOTW_GLYPH_TEXTURE)),
            "plate {layer} wears the paw"
        );
        let lit = frame
            .iter()
            .filter(|&&(x, y)| texel_adds_light(material, texel(x, y)))
            .count();
        assert_eq!(
            lit,
            0,
            "plate {layer}: {lit} of {} frame texels add light under {:?} (unlit: {})",
            frame.len(),
            material.alpha_mode,
            material.unlit
        );
        // ...while the paw itself still draws.
        assert!(
            paw_texels.iter().all(|t| texel_adds_light(material, *t)),
            "plate {layer} draws the paw"
        );
    }
}
