use std::collections::HashMap;

use bevy::prelude::*;

use super::hunter_shots::{
    draw, particle_rotation, scale_all, spawn_client_particle, ClientEmitter, EmitterAssets,
    EmitterSprite,
};
use super::school_impact::impact_origin;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ==============================================================================
// Weapon poison landing — the green cloud every Rogue poison lands with
// ==============================================================================
//
// From the Classic client data, joined in
// `docs/design/2026-10-17-baseline-kit-client-data.md` §2 (AS-333): every
// proc spell of Deadly, Wound, Mind-numbing and Crippling Poison carries ONE
// visual, whose only drawn event is the impact kit 3031,
// `poison_impact_chest.m2` at the victim's chest. Era and TBC agree on every
// rate, life, size and colour. The client draws no persistent "poisoned"
// state for any of them, and nothing per Deadly Poison tick.
//
// Three emitters, transcribed below:
//
//   burst  -> about 25 green flares in the first 167 ms, out at 3.33 yd/s,
//             dying by 0.6 s;
//   puffs  -> olive cloud puffs at 20/s for 3.0 s, drifting at 0.28 yd/s
//             (±50%) and swelling 0.22 -> 0.69 yd over a 2.5 s life;
//   motes  -> additive pin-point motes on the puffs' timing and colours.
//
// Rulings (AS-333, user 2026-10-17), as the signed-off bench plays them
// (AS-333 Baseline Kit Visuals Bench, version 1792202136-e9a0, "Poison on
// hit", at its defaults):
//
// - one shared green for every poison — no per-poison tint;
// - the landing plays on EVERY application, as in the client (each proc is
//   the proc spell landing, and its only visual is this impact);
// - a new application RESTARTS the cloud already playing on that victim
//   rather than stacking a second one. At most one cloud per victim: its
//   emitters' shared timeline goes back to zero and the particles already
//   alive finish their lives, so a Rogue procing every swing keeps one steady
//   cloud and never a thickening pile.
//
// This replaces Crippling Poison's old stand-in (the Nature school impact on
// its first application only), and widens Crippling to every application as
// well. Crippling's slow ring at the feet (`slow_ring.rs`) is unchanged.
// Wound Poison's healing reduction draws Mortal Wounds' ash on each heal for
// free (`mortal_wounds.rs` keys on `HealingReduction`); nothing here adds to
// it.
//
// **How an application is seen.** Graphical-only, from the victim's
// `ActiveAuras`, in FixedUpdate after the sim's last phase — one observation
// per sim tick. Every application path ends with the poison's aura at its
// FULL duration: a fresh one (`apply_pending_auras`, or Crippling pushed
// directly onto the victim), a stacking refresh (`refresh_stacking_aura`) and
// Crippling's refresh in place all write it, and `update_auras` runs every
// tick down. So between two observations an aura's duration either FELL (no
// application) or ROSE (one landed), and a key that was not there before is a
// fresh landing. Two applications on consecutive ticks read as one — a
// restart one tick (1/60 s) apart, which no one can see. The detector is
// gated on the sim running (`combat_sim_runs`), so a frozen match never reads
// as a refresh.
//
// No `game_rng` draw (scatter is the shared deterministic hash), no sim write
// — the only entities it spawns are its own rigs and particles — and every
// system is registered in `states/mod.rs` only, so headless stays
// byte-identical by construction.

// ── The bench's knobs (signed off 2026-10-17, version 1792202136-e9a0) ──────

/// Multipliers over the client's sizes, speeds and timings (1.0 = client), as
/// `client_landings.rs` takes them. The bench's size, spread, duration and
/// density knobs all shipped at 1.0, and all three of its layers kept: the
/// flare burst, the cloud puffs and the pin-point motes.
pub const POISON_LANDING_SIZE: f32 = 1.0;
pub const POISON_LANDING_SPREAD: f32 = 1.0;
pub const POISON_LANDING_TIME: f32 = 1.0;

/// The bench's `POISON_LANDING_EMIT_SECS`: how long the cloud emits after the
/// latest application (seq 0, Stand, is 3000 ms).
pub const POISON_LANDING_EMIT_SECS: f32 = 3.0 * POISON_LANDING_TIME;
/// The bench's `POISON_LANDING_PUFF_LIFE_SECS`.
pub const POISON_LANDING_PUFF_LIFE_SECS: f32 = 2.5 * POISON_LANDING_TIME;

/// The puffs' and motes' launch speed varies ±50% about 0.28 yd/s (the M2's
/// speed variation); the burst's does not.
const SPEED_VARIANCE: [f32; 3] = [0.0, 0.5, 0.5];

