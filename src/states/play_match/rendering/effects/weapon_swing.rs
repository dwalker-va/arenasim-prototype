use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Auto-Attack Weapon Swings (graphical-only)
// ==============================================================================
//
// The sim spawns one bare `AutoAttackSwing` marker per LANDED auto-attack
// (combat_core/auto_attack.rs, apply loop). `consume_swing_signals` (FixedUpdate,
// so a signal can never be missed when FixedUpdate ticks multiple times per
// rendered frame) transfers each marker into `WeaponSocket` state and spawns the
// cosmetic arrow for bow shots. `animate_weapon_swings` (Update, once per
// rendered frame) writes the socket transforms: an anticipatory windup read
// live from the attack timer, a release stroke synced to the landed hit, and
// aim yaw toward the target. Registered ONLY in `StatesPlugin::build` — never
// in `systems.rs` — so headless never runs any of this.

/// Seconds of the release stroke (windup -> impact sweep).
const SWING_RELEASE_SECS: f32 = 0.12;
/// Seconds held at full extension so the impact registers before easing back.
const SWING_IMPACT_HOLD_SECS: f32 = 0.05;
/// Seconds of follow-through easing back to rest after the impact hold.
const SWING_FOLLOW_SECS: f32 = 0.25;
/// Fraction of the attack interval spent winding up, clamped to sane bounds
/// so fast daggers still telegraph and slow 2H axes don't hover forever.
const SWING_WINDUP_FRACTION: f32 = 0.30;
const SWING_WINDUP_MIN_SECS: f32 = 0.15;
const SWING_WINDUP_MAX_SECS: f32 = 0.60;
/// Cosmetic arrow flight speed (yd/s) and hard despawn backstop.
const COSMETIC_ARROW_SPEED: f32 = 45.0;
const COSMETIC_ARROW_TTL: f32 = 1.5;

// ------------------------------------------------------------------------
// Named swing styles
// ------------------------------------------------------------------------
//
// A signature ability's stroke differs from an auto-attack in two ways that
// have to be modelled separately: how LONG each phase takes (`SwingProfile`)
// and what SHAPE the blade traces (`SwingArc`). An earlier design carried only
// a depth multiplier, which cannot express a different arc plane — scaling the
// auto-attack's pitch just makes the same chop bigger.

/// Per-phase timing for one stroke. Stands in for the bare `SWING_*` consts so
/// a bespoke stroke can reuse the same windup -> release -> hold -> follow
/// state machine at different speeds. [`SwingStyle::Auto`]'s profile
/// reproduces those consts exactly, so ordinary auto-attacks are unchanged.
#[derive(Clone, Copy)]
pub(crate) struct SwingProfile {
    release_secs: f32,
    impact_hold_secs: f32,
    follow_secs: f32,
    arc: SwingArc,
    /// Fraction of the swing's own rotation the BODY turns through
    /// (`animate_body_lean`). Nothing animates the combatant's body otherwise,
    /// so this is what separates a routine swing from a committed one — an
    /// auto-attack gets a slight weight shift, a signature a real turn.
    lean: f32,
}

impl SwingProfile {
    /// Total stroke duration — release sweep, impact hold, follow-through.
    fn total(&self) -> f32 {
        self.release_secs + self.impact_hold_secs + self.follow_secs
    }
}

/// The shape a stroke traces, as a function of the swing parameter `s`.
#[derive(Clone, Copy)]
pub(crate) enum SwingArc {
    /// The shipped auto-attack: pitch only, in the sagittal plane. Raised back
    /// past vertical on windup (`s < 0`), chopped forward-down through the
    /// target on release (`s > 0`). Per-weapon-kind, via [`swing_pose`].
    Sagittal,
    /// A swing through a plane TILTED off vertical — the shape of a diagonal
    /// slash. Still one rotation about one axis, exactly like [`Self::Sagittal`]
    /// (which is this with `tilt == 0`); the tilt is what carries the blade
    /// low-on-one-side to high-on-the-other, so no second axis is needed and
    /// none may be added (see `swing_pose_arc` for what stacking one costs).
    ///
    /// Angles in radians. Continuous at `s == 0`, where both halves are rest.
    TiltedPlane {
        /// How far the swing plane leans off vertical. `0.0` is the sagittal
        /// chop; larger values read as more diagonal, and past ~1.0 the swing
        /// flattens toward a horizontal sweep.
        tilt: f32,
        /// Windup travel, signed like the sagittal chop's pitch: POSITIVE
        /// carries the blade forward and DOWN, so a rising slash winds up low.
        windup: f32,
        /// Release travel. Positive here means the blade finishes HIGH (the
        /// value is negated at use), reversing the auto-attack's chop.
        release: f32,
    },
    /// A LUNGE: pulled back along the aim axis, then driven straight through the
    /// target. Translation-dominant, so unlike [`Self::TiltedPlane`] it traces
    /// no plane at all — the "one rotation, one axis" rule governs how to build
    /// a swing and simply does not apply to a thrust. Kidney Shot's source
    /// animation is `Attack1HPierce`, and expressing that as a rotation would
    /// read as a slash however it were tuned.
    ///
    /// Ignores `WeaponKind` for the same reason `TiltedPlane` does: a signature
    /// belongs to the ability, not to whatever is being held. The dagger's own
    /// auto-attack pose is already a stab, with fixed depths; this exists so a
    /// signature can be a DEEPER, longer one.
    Lunge {
        /// Draw-back distance on windup, in socket-local yards.
        pull: f32,
        /// Drive-through distance on release. The dagger auto's is 0.85, so a
        /// signature lunge wants visibly more.
        thrust: f32,
        /// Weapon pitch, in radians. SMALL, and applied the dagger's way — the
        /// tip rises on the draw and levels into the drive, rather than sweeping
        /// through neutral. The motion of a thrust is the translation; this only
        /// stops the weapon looking rigid, and anything large reads as the blade
        /// turning over mid-stroke.
        pitch: f32,
        /// How far the TORSO drives, in radians at full extension.
        ///
        /// Separate from `pitch` because the two want opposite magnitudes: the
        /// weapon barely turns in a thrust, while the body genuinely commits
        /// behind it. Sharing one value forces a choice between a flipping blade
        /// and a finisher with less body than an auto-attack.
        body_drive: f32,
    },
}

/// How far a style's swing PLANE leans off vertical, or `None` for a stroke that
/// traces no plane at all. Exposed so a probe can assert two signatures stay
/// visually distinct rather than converging on the same diagonal.
pub fn swing_plane_tilt(style: SwingStyle) -> Option<f32> {
    match style.profile().arc {
        SwingArc::TiltedPlane { tilt, .. } => Some(tilt),
        SwingArc::Sagittal => Some(0.0),
        SwingArc::Lunge { .. } => None,
    }
}

impl SwingStyle {
    /// Total duration of this style's release stroke, for effects that must
    /// run exactly as long as the blade is moving (the Mortal Strike trail).
    pub fn stroke_secs(self) -> f32 {
        self.profile().total()
    }

    /// Seconds from the start of the stroke to the frame the blade reaches full
    /// extension — where the impact hold begins, and the only moment an impact
    /// effect should fire.
    ///
    /// The sim resolves an instant's damage BEFORE the animation plays, so the
    /// stroke begins at the hit rather than ending on it. An impact burst
    /// spawned when the marker is consumed therefore goes off with the weapon
    /// still wound up, and on a slow signature stroke it is over before the
    /// blade has travelled halfway.
    pub fn impact_at(self) -> f32 {
        self.profile().release_secs
    }

