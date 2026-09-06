//! Probes for the Warlock DoT aura visuals
//! (`rendering/effects/warlock_dots.rs`) — Corruption's darkening shroud,
//! the Curse of Agony skull apparition, and Unstable Affliction's authored
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
    ActiveAuras, Aura, AuraType, CoaSkullFired, CoaSkullRig, Combatant, CorruptionShroudRig,
    DotApplyBurst, DotMote, DotMoteKind, DotSprite, DotSpriteRole, DotWisp, Pet, PetType,
    UaStateRig,
};
use arenasim::states::play_match::{
    age_warlock_dot_particles, animate_coa_skulls, animate_corruption_shrouds,
    animate_dot_apply_bursts, animate_ua_states, billboard_warlock_dot_visuals, coa_envelope,
    cleanup_warlock_dot_visuals, dot_anchor, shroud_alpha, spawn_warlock_dot_visuals,
    ua_crackle_k, yaw_coa_skulls, APPLY_BURST_LIFE, COA_APPARITION_SECS, COA_FADE_IN_SECS,
    COA_SUSTAIN_WHISPER, CORRUPTION_AURA, COA_AURA, PULSE_PERIOD, SHROUD_DARKNESS,
    SHROUD_RADIUS, UA_AURA, UA_CRACKLE_PERIOD, UA_CRACKLE_SECS, UA_PULSE_PERIOD,
};
use arenasim::states::play_match::{
    COMBATANT_BODY_RADIUS, IMPACT_HEAD_Y, IMPACT_PET_BODY_Y, IMPACT_PET_STATURE,
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
                animate_coa_skulls,
                animate_ua_states,
                age_warlock_dot_particles,
                yaw_coa_skulls,
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
fn the_coa_envelope_fades_in_holds_and_is_gone_by_the_blessed_time() {
    assert_eq!(coa_envelope(0.0), 0.0);
    assert!(coa_envelope(COA_FADE_IN_SECS * 0.5) > 0.0);
    assert_eq!(coa_envelope(1.0), 1.0);
    assert!(coa_envelope(2.6) < 1.0, "fading out by 2.6s");
    assert_eq!(coa_envelope(COA_APPARITION_SECS), 0.0);
    assert_eq!(coa_envelope(COA_APPARITION_SECS + 1.0), 0.0);
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
    assert!(!COA_SUSTAIN_WHISPER, "CoA is apply-only — era-faithful and deliberate");
}

// ── Curse of Agony ─────────────────────────────────────────────────────────

/// The skull materializes ABOVE the victim's head in world space, crackles
/// with sparks, sheds downward motes — and is completely gone by
/// `COA_APPARITION_SECS` while the curse itself runs on (apply-only).
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
    h.tick((COA_APPARITION_SECS / TICK_SECS).ceil() as u32 + 8);
    assert_eq!(h.count::<CoaSkullRig>(), 0, "the apparition self-expires");
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
    assert_eq!(h.count::<CoaSkullRig>(), 0, "no skull re-fire mid-curse");
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
    assert_eq!(h.count::<CoaSkullRig>(), 1);

    // Mass dispel.
    h.dispel(victim, CORRUPTION_AURA);
    h.dispel(victim, UA_AURA);
    h.dispel(victim, COA_AURA);
    h.tick(2);
    assert_eq!(h.count::<CorruptionShroudRig>(), 0, "shroud ends at dispel");
    assert_eq!(h.count::<UaStateRig>(), 0, "UA state ends at dispel");
    assert!(
        h.app.world().get::<CoaSkullFired>(victim).is_none(),
        "the skull latch re-arms when the curse is gone"
    );

    // The skull itself is an apply-moment record and may play out; wait it
    // out, then re-curse — a fresh application fires a fresh skull.
    h.tick((COA_APPARITION_SECS / TICK_SECS).ceil() as u32 + 4);
    assert_eq!(h.count::<CoaSkullRig>(), 0);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(victim)
        .expect("auras")
        .auras
        .push(Harness::dot_aura(COA_AURA));
    h.tick(2);
    assert_eq!(h.count::<CoaSkullRig>(), 1, "a fresh curse fires a fresh skull");
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
