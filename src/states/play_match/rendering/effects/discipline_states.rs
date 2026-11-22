//! Power Infusion and Pain Suppression — the two Discipline buffs a Priest
//! puts on an ally, each drawn as its state on the ally for the buff's life
//! (AS-353).
//!
//! From the client data joined in
//! `docs/design/2026-10-17-priest-talent-client-data.md` §2-3 (AS-335):
//!
//! - **Power Infusion** (10060, visual 7553): the cast is kit 191,
//!   `holy_precast_high_hand.m2` on both hands; nothing lands; the state is
//!   kit 3769, Arcane Power's `arcanepower_state_chest.m2` at the ally's
//!   chest for 15 s. It reads as a crackling blue-white electric lattice with
//!   wide pale-blue glow discs throbbing round it, white sparks flying off,
//!   and a star and a long horizontal streak flashing in its first 1.33 s.
//! - **Pain Suppression** (TBC 33206, visual 8039): the cast is kit 165,
//!   Consecration's `holy_precast_uber_base.m2` at the Priest's feet plus the
//!   high hands; nothing lands; the state is kit 6915,
//!   `painsuppression_state.m2` at the ally for 8 s. It reads as a tall
//!   gold-and-teal shell standing in front of the ally, with bubbles swelling
//!   and light streaks drawn into it.
//!
//! Played as the signed-off AS-335 Priest Talent Visuals Bench draws them
//! (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
//! 1792207490-53c9, the "Power Infusion" and "Pain Suppression" tabs; its
//! source as committed with the sign-off is
//! `docs/design/benches/2026-10-17-priest-talent-bench.html`, identical to
//! `f71cdff0`). Rulings: Power Infusion keeps client size; Pain Suppression's
//! shell is faithful. Every layer on, every multiplier 1.0. The pieces are
//! the two tabs' `customDraw` and emitter records, one for one:
//!
//!   Power Infusion, at the chest (`ANCH.chest`, 1.45 yd above the feet):
//!   discs   -> three camera-facing glows (`bbGlow`): 4.6 yd (its size
//!              pulsing 0.85-1.0 on the bone's 433 ms loop) and 3.2 yd, each
//!              throbbing 0.1 <-> 0.5 on a 1 s loop, and a steady 2.2 yd one;
//!   lattice -> seven strands of five line segments (`zap1`), about 1.7 yd
//!              across and 1.5 tall, re-jittered every 67 ms, flickering to
//!              0.73 three times in each 1.1 s;
//!   onset   -> a 3.4 yd star glow and a 6.1 x 0.3 yd horizontal streak,
//!              rising and falling over the first 1.334 s only;
//!   sparks  -> white four-point sparks, 60/s out of the chest at 3.3 yd/s,
//!              living 0.2 s, 0.42 -> 0.28 -> 0.03 yd.
//!
//!   Pain Suppression, 1.3 yd above the ally's feet:
//!   shell   -> three upright camera-facing glows 0.4 yd in front of the ally:
//!              teal 3.4 x 4.4, pale yellow 2.4 x 3.1, gold 2.3 x 2.9 yd, their
//!              height squashing 0.52 -> 1.09 -> 0.9 -> 1 on an 867 ms loop;
//!              flashing to full at 467 ms and settling to 0.14 by 867 ms,
//!              then flickering 0.05 <-> 0.4 on the loop;
//!   bubbles -> soft glows, 10/s in a 1.0 x 1.2 x 1.0 yd box, drifting at
//!              0.15 yd/s, living 1.6 s, swelling to 2.2 yd across;
//!   streaks -> light streaks, 100/s, born on a 2.1 yd shell and pulled in
//!              at 4.7 yd/s, living 0.45 s, each with its tail;
//!   swirl   -> at the CASTER's feet, 1.067 s: a level ring contracting
//!              2.4 -> 1.4 yd across over 833 ms, orange -> gold -> violet,
//!              in by 333 ms and out by 833 ms, over a soft gold ground glow
//!              2.1 yd across at half its weight.
//!
//! Every piece is composited `lighter` by the bench (its `bbGlow`, `ring3`
//! and `groundDisc`, the lattice's own `'lighter'`, and three emitters that
//! pass no `blend`), so every piece here is `AlphaMode::Add`.
//!
//! **The cast.** Nothing lands on the ally: its state's onset is its
//! application, so both route off the aura band by name
//! (`AuraApplyRoute::for_instance`). The bench draws no hands; both kits are
//! the high Holy hands, the model Aura Mastery's kit 191 already plays, so
//! the caster's hands are the shipped instant Holy flash
//! (`HealCastKind::HolyFlash`), flashed on the aura's caster at the state's
//! onset and again on a refresh. Pain Suppression's refresh replays the
//! swirl too. Both buffs are applied inline by the Priest AI with no cast
//! state, so nothing else plays the hands ([`discipline_hands_at_onset`]).
//!
//! **Lifecycle.** Detected renderer-side off `ActiveAuras`, like Divine
//! Protection's globe: a living unit holding either buff wears one state of
//! it, raised on the first frame it holds it, kept through a refresh (its
//! clock running on, so the onset does not replay), and taken down on the
//! frame it no longer does: run out, purged or dispelled, its bearer dead
//! (auras stay on a corpse) or gone. A polymorphed bearer's state is hidden
//! with its body (the sheep wears nothing), its clock running on. Stealth
//! owns the body's material, which nothing here touches, so a state is the
//! same in and out of stealth. A swirl lives its 1.067 s, or until its
//! caster dies.
//!
//! **The particles are fixed pools** (the Shadow Dance idiom): each emitter
//! owns [`state_slots`] pieces, more than it can hold alive, births come off
//! the bench's own accumulator, and a particle's look is a pure function of
//! its birth index and its own age ([`state_particle_pose`]); the scatter is
//! the bench's hash.
//!
//! **Held in front of the body.** The bench draws every piece over the
//! bodies. Here the body is an `AlphaMode::Blend` capsule that writes no
//! depth, and the transparent pass sorts by each mesh's translation, so a
//! piece whose origin is deeper than the body's centre would be painted over
//! by it: the sparks flying back, the lattice's far strands, the far half of
//! the swirl, and the whole shell whenever its ally faces away from the
//! camera (it stands 0.4 yd along the ally's facing). Every piece is
//! therefore drawn on its own view ray, slid toward the camera until it
//! stands nearer than its body's nearest point, and scaled by the same ratio
//! ([`siphon_drawn_point`], AS-347's rule): the same screen footprint, sorted
//! in front. A quad scaled about the eye this way projects exactly where it
//! did, a level ring included. Standing in front of the whole capsule also
//! sorts every piece over a Power Word: Shield's blended outline on the same
//! ally, whose cards stand on the body's axis.
//!
//! **Departures from the bench**, each deliberate:
//!
//! 1. **The hold** above, which the 2D canvas does not need.
//! 2. **Strokes have a world width.** The bench strokes the lattice 1.5 px
//!    and the swirl ring 3 px on its canvas. The lattice is a soft band
//!    [`POWER_INFUSION_LATTICE_WIDTH`] wide, 1.5 px at the match camera; the
//!    ring is the shared ring sprite (`RING_STROKE`), 1-2 px there. The
//!    streaks' tails are the same soft band, 0.7 of the sprite's radius wide
//!    as the bench's are, with square ends where the bench's are round.
//! 3. **The sparks' white-hot core is tinted.** The bench draws a star's
//!    centre white over its coloured rays; the shared star sprite is white
//!    throughout and takes the spark's colour (white to pale blue).
//! 4. **Particles ride the rig in height too.** The bench's `follow` moves
//!    them with the ally in x and z only.
//! 5. **Hidden on a sheep**, as above (the bench has no sheep).
//! 6. **The state ends with its buff.** The bench loops forever; here the
//!    state goes on the frame the buff does, as Siphon Life's and
//!    Vengeance's do.
//!
//! Where the card and the bench disagree, the bench was signed off and wins:
//! the shell's loop flickers 0.05 <-> 0.4 (the card: 0 <-> 0.4); the light
//! streaks are one emitter, 100/s at -4.7 yd/s from a 2.1 yd shell (the card:
//! two of 50/s at -3.9 and -5.6); and the cast swirl is the ring, orange to
//! gold to violet, over its ground glow, with no tall swirl mesh and no
//! sparks (the card: a gold ring, a tall gold swirl and inward sparks).
//!
//! **At the match camera** (60 yd, pitch 38.7°, FOV 45°, about 22 px a yard
//! at 1080p): Power Infusion's widest disc is about 100 px across at 0.28
//! alpha at most, its streak about 132 x 7 px for its first 1.3 s; Pain
//! Suppression's teal glow about 74 px wide and up to 104 px tall, at 0.22
//! alpha at most after its onset.
//! Additive soft glows centred on the ally, each fading to nothing at its
//! rim: they read as light round the ally, not as a screen-filling flash.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the bench's own hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use std::f32::consts::{PI, TAU};

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::cheat_death::{particle_sprite_image, GLOW_SOFT};
use super::druid_heals::{keyed, BODY_CENTRE_ABOVE_FLOOR};
use super::heal_cast::{flash_instant_hands, HealCastAssets};
use super::paladin_oneshots::{
    bench_hash, oneshot_glow_texture, oneshot_ground_texture, oneshot_ring_texture, piece_rotation,
    PieceFacing, PiecePose, ONESHOT_CHEST, RING_STROKE,
};
use super::siphon_life::{siphon_body_capsule, siphon_drawn_point};
use super::spell_bolts::{soft_band_texture, star_flash_texture};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792207490-53c9) ─────

