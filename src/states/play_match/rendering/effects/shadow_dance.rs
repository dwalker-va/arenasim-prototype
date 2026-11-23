//! Shadow Dance — a dark-red shadow state on the Rogue's chest for the whole
//! window, thinning and flickering over its last 1.5 s (AS-368).
//!
//! From the client data joined in
//! `docs/design/2026-10-17-rogue-talent-client-data.md` §4 (AS-336; Wrath
//! Classic 3.4.5.63697, spell 51713, SpellVisual 11827): one kit, 10921, plays
//! `shadowdance_state.m2` at the chest (attachment 34), a LOOPING model (1.967
//! s, every emitter at a constant rate): a slow dark-red bloom swelling from
//! the chest, dark-red shadow ribbons drifting off the body, and black shadow
//! drawn into it. Its fourth emitter (a faint cloud, alpha at most 0.25,
//! 0.01-0.03 yd) the research calls negligible and the bench does not draw.
//!
//! Played as the signed-off AS-336 Rogue Talent Visuals Bench draws it
//! (https://claude.ai/artifact/9oFjSgkeqQ4t5u4VNtyGMz, version
//! 1792302646-d16f, "Shadow Dance" tab; its source is committed as
//! `docs/design/benches/2026-10-17-rogue-talent-bench.html`, the version's id
//! a commit time, and the build follows `f559b28f`, committed 4 s after it).
//! Its constants block, at the ruling (the state held for the whole window,
//! every layer on, the authored ending kept, every multiplier 1.0), is the
//! block below. The pieces are its three emitter records, one for one:
//!
//!   bloom  -> `genericglow2c` (the bench's `glow` sprite), 3/s, each living
//!             1.5 s and still at the chest: white -> dark red (144,35,35) ->
//!             near black (43,5,5), alpha 0 -> 0.78 -> 0, radius 0.04 -> 0.43
//!             -> 1.74 yd. `blend: 'alpha'`: the bench's `Emitter.draw` paints
//!             it source-over, so it is blended and DARKENS what is behind it.
//!   red    -> the robe texture (the bench's `smoke` sprite), 33/s, each
//!             living 1.2 s, born in a 0.5 x 1.2 x 0.5 yd box round the chest
//!             and drifting out at 0.56 yd/s in its own direction: (55,47,47)
//!             -> (126,44,44) -> (92,0,0), alpha 0 -> 1 -> 0, radius 0.18 ->
//!             0.18 -> 0.24 yd. No `blend`, so `Emitter.draw` composites it
//!             `lighter`: it adds light.
//!   black  -> the robe texture again (`smoke`), 33/s, each living 1.0 s, born
//!             on a shell 0.78-1.3 yd from the chest (flattened 1.2 up and
//!             down) and drawn in through it at 1.39 yd/s: (117,47,47) -> grey
//!             (44,44,44) -> black, alpha 0 -> 1 -> 0, radius 0.16 -> 0.36 ->
//!             0.48 yd. `blend: 'alpha'`: blended, darkening.
//!
//! The card's blends (the bloom and the drawn-in shadow alpha-blended, the
//! ribbons additive) agree with the bench call for call.
//!
//! **The ending** (the bench's `rateMul`, authored, kept by the ruling): over
//! the window's last [`SHADOW_DANCE_ENDING_SECS`] every emitter's rate falls
//! linearly to zero, and every third 120 ms step of it runs at 15 %
//! ([`dance_rate_mul`]). It is read off the aura's own remaining time, so a
//! refresh puts the rates back to full and the flicker runs only before a
//! real end; it draws no `game_rng`. The step phase is the bench's own,
//! counted from the window's start ([`SHADOW_DANCE_WINDOW_SECS`], the RON's
//! 8 s, which a test pins).
//!
//! **Lifecycle.** Detected renderer-side off `ActiveAuras`, like Vengeance's
//! hands and Divine Protection's globe: a living Rogue holding the Dance
//! (`AuraType::ShadowDance`, whose only user it is) wears one rig, raised on
//! the first frame it holds the aura. A refresh keeps the rig and its
//! particles. When the aura ends — run out, or removed early — the rig emits
//! nothing more and its particles already in the air live out their own
//! lives, as the bench's do once its window closes, so the state goes
//! cleanly rather than popping; the rig is gone when its last particle is.
//! Its Rogue's death, or its being gone, takes the rig down at once, every
//! particle with it: nothing is left on a corpse. A polymorphed Rogue's state
//! is hidden with its body (the sheep wears nothing), its clock running on.
//!
//! **Never through the body material.** Stealth owns the Rogue's body (40 %
//! alpha, darkened, `update_stealth_visuals`), and the Rogue may Dance while
//! visible, so nothing here reads `stealthed` or touches the body: the state
//! is emitters only, the same in and out of stealth.
//!
//! **The particles are fixed pools.** Each emitter owns [`dance_slots`]
//! pieces; birth `j` takes slot `j mod N`, and `N` exceeds the most particles
//! the emitter can hold alive (its rate times its life, plus a frame of up to
//! [`MAX_FRAME_SECS`]), so a birth never takes a live particle's slot. Births
//! come off the bench's own accumulator (`acc += rate × dt × rateMul`, one
//! particle per whole unit), and a piece's look is a pure function of its
//! birth index and its own age ([`dance_particle_pose`]). The scatter is the
//! bench's hash ([`bench_hash`]).
//!
//! **Anchor and draw order.** The rig stands at the chest of the Rogue's
//! RENDERED body (its `VisualBody`: walk bob, swing step, Cyclone lift),
//! [`ONESHOT_CHEST`] (the bench's `ANCH.chest`, 1.45 yd) above the feet,
//! placed in `PostUpdate` after every writer of the body, unrotated, so a
//! particle's local pose is its world offset and the particles ride the Rogue
//! (the bench's `follow`). The blended capsule writes no depth, so nothing is
//! buried inside it: each particle draws before or after the body by its own
//! origin's distance from the camera. The bloom stands on the body's axis
//! above its centre, so it sorts over the body at every camera pitch.
//!
//! **Departures from the bench:**
//!
//! - The bench's onset-only mode (`ShadowDanceState::OnsetOnly`, the client
//!   model played once) is not built: the ruling holds the state.
//! - Colour is blended in linear light where the bench's canvas blends sRGB
//!   bytes, as for every client effect in the engine.
//! - The bench's 0.6 px floor on a sprite's radius is not kept (a bloom is
//!   born 0.08 yd across, about 1.7 px at the match camera, and is invisible
//!   at birth anyway, its alpha starting at 0).
//! - A particle's first draw is one frame after its birth, as the bench's is,
//!   but the rig appears the frame after the aura lands (detection off
//!   `ActiveAuras`), so the whole state runs one frame behind the aura.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn, and
//! every system is registered in `states/mod.rs` only. Headless never runs any
//! of it.

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{BodyHold, HoldDisjoint};