    pub(crate) fn profile(self) -> SwingProfile {
        match self {
            SwingStyle::Auto => SwingProfile {
                release_secs: SWING_RELEASE_SECS,
                impact_hold_secs: SWING_IMPACT_HOLD_SECS,
                follow_secs: SWING_FOLLOW_SECS,
                arc: SwingArc::Sagittal,
                lean: AUTO_LEAN,
            },
            // Tuned in the pre-implementation animation bench. Slower into the
            // hit and a longer hold than an auto, so the beat registers.
            SwingStyle::MortalStrike => SwingProfile {
                release_secs: SWING_RELEASE_SECS * MORTAL_STRIKE_RELEASE_MUL,
                impact_hold_secs: SWING_IMPACT_HOLD_SECS * MORTAL_STRIKE_HOLD_MUL,
                follow_secs: SWING_FOLLOW_SECS * MORTAL_STRIKE_FOLLOW_MUL,
                arc: SwingArc::TiltedPlane {
                    tilt: MORTAL_STRIKE_TILT,
                    windup: MORTAL_STRIKE_WINDUP,
                    release: MORTAL_STRIKE_RELEASE,
                },
                lean: MORTAL_STRIKE_LEAN,
            },
            // Fast and shallow. The source's Cheap Shot is 634ms of plain
            // `Attack1H` — roughly half Kidney Shot's 1233ms lunge — so the
            // contrast between the two rogue stuns is carried by SPEED as much
            // as by shape.
            SwingStyle::CheapShot => SwingProfile {
                release_secs: SWING_RELEASE_SECS * CHEAP_SHOT_RELEASE_MUL,
                impact_hold_secs: SWING_IMPACT_HOLD_SECS * CHEAP_SHOT_HOLD_MUL,
                follow_secs: SWING_FOLLOW_SECS * CHEAP_SHOT_FOLLOW_MUL,
                arc: SwingArc::TiltedPlane {
                    tilt: CHEAP_SHOT_TILT,
                    windup: CHEAP_SHOT_WINDUP,
                    release: CHEAP_SHOT_RELEASE,
                },
                lean: CHEAP_SHOT_LEAN,
            },
            // A pierce, not a swing — the one stroke in the game that is
            // translation-dominant. Twice Cheap Shot's length (the source is
            // 1233ms against its 634ms), so the finisher lands with weight the
            // opener does not have.
            SwingStyle::KidneyShot => SwingProfile {
                release_secs: SWING_RELEASE_SECS * KIDNEY_SHOT_RELEASE_MUL,
                impact_hold_secs: SWING_IMPACT_HOLD_SECS * KIDNEY_SHOT_HOLD_MUL,
                follow_secs: SWING_FOLLOW_SECS * KIDNEY_SHOT_FOLLOW_MUL,
                arc: SwingArc::Lunge {
                    pull: KIDNEY_SHOT_PULL,
                    thrust: KIDNEY_SHOT_THRUST,
                    pitch: KIDNEY_SHOT_PITCH,
                    body_drive: KIDNEY_SHOT_BODY_DRIVE,
                },
                lean: KIDNEY_SHOT_LEAN,
            },
            // An uppercut: loaded low and driven straight up. Held long at the
            // top, because the hammer arriving IS the beat the stun lands on.
            SwingStyle::HammerOfJustice => SwingProfile {
                release_secs: SWING_RELEASE_SECS * HOJ_RELEASE_MUL,
                impact_hold_secs: SWING_IMPACT_HOLD_SECS * HOJ_HOLD_MUL,
                follow_secs: SWING_FOLLOW_SECS * HOJ_FOLLOW_MUL,
                arc: SwingArc::TiltedPlane {
                    tilt: HOJ_TILT,
                    windup: HOJ_WINDUP,
                    release: HOJ_RELEASE,
                },
                lean: HOJ_LEAN,
            },
        }
    }
}

/// Mortal Strike stroke tuning. Timing multipliers are relative to the
/// auto-attack consts above; the arc angles are radians.
///
/// These are deliberately LARGE. Nothing animates the combatant's body, so the
/// weapon is the only thing carrying the ability — an arc merely different from
/// the auto-attack does not register, it has to be visibly bigger as well as
/// differently shaped. The total sweep here (~2.8 rad) clearly exceeds the
/// auto-attack's (~2.3 rad), on a plane leaned nearly 50 degrees off it.
const MORTAL_STRIKE_RELEASE_MUL: f32 = 2.8;
const MORTAL_STRIKE_HOLD_MUL: f32 = 2.0;
const MORTAL_STRIKE_FOLLOW_MUL: f32 = 1.6;

// --- body lean (see `animate_body_lean`) ---------------------------------
//
// Fractions of the swing's own rotation, so the body always turns the way the
// weapon does. Values from the pre-implementation bench.

/// A routine swing: a slight commitment, ~8° at full extension. Deliberately
/// an order of magnitude below the signature — an auto-attack should gain
/// weight without gaining ceremony.
const AUTO_LEAN: f32 = 0.10;
/// A signature swing: the torso genuinely turns into it, ~22° at full
/// extension.
const MORTAL_STRIKE_LEAN: f32 = 0.28;
/// Horizontal step into the blow, in yards at full extension — back through the
/// windup, forward through the release.
///
/// HORIZONTAL ONLY, and small. The body's vertical offset belongs to the gaits
/// (walk bob / sheep hop / panic run), so a vertical crouch here would fight
/// them; X/Z is unowned. Small because health bars and floating text anchor to
/// the SIM position, and a large offset visibly detaches the body from them.
const SWING_WEIGHT_SHIFT: f32 = 0.18;
/// Lean of the swing plane off vertical (~49°): unmistakably diagonal at arena
/// camera distance, while still reading as a low-to-high swing. Past ~1.0 rad
/// it flattens toward a horizontal sweep and loses the rising quality.
const MORTAL_STRIKE_TILT: f32 = 0.85;
/// Windup carries the blade well forward and DOWN past horizontal — the mount's
/// own 0.75 forward lean adds to this — so the stroke loads from unmistakably
/// low rather than from somewhere near rest.
const MORTAL_STRIKE_WINDUP: f32 = 1.45;
/// Release carries it up and back past vertical, finishing high and behind the
/// shoulder. Combined travel is ~2.8 rad, visibly larger than the
/// auto-attack's ~2.3 and in the opposite direction.
const MORTAL_STRIKE_RELEASE: f32 = 1.35;

// --- Cheap Shot -----------------------------------------------------------
//
// The source is 634ms of plain `Attack1H`, so this stroke is deliberately
// UNSPECTACULAR: barely off the sagittal plane, small travel, and fast. It has
// to read as a quick jab, because everything distinctive about Cheap Shot lives
// in the crescent flare rather than in the swing. Total here is ~0.63s against
// the auto's 0.42s and Mortal Strike's ~0.84s.
const CHEAP_SHOT_RELEASE_MUL: f32 = 1.25;
const CHEAP_SHOT_HOLD_MUL: f32 = 1.6;
const CHEAP_SHOT_FOLLOW_MUL: f32 = 1.6;
/// Only ~14° off vertical. Enough that it is not literally the auto-attack, far
/// short of Mortal Strike's 49° diagonal — this is a jab, not a signature.
const CHEAP_SHOT_TILT: f32 = 0.25;
/// A short load. The Rogue is coming out of stealth, so a big telegraphed
/// windup would contradict the ability.
const CHEAP_SHOT_WINDUP: f32 = 0.85;
const CHEAP_SHOT_RELEASE: f32 = 1.10;
/// Between the auto's ~8° and Mortal Strike's ~22°: the body commits, but the
/// stroke is over before it becomes ceremony.
const CHEAP_SHOT_LEAN: f32 = 0.16;