// Power Infusion: arcanepower_state_chest.m2 (kit 3769) on the ally's chest for the buff. AS-335 bench.
pub const POWER_INFUSION_DISCS: bool = true; // 3.2 / 4.6 / 2.2 yd x size
pub const POWER_INFUSION_LATTICE: bool = true;
pub const POWER_INFUSION_ONSET_FLASH: bool = true;
pub const POWER_INFUSION_SPARKS: bool = true;
pub const POWER_INFUSION_SIZE_MUL: f32 = 1.00;
pub const POWER_INFUSION_OPACITY_MUL: f32 = 1.00;
// Cast: holy_precast_high_hand.m2 on the Priest (gold, 0.88 yd glow + 3 ribbons).

// Pain Suppression: painsuppression_state.m2 (kit 6915) on the ally for 8 s; cast adds holy_precast_uber_base.m2 at the Priest's feet. AS-335 bench.
pub const PAIN_SUPPRESSION_SHELL: bool = true; // 3.4 x 4.4 yd largest quad x size
pub const PAIN_SUPPRESSION_BUBBLES: bool = true;
pub const PAIN_SUPPRESSION_STREAKS: bool = true;
pub const PAIN_SUPPRESSION_CAST_SWIRL: bool = true;
pub const PAIN_SUPPRESSION_SIZE_MUL: f32 = 1.00;
pub const PAIN_SUPPRESSION_OPACITY_MUL: f32 = 1.00;
// AuraApplyRoute: both owned by their states (by name), not the band.

// ── Where the states stand ───────────────────────────────────────────────────

/// Power Infusion's state: the chest attachment, yards above the feet (the
/// bench's `c`, y 1.45).
pub const POWER_INFUSION_CHEST: f32 = ONESHOT_CHEST;
/// Pain Suppression's state: yards above the feet (the bench's shell `p` and
/// its emitters' `pos`, y 1.3).
pub const SUPPRESSION_HEIGHT: f32 = 1.3;
/// The shell stands this far in front of the ally, along its facing (the
/// bench's `vp[2] + 0.4`; the client's quads stand in front of the body).
pub const SUPPRESSION_SHELL_FORWARD: f32 = 0.4;

// ── Power Infusion's tracks (seconds) ────────────────────────────────────────

/// The two throbbing discs' weight: 0.1 -> 0.5 -> 0.1 on a 1 s loop.
pub const POWER_INFUSION_THROB_SECS: f32 = 1.0;
pub const POWER_INFUSION_THROB: [(f32, f32); 3] = [(0.0, 0.1), (0.5, 0.5), (1.0, 0.1)];
/// The widest disc's size pulse (the bone's 433 ms loop, 1 -> 0.62 -> 1.16
/// -> 0.5 -> 1), worked in at [`POWER_INFUSION_PULSE_SHARE`] of its size.
pub const POWER_INFUSION_PULSE_SECS: f32 = 0.433;
pub const POWER_INFUSION_PULSE: [(f32, f32); 5] = [
    (0.0, 1.0),
    (0.108, 0.62),
    (0.216, 1.16),
    (0.324, 0.5),
    (0.433, 1.0),
];
/// `size × (fixed + pulse × share)`.
pub const POWER_INFUSION_PULSE_SHARE: (f32, f32) = (0.85, 0.15);
/// The discs: diameter (yd), colour, alpha over the throb (the core disc's
/// is its whole alpha: it does not throb).
pub const POWER_INFUSION_WIDE_DISC: (f32, [u8; 3], f32) = (4.6, [110, 150, 255], 0.55);
pub const POWER_INFUSION_MID_DISC: (f32, [u8; 3], f32) = (3.2, [140, 180, 255], 0.6);
pub const POWER_INFUSION_CORE_DISC: (f32, [u8; 3], f32) = (2.2, [190, 215, 255], 0.35);

/// The lattice's flicker: to 0.73 three times in each 1.1 s, drawn only
/// above [`POWER_INFUSION_LATTICE_FLOOR`].
pub const POWER_INFUSION_FLICKER_SECS: f32 = 1.1;
pub const POWER_INFUSION_FLICKER: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.183, 0.73),
    (0.366, 0.0),
    (0.55, 0.73),
    (0.733, 0.0),
    (0.916, 0.73),
    (1.1, 0.0),
];
pub const POWER_INFUSION_LATTICE_FLOOR: f32 = 0.05;
/// Seven strands of six points, so five segments each.
pub const POWER_INFUSION_STRANDS: u8 = 7;
pub const POWER_INFUSION_SEGMENTS: u8 = 5;
/// A point's angle: `strand × 0.9 + point × 0.5 + floor(t / 67 ms) × 0.37`.
pub const POWER_INFUSION_STRAND_TURN: f32 = 0.9;
pub const POWER_INFUSION_POINT_TURN: f32 = 0.5;
pub const POWER_INFUSION_JITTER_SECS: f32 = 0.067;
pub const POWER_INFUSION_JITTER_TURN: f32 = 0.37;
/// A point's radius: `0.85 × (0.6 + 0.4 sin(3a + point))`; its place
/// `(r cos 2.1a, (point / 5 - 0.5) × 1.5, r sin 1.7a)` from the chest.
pub const POWER_INFUSION_LATTICE_RADIUS: f32 = 0.85;
pub const POWER_INFUSION_LATTICE_SPREAD: (f32, f32) = (0.6, 0.4);
pub const POWER_INFUSION_LATTICE_TALL: f32 = 1.5;
pub const POWER_INFUSION_LATTICE_AXES: (f32, f32) = (2.1, 1.7);
pub const POWER_INFUSION_LATTICE_RGB: [u8; 3] = [210, 235, 255];
/// A segment's width, yards: the bench's 1.5 px stroke at the match camera
/// (departure 2).
pub const POWER_INFUSION_LATTICE_WIDTH: f32 = 0.07;

/// The onset: the first 1.334 s, its weight `sin(π t / 1.334)`.
pub const POWER_INFUSION_ONSET_SECS: f32 = 1.334;
/// The star glow (diameter, colour, alpha over the weight) and the streak
/// (`(width, height)`, colour, alpha).
pub const POWER_INFUSION_STAR: (f32, [u8; 3], f32) = (3.4, [235, 240, 255], 0.5);
pub const POWER_INFUSION_STREAK: (Vec2, [u8; 3], f32) = (Vec2::new(6.1, 0.3), [240, 245, 255], 0.9);