/// Keeps float drift from dropping the last particle of an exact count.
const EMIT_EPSILON: f32 = 1e-4;

// ── The table ──────────────────────────────────────────────────────────────

const PUFF_COLOR: [[u8; 3]; 3] = [[18, 55, 0], [106, 146, 0], [163, 196, 64]];
const PUFF_WINDOW: (f32, f32) = (0.0, 3.0);

/// `poison_impact_chest.m2` (kit 3031, fdid 166643) at the CHEST.
const POISON_IMPACT_CHEST: [ClientEmitter; 3] = [
    // P1: the burst — 150/s for 167 ms (about 25 flares), alpha-blended.
    ClientEmitter {
        name: "poison_impact_chest/p1 flare burst",
        sprite: EmitterSprite::Flare,
        additive: false,
        rate: 150.0,
        window: Some((0.0, 0.167)),
        life: 0.6,
        speed: 3.33,
        gravity: 0.0,
        area: 0.28,
        column: None,
        rise: false,
        color: [[49, 102, 1], [99, 136, 20], [115, 152, 1]],
        alpha: [1.0, 1.0, 0.0],
        size: [0.11, 0.06, 0.03],
        mid: 0.5,
        track: None,
    },
    // P0: the cloud puffs (`clouds8x8fade`), alpha-blended, born in a 0.28 yd
    // radius (a 0.56 yd cube).
    ClientEmitter {
        name: "poison_impact_chest/p0 cloud puffs",
        sprite: EmitterSprite::Glow,
        additive: false,
        rate: 20.0,
        window: Some(PUFF_WINDOW),
        life: 2.5,
        speed: 0.28,
        gravity: 0.0,
        area: 0.56,
        column: None,
        rise: false,
        color: PUFF_COLOR,
        alpha: [0.39, 1.0, 0.0],
        size: [0.22, 0.31, 0.69],
        mid: 0.5,
        track: None,
    },
    // P2: pin-point motes (`flare`, blend 4) on the puffs' timing and colours,
    // at a constant 0.056 yd.
    ClientEmitter {
        name: "poison_impact_chest/p2 pin-point motes",
        sprite: EmitterSprite::Flare,
        additive: true,
        rate: 20.0,
        window: Some(PUFF_WINDOW),
        life: 2.5,
        speed: 0.28,
        gravity: 0.0,
        area: 0.56,
        column: None,
        rise: false,
        color: PUFF_COLOR,
        alpha: [0.39, 1.0, 0.0],
        size: [0.056, 0.056, 0.056],
        mid: 0.5,
        track: None,
    },
];

/// What the landing actually plays: the table at the bench's knobs.
static POISON_LANDING: [ClientEmitter; 3] = scale_all(
    POISON_IMPACT_CHEST,
    POISON_LANDING_SIZE,
    POISON_LANDING_SPREAD,
    POISON_LANDING_TIME,
);

/// The emitters one landing plays: burst, puffs, motes.
pub fn poison_landing_emitters() -> &'static [ClientEmitter; 3] {
    &POISON_LANDING
}

/// Seconds from the latest application until the last particle it emitted
/// has died: the latest window close plus that emitter's life.
pub fn poison_landing_secs() -> f32 {
    POISON_LANDING
        .iter()
        .map(|e| e.end() + e.life)
        .fold(0.0, f32::max)
}

// ── Routing ────────────────────────────────────────────────────────────────

/// Every weapon poison, with the aura it lands on its victim: the type and
/// the RON `name:` the aura carries. The single list the detector, the
/// lands-silently audit and the probes derive from.
pub const WEAPON_POISON_AURAS: [(AbilityType, AuraType, &str); 4] = [
    (
        AbilityType::CripplingPoison,
        AuraType::MovementSpeedSlow,
        "Crippling Poison",
    ),
    (
        AbilityType::DeadlyPoison,
        AuraType::DamageOverTime,
        "Deadly Poison",
    ),
    (
        AbilityType::WoundPoison,
        AuraType::HealingReduction,
        "Wound Poison",
    ),
    (
        AbilityType::MindNumbingPoison,
        AuraType::CastTimeIncrease,
        "Mind-numbing Poison",
    ),
];

/// Whether `ability` lands the weapon poison cloud on its victim.
pub fn lands_poison_cloud(ability: AbilityType) -> bool {
    WEAPON_POISON_AURAS.iter().any(|(a, _, _)| *a == ability)
}