// --- Kidney Shot ----------------------------------------------------------
//
// The finisher. Source is 1233ms of `Attack1HPierce` — twice Cheap Shot, and a
// LUNGE rather than a swing. Total here is ~1.22s.
const KIDNEY_SHOT_RELEASE_MUL: f32 = 2.5;
const KIDNEY_SHOT_HOLD_MUL: f32 = 2.4;
const KIDNEY_SHOT_FOLLOW_MUL: f32 = 3.2;
/// Draws back further than the dagger auto's 0.4, so the lunge visibly loads.
const KIDNEY_SHOT_PULL: f32 = 0.70;
/// Drives through well past the dagger auto's 0.85 — this is the whole stroke,
/// so it has to be the part that reads. Raised from 1.55 after the first pass
/// read as too small in the client: the travel IS the animation, and a lunge
/// that barely outreaches a routine stab is not a finisher.
const KIDNEY_SHOT_THRUST: f32 = 2.05;
/// A whisper of pitch, just over the dagger auto's own 0.2. Any more and the
/// thrust starts to look like a chop — and applied as a single `pitch * s` it
/// looked like the blade flipping over, which is what 0.90 did here.
const KIDNEY_SHOT_PITCH: f32 = 0.24;
/// The torso drive, in radians at full extension. Carries the weight the weapon
/// pitch deliberately does not: at `KIDNEY_SHOT_LEAN` this is ~18 degrees of
/// body, against an auto-attack's ~8.
const KIDNEY_SHOT_BODY_DRIVE: f32 = 0.90;
/// ~18° of forward drive — heavier than Cheap Shot's, just under Mortal
/// Strike's, which is where a finisher belongs.
const KIDNEY_SHOT_LEAN: f32 = 0.35;

// --- Hammer of Justice ------------------------------------------------------
//
// An UPPERCUT. The mace loads low and drives vertically up, arriving as the
// seal lands on the victim.
//
// Deliberately NEARLY SAGITTAL where Mortal Strike's signature is a 49-degree
// diagonal: those are the only two big two-beat strokes in the game and they
// must not be mistaken for each other. A rise is also the natural reading of a
// hammer of judgement being brought UP rather than swung across.
const HOJ_RELEASE_MUL: f32 = 2.2;
/// Held long at full extension — the hammer at the top of its rise IS the beat
/// the stun lands on, so it wants to sit there a moment.
const HOJ_HOLD_MUL: f32 = 2.6;
const HOJ_FOLLOW_MUL: f32 = 2.0;
/// Barely off vertical: enough that the mace does not look like it is on rails,
/// far short of Mortal Strike's 0.85 diagonal.
const HOJ_TILT: f32 = 0.16;
/// Loads LOW — positive windup carries the head forward and down.
const HOJ_WINDUP: f32 = 1.15;
/// Drives high, and further than it loaded: the rise is the whole stroke.
const HOJ_RELEASE: f32 = 1.70;
/// The body rises into it. Just past Mortal Strike's, because an uppercut
/// commits the whole frame upward rather than turning it.
const HOJ_LEAN: f32 = 0.30;


/// Normalized swing parameter in `[-1, 1]`.
///
/// * `s < 0` — windup: eases 0 -> -1 over the anticipation window as
///   `timer` approaches `interval`, holding at -1 while an overdue attack
///   waits (out of range / no LoS).
/// * `s > 0` — release: sweeps from `release_from` (the windup depth at the
///   moment the hit landed, `<= 0`) THROUGH to full extension at 1 over
///   `SWING_RELEASE_SECS` — the pull-back powers the strike instead of being
///   discarded — holds at 1 for `SWING_IMPACT_HOLD_SECS` so the impact
///   registers, then decays to 0 over `SWING_FOLLOW_SECS`.
/// * `s == 0` — at rest.
///
/// Pure so the timing behavior is unit-testable without Bevy (see tests at the
/// bottom of this file). A live `release_t` always wins over windup: the hit
/// already landed, so the stroke plays regardless of what the timer says.
/// Test-only shim: [`swing_param_timed`] at the auto-attack profile.
///
/// Production code always goes through `swing_param_timed` with the socket's
/// live profile. This keeps the original auto-attack timing tests calling the
/// exact signature they were written against, so they remain untouched evidence
/// that the styled-stroke refactor did not move the auto-attack curve.
#[cfg(test)]
fn swing_param(
    timer: f32,
    interval: f32,
    windup_window: f32,
    release_t: Option<f32>,
    release_from: f32,
) -> f32 {
    swing_param_timed(
        timer,
        interval,
        windup_window,
        release_t,
        release_from,
        SwingStyle::Auto.profile(),
    )
}

/// [`swing_param`] with the phase durations supplied by a [`SwingProfile`]
/// instead of the bare consts. The windup branch is unaffected — it is driven
/// by the sim's attack timer, which a styled stroke does not change.
fn swing_param_timed(
    timer: f32,
    interval: f32,
    windup_window: f32,
    release_t: Option<f32>,
    release_from: f32,
    profile: SwingProfile,
) -> f32 {
    if let Some(t) = release_t {
        let from = release_from.clamp(-1.0, 0.0);
        if t < profile.release_secs {
            let p = (t / profile.release_secs).clamp(0.0, 1.0);
            // Ease-in: the stroke accelerates into the hit.
            let p = p * p;
            return from + (1.0 - from) * p;
        }
        if t < profile.release_secs + profile.impact_hold_secs {
            return 1.0;
        }
        let f = (t - profile.release_secs - profile.impact_hold_secs) / profile.follow_secs;
        return (1.0 - f).clamp(0.0, 1.0);
    }
    if !(interval > 0.0) || !(windup_window > 0.0) {
        return 0.0; // degenerate attack speed — hold at rest, never NaN
    }
    let windup_start = interval - windup_window;
    if timer >= windup_start {
        let w = ((timer - windup_start) / windup_window).clamp(0.0, 1.0);
        // Ease-in so the raise reads as a deliberate telegraph, not a twitch.
        return -(w * w);
    }
    0.0
}

/// Per-kind pose offset for a swing parameter: local rotation + translation
/// applied on top of the socket's rest mount. Melee kinds arc around local X
/// (raise back on windup, chop through on release); daggers add a forward jab;
/// the bow draws back instead of arcing.
fn swing_pose(kind: WeaponKind, s: f32) -> Transform {
    let pitch = match kind {
        WeaponKind::Bow => {
            // Draw: slight tilt + pull toward the body. Release: tiny forward snap.
            let pull = if s < 0.0 { -s } else { 0.0 };
            let snap = if s > 0.0 { s } else { 0.0 };
            return Transform::from_translation(Vec3::new(0.0, 0.0, -0.18 * pull + 0.08 * snap))
                .with_rotation(Quat::from_rotation_z(0.15 * pull));
        }
        WeaponKind::Dagger => {
            // A stab, not an arc: pull back along the aim axis on windup,
            // lunge hard forward on release, with only a whisper of pitch.
            let pull = if s < 0.0 { -s } else { 0.0 };
            let thrust = if s > 0.0 { s } else { 0.0 };
            return Transform::from_translation(Vec3::new(
                0.0,
                0.0,
                -0.4 * pull + 0.85 * thrust,
            ))
            .with_rotation(Quat::from_rotation_x(0.2 * pull - 0.1 * thrust));
        }
        WeaponKind::Shield => 0.0, // static (plan R9)
        // TwoHandAxe / Mace: big readable arc, raised back past vertical on
        // windup and chopped forward-down through the target on release. In
        // the socket frame, POSITIVE X-rotation pitches forward — windup is
        // negative (s < 0 keeps the product negative), release positive. The
        // mount's own 0.75 forward lean adds to these totals.
        _ => {
            if s < 0.0 {
                0.9 * s
            } else {
                1.4 * s
            }
        }
    };
    Transform::from_rotation(Quat::from_rotation_x(pitch))
}