// ── Pain Suppression's tracks (seconds) ──────────────────────────────────────

/// The shell's loop.
pub const SUPPRESSION_LOOP_SECS: f32 = 0.867;
/// Its height squash over each loop (the onset's too).
pub const SUPPRESSION_SQUASH: [(f32, f32); 4] =
    [(0.0, 0.52), (0.3, 1.09), (0.6, 0.9), (0.867, 1.0)];
/// Its weight over the onset (the first loop)...
pub const SUPPRESSION_ONSET: [(f32, f32); 3] = [(0.0, 0.0), (0.467, 1.0), (0.867, 0.14)];
/// ...and over every loop after it.
pub const SUPPRESSION_FLICKER: [(f32, f32); 5] = [
    (0.0, 0.05),
    (0.217, 0.4),
    (0.433, 0.1),
    (0.65, 0.35),
    (0.867, 0.05),
];
/// Each quad's alpha over the weight.
pub const SUPPRESSION_SHELL_ALPHA: f32 = 0.55;
/// The three quads: `(width, height)` yards and colour.
pub const SUPPRESSION_TEAL: (Vec2, [u8; 3]) = (Vec2::new(3.4, 4.4), [34, 236, 219]);
pub const SUPPRESSION_YELLOW: (Vec2, [u8; 3]) = (Vec2::new(2.4, 3.1), [254, 240, 81]);
pub const SUPPRESSION_GOLD: (Vec2, [u8; 3]) = (Vec2::new(2.3, 2.9), [212, 176, 34]);

/// The cast swirl at the caster's feet: its life (the model's 1067 ms
/// sequence; the bench draws it while `ms < 1067`)...
pub const SUPPRESSION_SWIRL_SECS: f32 = 1.067;
/// ...its ring's diameter, 2.4 -> 1.4 yd over 833 ms...
pub const SUPPRESSION_SWIRL_DIAMETER: [(f32, f32); 2] = [(0.0, 2.4), (0.833, 1.4)];
/// ...the weight both pieces share, in by 333 ms and out by 833 ms...
pub const SUPPRESSION_SWIRL_WEIGHT: [(f32, f32); 3] = [(0.0, 0.0), (0.333, 1.0), (0.833, 0.0)];
/// ...the ring's colour, orange -> gold -> violet...
pub const SUPPRESSION_SWIRL_RGB: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 132, 0]),
    (0.333, [255, 216, 0]),
    (0.833, [160, 80, 255]),
];
/// ...and the heights (yd above the feet) of the ring and of the ground glow
/// beneath it, whose radius, colour and alpha over the weight follow.
pub const SUPPRESSION_SWIRL_RING_LIFT: f32 = 0.04;
pub const SUPPRESSION_SWIRL_GLOW_LIFT: f32 = 0.03;
pub const SUPPRESSION_SWIRL_GLOW: (f32, [u8; 3], f32) = (1.05, [255, 200, 60], 0.5);

// ── The emitters ─────────────────────────────────────────────────────────────

/// One of the three particle emitters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum StateLayer {
    /// Power Infusion's white sparks.
    InfusionSparks,
    /// Pain Suppression's swelling bubbles.
    SuppressionBubbles,
    /// Pain Suppression's inward light streaks.
    SuppressionStreaks,
}

impl StateLayer {
    pub const ALL: [StateLayer; 3] = [
        StateLayer::InfusionSparks,
        StateLayer::SuppressionBubbles,
        StateLayer::SuppressionStreaks,
    ];

    /// The layer's index in [`StateLayer::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its record.
    pub fn emitter(self) -> &'static StateEmitter {
        &STATE_EMITTERS[self.index()]
    }

    /// The state it belongs to.
    pub fn state(self) -> DisciplineState {
        match self {
            StateLayer::InfusionSparks => DisciplineState::InfusionCrackle,
            StateLayer::SuppressionBubbles | StateLayer::SuppressionStreaks => {
                DisciplineState::SuppressionShell
            }
        }
    }
}

/// Where an emitter's particles start, and which way they go.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StateLaunch {
    /// Out along a random point of the cube `[-1, 1]³` normalised (the
    /// bench's `vRange` 3.14), from a random point of a box `across` wide and
    /// `tall` high round the anchor (`area`, `areaY`).
    Burst { across: f32, tall: f32 },
    /// In through the anchor from a shell of radius `shell` × 0.6-1.0, the
    /// directions stretched `flat` up and down (`inward`, `inwardFlat`).
    Inward { shell: f32, flat: f32 },
}

/// What a piece's sprite is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateSprite {
    /// The camera-facing soft glow panel (`bbGlow`).
    Panel,
    /// A particle's soft glow (`sprite('glow')`).
    Glow,
    /// A four-point star (`sprite('star')`).
    Star,
    /// A stroked line (the lattice, a streak's tail): a soft band.
    Band,
    /// A thin level ring (`ring3`).
    Ring,
    /// A soft level disc (`groundDisc`).
    Ground,
}

/// One emitter's record, as the bench declares it. The tracks are keyed over
/// the particle's life, 0..1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StateEmitter {
    pub layer: StateLayer,
    /// The bench's layer toggle.
    pub on: bool,
    pub rate: f32,
    pub life: f32,
    /// Speed, yd/s: out along the direction, or in for an inward launch.
    pub speed: f32,
    pub launch: StateLaunch,
    pub rgb: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// The bench's `scale`: the sprite's RADIUS, yards.
    pub radius: &'static [(f32, f32)],
    pub sprite: StateSprite,
    /// The bench's `tail` (0: none).
    pub tail: f32,
    /// The state's size and opacity knobs.
    pub size_mul: f32,
    pub opacity_mul: f32,
}

/// The three emitters, in [`StateLayer::ALL`] order.
pub const STATE_EMITTERS: [StateEmitter; 3] = [
    // `{anchor:'chest', vRange:3.14, speed:3.3, life:0.2, rate:60,
    // color:[[0,white],[1,(200,220,255)]], alpha:[[0,1],[1,0]],
    // scale:[[0,0.42],[0.5,0.28],[1,0.03]], kind:'star', follow:true}`.
    StateEmitter {
        layer: StateLayer::InfusionSparks,
        on: POWER_INFUSION_SPARKS,
        rate: 60.0,
        life: 0.2,
        speed: 3.3,
        launch: StateLaunch::Burst {
            across: 0.0,
            tall: 0.0,
        },
        rgb: &[(0.0, [255, 255, 255]), (1.0, [200, 220, 255])],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        radius: &[(0.0, 0.42), (0.5, 0.28), (1.0, 0.03)],
        sprite: StateSprite::Star,
        tail: 0.0,
        size_mul: POWER_INFUSION_SIZE_MUL,
        opacity_mul: POWER_INFUSION_OPACITY_MUL,
    },
    // `{anchor:'feet', pos:[0,0,1.3], vRange:3.14, speed:0.15, life:1.6,
    // rate:10, area:1.0, areaY:1.2, color:[[0,(255,250,200)],[1,(180,240,230)]],
    // alpha:[[0,0.61],[1,0]], scale:[[0,0.2],[1,1.1]], kind:'glow',
    // follow:true}`.
    StateEmitter {
        layer: StateLayer::SuppressionBubbles,
        on: PAIN_SUPPRESSION_BUBBLES,
        rate: 10.0,
        life: 1.6,
        speed: 0.15,
        launch: StateLaunch::Burst {
            across: 1.0,
            tall: 1.2,
        },
        rgb: &[(0.0, [255, 250, 200]), (1.0, [180, 240, 230])],
        alpha: &[(0.0, 0.61), (1.0, 0.0)],
        radius: &[(0.0, 0.2), (1.0, 1.1)],
        sprite: StateSprite::Glow,
        tail: 0.0,
        size_mul: PAIN_SUPPRESSION_SIZE_MUL,
        opacity_mul: PAIN_SUPPRESSION_OPACITY_MUL,
    },
    // `{anchor:'feet', pos:[0,0,1.3], inward:2.1, inwardFlat:1.3, speed:-4.7,
    // life:0.45, rate:100, tail:2, color:[[0,(255,240,120)],[0.5,(80,230,230)],
    // [1,(60,120,255)]], alpha:[[0,0],[0.5,0.9],[1,0]],
    // scale:[[0,0.05],[1,0.03]], kind:'glow', follow:true}`.
    StateEmitter {
        layer: StateLayer::SuppressionStreaks,
        on: PAIN_SUPPRESSION_STREAKS,
        rate: 100.0,
        life: 0.45,
        speed: 4.7,
        launch: StateLaunch::Inward {
            shell: 2.1,
            flat: 1.3,
        },
        rgb: &[
            (0.0, [255, 240, 120]),
            (0.5, [80, 230, 230]),
            (1.0, [60, 120, 255]),
        ],
        alpha: &[(0.0, 0.0), (0.5, 0.9), (1.0, 0.0)],
        radius: &[(0.0, 0.05), (1.0, 0.03)],
        sprite: StateSprite::Glow,
        tail: 2.0,
        size_mul: PAIN_SUPPRESSION_SIZE_MUL,
        opacity_mul: PAIN_SUPPRESSION_OPACITY_MUL,
    },
];