use super::cheat_death::{particle_sprite_image, GLOW_SOFT, SMOKE_SOFT};
use super::druid_heals::{keyed, BODY_CENTRE_ABOVE_FLOOR};
use super::paladin_oneshots::{bench_hash, piece_rotation, PieceFacing, PiecePose, ONESHOT_CHEST};
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792302646-d16f) ─────

// Shadow Dance: shadowdance_state.m2 (Wrath kit 10921) on the Rogue's chest.
// AS-336 bench. `SHADOW_DANCE_STATE` is `HeldForWindow`: the state is held for
// the whole window (the only mode built).
pub const SHADOW_DANCE_BLOOM: bool = true;
pub const SHADOW_DANCE_RED_RIBBONS: bool = true;
pub const SHADOW_DANCE_BLACK_INWARD: bool = true;
/// Thin + flicker before the window ends.
pub const SHADOW_DANCE_ENDING_SECS: f32 = 1.5;
pub const SHADOW_DANCE_SIZE_MUL: f32 = 1.0;
pub const SHADOW_DANCE_OPACITY_MUL: f32 = 1.0;
// Emitters on the Rogue; never through the body material (stealth owns it).
// AuraApplyRoute: owned by this state (by type), not the band.

/// Where the state stands: the chest attachment, yards above the floor (the
/// bench's `ANCH.chest`).
pub const SHADOW_DANCE_CHEST: f32 = ONESHOT_CHEST;

// ── The ending ───────────────────────────────────────────────────────────────

/// The window the ending's flicker is phased against: the Dance's aura
/// duration in `abilities.ron` (the bench's `durMs`, 8000).
pub const SHADOW_DANCE_WINDOW_SECS: f32 = 8.0;
/// The flicker's step, seconds.
pub const SHADOW_DANCE_FLICKER_STEP_SECS: f32 = 0.12;
/// Every third step runs at this share of the rate.
pub const SHADOW_DANCE_FLICKER_LOW: f32 = 0.15;