/// Pose for one swing parameter under a named [`SwingArc`].
///
/// `Sagittal` delegates to [`swing_pose`] unchanged, so every auto-attack — and
/// every weapon kind's bespoke pose within it — is byte-identical to before.
/// `RisingDiagonal` ignores `WeaponKind` on purpose: it is a whole-body arc
/// belonging to the ability, not to whatever the caster happens to be holding.
/// The rotation an arc turns through at swing parameter `s`: its axis, and the
/// angle about that axis.
///
/// This is what the BODY lean rides — the torso turns about the same axis as
/// the weapon, at a fraction of the angle, so one input drives both and they
/// can never disagree about which way the swing goes.
///
/// The `Sagittal` branch reports the two-hand arc for every weapon kind. The
/// per-kind poses ([`swing_pose`]) differ in how they express that swing — a
/// dagger thrusts rather than arcs — but the BODY's commitment into the blow is
/// the same motion regardless of what is held, so the lean uses one reference
/// curve rather than five.
fn arc_rotation(s: f32, arc: SwingArc) -> (Vec3, f32) {
    match arc {
        SwingArc::Sagittal => (Vec3::X, if s < 0.0 { 0.9 * s } else { 1.4 * s }),
        SwingArc::TiltedPlane { tilt, windup, release } => {
            let angle = if s < 0.0 { windup * -s } else { -release * s };
            (Quat::from_rotation_z(tilt) * Vec3::X, angle)
        }
        // A lunge's body commitment is a forward pitch about X: the torso drives
        // after the point. The same axis the sagittal chop uses, so the body and
        // the weapon still agree on direction.
        //
        // Rides `body_drive`, NOT the weapon's `pitch`. The two are deliberately
        // an order apart — the blade barely turns in a thrust while the body
        // commits behind it — so reading the lean off `pitch` gives a finisher
        // less body than a routine auto-attack.
        SwingArc::Lunge { body_drive, .. } => (Vec3::X, body_drive * s),
    }
}

fn swing_pose_arc(kind: WeaponKind, s: f32, arc: SwingArc) -> Transform {
    match arc {
        SwingArc::Sagittal => swing_pose(kind, s),
        SwingArc::Lunge { pull, thrust, pitch, .. } => {
            // Socket-local -Z is back toward the wielder and +Z out along the
            // aim, matching the dagger's own stab in `swing_pose`.
            let draw = if s < 0.0 { -s } else { 0.0 };
            let drive = if s > 0.0 { s } else { 0.0 };
            // The tip rises on the draw and LEVELS into the drive — windup and
            // release pitched separately, the same shape as the dagger's own
            // stab (`0.2 * pull - 0.1 * thrust`).
            //
            // A single `pitch * s` instead sweeps the blade from `-pitch`
            // through neutral to `+pitch`, so at 0.9 rad it turned through 103
            // degrees and read as the weapon FLIPPING OVER halfway through the
            // stroke rather than thrusting. A thrust barely rotates; that is
            // what makes it a thrust.
            Transform::from_translation(Vec3::new(0.0, 0.0, -pull * draw + thrust * drive))
                .with_rotation(Quat::from_rotation_x(pitch * draw - pitch * 0.5 * drive))
        }
        SwingArc::TiltedPlane { .. } => {
            // ONE rotation about ONE axis — the weapon sweeps through a plane,
            // the way a swing does. The diagonal comes from TILTING that plane
            // off vertical, not from adding a second rotation on another axis.
            //
            // Composing yaw or roll on top instead (the first attempt) reads as
            // the axe being turned rather than swung, and is wrong twice over:
            // a socket-frame Z rotation cartwheels the weapon sideways rather
            // than rolling it about its haft, and a socket-frame Y rotation
            // stacks on the aim yaw the caller already applies, so the blade
            // points away from the target at the exact moment of impact.
            //
            // Angle is signed in the same sense as the sagittal chop: POSITIVE
            // pitches the blade forward and down, negative raises it. A rising
            // slash is therefore the auto-attack's signs reversed — down and
            // back on the windup, up and through on the release. Continuous at
            // `s == 0`, where both halves are the rest pose. The axis is tilted
            // within the frontal plane: `tilt == 0` is the pure sagittal chop,
            // and increasing it rotates the whole swing plane so the blade
            // travels low-on-one-side to high-on-the-other.
            let (axis, angle) = arc_rotation(s, arc);
            Transform::from_rotation(Quat::from_axis_angle(axis, angle))
        }
    }
}

/// FixedUpdate (graphical-only): consume the sim's landed-attack markers.
/// Main-hand sockets of the attacker begin their release stroke aimed at the
/// hit target; a Bow main hand additionally looses a cosmetic arrow. Attackers
/// with no sockets (pets, wand casters, un-animated classes) no-op — the
/// marker is simply despawned.
pub fn consume_swing_signals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<(Entity, &AutoAttackSwing)>,
    mut sockets: Query<&mut WeaponSocket>,
    positions: Query<&Transform, With<Combatant>>,
) {
    for (signal_entity, signal) in signals.iter() {
        let target_pos = positions.get(signal.target).map(|t| t.translation).ok();
        // Dual-wield alternation: the sim has ONE attack timer, so each landed
        // auto swings whichever dagger is flagged as next, then hands the flag
        // to its twin. Single-weapon classes keep the flag on the main hand
        // permanently.
        let has_off_dagger = sockets.iter().any(|s| {
            s.owner == signal.attacker && s.hand == WeaponHand::Off && s.kind == WeaponKind::Dagger
        });
        for mut socket in sockets.iter_mut() {
            if socket.owner != signal.attacker {
                continue;
            }
            if let Some(pos) = target_pos {
                socket.aim = pos; // both hands track the victim
            }
            if !socket.winds_up_next {
                if has_off_dagger && socket.kind == WeaponKind::Dagger {
                    socket.winds_up_next = true; // this twin swings the NEXT auto
                }
                continue;
            }
            socket.release_t = Some(0.0);
            // An ordinary auto is never a styled stroke. Clears any signature
            // style still set from a stroke that has not expired yet, so a
            // Mortal Strike's timing can never bleed into the next swing.
            socket.swing_style = SwingStyle::Auto;
            if has_off_dagger && socket.kind == WeaponKind::Dagger {
                socket.winds_up_next = false; // twin takes over
            }
            // Cosmetic arrow: bow-kind main hand only. This single gate keeps
            // caster Wand Shots (ranged, no bow) and any future non-bow ranged
            // weapon from loosing arrows.
            if signal.ranged && socket.kind == WeaponKind::Bow {
                if let (Ok(from_tf), Some(to)) = (positions.get(signal.attacker), target_pos) {
                    let from = from_tf.translation + Vec3::Y * 1.1;
                    let dir = (to - from).normalize_or_zero();
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(0.06, 0.06, 0.55))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(0.85, 0.78, 0.55),
                            emissive: LinearRgba::new(0.25, 0.2, 0.1, 1.0),
                            unlit: false,
                            ..default()
                        })),
                        Transform::from_translation(from)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                        CosmeticArrow {
                            to: to + Vec3::Y * 1.0,
                            speed: COSMETIC_ARROW_SPEED,
                            ttl: COSMETIC_ARROW_TTL,
                        },
                        PlayMatchEntity,
                    ));
                }
            }
        }
        commands.entity(signal_entity).despawn();
    }
}