/// A shell launch's inner radius over its shell (the bench's `0.6 + 0.4 rnd`).
pub const STATE_SHELL_INNER: f32 = 0.6;
/// A tail is drawn only above this speed, yd/s (the bench's `sp > 0.2`).
pub const STATE_TAIL_MIN_SPEED: f32 = 0.2;
/// A tail's length: `min(tail × 0.12, speed × 0.06)` × size...
pub const STATE_TAIL_PER_TAIL: f32 = 0.12;
pub const STATE_TAIL_PER_SPEED: f32 = 0.06;
/// ...its alpha over the sprite's, and its width over the sprite's radius.
pub const STATE_TAIL_ALPHA: f32 = 0.85;
pub const STATE_TAIL_WIDTH: f32 = 0.7;

/// The longest frame a pool is sized to absorb without a birth taking a live
/// particle's slot.
pub const STATE_MAX_FRAME_SECS: f32 = 0.1;

/// Pieces an emitter's pool holds: more than it can hold alive at once (its
/// rate times its life and one frame, plus a birth's rounding).
pub fn state_slots(e: &StateEmitter) -> usize {
    (e.rate * (e.life + STATE_MAX_FRAME_SECS)).ceil() as usize + 1
}

// ── The states and their pieces ──────────────────────────────────────────────

/// Which of the two states a rig is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum DisciplineState {
    /// Power Infusion's crackle at the ally's chest.
    InfusionCrackle,
    /// Pain Suppression's shell in front of the ally.
    SuppressionShell,
}

impl DisciplineState {
    pub const ALL: [DisciplineState; 2] = [
        DisciplineState::InfusionCrackle,
        DisciplineState::SuppressionShell,
    ];

    /// The ability whose aura it draws (and whose hands it flashes).
    pub fn ability(self) -> AbilityType {
        match self {
            DisciplineState::InfusionCrackle => AbilityType::PowerInfusion,
            DisciplineState::SuppressionShell => AbilityType::PainSuppression,
        }
    }

    /// Whether `aura` is this state's buff.
    pub fn holds(self, aura: &Aura) -> bool {
        match self {
            DisciplineState::InfusionCrackle => {
                is_power_infusion(aura.effect_type, &aura.ability_name)
            }
            DisciplineState::SuppressionShell => {
                is_pain_suppression(aura.effect_type, &aura.ability_name)
            }
        }
    }

    /// Where the state stands, yards above the bearer's feet.
    pub fn height(self) -> f32 {
        match self {
            DisciplineState::InfusionCrackle => POWER_INFUSION_CHEST,
            DisciplineState::SuppressionShell => SUPPRESSION_HEIGHT,
        }
    }
}

/// One of the shell's three quads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum ShellQuad {
    Teal,
    Yellow,
    Gold,
}

impl ShellQuad {
    pub const ALL: [ShellQuad; 3] = [ShellQuad::Teal, ShellQuad::Yellow, ShellQuad::Gold];

    fn look(self) -> (Vec2, [u8; 3]) {
        match self {
            ShellQuad::Teal => SUPPRESSION_TEAL,
            ShellQuad::Yellow => SUPPRESSION_YELLOW,
            ShellQuad::Gold => SUPPRESSION_GOLD,
        }
    }
}

/// One drawn piece of a state or of a swirl.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum StatePiece {
    /// Power Infusion's 4.6 yd disc.
    WideDisc,
    /// Its 3.2 yd disc.
    MidDisc,
    /// Its steady 2.2 yd disc.
    CoreDisc,
    /// One segment of its lattice: from point `segment` to `segment + 1` of
    /// `strand`.
    Lattice { strand: u8, segment: u8 },
    /// Its onset's star glow.
    OnsetStar,
    /// Its onset's horizontal streak.
    OnsetStreak,
    /// One of Pain Suppression's shell quads.
    Shell(ShellQuad),
    /// One slot of an emitter's pool.
    Particle { layer: StateLayer, slot: u16 },
    /// The tail of the particle in one slot of a tailed emitter.
    Tail { layer: StateLayer, slot: u16 },
    /// The swirl's contracting ring.
    SwirlRing,
    /// The swirl's ground glow.
    SwirlGlow,
}

impl StatePiece {
    /// The sprite it is drawn with.
    pub fn sprite(self) -> StateSprite {
        match self {
            StatePiece::WideDisc
            | StatePiece::MidDisc
            | StatePiece::CoreDisc
            | StatePiece::OnsetStar
            | StatePiece::OnsetStreak
            | StatePiece::Shell(_) => StateSprite::Panel,
            StatePiece::Lattice { .. } | StatePiece::Tail { .. } => StateSprite::Band,
            StatePiece::Particle { layer, .. } => layer.emitter().sprite,
            StatePiece::SwirlRing => StateSprite::Ring,
            StatePiece::SwirlGlow => StateSprite::Ground,
        }
    }

    /// How it composites: every call the bench makes for these two tabs is
    /// `lighter` (see the module docs).
    pub fn alpha_mode(self) -> AlphaMode {
        AlphaMode::Add
    }
}

/// The pieces a state draws, by its layer toggles: its fixed pieces, then
/// every slot of its emitters' pools (and their tails).
pub fn state_pieces(state: DisciplineState) -> Vec<StatePiece> {
    let mut pieces = Vec::new();
    match state {
        DisciplineState::InfusionCrackle => {
            if POWER_INFUSION_DISCS {
                pieces.extend([
                    StatePiece::WideDisc,
                    StatePiece::MidDisc,
                    StatePiece::CoreDisc,
                ]);
            }
            if POWER_INFUSION_LATTICE {
                for strand in 0..POWER_INFUSION_STRANDS {
                    for segment in 0..POWER_INFUSION_SEGMENTS {
                        pieces.push(StatePiece::Lattice { strand, segment });
                    }
                }
            }
            if POWER_INFUSION_ONSET_FLASH {
                pieces.extend([StatePiece::OnsetStar, StatePiece::OnsetStreak]);
            }
        }
        DisciplineState::SuppressionShell => {
            if PAIN_SUPPRESSION_SHELL {
                pieces.extend(ShellQuad::ALL.map(StatePiece::Shell));
            }
        }
    }
    for layer in StateLayer::ALL {
        let e = layer.emitter();
        if layer.state() != state || !e.on {
            continue;
        }
        for slot in 0..state_slots(e) as u16 {
            pieces.push(StatePiece::Particle { layer, slot });
            if e.tail > 0.0 {
                pieces.push(StatePiece::Tail { layer, slot });
            }
        }
    }
    pieces
}