/// Whether `aura` is a weapon poison's victim aura — matched on type AND name,
/// so the Rogue's own weapon coating (a `WeaponPoison` aura of the same name)
/// never reads as a landing.
pub fn is_poison_landing_aura(aura: &Aura) -> bool {
    WEAPON_POISON_AURAS
        .iter()
        .any(|(_, t, name)| aura.effect_type == *t && aura.ability_name == *name)
}

/// Per-bearer memory of the poison auras seen at the last observation:
/// `(name, caster, duration)`. Renderer-local state (a `Local` of the
/// detector), never read by the sim.
#[derive(Default)]
pub struct PoisonLandingLedger {
    seen: HashMap<Entity, Vec<(String, Option<Entity>, f32)>>,
}

impl PoisonLandingLedger {
    /// Observe every bearer's auras; return the bearers a poison LANDED on
    /// since the last observation — a key not seen before, or a duration that
    /// rose instead of running down. Bearers absent from `bearers` are
    /// forgotten.
    pub fn observe<'a>(
        &mut self,
        bearers: impl IntoIterator<Item = (Entity, &'a [Aura])>,
    ) -> Vec<Entity> {
        let mut next = HashMap::new();
        let mut landed = Vec::new();
        for (bearer, auras) in bearers {
            let before = self.seen.get(&bearer);
            let mut now = Vec::new();
            let mut hit = false;
            for aura in auras.iter().filter(|a| is_poison_landing_aura(a)) {
                let prev = before.and_then(|b| {
                    b.iter().find(|(name, caster, _)| {
                        *name == aura.ability_name && *caster == aura.caster
                    })
                });
                if prev.is_none_or(|(_, _, duration)| aura.duration > *duration) {
                    hit = true;
                }
                now.push((aura.ability_name.clone(), aura.caster, aura.duration));
            }
            if hit {
                landed.push(bearer);
            }
            if !now.is_empty() {
                next.insert(bearer, now);
            }
        }
        self.seen = next;
        landed
    }
}

// ── Systems ────────────────────────────────────────────────────────────────

/// FixedUpdate (graphical-only), after the sim's last phase: land the cloud on
/// every bearer a poison landed on this tick — restarting that bearer's cloud
/// if one is already playing, spawning it otherwise.
pub fn detect_poison_landings(
    mut commands: Commands,
    mut ledger: Local<PoisonLandingLedger>,
    bearers: Query<(Entity, &ActiveAuras)>,
    mut clouds: Query<&mut PoisonCloud>,
) {
    let landed = ledger.observe(bearers.iter().map(|(e, a)| (e, a.auras.as_slice())));
    for victim in landed {
        if let Some(mut cloud) = clouds.iter_mut().find(|c| c.victim == victim) {
            // Restart the timeline; the live particles finish their lives.
            cloud.age = 0.0;
            cloud.carry = [0.0; 3];
        } else {
            commands.spawn((
                PoisonCloud {
                    victim,
                    age: 0.0,
                    carry: [0.0; 3],
                    emitted: 0,
                },
                Transform::default(),
                Visibility::default(),
                PlayMatchEntity,
            ));
        }
    }
}

/// `e` with its launch speed jittered by `variance` (±fraction) for `seed`.
fn with_speed_jitter(e: &ClientEmitter, variance: f32, seed: u32) -> ClientEmitter {
    let mut e = *e;
    e.speed *= 1.0 + variance * (2.0 * draw(seed, 11) - 1.0);
    e
}