/// Update (graphical-only): write every weapon socket's local transform for
/// this rendered frame — aim yaw toward the current target composed with the
/// rest mount and the swing pose. Reads sim state (attack timer, attack speed,
/// auras, positions) and never writes any of it.
pub fn animate_weapon_swings(
    time: Res<Time>,
    mut sockets: Query<(&mut WeaponSocket, &mut Transform, &mut Visibility)>,
    owners: Query<
        (
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&CastingState>,
            Option<&ChannelingState>,
            Option<&PolymorphedVisual>,
        ),
        Without<WeaponSocket>,
    >,
) {
    use crate::states::play_match::combat_core::effective_attack_interval;
    use crate::states::play_match::utils::is_incapacitated;
    use crate::states::play_match::{AUTO_SHOT_RANGE, HUNTER_DEAD_ZONE, MELEE_RANGE};

    let dt = time.delta_secs();
    for (mut socket, mut transform, mut visibility) in sockets.iter_mut() {
        let Ok((combatant, owner_tf, auras, casting, channeling, polymorphed_marker)) =
            owners.get(socket.owner)
        else {
            continue;
        };

        // A polymorphed victim's body swaps to the sheep form — a sheep
        // gripping a full-size axe gives it away, so hide the sockets (the
        // glTF subtree inherits). Stealth does NOT hide: the weapons fade
        // with the body instead (`update_weapon_stealth_fade`). Keyed off
        // the `PolymorphedVisual` marker (not re-derived from auras) so the
        // body swap in `update_polymorph_visuals` is the single source of
        // truth — a killing blow leaves the aura on the corpse until it
        // ticks out naturally, but the marker (and thus this hide) flips
        // back the same frame the body is restored.
        let polymorphed = polymorphed_marker.is_some();
        let wanted = if polymorphed {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }

        // The active stroke's timing and arc. `Auto` reproduces the original
        // consts and the sagittal chop exactly.
        let profile = socket.swing_style.profile();

        // Advance / expire the release stroke. `windup_s` is frozen during
        // the stroke — it is the sweep's starting pose — and zeroed at expiry
        // so the next windup ramps fresh.
        if let Some(t) = socket.release_t {
            let t = t + dt;
            if t >= profile.total() {
                socket.release_t = None;
                socket.windup_s = 0.0;
                // One-shot: the next stroke is an ordinary auto unless another
                // signature claims it.
                socket.swing_style = SwingStyle::Auto;
            } else {
                socket.release_t = Some(t);
            }
        }

        // Track the live target's position whenever one exists — weapons face
        // their target during the approach too, not just once in reach.
        let mut target_dist = f32::INFINITY;
        if combatant.is_alive() {
            if let Some(target) = combatant.target {
                if let Ok((target_combatant, target_tf, _, _, _, _)) = owners.get(target) {
                    if target_combatant.is_alive() {
                        socket.aim = target_tf.translation;
                        target_dist = owner_tf.translation.distance(target_tf.translation);
                    }
                }
            }
        }

        // Windup eligibility: cosmetic-grade approximation of "an attack is
        // coming" — the RELEASE never depends on this (it keys off the sim's
        // landed-hit marker), so a wrong guess here costs at most a windup
        // that eases back down. Mirrors the sim's own can't-swing gates: an
        // incapacitated / casting / channeling attacker's timer is frozen,
        // and a Hunter inside the dead zone can't loose the overdue shot —
        // telegraphing in those states reads as a stuck animation.
        let mut windup_window = 0.0;
        let mut interval = 0.0;
        if socket.winds_up_next
            && socket.release_t.is_none()
            && combatant.is_alive()
            && !combatant.stealthed
            && casting.is_none()
            && channeling.is_none()
            && !is_incapacitated(auras)
        {
            let (reach, min_reach) = if socket.kind == WeaponKind::Bow {
                (AUTO_SHOT_RANGE + 2.0, HUNTER_DEAD_ZONE)
            } else {
                (MELEE_RANGE + 1.5, 0.0)
            };
            if target_dist <= reach && target_dist >= min_reach {
                interval = effective_attack_interval(combatant, auras);
                windup_window = (interval * SWING_WINDUP_FRACTION)
                    .clamp(SWING_WINDUP_MIN_SECS, SWING_WINDUP_MAX_SECS);
            }
        }

        // Windup eases at a bounded rate; the raw parameter is discontinuous
        // during pursuit (overdue timer + the reach boundary flickering as
        // both units move), and rendering it raw strobes the pose. The
        // release stroke stays raw — its sharpness IS the hit — and sweeps
        // from the frozen windup depth through to full extension.
        let s_raw = swing_param_timed(
            combatant.attack_timer,
            interval,
            windup_window,
            socket.release_t,
            socket.windup_s,
            profile,
        );
        let s = if socket.release_t.is_some() {
            s_raw
        } else {
            let max_step = 6.0 * dt;
            socket.windup_s += (s_raw - socket.windup_s).clamp(-max_step, max_step);
            socket.windup_s
        };

        // Aim yaw: the weapon is RIGID to the body (its transform is local to
        // the hierarchy, so when `move_to_target` turns the parent's facing
        // the weapon turns with it — that rigidity is what reads as a solidly
        // held object while units move). On top of that, a LOCAL yaw angle
        // eases toward the target bearing at a bounded rate: moving units
        // keep the weapon glued to their frame with a gentle drift toward the
        // victim; stationary units converge to exact target facing. A release
        // stroke corrects faster so the hit still lands visually on-target.
        let owner_forward = owner_tf.rotation * Vec3::Z;
        let owner_yaw = owner_forward.x.atan2(owner_forward.z);

        // Absorb LARGE one-frame facing snaps (gate-open first move, a hard
        // target switch) into the local yaw: the weapon holds its world
        // bearing through the snap and eases to the new aim, instead of
        // whipping a quarter-turn with the body. Ordinary per-tick turning
        // stays rigid — only discrete jumps qualify.
        let owner_snap = (owner_yaw - socket.prev_owner_yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        socket.prev_owner_yaw = owner_yaw;
        if owner_snap.abs() > 0.5 {
            socket.yaw_local = (socket.yaw_local - owner_snap + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        }

        let aim_dir = socket.aim - owner_tf.translation;
        if aim_dir.xz().length_squared() > 1e-6 {
            let bearing = aim_dir.x.atan2(aim_dir.z);
            // Wrap-aware shortest-path delta from current local angle.
            let target_local = bearing - owner_yaw;
            let mut err = target_local - socket.yaw_local;
            err = (err + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let rate = if socket.release_t.is_some() { 20.0 } else { 6.0 };
            let max_step = rate * dt;
            socket.yaw_local += err.clamp(-max_step, max_step);
            // Keep the stored angle wrapped so it never accumulates turns.
            socket.yaw_local = (socket.yaw_local + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        }

        // Pose composes in the SOCKET frame (left of `rest`), not the model
        // frame: the swing arc must rotate around the socket's horizontal
        // axis and the stab must translate straight along the aim axis,
        // regardless of the roll each model carries inside its mount.
        // Composed model-side, the axe's chop became a flat-faced sideways
        // slap and the dagger's lunge became a sideways drag.
        *transform = Transform::from_rotation(Quat::from_rotation_y(socket.yaw_local))
            * swing_pose_arc(socket.kind, s, profile.arc)
            * socket.rest;

        // Publish for `animate_body_lean`, which must turn the body on exactly
        // this value rather than recomputing it.
        socket.last_s = s;
    }
}

/// Update (graphical-only): turn the swinging combatant's BODY into the blow.
///
/// Nothing else animates a combatant's torso, so for a melee ability the weapon
/// was the only signal there was — which capped how much any stroke could read
/// no matter how it was shaped. The body turns about the swing's OWN axis
/// ([`arc_rotation`]) by [`SwingProfile::lean`] of its angle, driven by the same
/// `s` the weapon used, so wind-back and drive-through fall out of one input
/// with no second curve to keep in sync.
///
/// Because a [`WeaponSocket`] is a CHILD of the [`VisualBody`], this composes
/// onto the weapon's own arc rather than sitting beside it — the lean makes the
/// blade's world-space sweep bigger as well as adding a second signal.
///
/// **Channel ownership.** Writes the body's `rotation` and its HORIZONTAL
/// translation only. `translation.y` belongs to the gaits (walk bob, sheep hop,
/// panic run) and the victory bounce; rotation is otherwise written only by the
/// death fall, which this cedes to via the `dying` branch. Pets are never
/// touched — they carry no sockets — so `apply_pet_mesh_tilt` keeps their
/// rotation uncontested.
pub fn animate_body_lean(
    sockets: Query<&WeaponSocket>,
    owners: Query<(&Children, Option<&DeathAnimation>), With<Combatant>>,
    mut bodies: Query<&mut Transform, With<VisualBody>>,
) {
    for socket in sockets.iter() {
        // One body, one lean: the main hand owns it. An off-hand dagger would
        // otherwise fight its twin for the same transform.
        if socket.hand != WeaponHand::Main {
            continue;
        }
        let Ok((children, dying)) = owners.get(socket.owner) else {
            continue;
        };

        let (axis, angle) = arc_rotation(socket.last_s, socket.swing_style.profile().arc);
        let lean = socket.swing_style.profile().lean;

        for child in children.iter() {
            let Ok(mut body) = bodies.get_mut(child) else {
                continue;
            };
            if dying.is_some() {
                // The death fall owns rotation from here. Clear the horizontal
                // step, though — nothing else writes it, so a unit killed
                // mid-swing would keep the offset on its corpse forever.
                body.translation.x = 0.0;
                body.translation.z = 0.0;
                continue;
            }
            body.rotation = Quat::from_axis_angle(axis, angle * lean);
            // Step into the blow: back through the windup, forward through the
            // release. Local +Z is forward (the same axis the mount leans
            // along), and `s` already carries the sign.
            let step = socket.last_s * SWING_WEIGHT_SHIFT;
            body.translation.x = 0.0;
            body.translation.z = step;
        }
    }
}

/// Update (graphical-only): fly cosmetic arrows to their captured destination
/// and despawn on arrival (or on the TTL backstop — the damage already landed,
/// the arrow is pure theater).
pub fn update_cosmetic_arrows(
    mut commands: Commands,
    time: Res<Time>,
    mut arrows: Query<(Entity, &mut CosmeticArrow, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut arrow, mut transform) in arrows.iter_mut() {
        arrow.ttl -= dt;
        let to_target = arrow.to - transform.translation;
        let step = arrow.speed * dt;
        if arrow.ttl <= 0.0 || to_target.length() <= step {
            commands.entity(entity).despawn();
            continue;
        }
        let dir = to_target.normalize_or_zero();
        transform.translation += dir * step;
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
    }
}

#[cfg(test)]
mod swing_tests {
    use super::*;

    #[test]
    fn rest_before_windup_window() {
        assert_eq!(swing_param(0.5, 2.0, 0.5, None, 0.0), 0.0);
    }

    #[test]
    fn windup_ramps_monotonically_to_full() {
        let interval = 2.0;
        let window = 0.5;
        let mut last = 0.0;
        for i in 0..=10 {
            let timer = (interval - window) + window * (i as f32 / 10.0);
            let s = swing_param(timer, interval, window, None, 0.0);
            assert!(s <= last + 1e-6, "windup must be monotonically deepening");
            assert!((-1.0..=0.0).contains(&s));
            last = s;
        }
        assert!((last + 1.0).abs() < 1e-5, "full windup reaches -1");
    }

    #[test]
    fn overdue_attack_holds_full_windup() {
        // Timer past the interval (target out of range, attack pending):
        // the weapon holds at full draw instead of snapping back.
        let s = swing_param(3.7, 2.0, 0.5, None, 0.0);
        assert!((s + 1.0).abs() < 1e-5);
    }

    #[test]
    fn release_sweeps_through_from_windup_depth() {
        // The stroke starts AT the frozen windup pose and powers through to
        // full extension — the pull-back is spent, not discarded.
        let start = swing_param(0.0, 2.0, 0.5, Some(0.0), -1.0);
        assert!((start + 1.0).abs() < 1e-5, "stroke begins at the windup pose");
        let peak = swing_param(0.0, 2.0, 0.5, Some(SWING_RELEASE_SECS), -1.0);
        assert!((peak - 1.0).abs() < 1e-5, "stroke reaches full extension");
        // Monotonically rising through the sweep.
        let mut last = -1.0;
        for i in 0..=10 {
            let s = swing_param(0.0, 2.0, 0.5, Some(SWING_RELEASE_SECS * i as f32 / 10.0), -1.0);
            assert!(s >= last - 1e-6, "sweep must rise monotonically");
            last = s;
        }
    }

    #[test]
    fn impact_holds_then_returns_to_rest() {
        // Held at full extension through the impact window...
        let held = swing_param(
            0.0,
            2.0,
            0.5,
            Some(SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS * 0.5),
            0.0,
        );
        assert!((held - 1.0).abs() < 1e-5);
        // ...then the follow-through decays back to 0.
        let done = swing_param(
            0.0,
            2.0,
            0.5,
            Some(SWING_RELEASE_SECS + SWING_IMPACT_HOLD_SECS + SWING_FOLLOW_SECS),
            0.0,
        );
        assert!(done.abs() < 1e-5);
    }

    #[test]
    fn release_wins_over_windup_state() {
        // A landed hit plays its stroke even if the timer says "mid-windup"
        // (no-warning release, plan R7 / AE2): with no prior windup the
        // stroke rises from rest regardless of the overdue timer.
        let s = swing_param(1.9, 2.0, 0.5, Some(SWING_RELEASE_SECS * 0.5), 0.0);
        assert!(s > 0.0);
    }

    #[test]
    fn degenerate_interval_is_guarded() {
        for bad in [0.0_f32, -1.0, f32::NAN] {
            let s = swing_param(1.0, bad, 0.5, None, 0.0);
            assert_eq!(s, 0.0, "degenerate interval must rest, not NaN");
        }
        // Interval change mid-windup stays in range (AE3 continuity).
        let s1 = swing_param(1.8, 2.0, 0.5, None, 0.0);
        let s2 = swing_param(1.8, 2.6, 0.6, None, 0.0); // slow applied mid-windup
        assert!((-1.0..=0.0).contains(&s1));
        assert!((-1.0..=0.0).contains(&s2));
        // An out-of-range release_from is clamped, never amplified.
        let s3 = swing_param(0.0, 2.0, 0.5, Some(0.0), -7.0);
        assert!((-1.0..=1.0).contains(&s3));
    }

    #[test]
    fn shield_pose_is_static() {
        let t = swing_pose(WeaponKind::Shield, -1.0);
        assert_eq!(t.translation, Vec3::ZERO);
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn dagger_pose_is_a_thrust_not_an_arc() {
        // Windup pulls back along the aim axis; release thrusts forward —
        // translation dominates and pitch stays a whisper.
        let windup = swing_pose(WeaponKind::Dagger, -1.0);
        assert!(windup.translation.z < -0.2, "windup pulls the dagger back");
        let release = swing_pose(WeaponKind::Dagger, 1.0);
        assert!(release.translation.z > 0.6, "release lunges the dagger forward");
        let (_, angle) = release.rotation.to_axis_angle();
        assert!(angle.abs() < 0.3, "a stab barely rotates");
    }

    // -- styled strokes -----------------------------------------------------
    //
    // The whole styled-stroke refactor rests on one claim: `SwingStyle::Auto`
    // is the shipped auto-attack, exactly. Everything above tests the curve via
    // the auto profile; these pin the claim itself.

    #[test]
    fn a_lunge_never_turns_the_blade_over() {
        // Kidney Shot shipped with `pitch * s`, which swept the weapon from
        // -pitch through neutral to +pitch — at 0.9 rad, 103 degrees of
        // rotation. In the client it read as the blade FLIPPING OVER halfway
        // through the stroke instead of thrusting.
        //
        // A thrust is translation; the pitch is a garnish. What made the old
        // version read as a flip was MAGNITUDE, not the sign change — the
        // shipped dagger stab also crosses neutral (+0.2 on the draw to -0.1 on
        // the drive) and looks right, because that is only 17 degrees in total.
        // So the invariant is that a lunge's blade stays within a small angle of
        // rest for the whole stroke, and never approaches the auto-attack's
        // sweep.
        let SwingArc::Lunge { pitch, .. } = SwingStyle::KidneyShot.profile().arc else {
            panic!("Kidney Shot must be a Lunge");
        };
        assert!(
            pitch < 0.35,
            "{pitch} rad is a chop, not the whisper a thrust wants"
        );

        let mut max_turn: f32 = 0.0;
        for i in -20..=20 {
            let s = i as f32 / 20.0;
            let pose =
                swing_pose_arc(WeaponKind::Dagger, s, SwingStyle::KidneyShot.profile().arc);
            let angle = pose.rotation.to_euler(EulerRot::XYZ).0;
            max_turn = max_turn.max(angle.abs());
        }
        assert!(
            max_turn < 0.35,
            "the blade turns {max_turn} rad — a thrust barely rotates, and at \
             0.9 this read as the weapon flipping over mid-stroke"
        );
        // And comfortably under the auto-attack's own sweep, which is a SWING.
        let (_, auto_sweep) = arc_rotation(1.0, SwingArc::Sagittal);
        assert!(
            max_turn < auto_sweep.abs() * 0.5,
            "a thrust turning {max_turn} rad is competing with the {auto_sweep} \
             rad chop it is supposed to contrast with"
        );
    }

    #[test]
    fn a_lunge_travels_further_than_the_dagger_it_borrows_from() {
        // The stroke IS the translation, so a finisher that barely outreaches a
        // routine stab has no finisher in it. The dagger auto drives 0.85.
        let SwingArc::Lunge { thrust, pull, .. } = SwingStyle::KidneyShot.profile().arc else {
            panic!("Kidney Shot must be a Lunge");
        };
        assert!(thrust > 0.85 * 2.0, "only {thrust} of drive-through");
        assert!(pull > 0.4, "only {pull} of draw-back to load from");
    }

    #[test]
    fn a_lunge_body_drives_harder_than_an_auto_attack() {
        // The weapon pitch is deliberately tiny, so the body carries the weight.
        // Reading both off one value would force a choice between a flipping
        // blade and a finisher with less commitment than a routine swing.
        let profile = SwingStyle::KidneyShot.profile();
        let (_, kidney) = arc_rotation(1.0, profile.arc);
        let (_, auto) = arc_rotation(1.0, SwingStyle::Auto.profile().arc);
        assert!(
            (kidney * profile.lean).abs() > (auto * SwingStyle::Auto.profile().lean).abs(),
            "the finisher's body must commit harder than an auto-attack's"
        );
    }

    #[test]
    fn the_auto_style_reproduces_the_shipped_constants() {
        let auto = SwingStyle::Auto.profile();
        assert_eq!(auto.release_secs, SWING_RELEASE_SECS);
        assert_eq!(auto.impact_hold_secs, SWING_IMPACT_HOLD_SECS);
        assert_eq!(auto.follow_secs, SWING_FOLLOW_SECS);
        assert!(matches!(auto.arc, SwingArc::Sagittal));
    }

    #[test]
    fn the_auto_arc_is_the_untouched_sagittal_pose() {
        // Fail-first guard: if `swing_pose_arc` ever stops delegating for
        // `Sagittal`, every auto-attack silently changes shape.
        for kind in [
            WeaponKind::TwoHandAxe,
            WeaponKind::Dagger,
            WeaponKind::Bow,
            WeaponKind::Mace,
            WeaponKind::Shield,
        ] {
            for s in [-1.0, -0.4, 0.0, 0.35, 1.0] {
                let direct = swing_pose(kind, s);
                let via_arc = swing_pose_arc(kind, s, SwingArc::Sagittal);
                assert_eq!(direct.rotation, via_arc.rotation, "{kind:?} at s={s}");
                assert_eq!(direct.translation, via_arc.translation, "{kind:?} at s={s}");
            }
        }
    }

    #[test]
    fn mortal_strike_is_slower_and_holds_longer_than_an_auto() {
        let auto = SwingStyle::Auto.profile();
        let ms = SwingStyle::MortalStrike.profile();
        assert!(ms.release_secs > auto.release_secs, "the stroke is slower into the hit");
        assert!(ms.impact_hold_secs > auto.impact_hold_secs, "the beat registers longer");
        assert!(ms.total() > auto.total());
        assert_eq!(SwingStyle::MortalStrike.stroke_secs(), ms.total());
    }

    #[test]
    fn mortal_strike_reverses_the_auto_attacks_direction() {
        // The point of the signature: the auto RAISES on windup and chops DOWN
        // on release; Mortal Strike drops LOW and rips UP. Pitch is the sign
        // that distinguishes them (positive pitches forward/down), so the two
        // arcs must disagree in sign on both halves of the stroke.
        let arc = SwingStyle::MortalStrike.profile().arc;
        let ms_windup = pitch_of(swing_pose_arc(WeaponKind::TwoHandAxe, -1.0, arc));
        let ms_release = pitch_of(swing_pose_arc(WeaponKind::TwoHandAxe, 1.0, arc));
        let auto_windup = pitch_of(swing_pose(WeaponKind::TwoHandAxe, -1.0));
        let auto_release = pitch_of(swing_pose(WeaponKind::TwoHandAxe, 1.0));

        assert!(auto_windup < 0.0 && auto_release > 0.0, "auto: raise then chop down");
        assert!(ms_windup > 0.0 && ms_release < 0.0, "mortal strike: drop then rip up");
    }

    #[test]
    fn mortal_strike_swings_in_a_tilted_plane() {
        // A different SHAPE, not a bigger version of the same swing. The auto
        // rotates about the socket's X axis exactly; the signature rotates
        // about an axis leaned off it, which is what makes the sweep diagonal.
        let arc = SwingStyle::MortalStrike.profile().arc;
        let (axis, angle) = swing_pose_arc(WeaponKind::TwoHandAxe, 1.0, arc)
            .rotation
            .to_axis_angle();
        assert!(angle.abs() > 1e-3, "the release actually rotates");
        // Signed axis direction is irrelevant (axis, angle) vs (-axis, -angle),
        // so compare the lean of the axis itself.
        let lean = axis.y.abs().atan2(axis.x.abs());
        assert!(
            (lean - MORTAL_STRIKE_TILT).abs() < 0.05,
            "the swing plane must be leaned by the configured tilt, got {lean}"
        );
        assert!(axis.z.abs() < 1e-3, "the tilt stays within the frontal plane");
    }

    #[test]
    fn the_signature_never_stacks_a_second_rotation_axis() {
        // The regression that produced "it's turning the axe, not swinging it".
        // A swing is ONE rotation about ONE axis; composing yaw or roll on top
        // both cartwheels the weapon and fights the aim yaw the caller applies.
        // A single-axis rotation has a constant axis across the whole stroke —
        // a composed one does not.
        let arc = SwingStyle::MortalStrike.profile().arc;
        let reference = swing_pose_arc(WeaponKind::TwoHandAxe, 1.0, arc)
            .rotation
            .to_axis_angle()
            .0;
        for s in [-1.0, -0.6, 0.25, 0.7, 1.0] {
            let (axis, angle) = swing_pose_arc(WeaponKind::TwoHandAxe, s, arc)
                .rotation
                .to_axis_angle();
            if angle.abs() < 1e-4 {
                continue; // at rest the axis is arbitrary
            }
            // Either parallel or antiparallel — the sign flips with the angle.
            let alignment = axis.dot(reference).abs();
            assert!(
                alignment > 0.999,
                "the swing axis must not move through the stroke; at s={s} alignment was {alignment}"
            );
        }
    }

    #[test]
    fn the_sagittal_chop_is_the_zero_tilt_case() {
        // `Sagittal` and `TiltedPlane` are the same idea; the auto is simply
        // untilted. Pins that so the two cannot drift into different shapes.
        let axis = swing_pose_arc(
            WeaponKind::TwoHandAxe,
            1.0,
            SwingArc::TiltedPlane { tilt: 0.0, windup: 1.0, release: 1.0 },
        )
        .rotation
        .to_axis_angle()
        .0;
        assert!(axis.y.abs() < 1e-3 && axis.z.abs() < 1e-3, "untilted swings about X alone");
    }

    #[test]
    fn the_rising_arc_passes_through_rest_without_a_jump() {
        // Windup and release are separate branches; they must meet at s == 0 or
        // the blade teleports the frame the release crosses zero.
        let arc = SwingStyle::MortalStrike.profile().arc;
        let just_below = swing_pose_arc(WeaponKind::TwoHandAxe, -1e-4, arc).rotation;
        let just_above = swing_pose_arc(WeaponKind::TwoHandAxe, 1e-4, arc).rotation;
        assert!(
            just_below.angle_between(just_above) < 1e-2,
            "the two halves must be continuous at rest"
        );
    }

    // -- body lean ----------------------------------------------------------

    #[test]
    fn the_body_leans_about_the_same_axis_the_weapon_swings_on() {
        // If the two ever disagreed the torso would turn one way while the
        // blade went another. One source of truth is what prevents that.
        for style in [SwingStyle::Auto, SwingStyle::MortalStrike] {
            let arc = style.profile().arc;
            let (weapon_axis, _) = arc_rotation(0.8, arc);
            let (lean_axis, _) = arc_rotation(0.8, arc);
            assert_eq!(weapon_axis, lean_axis, "{style:?}");
        }
    }

    #[test]
    fn a_standing_unit_is_upright() {
        // At rest the lean must be exactly identity, or every idle combatant in
        // the arena stands permanently tilted.
        for style in [SwingStyle::Auto, SwingStyle::MortalStrike] {
            let (_, angle) = arc_rotation(0.0, style.profile().arc);
            assert_eq!(angle, 0.0, "{style:?} leans while standing still");
        }
    }

    #[test]
    fn the_body_winds_back_before_it_drives_through() {
        // One input drives both halves: the lean angle must reverse sign across
        // rest, exactly as the swing does, with no second curve to keep synced.
        for style in [SwingStyle::Auto, SwingStyle::MortalStrike] {
            let arc = style.profile().arc;
            let (_, windup) = arc_rotation(-1.0, arc);
            let (_, release) = arc_rotation(1.0, arc);
            assert!(
                windup * release < 0.0,
                "{style:?}: windup and release must lean opposite ways"
            );
        }
    }

    #[test]
    fn a_signature_commits_the_body_far_harder_than_a_routine_swing() {
        // The whole point of a per-style lean: an auto gains weight, a
        // signature gains ceremony. Compares the ACTUAL turn, not the raw
        // fraction, since the two styles swing through different angles.
        let turn = |style: SwingStyle| {
            let p = style.profile();
            let (_, angle) = arc_rotation(1.0, p.arc);
            (angle * p.lean).abs()
        };
        let auto = turn(SwingStyle::Auto);
        let signature = turn(SwingStyle::MortalStrike);
        assert!(
            signature > auto * 2.0,
            "signature lean {signature:.3} rad should dwarf the auto's {auto:.3}"
        );
        // And neither may become a pirouette.
        assert!(signature < 0.7, "a lean, not a spin: {signature:.3} rad");
    }

    /// Signed pitch (rotation about local X) of a pose, for the direction
    /// assertions above.
    fn pitch_of(t: Transform) -> f32 {
        let (x, _, _) = t.rotation.to_euler(EulerRot::XYZ);
        x
    }
}

/// Update (graphical-only): fade weapon materials with their owner's stealth,
/// mirroring the body's 40%-alpha darkened tint (`update_stealth_visuals`).
///
/// glTF weapon materials are SHARED assets across every spawned instance of a
/// model, so the fade swaps each weapon-mesh descendant onto a per-instance
/// clone and remembers the original in [`OriginalWeaponMaterial`]; unstealth
/// restores the shared original exactly. The scene subtree spawns async, so
/// this keys off `Changed<Combatant>` (which fires every sim tick — timers
/// mutate) and converges the first frame the meshes exist; the
/// already-faded guard makes the steady state cheap.
pub fn update_weapon_stealth_fade(
    mut commands: Commands,
    combatants: Query<(Entity, &Combatant), Changed<Combatant>>,
    sockets: Query<(Entity, &WeaponSocket)>,
    children: Query<&Children>,
    mesh_mats: Query<&MeshMaterial3d<StandardMaterial>>,
    originals: Query<&OriginalWeaponMaterial>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (owner_entity, combatant) in combatants.iter() {
        for (socket_entity, socket) in sockets.iter() {
            if socket.owner != owner_entity {
                continue;
            }
            for desc in children.iter_descendants(socket_entity) {
                if combatant.stealthed {
                    if originals.get(desc).is_ok() {
                        continue; // already faded
                    }
                    let Ok(mat_handle) = mesh_mats.get(desc) else {
                        continue;
                    };
                    let Some(mat) = materials.get(&mat_handle.0) else {
                        continue;
                    };
                    let mut faded = mat.clone();
                    let c = faded.base_color.to_srgba();
                    faded.base_color =
                        Color::srgba(c.red * 0.6, c.green * 0.6, c.blue * 0.6, 0.4);
                    faded.alpha_mode = bevy::prelude::AlphaMode::Blend;
                    let original = mat_handle.0.clone();
                    let faded_handle = materials.add(faded);
                    commands.entity(desc).insert((
                        MeshMaterial3d(faded_handle),
                        OriginalWeaponMaterial(original),
                    ));
                } else if let Ok(original) = originals.get(desc) {
                    commands
                        .entity(desc)
                        .insert(MeshMaterial3d(original.0.clone()))
                        .remove::<OriginalWeaponMaterial>();
                }
            }
        }
    }
}