/// The swirl's pieces.
pub fn swirl_pieces() -> Vec<StatePiece> {
    if PAIN_SUPPRESSION_CAST_SWIRL {
        vec![StatePiece::SwirlRing, StatePiece::SwirlGlow]
    } else {
        Vec::new()
    }
}

// ── The poses ────────────────────────────────────────────────────────────────

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
pub fn state_track_rgb(track: &[(f32, [u8; 3])], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// Power Infusion's throbbing discs' weight `age` seconds in.
pub fn infusion_throb(age: f32) -> f32 {
    keyed(
        &POWER_INFUSION_THROB,
        age.max(0.0).rem_euclid(POWER_INFUSION_THROB_SECS),
    )
}

/// The widest disc's size pulse `age` seconds in.
pub fn infusion_pulse(age: f32) -> f32 {
    keyed(
        &POWER_INFUSION_PULSE,
        age.max(0.0).rem_euclid(POWER_INFUSION_PULSE_SECS),
    )
}

/// The lattice's flicker `age` seconds in (before its floor).
pub fn infusion_flicker(age: f32) -> f32 {
    keyed(
        &POWER_INFUSION_FLICKER,
        age.max(0.0).rem_euclid(POWER_INFUSION_FLICKER_SECS),
    )
}

/// The onset's weight `age` seconds in: `sin(π t / 1.334)` inside it, 0
/// after.
pub fn infusion_onset(age: f32) -> f32 {
    if (0.0..POWER_INFUSION_ONSET_SECS).contains(&age) {
        (age / POWER_INFUSION_ONSET_SECS * PI).sin()
    } else {
        0.0
    }
}

/// Point `point` (0..=5) of lattice strand `strand`, `age` seconds in, from
/// the chest.
pub fn lattice_point(strand: u8, point: u8, age: f32) -> Vec3 {
    let jitter = (age.max(0.0) / POWER_INFUSION_JITTER_SECS).floor();
    let (k, j) = (strand as f32, point as f32);
    let a = k * POWER_INFUSION_STRAND_TURN
        + j * POWER_INFUSION_POINT_TURN
        + jitter * POWER_INFUSION_JITTER_TURN;
    let (fixed, swing) = POWER_INFUSION_LATTICE_SPREAD;
    let r = POWER_INFUSION_LATTICE_RADIUS * (fixed + swing * (a * 3.0 + j).sin());
    let (ax, az) = POWER_INFUSION_LATTICE_AXES;
    Vec3::new(
        r * (a * ax).cos(),
        (j / POWER_INFUSION_SEGMENTS as f32 - 0.5) * POWER_INFUSION_LATTICE_TALL,
        r * (a * az).sin(),
    ) * POWER_INFUSION_SIZE_MUL
}

/// The shell's height squash `age` seconds in.
pub fn suppression_squash(age: f32) -> f32 {
    keyed(
        &SUPPRESSION_SQUASH,
        age.max(0.0).rem_euclid(SUPPRESSION_LOOP_SECS),
    )
}

/// The shell's weight `age` seconds in: the onset's flash over the first
/// loop, the flicker after.
pub fn suppression_weight(age: f32) -> f32 {
    if age < SUPPRESSION_LOOP_SECS {
        keyed(&SUPPRESSION_ONSET, age.max(0.0))
    } else {
        keyed(&SUPPRESSION_FLICKER, age.rem_euclid(SUPPRESSION_LOOP_SECS))
    }
}

/// A fixed piece's pose `age` seconds after its state's onset (a swirl's:
/// after its cast), from the rig: its place, size (a quad's width and
/// height; a lattice segment's 3D length and its width), facing, colour and
/// alpha, the knobs applied. `forward` is the bearer's horizontal facing (the
/// shell stands along it). Pure: the probes and the placement read one
/// definition. `None` for a particle or a tail, which [`state_particle_pose`]
/// poses.
pub fn state_piece_pose(piece: StatePiece, age: f32, forward: Vec3) -> Option<PiecePose> {
    let panel = |at: Vec3, size: Vec2, rgb: [u8; 3], alpha: f32| PiecePose {
        at,
        size,
        facing: PieceFacing::Camera,
        rgb,
        alpha,
    };
    let pi_size = POWER_INFUSION_SIZE_MUL;
    let pi_op = POWER_INFUSION_OPACITY_MUL;
    let ps_size = PAIN_SUPPRESSION_SIZE_MUL;
    let ps_op = PAIN_SUPPRESSION_OPACITY_MUL;
    let pose = match piece {
        StatePiece::WideDisc => {
            let (d, rgb, gain) = POWER_INFUSION_WIDE_DISC;
            let (fixed, share) = POWER_INFUSION_PULSE_SHARE;
            let d = d * pi_size * (fixed + share * infusion_pulse(age));
            panel(
                Vec3::ZERO,
                Vec2::splat(d),
                rgb,
                infusion_throb(age) * gain * pi_op,
            )
        }
        StatePiece::MidDisc => {
            let (d, rgb, gain) = POWER_INFUSION_MID_DISC;
            panel(
                Vec3::ZERO,
                Vec2::splat(d * pi_size),
                rgb,
                infusion_throb(age) * gain * pi_op,
            )
        }
        StatePiece::CoreDisc => {
            let (d, rgb, alpha) = POWER_INFUSION_CORE_DISC;
            panel(Vec3::ZERO, Vec2::splat(d * pi_size), rgb, alpha * pi_op)
        }
        StatePiece::Lattice { strand, segment } => {
            let a = lattice_point(strand, segment, age);
            let b = lattice_point(strand, segment + 1, age);
            let f = infusion_flicker(age);
            PiecePose {
                at: (a + b) * 0.5,
                size: Vec2::new((b - a).length(), POWER_INFUSION_LATTICE_WIDTH),
                facing: PieceFacing::Camera,
                rgb: POWER_INFUSION_LATTICE_RGB,
                alpha: if f > POWER_INFUSION_LATTICE_FLOOR {
                    f * pi_op
                } else {
                    0.0
                },
            }
        }
        StatePiece::OnsetStar => {
            let (d, rgb, gain) = POWER_INFUSION_STAR;
            panel(
                Vec3::ZERO,
                Vec2::splat(d * pi_size),
                rgb,
                infusion_onset(age) * gain * pi_op,
            )
        }
        StatePiece::OnsetStreak => {
            let (size, rgb, gain) = POWER_INFUSION_STREAK;
            panel(
                Vec3::ZERO,
                size * pi_size,
                rgb,
                infusion_onset(age) * gain * pi_op,
            )
        }
        StatePiece::Shell(quad) => {
            let (size, rgb) = quad.look();
            let ahead = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
            panel(
                ahead * SUPPRESSION_SHELL_FORWARD * ps_size,
                Vec2::new(size.x, size.y * suppression_squash(age)) * ps_size,
                rgb,
                suppression_weight(age) * SUPPRESSION_SHELL_ALPHA * ps_op,
            )
        }
        StatePiece::SwirlRing => {
            let radius = keyed(&SUPPRESSION_SWIRL_DIAMETER, age) * 0.5 * ps_size;
            PiecePose {
                at: Vec3::Y * SUPPRESSION_SWIRL_RING_LIFT,
                // The ring sprite's stroke runs at RING_STROKE.0 of the
                // quad's half-width, so the quad is sized to put it at
                // `radius`.
                size: Vec2::splat(2.0 * radius / RING_STROKE.0),
                facing: PieceFacing::Flat { turn: 0.0 },
                rgb: state_track_rgb(&SUPPRESSION_SWIRL_RGB, age),
                alpha: swirl_weight(age) * ps_op,
            }
        }
        StatePiece::SwirlGlow => {
            let (radius, rgb, gain) = SUPPRESSION_SWIRL_GLOW;
            PiecePose {
                at: Vec3::Y * SUPPRESSION_SWIRL_GLOW_LIFT,
                size: Vec2::splat(2.0 * radius * ps_size),
                facing: PieceFacing::Flat { turn: 0.0 },
                rgb,
                alpha: swirl_weight(age) * gain * ps_op,
            }
        }
        StatePiece::Particle { .. } | StatePiece::Tail { .. } => return None,
    };
    Some(pose)
}

/// The swirl's weight `age` seconds after its cast (nothing past its life).
pub fn swirl_weight(age: f32) -> f32 {
    if (0.0..SUPPRESSION_SWIRL_SECS).contains(&age) {
        keyed(&SUPPRESSION_SWIRL_WEIGHT, age)
    } else {
        0.0
    }
}

/// The bench's scatter for particle `j` of `layer`, draw `k`: 0..1.
fn scatter(layer: StateLayer, j: u32, k: u32) -> f32 {
    bench_hash(
        j.wrapping_mul(3)
            .wrapping_add(layer.index() as u32)
            .wrapping_mul(8)
            .wrapping_add(k),
    )
}

/// Where particle `j` of `layer` is born, from its rig, and its velocity.
pub fn state_launch(layer: StateLayer, j: u32) -> (Vec3, Vec3) {
    let e = layer.emitter();
    let h = |k: u32| scatter(layer, j, k);
    let s = e.size_mul;
    match e.launch {
        StateLaunch::Burst { across, tall } => {
            let dir = Vec3::new(h(1) * 2.0 - 1.0, h(2) * 2.0 - 1.0, h(3) * 2.0 - 1.0)
                .try_normalize()
                .unwrap_or(Vec3::Y);
            let at = Vec3::new(
                (h(4) - 0.5) * across,
                (h(5) - 0.5) * tall,
                (h(6) - 0.5) * across,
            );
            (at * s, dir * e.speed * s)
        }
        StateLaunch::Inward { shell, flat } => {
            let dir = Vec3::new(
                h(1) * 2.0 - 1.0,
                (h(2) * 2.0 - 1.0) * flat,
                h(3) * 2.0 - 1.0,
            )
            .try_normalize()
            .unwrap_or(Vec3::Y);
            let r = shell * (STATE_SHELL_INNER + (1.0 - STATE_SHELL_INNER) * h(4));
            (dir * r * s, -dir * e.speed * s)
        }
    }
}

/// One particle's look at a moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ParticlePose {
    /// From its rig.
    pub at: Vec3,
    pub velocity: Vec3,
    /// The sprite's full width (twice the bench's radius).
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    /// The sprite's roll in the camera plane (the bench's `ang`).
    pub roll: f32,
}