/// Every emitter's rate multiplier with `remaining` seconds of the aura left:
/// the bench's `rateMul`. Full until the last [`SHADOW_DANCE_ENDING_SECS`];
/// then falling linearly to zero, every step whose index (counted from the
/// window's start, the bench's `floor(ms / 120)`) is a multiple of three at
/// [`SHADOW_DANCE_FLICKER_LOW`] of that; nothing once the window is over.
pub fn dance_rate_mul(remaining: f32) -> f32 {
    if remaining > SHADOW_DANCE_ENDING_SECS {
        return 1.0;
    }
    if remaining < 0.0 {
        return 0.0;
    }
    let elapsed = SHADOW_DANCE_WINDOW_SECS - remaining;
    let step = (elapsed / SHADOW_DANCE_FLICKER_STEP_SECS).floor() as i64;
    let flicker = if step.rem_euclid(3) == 0 {
        SHADOW_DANCE_FLICKER_LOW
    } else {
        1.0
    };
    (remaining / SHADOW_DANCE_ENDING_SECS).max(0.0) * flicker
}

// ── The emitters ─────────────────────────────────────────────────────────────

/// One of the three emitters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum DanceLayer {
    /// The dark-red bloom, still at the chest.
    Bloom,
    /// The red shadow ribbons drifting out.
    RedRibbons,
    /// The black shadow drawn in.
    BlackInward,
}

impl DanceLayer {
    pub const ALL: [DanceLayer; 3] = [
        DanceLayer::Bloom,
        DanceLayer::RedRibbons,
        DanceLayer::BlackInward,
    ];

    /// The layer's index in [`DanceLayer::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its record.
    pub fn emitter(self) -> &'static DanceEmitter {
        &DANCE_EMITTERS[self.index()]
    }
}

/// One emitter's record, as the bench declares it. The tracks are keyed over
/// the particle's life, 0..1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DanceEmitter {
    pub layer: DanceLayer,
    /// The bench's layer toggle.
    pub on: bool,
    /// Particles a second, before the ending's multiplier.
    pub rate: f32,
    /// Each particle's life, seconds.
    pub life: f32,
    /// Speed, yd/s: out along its direction, or in for the drawn-in shadow.
    pub speed: f32,
    /// sRGB over the life (the bench truncates each channel, `rgb()`'s `|0`).
    pub rgb: [(f32, [u8; 3]); 3],
    pub alpha: [(f32, f32); 3],
    /// The bench's `scale`: the sprite's RADIUS, yards (`sprite` draws
    /// `scale × size × 2` across).
    pub radius: [(f32, f32); 3],
    /// The bench sprite's radial alpha stops ([`GLOW_SOFT`], [`SMOKE_SOFT`]).
    pub soft: [f32; 3],
    /// Composited `lighter` (added) rather than source-over (blended).
    pub additive: bool,
}