/// Follow each cloud's victim, emit what its timeline owes this frame, retire
/// its spent particles, and despawn it once everything it emitted has died.
///
/// The rig outlives any one landing while procs keep restarting it, so a
/// spent particle is despawned here rather than left collapsed under it.
pub fn update_poison_clouds(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<EmitterAssets>>,
    mut clouds: Query<(Entity, &mut PoisonCloud, &mut Transform, Option<&Children>)>,
    victims: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<PoisonCloud>)>,
    particles: Query<&ClientParticle>,
) {
    let dt = time.delta_secs();
    let end = poison_landing_secs();
    for (entity, mut cloud, mut transform, children) in clouds.iter_mut() {
        if cloud.age >= end {
            commands.entity(entity).despawn();
            continue;
        }
        // Attached: ride a victim that is still there; otherwise finish where
        // it was.
        if let Ok((victim, pet)) = victims.get(cloud.victim) {
            transform.translation =
                impact_origin(ImpactAnchor::Chest, victim.translation, pet.is_some());
        }
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if particles.get(child).is_ok_and(|p| p.age >= p.life) {
                commands.entity(child).despawn();
            }
        }

        let (from, to) = (cloud.age, cloud.age + dt);
        cloud.age = to;
        let assets = assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        let quad = assets.quad();
        for (i, emitter) in POISON_LANDING.iter().enumerate() {
            cloud.carry[i] += emitter.count_between(from, to);
            if cloud.carry[i] < 1.0 - EMIT_EPSILON {
                continue;
            }
            let palette = assets.palette(&mut materials, emitter);
            while cloud.carry[i] >= 1.0 - EMIT_EPSILON {
                cloud.carry[i] -= 1.0;
                let seed = entity
                    .index()
                    .wrapping_add(cloud.emitted.wrapping_mul(0x85EB_CA6B));
                cloud.emitted = cloud.emitted.wrapping_add(1);
                let launched = with_speed_jitter(emitter, SPEED_VARIANCE[i], seed);
                let particle = spawn_client_particle(
                    &mut commands,
                    &quad,
                    &launched,
                    &palette,
                    Vec3::ZERO,
                    seed,
                    true,
                );
                commands.entity(entity).add_child(particle);
            }
        }
    }
}

/// Turn every cloud particle to face the camera.
pub fn billboard_poison_clouds(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<PoisonCloud>,
            Without<ClientParticle>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<PoisonCloud>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (Without<Camera3d>, Without<PoisonCloud>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        for child in children.iter() {
            if let Ok((particle, mut part)) = particles.get_mut(child) {
                part.rotation = particle_rotation(particle, rig.rotation, cam.rotation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_burst_is_about_twenty_five_flares() {
        let burst = POISON_LANDING[0].count_between(0.0, 10.0);
        assert!((burst - 25.05).abs() < 1e-2, "{burst}");
    }

    #[test]
    fn the_cloud_emits_twenty_a_second_for_three_seconds() {
        for e in &POISON_LANDING[1..] {
            assert!(
                (e.count_between(0.0, 10.0) - 60.0).abs() < 1e-3,
                "{}",
                e.name
            );
        }
    }

    #[test]
    fn one_landing_lingers_five_and_a_half_seconds() {
        assert!((poison_landing_secs() - 5.5).abs() < 1e-6);
        assert!(
            (POISON_LANDING_EMIT_SECS + POISON_LANDING_PUFF_LIFE_SECS - poison_landing_secs())
                .abs()
                < 1e-6
        );
    }

    /// A fixture reshaped from an existing constructor rather than a new
    /// `Aura {}` literal, so `tests/aura_catalog_audit.rs` has nothing new to
    /// account for — these never reach a match.
    fn aura(effect_type: AuraType, name: &str, duration: f32) -> Aura {
        let mut aura =
            weapon_poison_marker_aura(crate::states::match_config::RoguePoison::Crippling);
        aura.effect_type = effect_type;
        aura.duration = duration;
        aura.ability_name = name.to_string();
        aura
    }

    fn e(i: u32) -> Entity {
        Entity::from_raw(i)
    }

    #[test]
    fn a_rising_duration_lands_and_a_falling_one_does_not() {
        let mut ledger = PoisonLandingLedger::default();
        let deadly = |d| [aura(AuraType::DamageOverTime, "Deadly Poison", d)];
        assert_eq!(ledger.observe([(e(1), &deadly(12.0)[..])]), vec![e(1)]);
        assert!(ledger.observe([(e(1), &deadly(11.9)[..])]).is_empty());
        assert!(ledger.observe([(e(1), &deadly(11.9)[..])]).is_empty());
        assert_eq!(ledger.observe([(e(1), &deadly(12.0)[..])]), vec![e(1)]);
    }

    #[test]
    fn the_rogues_own_coating_is_not_a_landing() {
        let mut ledger = PoisonLandingLedger::default();
        let coat = [aura(AuraType::WeaponPoison, "Deadly Poison", 300.0)];
        assert!(ledger.observe([(e(1), &coat[..])]).is_empty());
    }

    #[test]
    fn a_forgotten_bearer_lands_afresh() {
        let mut ledger = PoisonLandingLedger::default();
        let slow = [aura(AuraType::MovementSpeedSlow, "Crippling Poison", 2.0)];
        assert_eq!(ledger.observe([(e(1), &slow[..])]), vec![e(1)]);
        assert!(ledger.observe([(e(2), &[][..])]).is_empty());
        assert_eq!(ledger.observe([(e(1), &slow[..])]), vec![e(1)]);
    }
}