/// Particle `j` of `layer`, `age` seconds after its birth; `None` once its
/// life is spent. Pure: the probes and the placement read one definition.
pub fn state_particle_pose(layer: StateLayer, j: u32, age: f32) -> Option<ParticlePose> {
    let e = layer.emitter();
    if !(0.0..e.life).contains(&age) {
        return None;
    }
    let t = age / e.life;
    let (at, velocity) = state_launch(layer, j);
    Some(ParticlePose {
        at: at + velocity * age,
        velocity,
        size: 2.0 * keyed(e.radius, t) * e.size_mul,
        rgb: state_track_rgb(e.rgb, t),
        alpha: keyed(e.alpha, t) * e.opacity_mul,
        // The canvas turns a sprite clockwise on screen.
        roll: -scatter(layer, j, 7) * TAU,
    })
}

/// A tailed particle's tail length, yards (the bench's
/// `min(tail × 0.12, speed × 0.06) × size`), or `None` when it draws none.
pub fn state_tail_length(e: &StateEmitter, speed: f32) -> Option<f32> {
    (e.tail > 0.0 && speed > STATE_TAIL_MIN_SPEED)
        .then(|| (e.tail * STATE_TAIL_PER_TAIL).min(speed * STATE_TAIL_PER_SPEED) * e.size_mul)
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Whether `ability`'s hands flash at its state's onset rather than off a
/// cast marker, so `heal_cast.rs` leaves a `Landed` marker naming it alone.
pub fn discipline_hands_at_onset(ability: AbilityType) -> bool {
    matches!(
        ability,
        AbilityType::PowerInfusion | AbilityType::PainSuppression
    )
}

/// The `state`'s buff a unit holds, if any: its remaining seconds and its
/// caster. A unit holds one at most (refresh-only stacking keys it by its
/// source, and its scope is the target).
pub fn discipline_state_held(
    state: DisciplineState,
    auras: &ActiveAuras,
) -> Option<(f32, Option<Entity>)> {
    auras
        .auras
        .iter()
        .find(|a| state.holds(a))
        .map(|a| (a.duration, a.caster))
}

// ── Components ───────────────────────────────────────────────────────────────

/// One particle in a pool slot: its birth index and the rig's age at its
/// birth.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StateBirth {
    pub j: u32,
    pub born: f32,
}

/// A remaining time that rises by more than this between two reads is a
/// refresh (the buff only ever runs down otherwise).
pub const STATE_REFRESH_EPS: f32 = 1e-3;

/// One bearer's state: a rig at its anchor, parent of the pieces. Lives
/// exactly as long as the buff is on a living bearer.
#[derive(Component, Debug)]
pub struct DisciplineStateRig {
    pub state: DisciplineState,
    pub bearer: Entity,
    /// Seconds since the state rose (a refresh does not reset it).
    pub age: f32,
    /// The buff's remaining seconds, as last read.
    pub remaining: f32,
    /// Particles owed per emitter, carried between frames (the bench's
    /// `acc`).
    pub carry: [f32; 3],
    /// Particles born per emitter: the next birth's index.
    pub emitted: [u32; 3],
    /// Each emitter's pool: the particle in each slot (empty for another
    /// state's emitters).
    pub slots: [Vec<Option<StateBirth>>; 3],
}

impl DisciplineStateRig {
    pub fn new(state: DisciplineState, bearer: Entity, remaining: f32) -> Self {
        Self {
            state,
            bearer,
            age: 0.0,
            remaining,
            carry: [0.0; 3],
            emitted: [0; 3],
            slots: StateLayer::ALL.map(|l| {
                let e = l.emitter();
                if l.state() == state && e.on {
                    vec![None; state_slots(e)]
                } else {
                    Vec::new()
                }
            }),
        }
    }

    /// Age the rig `dt` and emit what the frame owes (the bench's
    /// `Emitter.update`: each whole particle owed is born at the frame's
    /// start, so it is one frame old when first drawn).
    pub fn advance(&mut self, dt: f32, remaining: f32) {
        self.age += dt;
        self.remaining = remaining;
        for layer in StateLayer::ALL {
            let i = layer.index();
            if self.slots[i].is_empty() {
                continue;
            }
            self.carry[i] += layer.emitter().rate * dt;
            while self.carry[i] >= 1.0 {
                self.carry[i] -= 1.0;
                let j = self.emitted[i];
                let n = self.slots[i].len();
                self.slots[i][j as usize % n] = Some(StateBirth {
                    j,
                    born: self.age - dt,
                });
                self.emitted[i] += 1;
            }
        }
    }

    /// The particle in `slot` of `layer` and its own age, while it lives.
    pub fn particle(&self, layer: StateLayer, slot: usize) -> Option<(u32, f32)> {
        let birth = (*self.slots[layer.index()].get(slot)?)?;
        let age = self.age - birth.born;
        (age < layer.emitter().life).then_some((birth.j, age))
    }
}

/// Pain Suppression's cast swirl at its caster's feet: a rig parent of its
/// two pieces, for [`SUPPRESSION_SWIRL_SECS`].
#[derive(Component, Debug)]
pub struct SuppressionSwirl {
    pub caster: Entity,
    pub age: f32,
}