/// The three emitters, in [`DanceLayer::ALL`] order.
pub const DANCE_EMITTERS: [DanceEmitter; 3] = [
    // P0 `genericglow2c`: `{anchor:'chest', vRange:3.14, speed:0, life:1.5,
    // rate:3, blend:'alpha', kind:'glow', follow:true}`.
    DanceEmitter {
        layer: DanceLayer::Bloom,
        on: SHADOW_DANCE_BLOOM,
        rate: 3.0,
        life: 1.5,
        speed: 0.0,
        rgb: [
            (0.0, [255, 255, 255]),
            (0.5, [144, 35, 35]),
            (1.0, [43, 5, 5]),
        ],
        alpha: [(0.0, 0.0), (0.5, 0.78), (1.0, 0.0)],
        radius: [(0.0, 0.04), (0.5, 0.43), (1.0, 1.74)],
        soft: GLOW_SOFT,
        additive: false,
    },
    // P1, the robe texture: `{anchor:'chest', vRange:3.14, speed:0.56,
    // life:1.2, rate:33, area:0.5, areaY:1.2, kind:'smoke', follow:true}`
    // (no `blend`, so `lighter`).
    DanceEmitter {
        layer: DanceLayer::RedRibbons,
        on: SHADOW_DANCE_RED_RIBBONS,
        rate: 33.0,
        life: 1.2,
        speed: 0.56,
        rgb: [(0.0, [55, 47, 47]), (0.5, [126, 44, 44]), (1.0, [92, 0, 0])],
        alpha: [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        radius: [(0.0, 0.18), (0.5, 0.18), (1.0, 0.24)],
        soft: SMOKE_SOFT,
        additive: true,
    },
    // P2, the robe texture: `{anchor:'chest', inward:1.3, inwardFlat:1.2,
    // speed:-1.39, life:1.0, rate:33, blend:'alpha', kind:'smoke',
    // follow:true}`.
    DanceEmitter {
        layer: DanceLayer::BlackInward,
        on: SHADOW_DANCE_BLACK_INWARD,
        rate: 33.0,
        life: 1.0,
        speed: 1.39,
        rgb: [(0.0, [117, 47, 47]), (0.5, [44, 44, 44]), (1.0, [0, 0, 0])],
        alpha: [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        radius: [(0.0, 0.16), (0.5, 0.36), (1.0, 0.48)],
        soft: SMOKE_SOFT,
        additive: false,
    },
];

/// The ribbons' birth box round the chest, `(across, tall)` yards (the
/// bench's `area`, `areaY`).
pub const RED_BIRTH_BOX: (f32, f32) = (0.5, 1.2);
/// The drawn-in shadow's birth shell: radius 1.3 yd (the bench's `inward`)
/// times 0.6-1.0, its directions stretched 1.2 up and down (`inwardFlat`).
pub const BLACK_SHELL_RADIUS: f32 = 1.3;
pub const BLACK_SHELL_INNER: f32 = 0.6;
pub const BLACK_SHELL_FLAT: f32 = 1.2;

/// The longest frame a pool is sized to absorb without a birth taking a live
/// particle's slot.
pub const MAX_FRAME_SECS: f32 = 0.1;

/// Pieces an emitter's pool holds: more than it can hold alive at once (its
/// rate times its life and one frame, plus a birth's rounding).
pub fn dance_slots(e: &DanceEmitter) -> usize {
    (e.rate * (e.life + MAX_FRAME_SECS)).ceil() as usize + 1
}

// ── The particles ────────────────────────────────────────────────────────────

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
fn track_rgb(track: &[(f32, [u8; 3]); 3], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// The bench's scatter for particle `j` of `layer`, draw `k`: 0..1.
fn scatter(layer: DanceLayer, j: u32, k: u32) -> f32 {
    bench_hash(
        j.wrapping_mul(3)
            .wrapping_add(layer.index() as u32)
            .wrapping_mul(8)
            .wrapping_add(k),
    )
}

/// Where particle `j` of `layer` is born, from the chest, and its velocity:
/// the bloom at the chest, still; a ribbon in its box, flying out along a
/// random point of the cube `[-1, 1]³` normalised (the bench's `vRange`
/// 3.14); the drawn-in shadow on its shell, flying in through the chest.
pub fn dance_launch(layer: DanceLayer, j: u32) -> (Vec3, Vec3) {
    let e = layer.emitter();
    let h = |k: u32| scatter(layer, j, k);
    match layer {
        DanceLayer::Bloom => (Vec3::ZERO, Vec3::ZERO),
        DanceLayer::RedRibbons => {
            let dir = Vec3::new(h(1) * 2.0 - 1.0, h(2) * 2.0 - 1.0, h(3) * 2.0 - 1.0)
                .try_normalize()
                .unwrap_or(Vec3::Y);
            let (across, tall) = RED_BIRTH_BOX;
            let at = Vec3::new(
                (h(4) - 0.5) * across,
                (h(5) - 0.5) * tall,
                (h(6) - 0.5) * across,
            );
            (at, dir * e.speed)
        }
        DanceLayer::BlackInward => {
            let dir = Vec3::new(
                h(1) * 2.0 - 1.0,
                (h(2) * 2.0 - 1.0) * BLACK_SHELL_FLAT,
                h(3) * 2.0 - 1.0,
            )
            .try_normalize()
            .unwrap_or(Vec3::Y);
            let r = BLACK_SHELL_RADIUS * (BLACK_SHELL_INNER + (1.0 - BLACK_SHELL_INNER) * h(4));
            (dir * r, -dir * e.speed)
        }
    }
}

/// Particle `j` of `layer`, `age` seconds after its birth: its place from the
/// chest, its size (a quad's width and height, twice the bench's radius), its
/// colour and alpha, facing the camera; `None` once its life is spent. Pure:
/// the probes and the placement read one definition.
pub fn dance_particle_pose(layer: DanceLayer, j: u32, age: f32) -> Option<PiecePose> {
    let e = layer.emitter();
    if !(0.0..e.life).contains(&age) {
        return None;
    }
    let t = age / e.life;
    let (at, velocity) = dance_launch(layer, j);
    Some(PiecePose {
        at: at + velocity * age,
        size: Vec2::splat(2.0 * keyed(&e.radius, t) * SHADOW_DANCE_SIZE_MUL),
        facing: PieceFacing::Camera,
        rgb: track_rgb(&e.rgb, t),
        alpha: keyed(&e.alpha, t) * SHADOW_DANCE_OPACITY_MUL,
    })
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// The Dance's remaining seconds on a unit holding it (the longest, were
/// there two), or `None`.
pub fn shadow_dance_remaining(auras: &ActiveAuras) -> Option<f32> {
    auras
        .auras
        .iter()
        .filter(|a| a.effect_type == AuraType::ShadowDance)
        .map(|a| a.duration)
        .reduce(f32::max)
}

// ── Components ───────────────────────────────────────────────────────────────

/// One particle in a pool slot: its birth index and the rig's age at its
/// birth.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DanceBirth {
    pub j: u32,
    pub born: f32,
}

/// One Rogue's Dance: a rig at its chest, parent of every emitter's pool.
#[derive(Component, Debug)]
pub struct ShadowDanceRig {
    pub bearer: Entity,
    /// Seconds since the rig rose (a refresh does not reset it).
    pub age: f32,
    /// The aura's remaining seconds as last read, while it holds; `None`
    /// once it has ended (the rig is spending its last particles).
    pub remaining: Option<f32>,
    /// Particles owed per emitter, carried between frames (the bench's
    /// `acc`).
    pub carry: [f32; 3],
    /// Particles born per emitter: the next birth's index.
    pub emitted: [u32; 3],
    /// Each emitter's pool: the particle in each slot.
    pub slots: [Vec<Option<DanceBirth>>; 3],
}

impl ShadowDanceRig {
    pub fn new(bearer: Entity, remaining: f32) -> Self {
        Self {
            bearer,
            age: 0.0,
            remaining: Some(remaining),
            carry: [0.0; 3],
            emitted: [0; 3],
            slots: DanceLayer::ALL.map(|l| vec![None; dance_slots(l.emitter())]),
        }
    }

    /// Age the rig `dt` and emit what the frame owes, the aura having
    /// `remaining` seconds left (`None`: it has ended, and nothing is
    /// emitted). The bench's `Emitter.update`: the clock advances, the rate
    /// (times [`dance_rate_mul`]) accrues, and each whole particle owed is born
    /// at the frame's start, so it is one frame old when first drawn.
    pub fn advance(&mut self, dt: f32, remaining: Option<f32>) {
        self.age += dt;
        self.remaining = remaining;
        let Some(remaining) = remaining else {
            return;
        };
        let mul = dance_rate_mul(remaining);
        for layer in DanceLayer::ALL {
            let e = layer.emitter();
            if !e.on {
                continue;
            }
            let i = layer.index();
            self.carry[i] += e.rate * dt * mul;
            while self.carry[i] >= 1.0 {
                self.carry[i] -= 1.0;
                let j = self.emitted[i];
                let n = self.slots[i].len();
                self.slots[i][j as usize % n] = Some(DanceBirth {
                    j,
                    born: self.age - dt,
                });
                self.emitted[i] += 1;
            }
        }
    }

    /// The particle in `slot` of `layer` and its own age, while it lives.
    pub fn particle(&self, layer: DanceLayer, slot: usize) -> Option<(u32, f32)> {
        let birth = (*self.slots[layer.index()].get(slot)?)?;
        let age = self.age - birth.born;
        (age < layer.emitter().life).then_some((birth.j, age))
    }

    /// Whether any particle of the rig still lives.
    pub fn any_alive(&self) -> bool {
        DanceLayer::ALL.into_iter().any(|layer| {
            (0..self.slots[layer.index()].len()).any(|s| self.particle(layer, s).is_some())
        })
    }
}

/// One slot of a [`ShadowDanceRig`]'s pool.
#[derive(Component, Debug)]
pub struct ShadowDancePiece {
    pub layer: DanceLayer,
    pub slot: usize,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The quad and the two bench sprites every rig draws with, built once.
pub struct ShadowDanceAssets {
    quad: Handle<Mesh>,
    glow: Handle<Image>,
    smoke: Handle<Image>,
}

impl ShadowDanceAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: images.add(particle_sprite_image(GLOW_SOFT)),
            smoke: images.add(particle_sprite_image(SMOKE_SOFT)),
        }
    }

    fn texture(&self, layer: DanceLayer) -> Handle<Image> {
        if layer.emitter().soft == GLOW_SOFT {
            self.glow.clone()
        } else {
            self.smoke.clone()
        }
    }
}

/// How a layer composites: added light (`lighter`) or laid over (blended,
/// darkening).
pub fn dance_alpha_mode(layer: DanceLayer) -> AlphaMode {
    if layer.emitter().additive {
        AlphaMode::Add
    } else {
        AlphaMode::Blend
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): raise a rig on every living unit holding Shadow
/// Dance; age each rig and emit what the frame owes; stop emitting when the
/// aura ends and take the rig down once its last particle is spent; take it
/// down at once if its Rogue dies or is gone; hide it while its Rogue is a
/// sheep.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_shadow_dance(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ShadowDanceAssets>>,
    bearers: Query<(
        Entity,
        &Combatant,
        &Transform,
        &ActiveAuras,
        Has<PolymorphedVisual>,
    )>,
    mut rigs: Query<(Entity, &mut ShadowDanceRig, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let hidden = |sheep: bool| {
        if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    };
    let mut worn = Vec::new();
    for (entity, mut rig, mut visibility) in rigs.iter_mut() {
        let Some((_, _, _, auras, sheep)) = bearers
            .get(rig.bearer)
            .ok()
            .filter(|(_, c, _, _, _)| c.is_alive())
        else {
            // Dead or gone: nothing stays on a corpse.
            commands.entity(entity).despawn();
            continue;
        };
        rig.advance(dt, shadow_dance_remaining(auras));
        if rig.remaining.is_none() && !rig.any_alive() {
            commands.entity(entity).despawn();
            continue;
        }
        visibility.set_if_neq(hidden(sheep));
        worn.push(rig.bearer);
    }
    for (entity, combatant, transform, auras, sheep) in bearers.iter() {
        if !combatant.is_alive() || worn.contains(&entity) {
            continue;
        }
        let Some(remaining) = shadow_dance_remaining(auras) else {
            continue;
        };
        let assets =
            assets.get_or_insert_with(|| ShadowDanceAssets::build(&mut meshes, &mut images));
        let rig = ShadowDanceRig::new(entity, remaining);
        let pools: Vec<(DanceLayer, usize)> = DanceLayer::ALL
            .into_iter()
            .filter(|l| l.emitter().on)
            .flat_map(|l| (0..rig.slots[l.index()].len()).map(move |s| (l, s)))
            .collect();
        let rig_entity = commands
            .spawn((
                rig,
                Transform::from_translation(
                    transform.translation
                        + Vec3::Y * (SHADOW_DANCE_CHEST - BODY_CENTRE_ABOVE_FLOOR),
                ),
                hidden(sheep),
                PlayMatchEntity,
            ))
            .id();
        for (layer, slot) in pools {
            let e = layer.emitter();
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb_u8(e.rgb[0].1[0], e.rgb[0].1[1], e.rgb[0].1[2])
                    .with_alpha(0.0),
                base_color_texture: Some(assets.texture(layer)),
                // The bench paints colour × sprite × alpha and nothing else:
                // added (`lighter`) or laid over (source-over) as its
                // emitter composites.
                unlit: true,
                alpha_mode: dance_alpha_mode(layer),
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let child = commands
                .spawn((
                    ShadowDancePiece { layer, slot },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig_entity).add_child(child);
        }
    }
}

/// Where a Rogue's Dance stands: [`SHADOW_DANCE_CHEST`] above the feet of
/// its rendered body (`body_local`, its `VisualBody`'s translation).
pub fn shadow_dance_anchor(bearer: &Transform, body_local: Vec3) -> Vec3 {
    bearer.transform_point(body_local) + Vec3::Y * (SHADOW_DANCE_CHEST - BODY_CENTRE_ABOVE_FLOOR)
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each rig on its Rogue's rendered chest, and pose,
/// turn and colour every particle. The rig is unrotated, so a particle's local
/// pose is its world offset.
#[allow(clippy::type_complexity)]
pub fn place_shadow_dance(
    hold: BodyHold,
    mut rigs: Query<
        (&ShadowDanceRig, &mut Transform, &Children),
        (Without<ShadowDancePiece>, HoldDisjoint),
    >,
    mut pieces: Query<
        (
            &ShadowDancePiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<ShadowDanceRig>, HoldDisjoint),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cam = hold.view().map(|c| c.rotation);
    for (rig, mut transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.bearer) else {
            continue;
        };
        transform.translation = shadow_dance_anchor(&body.unit, body.local.translation);
        for child in children.iter() {
            let Ok((piece, mut part, material)) = pieces.get_mut(child) else {
                continue;
            };
            let pose = rig
                .particle(piece.layer, piece.slot)
                .and_then(|(j, age)| dance_particle_pose(piece.layer, j, age));
            let (rgb, alpha) = match pose {
                Some(pose) => {
                    part.translation = pose.at;
                    part.scale = pose.size.extend(1.0).max(Vec3::splat(1e-3));
                    part.rotation = piece_rotation(pose.facing, Quat::IDENTITY, cam);
                    (pose.rgb, pose.alpha.clamp(0.0, 1.0))
                }
                None => {
                    part.scale = Vec3::splat(1e-3);
                    let rgb = piece.layer.emitter().rgb[0].1;
                    (rgb, 0.0)
                }
            };
            let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).with_alpha(alpha);
            // Only touch the asset when the look moved: `get_mut` marks it
            // changed and re-uploads it.
            if materials
                .get(&material.0)
                .is_some_and(|m| m.base_color != color)
            {
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color = color;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants block as the signed-off bench exports it.
    #[test]
    fn the_constants_are_the_benchs() {
        const { assert!(SHADOW_DANCE_BLOOM && SHADOW_DANCE_RED_RIBBONS && SHADOW_DANCE_BLACK_INWARD) };
        assert_eq!(SHADOW_DANCE_ENDING_SECS, 1.5);
        assert_eq!(SHADOW_DANCE_SIZE_MUL, 1.0);
        assert_eq!(SHADOW_DANCE_OPACITY_MUL, 1.0);
        assert_eq!(SHADOW_DANCE_CHEST, 1.45);
    }

    /// The window the flicker is phased on is the RON's aura duration.
    #[test]
    fn the_window_is_the_auras_duration() {
        let defs = crate::states::play_match::ability_config::AbilityDefinitions::default();
        let aura = defs
            .get(&crate::states::play_match::abilities::AbilityType::ShadowDance)
            .and_then(|d| d.applies_aura.as_ref())
            .expect("Shadow Dance applies its aura");
        assert_eq!(aura.aura_type, AuraType::ShadowDance);
        assert_eq!(aura.duration, SHADOW_DANCE_WINDOW_SECS);
    }

    /// No pool can be outrun: at a steady 60 fps, a 0.1 s hitch and the
    /// ending's flicker, a birth never lands on a live particle's slot.
    #[test]
    fn no_birth_takes_a_live_slot() {
        for dt in [1.0 / 60.0, 1.0 / 144.0, MAX_FRAME_SECS] {
            let mut rig = ShadowDanceRig::new(Entity::from_raw(1), 8.0);
            let mut remaining = 8.0;
            while remaining > 0.0 {
                remaining -= dt;
                let before: Vec<Vec<Option<(u32, f32)>>> = DanceLayer::ALL
                    .iter()
                    .map(|l| {
                        (0..rig.slots[l.index()].len())
                            .map(|s| rig.particle(*l, s))
                            .collect()
                    })
                    .collect();
                let emitted = rig.emitted;
                rig.advance(dt, Some(remaining));
                for layer in DanceLayer::ALL {
                    let i = layer.index();
                    let n = rig.slots[i].len() as u32;
                    for j in emitted[i]..rig.emitted[i] {
                        let slot = (j % n) as usize;
                        // The slot's previous occupant was spent by now.
                        if let Some((_, age)) = before[i][slot] {
                            assert!(
                                age + dt >= layer.emitter().life - 1e-4,
                                "{layer:?} slot {slot} at dt {dt}"
                            );
                        }
                    }
                }
            }
        }
    }
}
