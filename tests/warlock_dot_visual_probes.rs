//! Probes for the Warlock DoT and curse aura visuals
//! (`rendering/effects/warlock_dots.rs`) — Corruption's darkening shroud,
//! the three table-driven curse apply apparitions (Agony's and Weakness's
//! skulls, Tongues' chest rune circle), and Unstable Affliction's authored
//! violet crackle state.
//!
//! These assert WORLD-SPACE GEOMETRY under propagated `GlobalTransform`s,
//! not stored fields: that the skull materializes ABOVE the head, that the
//! shroud's rendered surface wraps OUTSIDE the 0.5-yd body capsule (the
//! AS-10 buried-inside-the-capsule lesson), that the stacked case shows UA's
//! crackle pop at a radius that clears the shroud, and that the pet-stature
//! correction is applied (the AS-14 boar lesson — sim-y alone floats pet
//! effects 0.73 yd high).
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform`
//! never propagates and every assertion below would read a child's LOCAL
//! pose. The harness registers the FULL graphical chain in the same order
//! `states/mod.rs` does (spawn → animates → particle aging → skull yaw →
//! billboard → cleanup), and probes whose subject the billboard pass places
//! (the crackle pop) spawn a real `Camera3d`.
//!
//! The lifecycle probes drive `ActiveAuras` directly: the detectors key
//! purely on aura PRESENCE, so a dispel and a natural expiry are one code
//! path — removing the aura from the component IS the dispel path.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, CorruptionShroudRig, CurseApparitionRig,
    CurseApparitionsFired, CurseKind, DotApplyBurst, DotMote, DotMoteKind, DotSprite,
    DotSpriteRole, DotWisp, Pet, PetType, UaStateRig,
};
use arenasim::states::play_match::{
    age_warlock_dot_particles, animate_corruption_shrouds, animate_curse_apparitions,
    animate_dot_apply_bursts, animate_ua_states, billboard_warlock_dot_visuals,
    cleanup_warlock_dot_visuals, curse_envelope, curse_spec, dot_anchor,
    orient_curse_apparitions, shroud_alpha, spawn_warlock_dot_visuals, ua_crackle_k,
    APPLY_BURST_LIFE, CORRUPTION_AURA, COA_AURA, CURSE_SUSTAIN_WHISPER, PULSE_PERIOD,
    SHROUD_DARKNESS, SHROUD_RADIUS, UA_AURA, UA_CRACKLE_PERIOD, UA_CRACKLE_SECS,
    UA_PULSE_PERIOD,
};
use arenasim::states::play_match::{
    COMBATANT_BODY_RADIUS, IMPACT_HEAD_Y, IMPACT_PET_BODY_Y, IMPACT_PET_STATURE, UA_CAMERA_LIFT,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;

/// The real game height: combatants stand with capsule centres at world
/// y = 1.0 over the floor plane at world y = 0. Spawning at y = 0 would
/// shift every world-height assertion down and let anchor defects pass.
const COMBATANT_Y: f32 = 1.0;
/// The real pet sim height (`play_match/mod.rs` spawns pets at 0.75; the
/// rendered pet body hangs at 0.3 via `VisualBody::rest_y`).
const PET_SIM_Y: f32 = 0.75;

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
        // The exact chained contract graphical mode registers
        // (`states/mod.rs`).
        app.add_systems(
            Update,
            (
                spawn_warlock_dot_visuals,
                animate_dot_apply_bursts,
                animate_corruption_shrouds,
                animate_curse_apparitions,
                animate_ua_states,
                age_warlock_dot_particles,
                orient_curse_apparitions,
                billboard_warlock_dot_visuals,
                cleanup_warlock_dot_visuals,
            )
                .chain(),
        );
        Harness { app }
    }

    fn spawn_camera(&mut self, from: Vec3, look_at: Vec3) {
        let transform = Transform::from_translation(from).looking_at(look_at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn dot_aura(name: &str) -> Aura {
        Aura {
            effect_type: AuraType::DamageOverTime,
            duration: 24.0,
            magnitude: 10.0,
            tick_interval: 3.0,
            time_until_next_tick: 3.0,
            ability_name: name.to_string(),
            ..Default::default()
        }
    }

    /// A living victim at `at` carrying the named DoT auras.
    fn spawn_victim(&mut self, at: Vec3, dots: &[&str]) -> Entity {
        let auras = ActiveAuras {
            auras: dots.iter().map(|n| Self::dot_aura(n)).collect(),
        };
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
                auras,
            ))
            .id()
    }

    /// An aura matching one curse's spec — the curses apply three DIFFERENT
    /// aura types (`DamageOverTime` / `DamageReduction` / `CastTimeIncrease`),
    /// and the detector keys on the type as well as the name.
    fn curse_aura(curse: CurseKind) -> Aura {
        let spec = curse_spec(curse);
        Aura {
            effect_type: spec.aura_type,
            duration: 30.0,
            magnitude: 0.5,
            tick_interval: 4.0,
            time_until_next_tick: 4.0,
            ability_name: spec.aura_name.to_string(),
            ..Default::default()
        }
    }

    /// A living victim at `at` carrying the named curses.
    fn spawn_cursed_victim(&mut self, at: Vec3, curses: &[CurseKind]) -> Entity {
        let auras = ActiveAuras {
            auras: curses.iter().map(|c| Self::curse_aura(*c)).collect(),
        };
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
                auras,
            ))
            .id()
    }

    /// Strip one curse from a victim (the dispel path).
    fn uncurse(&mut self, victim: Entity, curse: CurseKind) {
        let name = curse_spec(curse).aura_name;
        let mut auras = self
            .app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .expect("victim has auras");
        auras.auras.retain(|a| a.ability_name != name);
    }

    /// A pet victim (the Felhunter case): `Pet` component, real pet sim
    /// height.
    fn spawn_pet_victim(&mut self, at: Vec3, dots: &[&str]) -> Entity {
        let owner = self.spawn_victim(at + Vec3::X * 5.0, &[]);
        let auras = ActiveAuras {
            auras: dots.iter().map(|n| Self::dot_aura(n)).collect(),
        };
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Felhunter,
                },
                Transform::from_translation(at),
                auras,
            ))
            .id()
    }

    /// Strip the named DoT from a victim — the dispel path (the detectors
    /// key on aura presence, so dispel and expiry are one code path).
    fn dispel(&mut self, victim: Entity, name: &str) {
        let mut auras = self
            .app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .expect("victim has auras");
        auras.auras.retain(|a| a.ability_name != name);
    }

    fn sprites(&mut self) -> Vec<(DotSpriteRole, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&DotSprite, &GlobalTransform)>();
        q.iter(self.app.world()).map(|(s, g)| (s.role, *g)).collect()
    }

    fn sprite_entities(&mut self) -> Vec<(Entity, DotSpriteRole, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &DotSprite, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, sp, g)| (e, sp.role, *g))
            .collect()
    }

    fn sprite_handles(&mut self) -> Vec<(DotSpriteRole, Handle<StandardMaterial>)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&DotSprite, &MeshMaterial3d<StandardMaterial>)>();
        q.iter(self.app.world())
            .map(|(s, m)| (s.role, m.0.clone()))
            .collect()
    }

    fn motes(&mut self) -> Vec<(DotMoteKind, Vec3, Vec3)> {
        let mut q = self.app.world_mut().query::<(&DotMote, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(m, g)| (m.kind, m.velocity, g.translation()))
            .collect()
    }

    fn wisps(&mut self) -> Vec<(f32, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&DotWisp, &Transform, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(_, t, g)| (t.scale.x, g.translation()))
            .collect()
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&C>();
        q.iter(self.app.world()).count()
    }

    fn material_of(&self, handle: &Handle<StandardMaterial>) -> StandardMaterial {
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .expect("material exists")
            .clone()
    }
}

/// Fail loudly if a collection-conditional probe went vacuous.
fn assert_min(label: &str, actual: usize, min: usize) {
    assert!(
        actual >= min,
        "{label}: expected at least {min}, saw {actual} — the probe went vacuous"
    );
}

// ── pure timing/envelope checks ────────────────────────────────────────────

#[test]
fn every_curse_envelope_fades_in_holds_and_is_gone_by_its_blessed_time() {
    for curse in CurseKind::ALL {
        let spec = curse_spec(curse);
        assert_eq!(curse_envelope(curse, 0.0), 0.0, "{curse:?} starts invisible");
        assert!(
            curse_envelope(curse, spec.fade_in * 0.5) > 0.0,
            "{curse:?} is fading in half way through its fade-in"
        );
        let held = curse_envelope(curse, (spec.fade_in + spec.fade_out_start) * 0.5);
        assert!(
            (held - spec.peak_alpha).abs() < 1e-5,
            "{curse:?} holds at its peak alpha {}, got {held}",
            spec.peak_alpha
        );
        assert!(
            curse_envelope(curse, spec.fade_out_start + (spec.life - spec.fade_out_start) * 0.5)
                < spec.peak_alpha,
            "{curse:?} is fading out past {}s",
            spec.fade_out_start
        );
        assert_eq!(curse_envelope(curse, spec.life), 0.0, "{curse:?} is spent");
        assert_eq!(curse_envelope(curse, spec.life + 1.0), 0.0);
    }
}

#[test]
fn the_shroud_reblooms_on_its_period() {
    let just_bloomed = shroud_alpha(0.01);
    let mid_fade = shroud_alpha(PULSE_PERIOD * 0.5);
    let nearly_gone = shroud_alpha(PULSE_PERIOD * 0.99);
    let rebloomed = shroud_alpha(PULSE_PERIOD + 0.01);
    assert!(just_bloomed > mid_fade && mid_fade > nearly_gone);
    assert!(
        rebloomed > nearly_gone + 0.4,
        "the re-bloom must snap back up: {nearly_gone} -> {rebloomed}"
    );
    assert!(just_bloomed <= SHROUD_DARKNESS);
}

#[test]
fn the_crackle_fires_at_cycle_end_not_on_apply() {
    assert_eq!(ua_crackle_k(0.0), None, "no discharge on top of the apply burst");
    assert_eq!(ua_crackle_k(1.0), None);
    let start = UA_CRACKLE_PERIOD - UA_CRACKLE_SECS;
    assert!(ua_crackle_k(start + 0.01).is_some());
    assert!(ua_crackle_k(UA_CRACKLE_PERIOD - 0.001).is_some());
    assert_eq!(ua_crackle_k(UA_CRACKLE_PERIOD + 0.01), None);
}

/// The stacking design's period contrast is a constant, so a retune that
/// collapses the two rhythms into one fails here.
#[test]
fn the_two_sustained_pulses_run_on_different_periods() {
    assert!(
        (PULSE_PERIOD - UA_PULSE_PERIOD).abs() > 1.0,
        "Corruption ({PULSE_PERIOD}s) and UA ({UA_PULSE_PERIOD}s) must not share a rhythm"
    );
    assert!(
        !CURSE_SUSTAIN_WHISPER,
        "the curse apparitions are apply-only — era-faithful and deliberate"
    );
}

// ── Curse of Agony ─────────────────────────────────────────────────────────

/// The skull materializes ABOVE the victim's head in world space, crackles
/// with sparks, sheds downward motes — and is completely gone by
/// its spec's `life` while the curse itself runs on (apply-only).
#[test]
fn coa_skull_hangs_above_the_head_and_dissolves_while_the_curse_runs_on() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, COMBATANT_Y, -2.0);
    let victim = h.spawn_victim(at, &[COA_AURA]);
    h.spawn_camera(Vec3::new(0.0, 8.0, 20.0), at);
    h.tick(12); // ~0.19s: past fade-in, sparks flowing

    let head_world_y = at.y + IMPACT_HEAD_Y;
    let craniums: Vec<Vec3> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, g)| {
            matches!(role, DotSpriteRole::SkullCranium).then(|| g.translation())
        })
        .collect();
    assert_eq!(craniums.len(), 1, "one skull cranium");
    let cranium = craniums[0];
    assert!(
        cranium.y > head_world_y + 0.3,
        "the skull must hang ABOVE the head: cranium world y {} vs head {}",
        cranium.y,
        head_world_y
    );
    let horizontal = Vec2::new(cranium.x - at.x, cranium.z - at.z).length();
    assert!(horizontal < 0.2, "the skull sits over the head, off by {horizontal}");

    // Eye sockets and core exist (the face), sparks crackle, motes sink.
    let roles: Vec<DotSpriteRole> = h.sprites().into_iter().map(|(r, _)| r).collect();
    assert_eq!(
        roles.iter().filter(|r| matches!(r, DotSpriteRole::SkullEye)).count(),
        2,
        "two eye sockets"
    );
    assert_eq!(
        roles.iter().filter(|r| matches!(r, DotSpriteRole::SkullCore)).count(),
        1,
        "one yellow core glow"
    );
    let motes = h.motes();
    let sparks = motes
        .iter()
        .filter(|(k, ..)| *k == DotMoteKind::SkullSpark)
        .count();
    assert_min("skull sparks", sparks, 5);
    for (kind, velocity, _) in &motes {
        if *kind == DotMoteKind::SkullFall {
            assert!(velocity.y < 0.0, "glow motes sink, got {velocity}");
        }
    }

    // Past the apparition window: everything skull is gone — and the curse
    // aura is STILL on the victim (24s duration), which is the point.
    h.tick((curse_spec(CurseKind::Agony).life / TICK_SECS).ceil() as u32 + 8);
    assert_eq!(h.count::<CurseApparitionRig>(), 0, "the apparition self-expires");
    assert_eq!(
        h.sprites().len(),
        0,
        "no sustained CoA pieces — apply-only is the era-faithful design"
    );
    let still_cursed = h
        .app
        .world()
        .get::<ActiveAuras>(victim)
        .is_some_and(|a| a.auras.iter().any(|au| au.ability_name == COA_AURA));
    assert!(still_cursed, "the curse itself must still be running");

    // And the latch holds: no re-fire while the same application persists.
    h.tick(30);
    assert_eq!(h.count::<CurseApparitionRig>(), 0, "no skull re-fire mid-curse");
}

// ── Corruption ─────────────────────────────────────────────────────────────

/// The shroud's rendered surface wraps OUTSIDE the body capsule (radius
/// 0.5 — a shell at or under that radius is swallowed by the opaque body),
/// it DARKENS via the one deliberate `AlphaMode::Blend` exception, and it
/// re-blooms on `PULSE_PERIOD`.
#[test]
fn corruption_shroud_wraps_outside_the_body_and_reblooms() {
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, COMBATANT_Y, 4.0);
    let victim = h.spawn_victim(at, &[CORRUPTION_AURA]);
    h.tick(4);

    let shells: Vec<GlobalTransform> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, g)| matches!(role, DotSpriteRole::ShroudShell).then_some(g))
        .collect();
    assert_eq!(shells.len(), 1, "one shroud shell");
    let shell = shells[0];
    // The capsule mesh's equator point, through the propagated transform.
    let edge = shell.transform_point(Vec3::X * SHROUD_RADIUS);
    let reach = Vec2::new(edge.x - at.x, edge.z - at.z).length();
    assert!(
        reach > COMBATANT_BODY_RADIUS + 0.1,
        "the shroud's rendered surface reaches only {reach}yd from the spine — \
         buried inside the {COMBATANT_BODY_RADIUS}yd body capsule"
    );
    // Head/torso, not feet: the shell centre sits in the upper body.
    let centre = shell.translation();
    assert!(
        centre.y > at.y + 0.2 && centre.y < at.y + IMPACT_HEAD_Y,
        "shroud centred on head/torso, got world y {}",
        centre.y
    );

    // The Blend exception: darkening, not additive.
    let shell_material = h
        .sprite_handles()
        .into_iter()
        .find_map(|(role, m)| matches!(role, DotSpriteRole::ShroudShell).then_some(m))
        .expect("shell material");
    let material = h.material_of(&shell_material);
    assert_eq!(
        material.alpha_mode,
        AlphaMode::Blend,
        "the shroud DARKENS — additive cannot dim the victim (module docs)"
    );
    let alpha_early = material.base_color.alpha();
    assert!(
        alpha_early > SHROUD_DARKNESS * 0.8,
        "freshly bloomed shroud sits near SHROUD_DARKNESS, got {alpha_early}"
    );

    // The 3s throb: faded by late cycle, re-bloomed after the period.
    let frames_to = |secs: f32| (secs / TICK_SECS).ceil() as u32;
    h.tick(frames_to(PULSE_PERIOD * 0.9) - 4);
    let alpha_late = h.material_of(&shell_material).base_color.alpha();
    assert!(
        alpha_late < alpha_early * 0.4,
        "late in the cycle the shroud has faded: {alpha_early} -> {alpha_late}"
    );
    h.tick(frames_to(PULSE_PERIOD * 0.2));
    let alpha_rebloom = h.material_of(&shell_material).base_color.alpha();
    assert!(
        alpha_rebloom > alpha_late + 0.2,
        "the shroud re-blooms every {PULSE_PERIOD}s: {alpha_late} -> {alpha_rebloom}"
    );

    // Wisps swell, fizz rises.
    let wisps = h.wisps();
    assert_min("corruption wisps", wisps.len(), 5);
    let fizz: Vec<Vec3> = h
        .motes()
        .into_iter()
        .filter_map(|(k, v, _)| (k == DotMoteKind::Fizz).then_some(v))
        .collect();
    assert_min("fizz motes", fizz.len(), 10);
    for v in &fizz {
        assert!(v.y > 0.0, "fizz motes rise, got {v}");
    }

    // The state ends with the aura — no flourish, nothing left behind.
    h.dispel(victim, CORRUPTION_AURA);
    h.tick(2);
    assert_eq!(h.count::<CorruptionShroudRig>(), 0, "shroud despawns on dispel");
}

/// Wisps GROW over their life (the client's 0.22→0.69 swelling track) —
/// asserted on the rendered scale of one tracked wisp across frames.
#[test]
fn corruption_wisps_swell_as_they_age() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    h.spawn_victim(at, &[CORRUPTION_AURA]);
    h.tick(10); // ~0.16s at 20 wisps/s: a few young wisps to track

    let mut q = h
        .app
        .world_mut()
        .query::<(Entity, &DotWisp, &Transform)>();
    let tracked: Vec<(Entity, f32)> = q
        .iter(h.app.world())
        .map(|(e, _, t)| (e, t.scale.x))
        .collect();
    assert_min("tracked wisps", tracked.len(), 2);

    h.tick(30); // ~0.5s later — well inside the 2.5s life
    let mut grew = 0;
    for (entity, scale_before) in &tracked {
        if let Some(t) = h.app.world().get::<Transform>(*entity) {
            if t.scale.x > *scale_before {
                grew += 1;
            }
        }
    }
    assert_min("wisps that grew", grew, 1);
}

// ── the shared apply burst ─────────────────────────────────────────────────

/// Corruption AND Unstable Affliction fire the same kit-117 apply: an
/// expanding ring at the chest plus an outward spark sphere, self-expiring.
#[test]
fn the_apply_ring_expands_at_the_chest_for_both_corruption_and_ua() {
    for dot in [CORRUPTION_AURA, UA_AURA] {
        let mut h = Harness::new();
        let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
        h.spawn_victim(at, &[dot]);
        h.tick(3);

        assert_eq!(h.count::<DotApplyBurst>(), 1, "{dot}: one apply burst");
        let ring_scale = |h: &mut Harness| -> f32 {
            h.sprites()
                .into_iter()
                .find_map(|(role, g)| {
                    matches!(role, DotSpriteRole::ShadowRing).then(|| g.scale().x)
                })
                .expect("ring present")
        };
        let early = ring_scale(&mut h);
        h.tick(12);
        let later = ring_scale(&mut h);
        assert!(
            later > early * 1.5,
            "{dot}: the shadow ring must EXPAND: {early} -> {later}"
        );

        let sparks: Vec<Vec3> = h
            .motes()
            .into_iter()
            .filter_map(|(k, v, _)| (k == DotMoteKind::ApplySpark).then_some(v))
            .collect();
        assert_min("apply sparks", sparks.len(), 8);
        // Sparks blow outward in ALL directions (a sphere, not a fountain).
        assert!(
            sparks.iter().any(|v| v.y > 0.5) && sparks.iter().any(|v| v.y < -0.5),
            "{dot}: spark sphere must cover up and down"
        );

        // The one-shot retires itself even though the aura persists.
        h.tick((APPLY_BURST_LIFE / TICK_SECS).ceil() as u32 + 4);
        assert_eq!(h.count::<DotApplyBurst>(), 0, "{dot}: burst self-expires");
    }
}

// ── Unstable Affliction + stacking ─────────────────────────────────────────

/// THE stacking complaint that triggered the redesign: on a victim carrying
/// BOTH Corruption and UA, the crackle pop must read through the shroud —
/// its rendered radius clears the shroud's, so the flash rims out past the
/// darkening silhouette. Billboard-placed, so it runs under a real camera.
#[test]
fn stacked_ua_crackle_pop_reads_outside_corruptions_shroud() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, COMBATANT_Y, -1.0);
    h.spawn_victim(at, &[CORRUPTION_AURA, UA_AURA]);
    h.spawn_camera(Vec3::new(0.0, 9.0, 24.0), at);

    // Both states coexist.
    h.tick(4);
    assert_eq!(h.count::<CorruptionShroudRig>(), 1);
    assert_eq!(h.count::<UaStateRig>(), 1);

    // Advance into the first crackle discharge window.
    let discharge_at = UA_CRACKLE_PERIOD - UA_CRACKLE_SECS + 0.05;
    h.tick((discharge_at / TICK_SECS).ceil() as u32);

    let pops: Vec<GlobalTransform> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, g)| matches!(role, DotSpriteRole::CracklePop).then_some(g))
        .collect();
    assert_eq!(pops.len(), 1, "one crackle pop");
    let pop = pops[0];
    // The rendered edge of the unit quad, through scale and billboard pose.
    let edge = pop.transform_point(Vec3::new(0.5, 0.0, 0.0));
    let reach = (edge - pop.translation()).length();
    assert!(
        reach > SHROUD_RADIUS + 0.2,
        "the crackle pop reaches only {reach}yd — inside the {SHROUD_RADIUS}yd \
         shroud, it cannot read on a Corruption-stacked victim"
    );

    // The discharge's jagged bolts are out.
    let bolts = h
        .sprites()
        .into_iter()
        .filter(|(role, _)| matches!(role, DotSpriteRole::CrackleBolt))
        .count();
    assert_min("crackle bolt segments", bolts, 5);
}

/// Round-2: the billboard pass LIFTS the UA glow and crackle pop toward the
/// camera. Anchored at the torso centre, the quads lose the depth test to
/// the opaque body capsule and only the sprite's dim outer annulus reads —
/// the round-2 "blue speckles" finding; stacked, Corruption's Blend shroud
/// sorts in front and dims the flash besides. Asserted in WORLD SPACE off
/// the propagated `GlobalTransform`s, per the geometry-not-bookkeeping rule.
#[test]
fn ua_glow_and_pop_lift_toward_the_camera_past_body_and_shroud() {
    // The lift clears BOTH occluders — derived from the constants so a
    // retune keeps the band honest.
    assert!(
        UA_CAMERA_LIFT > COMBATANT_BODY_RADIUS + 0.05,
        "UA_CAMERA_LIFT ({UA_CAMERA_LIFT}) must clear the body capsule"
    );
    assert!(
        UA_CAMERA_LIFT > SHROUD_RADIUS,
        "UA_CAMERA_LIFT ({UA_CAMERA_LIFT}) must clear the stacked shroud shell"
    );

    let mut h = Harness::new();
    let at = Vec3::new(2.0, COMBATANT_Y, -1.0);
    h.spawn_victim(at, &[UA_AURA]);
    let cam_from = Vec3::new(0.0, 9.0, 24.0);
    h.spawn_camera(cam_from, at);
    h.tick(4);

    let rig_pos = {
        let mut q = h.app.world_mut().query::<(&UaStateRig, &Transform)>();
        q.iter(h.app.world()).next().expect("UA rig").1.translation
    };
    let to_cam = (cam_from - rig_pos).normalize();
    for role in [DotSpriteRole::UaGlow, DotSpriteRole::CracklePop] {
        let placed: Vec<Vec3> = h
            .sprites()
            .into_iter()
            .filter_map(|(r, g)| (r == role).then(|| g.translation()))
            .collect();
        assert_eq!(placed.len(), 1, "one {role:?}");
        let lift = placed[0] - rig_pos;
        assert!(
            lift.length() > COMBATANT_BODY_RADIUS,
            "{role:?} sits {} yd from its anchor — buried in the body, only \
             the sprite's dim annulus can read",
            lift.length()
        );
        assert!(
            lift.normalize().dot(to_cam) > 0.99,
            "{role:?} lift must point at the camera, not sideways: {lift:?}"
        );
    }
}

/// The dispel path, all three DoTs at once: state rigs die the frame their
/// aura is gone, the skull latch re-arms, and a fresh curse fires a fresh
/// skull.
#[test]
fn dispel_ends_the_states_and_rearms_the_skull() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let victim = h.spawn_victim(at, &[CORRUPTION_AURA, UA_AURA, COA_AURA]);
    h.tick(6);
    assert_eq!(h.count::<CorruptionShroudRig>(), 1);
    assert_eq!(h.count::<UaStateRig>(), 1);
    assert_eq!(h.count::<CurseApparitionRig>(), 1);

    // Mass dispel.
    h.dispel(victim, CORRUPTION_AURA);
    h.dispel(victim, UA_AURA);
    h.dispel(victim, COA_AURA);
    h.tick(2);
    assert_eq!(h.count::<CorruptionShroudRig>(), 0, "shroud ends at dispel");
    assert_eq!(h.count::<UaStateRig>(), 0, "UA state ends at dispel");
    assert!(
        h.app.world().get::<CurseApparitionsFired>(victim).is_none(),
        "the skull latch re-arms when the curse is gone"
    );

    // The skull itself is an apply-moment record and may play out; wait it
    // out, then re-curse — a fresh application fires a fresh skull.
    h.tick((curse_spec(CurseKind::Agony).life / TICK_SECS).ceil() as u32 + 4);
    assert_eq!(h.count::<CurseApparitionRig>(), 0);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(victim)
        .expect("auras")
        .auras
        .push(Harness::dot_aura(COA_AURA));
    h.tick(2);
    assert_eq!(h.count::<CurseApparitionRig>(), 1, "a fresh curse fires a fresh skull");
}

/// Death ends the states too: auras linger on corpses (`update_auras` skips
/// the dead — the fear-husk lesson), so dying must count as an exit path.
#[test]
fn death_ends_the_state_visuals() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA, UA_AURA]);
    h.tick(4);
    assert_eq!(h.count::<CorruptionShroudRig>(), 1);

    h.app
        .world_mut()
        .get_mut::<Combatant>(victim)
        .expect("victim")
        .current_health = 0.0;
    h.tick(2);
    assert_eq!(h.count::<CorruptionShroudRig>(), 0, "shroud dies with the victim");
    assert_eq!(h.count::<UaStateRig>(), 0, "UA state dies with the victim");
}

/// The state rigs follow a moving victim — a kited runner carries their
/// murk.
#[test]
fn the_state_rigs_follow_a_moving_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::new(0.0, COMBATANT_Y, 0.0), &[CORRUPTION_AURA]);
    h.tick(3);

    let rig_pos = |h: &mut Harness| -> Vec3 {
        let mut q = h
            .app
            .world_mut()
            .query::<(&CorruptionShroudRig, &GlobalTransform)>();
        q.iter(h.app.world()).next().expect("rig").1.translation()
    };
    let before = rig_pos(&mut h);
    let moved = Vec3::new(6.0, COMBATANT_Y, 3.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = moved;
    h.tick(2);
    let after = rig_pos(&mut h);
    assert!(
        (after - before).length() > 5.0,
        "the shroud must follow: {before} -> {after}"
    );
}

// ── pets (the Felhunter case) ──────────────────────────────────────────────

/// A DoT on a pet rides the pet-stature correction: the skull hangs above
/// the pet's RENDERED head, not 0.73 yd up in the air over sim-y (the AS-14
/// boar lesson), and the shroud centres on the pet's body.
#[test]
fn pet_dots_use_the_pet_stature_correction() {
    let mut h = Harness::new();
    let at = Vec3::new(4.0, PET_SIM_Y, 4.0);
    h.spawn_pet_victim(at, &[COA_AURA, CORRUPTION_AURA]);
    h.tick(6);

    // The expected pet head height, from RAW constants — deliberately NOT
    // via `dot_anchor`, so a defect in the shared helper breaks the rendered
    // pose and not the expectation with it (the geometry-not-bookkeeping
    // rule). The `school_impact.rs` correction: body centre at
    // sim y + IMPACT_PET_BODY_Y, heights scaled by IMPACT_PET_STATURE.
    let pet_head_y = at.y + IMPACT_PET_BODY_Y + IMPACT_HEAD_Y * IMPACT_PET_STATURE;
    assert!(
        (pet_head_y - 0.8775).abs() < 1e-3,
        "constant drift — update this probe's derivation: {pet_head_y}"
    );
    let craniums: Vec<Vec3> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, g)| {
            matches!(role, DotSpriteRole::SkullCranium).then(|| g.translation())
        })
        .collect();
    assert_eq!(craniums.len(), 1);
    let cranium = craniums[0];
    assert!(
        cranium.y > pet_head_y,
        "the skull hangs above the pet's rendered head: {} vs {}",
        cranium.y,
        pet_head_y
    );
    // A tight band above the PET's head: an uncorrected sim-y anchor parks
    // the cranium ~2.15 (combatant proportions over a pet — the boar
    // lesson's 0.73 yd float), well past this bound.
    assert!(
        cranium.y < pet_head_y + 0.7,
        "the skull floats too high over the pet ({} vs head {}) — an \
         uncorrected sim-y anchor, the boar lesson",
        cranium.y,
        pet_head_y
    );
    // Sanity: the helper agrees with the raw derivation on the good path.
    assert!((dot_anchor(IMPACT_HEAD_Y, at, true).y - pet_head_y).abs() < 1e-3);

    // The shroud centres on the pet's rendered body, below the sim centre.
    let shells: Vec<Vec3> = h
        .sprites()
        .into_iter()
        .filter_map(|(role, g)| {
            matches!(role, DotSpriteRole::ShroudShell).then(|| g.translation())
        })
        .collect();
    assert_eq!(shells.len(), 1);
    assert!(
        shells[0].y < at.y,
        "the pet shroud centres on the rendered body (below sim y {}), got {}",
        at.y,
        shells[0].y
    );
}

// ── Curse of Weakness / Curse of Tongues (AS-19) ───────────────────────────

/// Curse of Weakness's apparition is a skull WITH A BONE above the victim's
/// head, on Curse of Agony's shared envelope — and it sheds green sparks and
/// swelling violet blooms but NO downward glow motes (client kit 719 has no
/// downward emitter). World-space geometry throughout.
#[test]
fn cow_apparition_hangs_above_the_head_with_its_bone() {
    let mut h = Harness::new();
    let at = Vec3::new(-3.0, COMBATANT_Y, 1.0);
    let victim = h.spawn_cursed_victim(at, &[CurseKind::Weakness]);
    h.spawn_camera(Vec3::new(0.0, 8.0, 20.0), at);
    h.tick(12); // ~0.19s: past the 134ms fade-in, emitters flowing

    let head_world_y = at.y + IMPACT_HEAD_Y;
    let placed = |h: &mut Harness, role: DotSpriteRole| -> Vec<Vec3> {
        h.sprites()
            .into_iter()
            .filter_map(|(r, g)| (r == role).then(|| g.translation()))
            .collect()
    };
    let craniums = placed(&mut h, DotSpriteRole::SkullCranium);
    assert_eq!(craniums.len(), 1, "one skull cranium");
    assert!(
        craniums[0].y > head_world_y + 0.3,
        "the skull must hang ABOVE the head: cranium world y {} vs head {}",
        craniums[0].y,
        head_world_y
    );

    // The bone — the silhouette that tells CoW from CoA. It sits BESIDE the
    // cranium (a real horizontal separation in world space), not inside it.
    let bones = placed(&mut h, DotSpriteRole::SkullBone);
    assert_eq!(bones.len(), 1, "one bone beside the skull");
    let apart = Vec2::new(bones[0].x - craniums[0].x, bones[0].z - craniums[0].z).length();
    assert!(
        apart > 0.2,
        "the bone is buried inside the cranium — only {apart}yd from its centre"
    );
    assert!(
        (bones[0].y - craniums[0].y).abs() < 0.3,
        "the bone rides alongside the skull, not above or below it"
    );

    // Green sparks and swelling violet blooms; NO downward glow motes.
    let motes = h.motes();
    let sparks = motes
        .iter()
        .filter(|(k, ..)| *k == DotMoteKind::SkullSpark)
        .count();
    assert_min("CoW sparks", sparks, 5);
    assert_eq!(
        motes
            .iter()
            .filter(|(k, ..)| *k == DotMoteKind::SkullFall)
            .count(),
        0,
        "client kit 719 has no downward emitter — CoW sheds no falling motes"
    );
    assert_min("CoW blooms", h.wisps().len(), 3);

    // Apply-only: gone by the spec's life while the curse itself runs on.
    h.tick((curse_spec(CurseKind::Weakness).life / TICK_SECS).ceil() as u32 + 8);
    assert_eq!(h.count::<CurseApparitionRig>(), 0, "the apparition self-expires");
    assert_eq!(h.sprites().len(), 0, "no sustained CoW pieces");
    let still_cursed = h
        .app
        .world()
        .get::<ActiveAuras>(victim)
        .is_some_and(|a| a.auras.iter().any(|au| au.effect_type == AuraType::DamageReduction));
    assert!(still_cursed, "the curse itself must still be running");
    h.tick(30);
    assert_eq!(h.count::<CurseApparitionRig>(), 0, "no re-fire mid-curse");
}

/// Curse of Tongues' apparition is a rune CIRCLE at the CHEST — discs and
/// upright tablets ringing the body OUTSIDE the 0.5yd capsule, each tablet
/// turned to face outward, and the whole ring turning on its own axis. The
/// tablet facing is read off the propagated `GlobalTransform`'s world axes,
/// not a stored field.
#[test]
fn cot_rune_circle_rings_the_chest_facing_outward_and_turns() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, COMBATANT_Y, -4.0);
    h.spawn_cursed_victim(at, &[CurseKind::Tongues]);
    h.spawn_camera(Vec3::new(0.0, 8.0, 20.0), at);
    h.tick(25); // ~0.4s: past the 333ms fade-in, well before the 1.67s end

    let sprites = h.sprite_entities();
    let discs: Vec<&(Entity, DotSpriteRole, GlobalTransform)> = sprites
        .iter()
        .filter(|(_, r, _)| *r == DotSpriteRole::RuneDisc)
        .collect();
    let tablets: Vec<&(Entity, DotSpriteRole, GlobalTransform)> = sprites
        .iter()
        .filter(|(_, r, _)| *r == DotSpriteRole::RuneTablet)
        .collect();
    assert_eq!(discs.len(), 2, "two flat rune discs");
    assert_eq!(tablets.len(), 5, "five upright glyph tablets");
    assert!(
        sprites
            .iter()
            .all(|(_, r, _)| matches!(r, DotSpriteRole::RuneDisc | DotSpriteRole::RuneTablet)),
        "the rune circle builds no skull pieces"
    );

    // Chest, not head: every piece sits below the head anchor.
    let head_world_y = at.y + IMPACT_HEAD_Y;
    for (_, role, g) in &sprites {
        assert!(
            g.translation().y < head_world_y,
            "{role:?} sits at world y {} — at or above the head anchor {head_world_y}, \
             but Curse of Tongues attaches at the CHEST",
            g.translation().y
        );
    }

    // The discs' rendered rims clear the body capsule (the AS-10
    // buried-inside-the-capsule lesson).
    for (_, _, g) in &discs {
        let rim = g.transform_point(Vec3::X);
        let reach = Vec2::new(rim.x - at.x, rim.z - at.z).length();
        assert!(
            reach > COMBATANT_BODY_RADIUS + 0.1,
            "a rune disc reaches only {reach}yd from the spine — swallowed by \
             the {COMBATANT_BODY_RADIUS}yd body capsule"
        );
    }

    // Each tablet stands outside the body with its face pointing OUTWARD:
    // the world-space quad normal (local +Z through the propagated
    // transform) aligns with the radial direction from the victim's spine.
    for (_, _, g) in &tablets {
        let offset = g.translation() - at;
        let outward = Vec3::new(offset.x, 0.0, offset.z);
        assert!(
            outward.length() > COMBATANT_BODY_RADIUS,
            "a tablet sits {}yd from the spine — inside the body",
            outward.length()
        );
        let normal = g.affine().transform_vector3(Vec3::Z).normalize();
        assert!(
            normal.dot(outward.normalize()).abs() > 0.9,
            "a tablet's face points {normal:?}, not radially outward ({:?}) — \
             the ring reads edge-on",
            outward.normalize()
        );
    }

    // The ring TURNS: one tracked tablet's world azimuth about the victim
    // moves over a quarter of the 2s spin period.
    let tracked = tablets[0].0;
    let azimuth = |h: &mut Harness| -> f32 {
        let g = *h
            .app
            .world()
            .get::<GlobalTransform>(tracked)
            .expect("tracked tablet");
        let d = g.translation() - at;
        d.z.atan2(d.x)
    };
    let before = azimuth(&mut h);
    h.tick((0.5 / TICK_SECS).ceil() as u32);
    let after = azimuth(&mut h);
    let delta = (after - before).abs();
    let turned = delta.min(std::f32::consts::TAU - delta);
    assert!(
        turned > 0.8,
        "the rune ring must turn — the tracked tablet moved only {turned} rad in 0.5s"
    );

    // Apply-only, and short: gone by 1.67s.
    h.tick((curse_spec(CurseKind::Tongues).life / TICK_SECS).ceil() as u32 + 4);
    assert_eq!(h.count::<CurseApparitionRig>(), 0, "the rune circle self-expires");
    assert_eq!(h.sprites().len(), 0, "no sustained CoT pieces");
}

/// The card's distinctness check, pinned: the three curse apparitions must
/// read apart at a glance. The client differentiates them by ATTACH,
/// SILHOUETTE and PALETTE — this asserts all three axes on the rendered
/// rigs, so a retune that collapses two curses into one look fails here.
#[test]
fn distinct_curse_apparitions_read_apart() {
    /// The rendered colour's chroma — hue and saturation with brightness
    /// divided out, so a dim/bright retune does not move the reading.
    fn chroma(color: Color) -> Vec3 {
        let c = color.to_linear();
        let v = Vec3::new(c.red, c.green, c.blue);
        v / v.max_element().max(1e-4)
    }

    struct Read {
        curse: CurseKind,
        /// World height of the topmost rendered piece — head vs chest.
        top_y: f32,
        roles: Vec<String>,
        shell: Vec3,
        core: Vec3,
    }

    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let reads: Vec<Read> = CurseKind::ALL
        .iter()
        .map(|&curse| {
            let mut h = Harness::new();
            h.spawn_cursed_victim(at, &[curse]);
            h.spawn_camera(Vec3::new(0.0, 8.0, 20.0), at);
            h.tick(30);

            let sprites = h.sprites();
            assert!(!sprites.is_empty(), "{curse:?} rendered nothing");
            let top_y = sprites
                .iter()
                .map(|(_, g)| g.translation().y)
                .fold(f32::MIN, f32::max);
            let mut roles: Vec<String> =
                sprites.iter().map(|(r, _)| format!("{r:?}")).collect();
            roles.sort();
            roles.dedup();

            // The apparition's two colour channels: its shell/body and its
            // accent. Skulls carry cranium + core; the rune circle carries
            // disc + tablet.
            let handles = h.sprite_handles();
            let pick = |role: DotSpriteRole| -> Handle<StandardMaterial> {
                handles
                    .iter()
                    .find_map(|(r, m)| (*r == role).then(|| m.clone()))
                    .unwrap_or_else(|| panic!("{curse:?} has no {role:?}"))
            };
            let (shell_role, core_role) = match curse {
                CurseKind::Tongues => (DotSpriteRole::RuneDisc, DotSpriteRole::RuneTablet),
                _ => (DotSpriteRole::SkullCranium, DotSpriteRole::SkullCore),
            };
            let shell = chroma(h.material_of(&pick(shell_role)).base_color);
            let core = chroma(h.material_of(&pick(core_role)).base_color);
            Read { curse, top_y, roles, shell, core }
        })
        .collect();

    // ATTACH: the two skulls hang at the head, the rune circle rides the
    // chest — a real world-height separation, not a stored anchor field.
    let head_top = reads
        .iter()
        .filter(|r| r.curse != CurseKind::Tongues)
        .map(|r| r.top_y)
        .fold(f32::MAX, f32::min);
    let rune_top = reads
        .iter()
        .find(|r| r.curse == CurseKind::Tongues)
        .expect("Tongues read")
        .top_y;
    assert!(
        head_top > rune_top + 0.5,
        "Curse of Tongues must ride LOWER than the head apparitions: \
         rune top {rune_top} vs skull top {head_top}"
    );

    for (i, a) in reads.iter().enumerate() {
        for b in reads.iter().skip(i + 1) {
            // SILHOUETTE: the role sets differ.
            assert_ne!(
                a.roles, b.roles,
                "{:?} and {:?} build the same pieces — no silhouette contrast",
                a.curse, b.curse
            );
            // PALETTE: shell and accent chroma, combined.
            let separation =
                Vec2::new((a.shell - b.shell).length(), (a.core - b.core).length()).length();
            assert!(
                separation > 0.6,
                "{:?} and {:?} sit {separation} apart in chroma — too close to \
                 read apart at a glance",
                a.curse,
                b.curse
            );
        }
    }
}

/// Three curses on one victim coexist, and each latch bit re-arms on its own:
/// dispelling one curse must not re-fire the others' apparitions.
#[test]
fn each_curse_latch_bit_rearms_independently() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let victim = h.spawn_cursed_victim(at, &CurseKind::ALL);
    h.spawn_camera(Vec3::new(0.0, 8.0, 20.0), at);
    h.tick(4);
    assert_eq!(
        h.count::<CurseApparitionRig>(),
        3,
        "all three curses fire their own apparition"
    );
    let latch = |h: &Harness| -> u8 {
        h.app
            .world()
            .get::<CurseApparitionsFired>(victim)
            .map(|l| l.fired)
            .unwrap_or(0)
    };
    assert_eq!(latch(&h), 0b111, "every bit latched");

    // Drop ONE curse: its bit clears, the others hold.
    h.uncurse(victim, CurseKind::Weakness);
    h.tick(2);
    assert_eq!(
        latch(&h),
        CurseKind::Agony.bit() | CurseKind::Tongues.bit(),
        "only the dispelled curse's bit clears"
    );

    // Let every apparition play out, then re-curse just the dispelled one —
    // exactly one fresh apparition, not three.
    let longest = CurseKind::ALL
        .iter()
        .map(|c| curse_spec(*c).life)
        .fold(0.0_f32, f32::max);
    h.tick((longest / TICK_SECS).ceil() as u32 + 8);
    assert_eq!(h.count::<CurseApparitionRig>(), 0);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(victim)
        .expect("auras")
        .auras
        .push(Harness::curse_aura(CurseKind::Weakness));
    h.tick(2);
    assert_eq!(
        h.count::<CurseApparitionRig>(),
        1,
        "a re-applied curse fires exactly one fresh apparition"
    );
}