/// One piece of a [`DisciplineStateRig`] or a [`SuppressionSwirl`].
#[derive(Component, Debug)]
pub struct DisciplineStatePiece {
    pub piece: StatePiece,
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The quad and the sprites every state and swirl draws with, built once.
pub struct DisciplineStateAssets {
    quad: Handle<Mesh>,
    panel: Handle<Image>,
    glow: Handle<Image>,
    star: Handle<Image>,
    band: Handle<Image>,
    ring: Handle<Image>,
    ground: Handle<Image>,
}

impl DisciplineStateAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            panel: images.add(oneshot_glow_texture()),
            glow: images.add(particle_sprite_image(GLOW_SOFT)),
            star: images.add(star_flash_texture()),
            band: images.add(soft_band_texture()),
            ring: images.add(oneshot_ring_texture()),
            ground: images.add(oneshot_ground_texture()),
        }
    }

    fn texture(&self, sprite: StateSprite) -> Handle<Image> {
        match sprite {
            StateSprite::Panel => self.panel.clone(),
            StateSprite::Glow => self.glow.clone(),
            StateSprite::Star => self.star.clone(),
            StateSprite::Band => self.band.clone(),
            StateSprite::Ring => self.ring.clone(),
            StateSprite::Ground => self.ground.clone(),
        }
    }
}

/// Spawn `pieces` under `rig`, each on its own unlit additive material.
fn spawn_pieces(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &DisciplineStateAssets,
    rig: Entity,
    pieces: Vec<StatePiece>,
) {
    for piece in pieces {
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE.with_alpha(0.0),
            base_color_texture: Some(assets.texture(piece.sprite())),
            // Unlit: the bench draws colour × sprite × alpha, added, and
            // nothing else.
            unlit: true,
            alpha_mode: piece.alpha_mode(),
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        let child = commands
            .spawn((
                DisciplineStatePiece { piece },
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(1e-3)),
                NotShadowCaster,
            ))
            .id();
        commands.entity(rig).add_child(child);
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Update (graphical-only): raise a state on every living unit holding Power
/// Infusion or Pain Suppression and flash its caster's Holy hands (and, for
/// Pain Suppression, raise the swirl at its caster's feet); age each state
/// and emit what the frame owes, replaying the cast on a refresh; hide it
/// while its bearer is a sheep; take it down when its buff ends or its bearer
/// dies or is gone. Age each swirl and take it down after its life or with
/// its caster.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_discipline_states(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DisciplineStateAssets>>,
    mut hand_assets: Local<Option<HealCastAssets>>,
    bearers: Query<(Entity, &Combatant, &ActiveAuras, Has<PolymorphedVisual>)>,
    combatants: Query<&Combatant>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
    mut rigs: Query<(Entity, &mut DisciplineStateRig, &mut Visibility)>,
    mut swirls: Query<(Entity, &mut SuppressionSwirl)>,
) {
    let dt = time.delta_secs();
    let alive = |e: Entity| combatants.get(e).is_ok_and(|c| c.is_alive());
    let hidden = |sheep: bool| {
        if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    };

    for (entity, mut swirl) in swirls.iter_mut() {
        swirl.age += dt;
        if swirl.age >= SUPPRESSION_SWIRL_SECS || !alive(swirl.caster) {
            commands.entity(entity).despawn();
        }
    }

    // Casts to cue this frame: (state, caster).
    let mut cues: Vec<(DisciplineState, Entity)> = Vec::new();
    let mut worn: Vec<(Entity, DisciplineState)> = Vec::new();
    for (entity, mut rig, mut visibility) in rigs.iter_mut() {
        let held = bearers
            .get(rig.bearer)
            .ok()
            .filter(|(_, c, _, _)| c.is_alive())
            .and_then(|(_, _, auras, sheep)| {
                discipline_state_held(rig.state, auras).map(|h| (h, sheep))
            });
        let Some(((remaining, caster), sheep)) = held else {
            // Run out, purged, dispelled, dead or gone: nothing stays.
            commands.entity(entity).despawn();
            continue;
        };
        if remaining > rig.remaining + STATE_REFRESH_EPS {
            if let Some(caster) = caster {
                cues.push((rig.state, caster));
            }
        }
        rig.advance(dt, remaining);
        visibility.set_if_neq(hidden(sheep));
        worn.push((rig.bearer, rig.state));
    }

    for (entity, combatant, auras, sheep) in bearers.iter() {
        if !combatant.is_alive() {
            continue;
        }
        for state in DisciplineState::ALL {
            if worn.contains(&(entity, state)) {
                continue;
            }
            let Some((remaining, caster)) = discipline_state_held(state, auras) else {
                continue;
            };
            let assets = assets
                .get_or_insert_with(|| DisciplineStateAssets::build(&mut meshes, &mut images));
            let rig = commands
                .spawn((
                    DisciplineStateRig::new(state, entity, remaining),
                    Transform::default(),
                    hidden(sheep),
                    PlayMatchEntity,
                ))
                .id();
            spawn_pieces(
                &mut commands,
                &mut materials,
                assets,
                rig,
                state_pieces(state),
            );
            if let Some(caster) = caster {
                cues.push((state, caster));
            }
        }
    }

    for (state, caster) in cues {
        flash_instant_hands(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut hand_assets,
            &casters,
            &bodies,
            caster,
            state.ability(),
        );
        if state == DisciplineState::SuppressionShell
            && PAIN_SUPPRESSION_CAST_SWIRL
            && alive(caster)
        {
            let assets = assets
                .get_or_insert_with(|| DisciplineStateAssets::build(&mut meshes, &mut images));
            let swirl = commands
                .spawn((
                    SuppressionSwirl { caster, age: 0.0 },
                    Transform::default(),
                    Visibility::default(),
                    PlayMatchEntity,
                ))
                .id();
            spawn_pieces(&mut commands, &mut materials, assets, swirl, swirl_pieces());
        }
    }
}

/// The centre of a unit's rendered body: its `VisualBody`'s translation
/// (`body_local`) under the unit.
pub fn discipline_body_centre(unit: &Transform, body_local: Vec3) -> Vec3 {
    unit.transform_point(body_local)
}

/// Where a rig stands, `height` yards above the feet of a rendered body
/// centred at `centre`.
pub fn discipline_anchor(centre: Vec3, height: f32) -> Vec3 {
    centre + Vec3::Y * (height - BODY_CENTRE_ABOVE_FLOOR)
}

/// A combatant's facing, level: it faces its local +Z.
fn facing(unit: &Transform) -> Vec3 {
    let f = unit.rotation * Vec3::Z;
    Vec3::new(f.x, 0.0, f.z).normalize_or_zero()
}

/// The camera, as the hold and the billboards need it.
#[derive(Clone, Copy)]
struct View {
    eye: Vec3,
    forward: Vec3,
    rotation: Quat,
}

/// Where a piece truly at `at` is drawn and the factor its quad is scaled
/// by: held in front of the body `capsule` for the camera (no camera: where
/// it is).
fn held(at: Vec3, capsule: (Vec3, Vec3, f32), view: Option<View>) -> (Vec3, f32) {
    match view {
        Some(v) => siphon_drawn_point(at, capsule, v.eye, v.forward),
        None => (at, 1.0),
    }
}

/// The quad that draws the segment `a`-`b` as the bench strokes it, a line
/// between the two points' projections: its centre, its rotation and its
/// length. It lies in the camera's plane at the depth of the segment's
/// middle, each end where the camera's ray to that end meets the plane, so
/// both ends project exactly where the bench's do. No camera: the segment
/// itself, unturned.
fn screen_segment(a: Vec3, b: Vec3, view: Option<View>) -> (Vec3, Quat, f32) {
    let Some(v) = view else {
        return ((a + b) * 0.5, Quat::IDENTITY, (b - a).length());
    };
    let depth = |p: Vec3| (p - v.eye).dot(v.forward);
    let mid = depth((a + b) * 0.5);
    let onto = |p: Vec3| {
        let d = depth(p);
        if d > 1e-4 {
            v.eye + (p - v.eye) * (mid / d)
        } else {
            p
        }
    };
    let (a, b) = (onto(a), onto(b));
    let along = b - a;
    let (right, up) = (v.rotation * Vec3::X, v.rotation * Vec3::Y);
    let roll = along.dot(up).atan2(along.dot(right));
    (
        (a + b) * 0.5,
        v.rotation * Quat::from_rotation_z(roll),
        along.length(),
    )
}

/// Pose one piece of a rig standing at `anchor`, `age` seconds in, held in
/// front of `capsule`. Returns its rig-local translation, rotation, scale,
/// colour and alpha.
#[allow(clippy::too_many_arguments)]
fn place_piece(
    piece: StatePiece,
    rig: Option<&DisciplineStateRig>,
    age: f32,
    anchor: Vec3,
    forward: Vec3,
    capsule: (Vec3, Vec3, f32),
    view: Option<View>,
) -> Option<(Transform, [u8; 3], f32)> {
    let cam = view.map(|v| v.rotation);
    match piece {
        StatePiece::Particle { layer, slot } => {
            let (j, page) = rig?.particle(layer, slot as usize)?;
            let p = state_particle_pose(layer, j, page)?;
            let (drawn, k) = held(anchor + p.at, capsule, view);
            let rotation = match cam {
                Some(c) => c * Quat::from_rotation_z(p.roll),
                None => Quat::IDENTITY,
            };
            Some((
                Transform {
                    translation: drawn - anchor,
                    rotation,
                    scale: Vec3::splat(p.size * k),
                },
                p.rgb,
                p.alpha,
            ))
        }
        StatePiece::Tail { layer, slot } => {
            let e = layer.emitter();
            let (j, page) = rig?.particle(layer, slot as usize)?;
            let p = state_particle_pose(layer, j, page)?;
            let speed = p.velocity.length();
            let length = state_tail_length(e, speed)?;
            // The bench's tail: a line from the particle back along its
            // flight.
            let at = anchor + p.at;
            let (centre, rotation, projected) =
                screen_segment(at, at - p.velocity / speed * length, view);
            let (drawn, k) = held(centre, capsule, view);
            Some((
                Transform {
                    translation: drawn - anchor,
                    rotation,
                    scale: Vec3::new(projected * k, p.size * 0.5 * STATE_TAIL_WIDTH * k, 1.0),
                },
                p.rgb,
                p.alpha * STATE_TAIL_ALPHA,
            ))
        }
        StatePiece::Lattice { strand, segment } => {
            let pose = state_piece_pose(piece, age, forward)?;
            let (centre, rotation, projected) = screen_segment(
                anchor + lattice_point(strand, segment, age),
                anchor + lattice_point(strand, segment + 1, age),
                view,
            );
            let (drawn, k) = held(centre, capsule, view);
            Some((
                Transform {
                    translation: drawn - anchor,
                    rotation,
                    scale: Vec3::new(projected * k, pose.size.y * k, 1.0),
                },
                pose.rgb,
                pose.alpha,
            ))
        }
        _ => {
            let pose = state_piece_pose(piece, age, forward)?;
            let (drawn, k) = held(anchor + pose.at, capsule, view);
            Some((
                Transform {
                    translation: drawn - anchor,
                    rotation: piece_rotation(pose.facing, Quat::IDENTITY, cam),
                    scale: (pose.size * k).extend(1.0),
                },
                pose.rgb,
                pose.alpha,
            ))
        }
    }
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each state on its bearer's rendered body and
/// each swirl at its caster's rendered feet, and pose, turn, hold and colour
/// every piece. The rigs are unrotated, so a piece's local pose is its world
/// offset.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn place_discipline_states(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<DisciplineStateRig>,
            Without<SuppressionSwirl>,
            Without<DisciplineStatePiece>,
        ),
    >,
    mut rigs: Query<
        (&DisciplineStateRig, &mut Transform, &Children),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<SuppressionSwirl>,
            Without<DisciplineStatePiece>,
        ),
    >,
    mut swirls: Query<
        (&SuppressionSwirl, &mut Transform, &Children),
        (
            Without<Combatant>,
            Without<VisualBody>,
            Without<DisciplineStateRig>,
            Without<DisciplineStatePiece>,
        ),
    >,
    units: Query<
        (&Transform, Option<&Children>),
        (
            With<Combatant>,
            Without<DisciplineStateRig>,
            Without<SuppressionSwirl>,
            Without<DisciplineStatePiece>,
        ),
    >,
    bodies: Query<
        &Transform,
        (
            With<VisualBody>,
            Without<Combatant>,
            Without<DisciplineStateRig>,
            Without<SuppressionSwirl>,
            Without<DisciplineStatePiece>,
        ),
    >,
    mut pieces: Query<
        (
            &DisciplineStatePiece,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (
            Without<DisciplineStateRig>,
            Without<SuppressionSwirl>,
            Without<Combatant>,
            Without<VisualBody>,
        ),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = camera.iter().next().map(|c| View {
        eye: c.translation,
        forward: c.forward().as_vec3(),
        rotation: c.rotation,
    });
    let body_of = |unit: Entity| {
        let (transform, children) = units.get(unit).ok()?;
        let body_local = children
            .and_then(|kids| kids.iter().find_map(|k| bodies.get(k).ok()))
            .map(|body| body.translation)
            .unwrap_or(Vec3::ZERO);
        Some((
            discipline_body_centre(transform, body_local),
            facing(transform),
        ))
    };
    let mut pose_children = |rig: Option<&DisciplineStateRig>,
                             age: f32,
                             anchor: Vec3,
                             forward: Vec3,
                             capsule: (Vec3, Vec3, f32),
                             children: &Children| {
        for child in children.iter() {
            let Ok((piece, mut part, material)) = pieces.get_mut(child) else {
                continue;
            };
            let placed = place_piece(piece.piece, rig, age, anchor, forward, capsule, view);
            let (rgb, alpha) = match placed {
                Some((transform, rgb, alpha)) => {
                    part.translation = transform.translation;
                    part.rotation = transform.rotation;
                    part.scale = transform.scale.max(Vec3::splat(1e-3));
                    (rgb, alpha.clamp(0.0, 1.0))
                }
                None => {
                    part.scale = Vec3::splat(1e-3);
                    ([255, 255, 255], 0.0)
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
    };
    for (rig, mut transform, children) in rigs.iter_mut() {
        let Some((centre, forward)) = body_of(rig.bearer) else {
            continue;
        };
        let anchor = discipline_anchor(centre, rig.state.height());
        transform.translation = anchor;
        let capsule = siphon_body_capsule(centre, false);
        pose_children(Some(rig), rig.age, anchor, forward, capsule, children);
    }
    for (swirl, mut transform, children) in swirls.iter_mut() {
        let Some((centre, forward)) = body_of(swirl.caster) else {
            continue;
        };
        let anchor = discipline_anchor(centre, 0.0);
        transform.translation = anchor;
        let capsule = siphon_body_capsule(centre, false);
        pose_children(None, swirl.age, anchor, forward, capsule, children);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants block, as the signed-off bench exports it.
    #[test]
    fn the_constants_are_the_benchs() {
        assert_eq!(POWER_INFUSION_SIZE_MUL, 1.0);
        assert_eq!(POWER_INFUSION_OPACITY_MUL, 1.0);
        assert_eq!(PAIN_SUPPRESSION_SIZE_MUL, 1.0);
        assert_eq!(PAIN_SUPPRESSION_OPACITY_MUL, 1.0);
        assert_eq!(POWER_INFUSION_CHEST, 1.45);
    }

    /// A pool always outnumbers what its emitter can hold alive.
    #[test]
    fn every_pool_outnumbers_its_live_particles() {
        for layer in StateLayer::ALL {
            let e = layer.emitter();
            assert!(state_slots(e) as f32 > e.rate * e.life + 1.0, "{layer:?}");
        }
    }
}
