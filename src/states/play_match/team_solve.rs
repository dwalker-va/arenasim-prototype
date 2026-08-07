//! Step 4 of `design-docs/team-level-positioning-ai.md`: positioning as
//! constraint satisfaction, solved for the whole team at once and rooted at a
//! focal unit.
//!
//! ## Consumption status (2026-08-06) — read this before extending
//!
//! | Piece | Status |
//! |---|---|
//! | `OccupyCover` via [`solve_position`] | **LIVE** under `TeamPlan` (healer PRESSURED/FREE, `healer_postures.rs`). Measured at n=100 head-to-head: +36pt Warlock+Priest, +14pt Hunter+Priest, +10pt Warrior+Priest, -6pt (noise) Rogue+Priest. |
//! | `HoldRange` | Wired to the Mage/Hunter kiter, **measured ~-17pt, reverted**. Constraint definition kept; see "the framing does not fit a kiter" in the design doc before retrying. |
//! | `ScreenPartner`, `PressTarget`, `StackAnchor` | **NEVER RUN IN BATTLE.** Unit-tested against their written definitions only. All three consumed intents were under-specified in ways only measurement exposed (sight-of-ally, castability, the range ceiling) — assume these carry the same debt and budget a measurement pass before trusting them. |
//! | [`solve_team`] / [`solve_order`] / [`focal_point`] / [`assign_intents`] / cohesion | **NO CALLERS.** The dependent team-level solve, kept because the design requires convergent AND divergent shapes from the start (retrofitting divergence would mean redoing the solve). It has never placed a unit in a real match. |
//! | `plan.kill_target` (produced in `team_plan.rs`) | Producer-only: step 5 measured every static held call as net-harmful for some side and the consumer was reverted. Mid-match switching is the prerequisite for consuming it. |
//!
//! Everything below is gated on the per-team `AiProfiles`; `Legacy` never enters
//! this module and its recorded baselines are byte-identical.
//!
//! ## Why a solve rather than more weights
//!
//! Today's positioning sums single-objective interest terms
//! (`cover_pull`, `los_seek`, `formation_pull`, ...) whose weights have to be
//! hand-balanced so they do not fight. The three cover behaviours — `cover_pull`,
//! `cover_seek`, `medic_chase` — are mutually exclusive by construction and are
//! kept apart by an HP threshold. "See my ally, and stay hidden from their
//! caster" is then unrepresentable: it is two behaviours arbitrated by a number,
//! not one query.
//!
//! Here it is one query. A unit's [`RoleIntent`] names a CONSTRAINT SET; any
//! position satisfying it is acceptable, and the tie-break picks between them.
//!
//! ## The solve is dependent, and the order is load-bearing
//!
//! Units are not independent: `StackAnchor` means "converge on where my team
//! already is", and a pincer means "go where they are NOT". Both need a unit to
//! see teammates' chosen positions, so the solve runs in a defined order and each
//! unit reads the placements made before it — see [`solve_order`].
//!
//! Supporting BOTH shapes from the start is a requirement, not polish: the design
//! doc calls out that retrofitting divergence later would mean redoing the solve.
//! Divergence is why [`SolveContext::placed`] exists even though no intent
//! currently spreads the team.

use bevy::prelude::*;
use std::collections::BTreeMap;

use super::arena_bounds::ArenaBounds;
use super::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT, MOVER_RADIUS};
use super::team_plan::{Anchor, RoleIntent, Stance};

/// Where a candidate position is probed from, as an offset ring around the
/// unit's current position. Matches the existing scorer's 16-way compass so the
/// two can be compared like for like during the migration.
pub const SOLVE_DIRECTIONS: usize = 16;

/// How far ahead a candidate sits, in yards. Same role as the scorer's
/// `lookahead`: far enough to see a wall or a pillar edge coming, short enough
/// that the executor's per-frame step stays on the chosen bearing.
pub const SOLVE_LOOKAHEAD: f32 = 2.0;

/// One unit, reduced to what the solve reads. Pets are included because they are
/// threats and blockers, but they never receive an intent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolveUnit {
    pub entity: Entity,
    pub team: u8,
    pub slot: u8,
    pub pos: Vec2,
    pub is_healer: bool,
    pub is_melee: bool,
    pub is_pet: bool,
    /// The unit's own preferred engagement range — `CharacterClass::preferred_range`.
    /// `HoldRange` uses it as the ring's OUTER edge.
    pub ability_range: f32,
    /// Can this unit land a heal RIGHT NOW — i.e. is it neither silenced nor
    /// locked out of its healing school? See [`violations`]'s `OccupyCover` arm
    /// for why positioning depends on it.
    pub can_cast_heal: bool,
}

/// Everything the solve needs about the world, owned so it is trivially testable
/// without a `World`.
#[derive(Clone, Debug, Default)]
pub struct SolveWorld {
    /// Living units on BOTH teams, in deterministic order.
    pub units: Vec<SolveUnit>,
    pub obstacles: Vec<ObstacleVolume>,
    pub bounds: Option<ArenaBounds>,
    /// `shared.heal_range` from `movement.ron` — the leash for `OccupyCover`
    /// and `StackAnchor`.
    pub heal_range: f32,
    /// Radius inside which an enemy counts as "threatening" for `HoldRange`.
    pub threat_radius: f32,
    /// The team's called kill target, if any.
    pub kill_target: Option<Entity>,
}

impl SolveWorld {
    fn unit(&self, entity: Entity) -> Option<&SolveUnit> {
        self.units.iter().find(|u| u.entity == entity)
    }

    fn pos_of(&self, entity: Entity) -> Option<Vec2> {
        self.unit(entity).map(|u| u.pos)
    }

    /// Living enemies of `team`, pets included — a pet blocks and threatens.
    fn enemies_of(&self, team: u8) -> impl Iterator<Item = &SolveUnit> {
        self.units.iter().filter(move |u| u.team != team)
    }

    /// Enemy units that can hurt this team from RANGE — the ones cover is for.
    /// Units that close are excluded: no amount of occlusion stops them.
    ///
    /// Pets are classified by what they can actually DO, not by class. A pet
    /// inherits its owner's class (`Combatant::new_pet` makes a Felhunter a
    /// `Warlock` and every Hunter pet a `Hunter`), so a class-derived
    /// `is_melee` reads EVERY pet as a ranged caster. That is wrong in general
    /// and yet mostly right by accident here, which is why the obvious
    /// correction — filtering all pets out — measured badly (wins 11/12 -> 9/12,
    /// heal 349 -> 119): it removed the Felhunter, and a Felhunter really is a
    /// ranged threat to a healer. Spell Lock and Devour Magic both reach 30yd,
    /// and Spell Lock is precisely what stops the heals. See
    /// [`SolveUnit::is_melee`]'s assignment in `world_from_context`.
    fn enemy_casters_of(&self, team: u8) -> impl Iterator<Item = &SolveUnit> {
        self.enemies_of(team).filter(|u| !u.is_melee)
    }

    fn allies_of(&self, team: u8, except: Entity) -> impl Iterator<Item = &SolveUnit> + '_ {
        self.units
            .iter()
            .filter(move |u| u.team == team && u.entity != except && !u.is_pet)
    }

    fn sees(&self, a: Vec2, b: Vec2) -> bool {
        has_line_of_sight(
            &self.obstacles,
            Vec3::new(a.x, EYE_HEIGHT, a.y),
            Vec3::new(b.x, EYE_HEIGHT, b.y),
        )
    }
}

/// What the team orients around this tick.
///
/// `Press` roots on an ENEMY (the called kill target) and `Hold` on a map
/// feature, so the focus is not always one of our units — which is why this is
/// not simply an `Entity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Focus {
    /// A unit, ours or theirs.
    Unit(Entity),
    /// A fixed spot — the anchor's hold position under `Hold`.
    Point(Vec2),
}

/// The focal unit for `stance`, per the design doc's stance table:
///
/// | Stance | Focal |
/// |---|---|
/// | `Press` | the called kill target |
/// | `Withdraw` | our own healer |
/// | `Hold` | the anchor |
///
/// Returns `None` when the stance's focus does not exist (no kill target called,
/// no healer alive, no anchor), which the caller must treat as "no team solve
/// this tick" rather than substituting a different focus — a silently swapped
/// focus would move the whole team for a reason nothing recorded.
pub fn focal_point(
    stance: Stance,
    anchor: Option<Anchor>,
    team: u8,
    world: &SolveWorld,
) -> Option<Focus> {
    match stance {
        Stance::Press => world.kill_target.map(Focus::Unit),
        Stance::Withdraw(_) => world
            .units
            .iter()
            .find(|u| u.team == team && u.is_healer && !u.is_pet)
            .map(|u| Focus::Unit(u.entity)),
        Stance::Hold => match anchor? {
            Anchor::Point(p) => Some(Focus::Point(p)),
            Anchor::Obstacle(i) => {
                let (center, _) = world.obstacles.get(i)?.footprint_disc();
                Some(Focus::Point(center))
            }
        },
    }
}

/// Resolve a [`Focus`] to a world position.
pub fn focus_position(focus: Focus, world: &SolveWorld) -> Option<Vec2> {
    match focus {
        Focus::Unit(e) => world.pos_of(e),
        Focus::Point(p) => Some(p),
    }
}

/// Assign each living non-pet member of `team` its positional job.
///
/// This is the initial table, and it is the knob most likely to want tuning once
/// the solve is wired up — it encodes "who does what" per stance, which the
/// design doc specifies only as prose. Pets are excluded: a pet owes its owner a
/// flank, not a role in the team's plan.
///
/// `Press` splits DPS by reach, because the two want opposite things from the
/// same focal unit: melee must close (`PressTarget`), ranged must not
/// (`HoldRange`).
pub fn assign_intents(stance: Stance, team: u8, world: &SolveWorld) -> BTreeMap<Entity, RoleIntent> {
    let mut intents = BTreeMap::new();
    // Does this team still have anyone worth healing? A healer with no living
    // non-pet partner is not a healer any more, and `OccupyCover` degenerates
    // badly for it: with no anchor ally the sight and leash constraints go
    // vacuous, leaving only "stay hidden from casters" — so the last unit
    // standing holds cover for a corpse, forever, instead of fighting or
    // kiting. Watching a 2v1 is what surfaced this.
    //
    // "Partner" is ANY living non-pet teammate, healer or not — it has to match
    // `SolveContext::anchor_ally_pos`, which is what actually makes the sight
    // and leash constraints non-vacuous, and that does not filter healers. An
    // earlier version required a NON-healer partner, which sent both members of
    // a double-healer comp to `HoldRange` even though each had a live ally to
    // cover and heal.
    let has_partner = |me: Entity| {
        world
            .units
            .iter()
            .any(|u| u.team == team && !u.is_pet && u.entity != me)
    };
    for unit in world.units.iter().filter(|u| u.team == team && !u.is_pet) {
        let intent = match stance {
            // A healer with nobody left to heal fights at range instead: stay
            // out of reach, keep sight of the target so it can still cast.
            _ if unit.is_healer && !has_partner(unit.entity) => RoleIntent::HoldRange,
            // The healer's job never changes with stance: be able to heal, and
            // not be shootable while doing it. What changes is who else does what.
            _ if unit.is_healer => RoleIntent::OccupyCover,
            Stance::Press if unit.is_melee => RoleIntent::PressTarget,
            Stance::Press => RoleIntent::HoldRange,
            Stance::Withdraw(_) => RoleIntent::ScreenPartner,
            Stance::Hold => RoleIntent::StackAnchor,
        };
        intents.insert(unit.entity, intent);
    }
    intents
}

/// The order units are solved in. **Dependent constraints read only the
/// placements made before them, so this order decides what "converge" and
/// "spread" can even mean.**
///
/// 1. The focal unit, when it is one of ours (`Withdraw` roots on our healer).
///    Everything else is defined relative to it, so it cannot be defined
///    relative to anything else.
/// 2. Then healers — `StackAnchor` is leashed to the healer's `heal_range`, so a
///    healer placed after its team would leash them to a stale position.
/// 3. Then the rest by slot, which is arbitrary but deterministic. Slot order is
///    only ever a tie-break here, never the primary rule: the design doc is
///    explicit that a focal-rooted solve is meaningful where slot order is merely
///    deterministic.
pub fn solve_order(focus: Option<Focus>, team: u8, world: &SolveWorld) -> Vec<Entity> {
    let focal_entity = match focus {
        Some(Focus::Unit(e)) if world.unit(e).is_some_and(|u| u.team == team) => Some(e),
        _ => None,
    };
    let mut rest: Vec<&SolveUnit> = world
        .units
        .iter()
        .filter(|u| u.team == team && !u.is_pet && Some(u.entity) != focal_entity)
        .collect();
    rest.sort_by_key(|u| (!u.is_healer, u.slot));

    focal_entity
        .into_iter()
        .chain(rest.into_iter().map(|u| u.entity))
        .collect()
}

/// What one unit's constraints are evaluated against.
pub struct SolveContext<'a> {
    pub world: &'a SolveWorld,
    pub unit: SolveUnit,
    pub focus: Option<Vec2>,
    /// Teammates already placed this tick, in solve order. **This is what makes
    /// convergent and divergent team shapes both expressible**: `StackAnchor`
    /// reads it to converge, and a future pincer intent reads the same field to
    /// deliberately spread.
    pub placed: &'a BTreeMap<Entity, Vec2>,
}

impl SolveContext<'_> {
    /// Where a teammate is *going* if it has already solved, else where it is.
    fn ally_pos(&self, ally: &SolveUnit) -> Vec2 {
        self.placed.get(&ally.entity).copied().unwrap_or(ally.pos)
    }

    /// The healer this unit is leashed to, if the team has one.
    fn healer_pos(&self) -> Option<Vec2> {
        if self.unit.is_healer {
            return None;
        }
        self.world
            .units
            .iter()
            .find(|u| u.team == self.unit.team && u.is_healer && !u.is_pet)
            .map(|u| self.ally_pos(u))
    }

    /// The ally an `OccupyCover` healer must stay able to reach — the nearest
    /// living non-pet teammate. `None` for a lone unit, which makes the leash
    /// vacuous rather than unsatisfiable.
    fn anchor_ally_pos(&self) -> Option<Vec2> {
        self.world
            .allies_of(self.unit.team, self.unit.entity)
            .map(|a| self.ally_pos(a))
            .min_by(|a, b| {
                a.distance(self.unit.pos)
                    .partial_cmp(&b.distance(self.unit.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// Which constraints a candidate position violates, as a bitmask. `0` means the
/// candidate fully satisfies the unit's intent.
///
/// A bitmask rather than a bool so the relaxation ladder can drop constraints in
/// a defined order, and so a trace can say WHICH constraint was impossible —
/// "the healer had nowhere to stand" is not an actionable diagnosis.
pub const C_OCCLUDED: u16 = 1 << 0;
/// Must keep line of sight to the thing named by the intent.
pub const C_SIGHT: u16 = 1 << 1;
/// Must stay within a range leash (`heal_range`, or ability range).
pub const C_LEASH: u16 = 1 << 2;
/// Must stay OUTSIDE enemy threat range.
pub const C_STANDOFF: u16 = 1 << 3;
/// Must stay inside the arena.
pub const C_BOUNDS: u16 = 1 << 4;
/// Must be on the same side of the focus as the rest of the team.
pub const C_COHESION: u16 = 1 << 5;

/// Evaluate `intent`'s constraint set at `candidate`.
///
/// Each arm is the design doc's bullet for that intent, transcribed:
///
/// - `OccupyCover` — occluded from enemy casters; within `heal_range` of the anchor ally.
/// - `ScreenPartner` — has LoS to the partner; lacks LoS to the enemy kill target.
/// - `PressTarget` — in ability range of the kill target; has LoS to it.
/// - `HoldRange` — outside enemy threat range; retains LoS to the kill target.
/// - `StackAnchor` — same side of the anchor as the rest of the team; occluded
///   from enemy casters; within `heal_range` of the healer.
pub fn violations(intent: RoleIntent, candidate: Vec2, ctx: &SolveContext) -> u16 {
    let world = ctx.world;
    let mut v = 0u16;

    if let Some(bounds) = &world.bounds {
        if !bounds.contains(Vec3::new(candidate.x, 0.0, candidate.y)) {
            v |= C_BOUNDS;
        }
    }

    let occluded_from_casters = |p: Vec2| {
        !world
            .enemy_casters_of(ctx.unit.team)
            .any(|e| world.sees(p, e.pos))
    };

    match intent {
        RoleIntent::OccupyCover => {
            if !occluded_from_casters(candidate) {
                v |= C_OCCLUDED;
            }
            if let Some(ally) = ctx.anchor_ally_pos() {
                if candidate.distance(ally) > world.heal_range {
                    v |= C_LEASH;
                }
                // SIGHT OF THE ALLY, but only while the healer can actually use
                // it. The design doc's bullet lists occlusion and heal range;
                // its prose is explicit that the point of the solve is to make
                // "LoS to my ally, no LoS to their caster" ONE query rather than
                // two behaviours arbitrated by a threshold. Without sight the
                // intent is satisfiable from a spot the healer cannot heal from
                // — the step-3 residual this step exists to fix.
                //
                // CONDITIONED ON CASTABILITY, and that is what resolves the
                // cover-versus-line fight. A line of sight is worth exactly
                // NOTHING during a school lockout, so holding one then buys no
                // healing and costs real exposure: on seed 10 the Priest was
                // counterspelled, held its line anyway, and was feared twice
                // through the window its partner died in. Static weighting
                // cannot express that — sight either outranks safety always
                // (measured: 5.0s of cover, 9.1s of hard CC) or never. It is
                // temporal, not a tradeoff.
                if ctx.unit.can_cast_heal && !world.sees(candidate, ally) {
                    v |= C_SIGHT;
                }
            }
            // NARROWING THIS TO "something is mid-cast at me" WAS TRIED AND LOST
            // BADLY: 11/12 -> 8/12 wins, heal 348 -> 258, match length 60s ->
            // 91s. The idea was to stop the duck wasting GCDs, since Mind Blast
            // is Shadow and therefore free during a Holy lockout. It is not
            // ability priority that stops it — Mind Blast already falls through
            // below Flash Heal with no health guard — it is POSITION: over one
            // measured match its rejections were 954 OutOfRange and 510
            // LosBlocked against 4 OnCooldown, i.e. ducking is exactly what puts
            // it out of reach.
            //
            // So the two are in real tension and the duck wins on the numbers.
            // (The predicate was also weak — `CombatantInfo::target` is the kill
            // target, not the cast's target, so "casting at me" rarely fired.
            // Fixing that needs the casting target exposed on the snapshot;
            // worth redoing only if the tension is revisited.)
            //
            // ...and while it CANNOT cast, buy distance instead. These two are
            // mutually exclusive by construction, so they never compete for
            // weight — which is why an unconditional standoff constraint failed
            // (it perturbed the choice without ever binding) and this does not.
            if !ctx.unit.can_cast_heal
                && world
                    .enemy_casters_of(ctx.unit.team)
                    .any(|e| candidate.distance(e.pos) < world.threat_radius)
            {
                v |= C_STANDOFF;
            }
        }
        RoleIntent::ScreenPartner => {
            if let Some(ally) = ctx.anchor_ally_pos() {
                if !world.sees(candidate, ally) {
                    v |= C_SIGHT;
                }
            }
            // Deny the enemy kill target its line. Unlike `OccupyCover` this is
            // about ONE enemy, because screening is a duty owed against a
            // specific threat rather than blanket cover.
            if let Some(kt) = world.kill_target.and_then(|e| world.pos_of(e)) {
                if world.sees(candidate, kt) {
                    v |= C_OCCLUDED;
                }
            }
        }
        RoleIntent::PressTarget => {
            if let Some(focus) = ctx.focus {
                if candidate.distance(focus) > world.heal_range.min(ABILITY_REACH) {
                    v |= C_LEASH;
                }
                if !world.sees(candidate, focus) {
                    v |= C_SIGHT;
                }
            }
        }
        RoleIntent::HoldRange => {
            if world
                .enemies_of(ctx.unit.team)
                .any(|e| candidate.distance(e.pos) < world.threat_radius)
            {
                v |= C_STANDOFF;
            }
            if let Some(focus) = ctx.focus {
                if !world.sees(candidate, focus) {
                    v |= C_SIGHT;
                }
                // AND an outer leash. The doc's bullet gives this intent a floor
                // ("outside enemy threat range") but no ceiling, which is not a
                // ring — it is a half-space, and it is satisfied best by walking
                // to the far wall and never casting again. The floor and the
                // ceiling together ARE the tuned `range_band` this replaces.
                if candidate.distance(focus) > ctx.unit.ability_range {
                    v |= C_LEASH;
                }
            }
        }
        RoleIntent::StackAnchor => {
            if !occluded_from_casters(candidate) {
                v |= C_OCCLUDED;
            }
            if let Some(healer) = ctx.healer_pos() {
                if candidate.distance(healer) > world.heal_range {
                    v |= C_LEASH;
                }
            }
            // "Same side of the anchor as the rest of the team" — the convergent
            // constraint, and the one that reads `placed`. A candidate more than
            // a right angle away from where the team already sits, as seen from
            // the focus, is on the wrong side.
            if let (Some(focus), Some(team_bearing)) = (ctx.focus, team_bearing(ctx)) {
                let cand_bearing = (candidate - focus).normalize_or_zero();
                if cand_bearing != Vec2::ZERO && cand_bearing.dot(team_bearing) < 0.0 {
                    v |= C_COHESION;
                }
            }
        }
    }
    v
}

/// Melee reach used by `PressTarget`'s leash. Deliberately generous — the intent
/// says "in ability range", and the tightest range (melee) is enforced by the
/// pursuit executor, not by the positioning solve.
const ABILITY_REACH: f32 = 30.0;

/// Mean bearing from the focus to the already-placed teammates. `None` when
/// nobody has been placed yet — the first unit to solve has no team to be on the
/// same side as, so cohesion is vacuous for it rather than unsatisfiable.
fn team_bearing(ctx: &SolveContext) -> Option<Vec2> {
    let focus = ctx.focus?;
    let mut sum = Vec2::ZERO;
    let mut n = 0;
    for (entity, pos) in ctx.placed {
        if *entity == ctx.unit.entity {
            continue;
        }
        let b = (*pos - focus).normalize_or_zero();
        if b != Vec2::ZERO {
            sum += b;
            n += 1;
        }
    }
    (n > 0).then(|| sum.normalize_or_zero()).filter(|b| *b != Vec2::ZERO)
}

/// Clearance beyond an obstacle's own footprint when standing in its shadow, so
/// a unit sits BEHIND cover rather than flush against its collision skin.
pub const COVER_STANDOFF: f32 = 1.5;

/// Bearings either side of a shadow's centre line, in radians, giving positions
/// at the shadow's EDGE — still mostly covered, but able to see past it. This is
/// the "poke out to heal" spot expressed as geometry rather than as a timer.
pub const PEEK_OFFSETS: [f32; 2] = [-0.45, 0.45];

/// Distances BEYOND the obstacle's skin to sample along a shadow's centre line,
/// in yards. Cover at depth is the same cover, but out of CC range.
pub const SHADOW_DEPTHS: [f32; 4] = [0.0, 8.0, 18.0, 28.0];

/// How far along the line toward the ally to sample, in yards.
const ALLY_APPROACH_STEPS: [f32; 3] = [4.0, 10.0, 20.0];

/// Candidate positions for a unit, DERIVED FROM THE MATCH GEOMETRY.
///
/// **Not a lattice around the mover, and that distinction is what makes the
/// movement look deliberate.** An earlier version sampled fixed rings at fixed
/// bearings relative to the unit. Those candidates are only stable in the unit's
/// OWN frame: as it moves, the whole lattice moves with it, so the winning
/// candidate hops from one lattice cell to the next and the unit visibly snaps
/// between arbitrary points, never settling. Watching a 2v1 is what made it
/// obvious — the healer "swapped between fixed positions" instead of holding one.
///
/// These candidates are anchored to things that exist in the world:
///
/// - the unit's current position (stand still),
/// - for every (obstacle, enemy caster) pair, the point that puts that obstacle
///   between the unit and that caster — plus the two [`PEEK_OFFSETS`] shoulders
///   of the same shadow,
/// - points along the line toward the ally that must be seen and reached,
/// - a small local ring, so there is always a fine-grained gradient to follow
///   even when none of the above helps.
///
/// A pillar's shadow point moves slowly and continuously as the caster moves, so
/// the unit walks to it and STAYS there — and when the fight shifts, the target
/// slides rather than teleporting to the next lattice cell.
pub fn candidates_for(ctx: &SolveContext) -> Vec<Vec2> {
    let world = ctx.world;
    let mut out = vec![ctx.unit.pos];

    // Cover geometry: for each (obstacle, caster) pair, sample ALONG the shadow
    // rather than only at the pillar's skin.
    //
    // A shadow is a REGION, not a point. Sampling only the ring hugs the pillar,
    // which is the worst place to stand when the caster is using that same
    // pillar — measured at 32.3yd mean distance to the nearest caster and 7.2s
    // of hard CC, both worse than sampling a plain lattice. Standing 25yd back
    // along the same bearing is equally hidden and far out of Fear range.
    //
    // The peek shoulders stay at the ring: peeking only works near the edge,
    // where a small step recovers the sightline.
    for obstacle in &world.obstacles {
        let (center, radius) = obstacle.footprint_disc();
        let ring = radius + MOVER_RADIUS + COVER_STANDOFF;
        for caster in world.enemy_casters_of(ctx.unit.team) {
            let away = (center - caster.pos).normalize_or_zero();
            if away == Vec2::ZERO {
                continue;
            }
            let bearing = away.to_angle();
            for depth in SHADOW_DEPTHS {
                out.push(center + away * (ring + depth));
            }
            for offset in PEEK_OFFSETS {
                out.push(center + Vec2::from_angle(bearing + offset) * ring);
            }
        }
    }

    // Toward the ally we have to be able to see and reach.
    if let Some(ally) = ctx.anchor_ally_pos() {
        let toward = (ally - ctx.unit.pos).normalize_or_zero();
        if toward != Vec2::ZERO {
            for step in ALLY_APPROACH_STEPS {
                out.push(ctx.unit.pos + toward * step);
            }
        }
    }

    // Local ring: the fine adjustment, and the fallback gradient when no
    // geometric candidate is an improvement.
    for i in 0..SOLVE_DIRECTIONS {
        let a = (i as f32) * std::f32::consts::TAU / SOLVE_DIRECTIONS as f32;
        out.push(ctx.unit.pos + Vec2::from_angle(a) * SOLVE_LOOKAHEAD);
    }
    out
}

/// How badly `candidate` misses `intent`, as a continuous measure that is
/// EXACTLY zero when every constraint is satisfied.
///
/// **This exists because satisfaction alone gives no gradient.** A candidate ring
/// is only [`SOLVE_LOOKAHEAD`] wide, so a unit 5yd inside a 12yd threat radius
/// has no satisfying candidate at all — every one violates `C_STANDOFF`. A pure
/// satisfy/relax ladder then drops the constraint, every candidate ties, and the
/// nearest-satisfying tie-break picks *standing still*: the unit sits inside the
/// threat radius forever. That is the statue pathology the U6 probes were written
/// against, rediscovered here by the solve's own unit test.
///
/// So: satisfaction stays binary and primary (a zero-cost candidate always wins),
/// and this measure only decides which way to walk when the feasible set is out
/// of reach. The per-constraint scales are priorities among *violated*
/// constraints, not competing interests — nothing here can pull a unit off a
/// position that already satisfies its intent.
fn infeasibility(intent: RoleIntent, candidate: Vec2, ctx: &SolveContext) -> f32 {
    let world = ctx.world;
    // Scales, in the order the design doc treats them: giving up sight or cover
    // defeats the intent, the leash and standoff degrade it, cohesion is a
    // preference.
    // Sight OUTRANKS cover, and deliberately: these two are near-opposite
    // demands for a healer (its partner fights in roughly the same direction as
    // the enemies shooting at it), so when no position satisfies both, the
    // solve must choose. A healer that cannot heal is worse than one that can be
    // shot at — the same call the step-3 camp made when it filtered candidates
    // on ally sight and only scored occlusion among the survivors.
    const W_SIGHT: f32 = 400.0;
    const W_OCCLUDED: f32 = 100.0;
    const W_LEASH: f32 = 10.0;
    const W_STANDOFF: f32 = 10.0;
    const W_COHESION: f32 = 1.0;

    let v = violations(intent, candidate, ctx);
    let mut cost = 0.0;

    // Binary constraints contribute their full scale — there is no "partly in
    // line of sight". The gradient for these comes from the geometry moving the
    // candidate in or out of the shadow, not from a magnitude.
    if v & C_SIGHT != 0 {
        cost += W_SIGHT;
    }
    if v & C_OCCLUDED != 0 {
        // Graded by HOW MANY casters still see the spot, so a healer breaking one
        // of two sightlines is scored better than breaking neither.
        let seen = world
            .enemy_casters_of(ctx.unit.team)
            .filter(|e| world.sees(candidate, e.pos))
            .count();
        cost += W_OCCLUDED * seen.max(1) as f32;
    }
    if v & C_COHESION != 0 {
        cost += W_COHESION;
    }

    // Range constraints are genuinely continuous, and this is where the gradient
    // that matters comes from: it points along the shortest path back into range.
    if v & C_LEASH != 0 {
        let leash_ref = match intent {
            RoleIntent::PressTarget | RoleIntent::HoldRange => ctx.focus,
            RoleIntent::StackAnchor => ctx.healer_pos(),
            _ => ctx.anchor_ally_pos(),
        };
        if let Some(r) = leash_ref {
            let limit = match intent {
                RoleIntent::PressTarget => world.heal_range.min(ABILITY_REACH),
                RoleIntent::HoldRange => ctx.unit.ability_range,
                _ => world.heal_range,
            };
            cost += W_LEASH * (candidate.distance(r) - limit).max(0.0);
        }
    }
    if v & C_STANDOFF != 0 {
        // Deficit against the nearest enemy: walking away shrinks it smoothly,
        // which is what turns "I cannot escape this tick" into "step outward".
        let nearest = world
            .enemies_of(ctx.unit.team)
            .map(|e| candidate.distance(e.pos))
            .fold(f32::MAX, f32::min);
        if nearest.is_finite() {
            cost += W_STANDOFF * (world.threat_radius - nearest).max(0.0);
        }
    }
    cost
}

/// Pick the best position for one unit under `intent`.
///
/// Among candidates satisfying the (possibly relaxed) constraint set, takes the
/// one NEAREST the unit's current position. That tie-break is not arbitrary: it
/// is the step-3 lesson. A solve that jumps to the globally "best" spot makes the
/// unit chase a target that moves with the fight and arrive nowhere — measured at
/// 3.6-5.8yd of permanent lag, with the healer unable to see its ally 60-94% of
/// the time it was trying to. Nearest-satisfying is self-stabilising: once a unit
/// satisfies its intent, standing still also satisfies it, and standing still is
/// nearest.
pub fn solve_unit(intent: RoleIntent, ctx: &SolveContext) -> Vec2 {
    // Out-of-bounds is the one hard exclusion: a position outside the arena is
    // not a position, and unlike every other constraint there is no useful
    // gradient in "how far through the wall". Everything else is ranked.
    let in_bounds = |c: &Vec2| {
        ctx.world.bounds.as_ref().is_none_or(|b| b.contains(Vec3::new(c.x, 0.0, c.y)))
    };

    let mut best: Option<(f32, f32, Vec2)> = None;
    for candidate in candidates_for(ctx) {
        if !in_bounds(&candidate) {
            continue;
        }
        // Lexicographic: satisfy the intent first, and only then prefer not
        // moving. Standing still is candidate 0 and ties at equal cost, which is
        // what keeps a satisfied unit planted.
        //
        // A distance-MAXIMISING secondary was tried for `HoldRange`, to mimic the
        // Hunter's `flee` term, and measured worse: no change for the Hunter comp
        // and -8pt for the Mage. Reverted.
        let key = (infeasibility(intent, candidate, ctx), candidate.distance(ctx.unit.pos));
        if best.is_none_or(|(bc, bd, _)| (key.0, key.1) < (bc, bd)) {
            best = Some((key.0, key.1, candidate));
        }
    }
    // Every candidate out of bounds (a unit already outside the arena): hold.
    // The executor clamps and slides, so this is a safe terminal answer.
    best.map(|(_, _, p)| p).unwrap_or(ctx.unit.pos)
}

/// Solve the whole team, in [`solve_order`], each unit seeing the placements
/// before it.
pub fn solve_team(
    stance: Stance,
    anchor: Option<Anchor>,
    team: u8,
    world: &SolveWorld,
) -> BTreeMap<Entity, Vec2> {
    let focus = focal_point(stance, anchor, team, world);
    let focus_pos = focus.and_then(|f| focus_position(f, world));
    let intents = assign_intents(stance, team, world);
    let mut placed: BTreeMap<Entity, Vec2> = BTreeMap::new();

    for entity in solve_order(focus, team, world) {
        let Some(unit) = world.unit(entity).copied() else {
            continue;
        };
        let Some(intent) = intents.get(&entity).copied() else {
            continue;
        };
        let spot = {
            let ctx = SolveContext { world, unit, focus: focus_pos, placed: &placed };
            solve_unit(intent, &ctx)
        };
        placed.insert(entity, spot);
    }
    placed
}

/// Does this pet only threaten in melee, so cover does nothing against it?
///
/// Judged on reach, from `abilities.ron`:
/// - Felhunter — Spell Lock and Devour Magic both 30yd. NOT melee: breaking its
///   line is what stops it interrupting a heal.
/// - Spider — Web 20yd, a ranged root. NOT melee.
/// - Boar — Charge 25yd, but that is a GAP CLOSER; once it arrives cover is
///   worthless, so treating it as a caster just drags a healer around for
///   nothing. Melee.
/// - Bird — Master's Call (40yd) targets an ALLY; it has no ranged offence.
///   Melee.
fn pet_is_melee(pet_type: super::components::PetType) -> bool {
    use super::components::PetType as P;
    match pet_type {
        P::Felhunter | P::Spider => false,
        P::Boar | P::Bird => true,
    }
}

/// The spell school a class heals in. `None` for classes that do not heal — the
/// answer is unused for them, since only `OccupyCover` reads castability.
fn heal_school(class: crate::states::match_config::CharacterClass) -> Option<super::abilities::SpellSchool> {
    use crate::states::match_config::CharacterClass as C;
    use super::abilities::SpellSchool;
    match class {
        C::Priest | C::Paladin => Some(SpellSchool::Holy),
        C::Shaman => Some(SpellSchool::Nature),
        _ => None,
    }
}

/// Whether `info` could land a heal this instant: not silenced, and not locked
/// out of its healing school.
///
/// Deliberately about the HEAL specifically rather than "can act at all" — a
/// Priest with Holy locked but Shadow free can still cast Mind Blast, and should
/// still be positioning for safety rather than for a heal it cannot deliver.
fn can_cast_heal(ctx: &super::class_ai::CombatContext, info: &super::class_ai::CombatantInfo) -> bool {
    let Some(school) = heal_school(info.class) else {
        return false;
    };
    let auras = ctx.active_auras.get(&info.entity);
    let silenced = auras.is_some_and(|a| {
        a.iter().any(|aura| aura.effect_type == super::components::AuraType::Silence)
    });
    if silenced {
        return false;
    }
    // `is_spell_school_locked` takes `ActiveAuras`; the context stores the aura
    // vec directly, so rebuild the thin wrapper rather than duplicate the
    // magnitude-to-school decoding it owns.
    let wrapped = auras.map(|a| super::components::ActiveAuras { auras: a.clone() });
    !super::abilities::is_spell_school_locked(school, wrapped.as_ref())
}

/// Build a [`SolveWorld`] from the AI's per-frame view.
///
/// Living units only, in the `BTreeMap` order `CombatContext` already
/// guarantees, so the solve's inputs are deterministic without a sort. Stealthed
/// enemies are EXCLUDED: the solve must not position around information the team
/// does not have, and every other threat consumer in this codebase filters them
/// the same way.
pub fn world_from_context(
    ctx: &super::class_ai::CombatContext,
    heal_range: f32,
    threat_radius: f32,
    kill_target: Option<Entity>,
) -> SolveWorld {
    let my_team = ctx.self_info().map(|c| c.team);
    let units = ctx
        .combatants
        .values()
        .filter(|c| c.is_alive)
        .filter(|c| Some(c.team) == my_team || !c.stealthed)
        .map(|c| SolveUnit {
            entity: c.entity,
            team: c.team,
            slot: c.slot,
            pos: Vec2::new(c.position.x, c.position.z),
            is_healer: c.class.is_healer(),
            // Pets: by ability reach, not by inherited class — see
            // `enemy_casters_of`.
            is_melee: if c.is_pet {
                c.pet_type.is_none_or(pet_is_melee)
            } else {
                c.class.is_melee()
            },
            is_pet: c.slot >= super::constants::PET_SLOT_BASE,
            ability_range: c.class.preferred_range(),
            can_cast_heal: can_cast_heal(ctx, c),
        })
        .collect();
    SolveWorld {
        units,
        obstacles: ctx.obstacles.to_vec(),
        bounds: Some(ctx.bounds),
        heal_range,
        threat_radius,
        kill_target,
    }
}

/// Solve one unit's position, or `None` when it is already where it wants to be.
///
/// Returns a POSITION, not a bearing, because the chosen spot can be tens of
/// yards away (see [`SOLVE_RADII`]) and getting there may mean rounding a pillar.
/// `MovementGoal::Point` already navigates — it runs `steer_toward_goal`, which
/// aims at an obstacle's tangent instead of oozing along its face — whereas a
/// bare direction would walk the unit into the pillar between it and the cover it
/// picked.
pub fn solve_position(
    intent: RoleIntent,
    entity: Entity,
    world: &SolveWorld,
    focus: Option<Vec2>,
) -> Option<Vec2> {
    let unit = *world.unit(entity)?;
    let ctx = SolveContext { world, unit, focus, placed: &BTreeMap::new() };
    let spot = solve_unit(intent, &ctx);
    (spot.distance(unit.pos) > 1e-3).then_some(spot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::team_plan::WithdrawReason;

    fn e(id: u32) -> Entity {
        Entity::from_raw(id)
    }

    fn unit(id: u32, team: u8, slot: u8, x: f32, z: f32) -> SolveUnit {
        SolveUnit {
            entity: e(id),
            team,
            slot,
            pos: Vec2::new(x, z),
            is_healer: false,
            is_melee: false,
            is_pet: false,
            ability_range: 30.0,
            // Test healers can heal unless a case says otherwise.
            can_cast_heal: true,
        }
    }

    fn healer(id: u32, team: u8, slot: u8, x: f32, z: f32) -> SolveUnit {
        SolveUnit { is_healer: true, ..unit(id, team, slot, x, z) }
    }

    fn melee(id: u32, team: u8, slot: u8, x: f32, z: f32) -> SolveUnit {
        SolveUnit { is_melee: true, ..unit(id, team, slot, x, z) }
    }

    fn world(units: Vec<SolveUnit>) -> SolveWorld {
        SolveWorld {
            units,
            obstacles: Vec::new(),
            bounds: None,
            heal_range: 40.0,
            threat_radius: 12.0,
            kill_target: None,
        }
    }

    /// A single pillar at the origin, for the occlusion constraints.
    fn pillar_at(x: f32, z: f32) -> ObstacleVolume {
        ObstacleVolume::Prism {
            center_xz: Vec2::new(x, z),
            circumradius: 6.0,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        }
    }

    // --- focus ---

    #[test]
    fn press_roots_on_the_called_kill_target() {
        let mut w = world(vec![melee(1, 1, 0, -10.0, 0.0), unit(2, 2, 0, 10.0, 0.0)]);
        w.kill_target = Some(e(2));
        assert_eq!(focal_point(Stance::Press, None, 1, &w), Some(Focus::Unit(e(2))));
    }

    #[test]
    fn withdraw_roots_on_our_own_healer() {
        let w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            healer(2, 1, 1, -12.0, 0.0),
            healer(3, 2, 0, 10.0, 0.0),
        ]);
        let focus = focal_point(Stance::Withdraw(WithdrawReason::Recover), None, 1, &w);
        assert_eq!(focus, Some(Focus::Unit(e(2))), "must be OUR healer, not theirs");
    }

    #[test]
    fn hold_roots_on_the_anchor_obstacle() {
        let mut w = world(vec![melee(1, 1, 0, -10.0, 0.0)]);
        w.obstacles = vec![pillar_at(-40.0, -20.0)];
        assert_eq!(
            focal_point(Stance::Hold, Some(Anchor::Obstacle(0)), 1, &w),
            Some(Focus::Point(Vec2::new(-40.0, -20.0)))
        );
    }

    /// A missing focus must NOT silently fall back to a different one — the whole
    /// team would reposition for a reason nothing recorded.
    #[test]
    fn a_missing_focus_yields_none_rather_than_a_substitute() {
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0)]);
        assert_eq!(focal_point(Stance::Press, None, 1, &w), None, "no kill target called");
        assert_eq!(focal_point(Stance::Hold, None, 1, &w), None, "no anchor");
        assert_eq!(
            focal_point(Stance::Withdraw(WithdrawReason::Draw), None, 1, &w),
            None,
            "no healer alive"
        );
    }

    // --- intents ---

    #[test]
    fn press_splits_dps_by_reach_and_keeps_the_healer_in_cover() {
        let w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            unit(2, 1, 1, -20.0, 0.0),
            healer(3, 1, 2, -25.0, 0.0),
        ]);
        let i = assign_intents(Stance::Press, 1, &w);
        assert_eq!(i[&e(1)], RoleIntent::PressTarget, "melee closes");
        assert_eq!(i[&e(2)], RoleIntent::HoldRange, "ranged must not");
        assert_eq!(i[&e(3)], RoleIntent::OccupyCover);
    }

    #[test]
    fn hold_stacks_everyone_but_the_healer() {
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), healer(2, 1, 1, -12.0, 0.0)]);
        let i = assign_intents(Stance::Hold, 1, &w);
        assert_eq!(i[&e(1)], RoleIntent::StackAnchor);
        assert_eq!(i[&e(2)], RoleIntent::OccupyCover);
    }

    #[test]
    fn withdraw_screens_the_healer() {
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), healer(2, 1, 1, -12.0, 0.0)]);
        let i = assign_intents(Stance::Withdraw(WithdrawReason::Recover), 1, &w);
        assert_eq!(i[&e(1)], RoleIntent::ScreenPartner);
        assert_eq!(i[&e(2)], RoleIntent::OccupyCover);
    }

    /// Pets take orders from their owner, not from the team plan. The step-3
    /// review caught exactly this leak in the camp consumer.
    #[test]
    fn pets_never_receive_an_intent() {
        let pet = SolveUnit { is_pet: true, ..unit(9, 1, 10, -11.0, 0.0) };
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), pet]);
        let i = assign_intents(Stance::Hold, 1, &w);
        assert!(i.contains_key(&e(1)));
        assert!(!i.contains_key(&e(9)), "a pet must not get a positional job");
    }

    /// A healer's partner may itself be a healer. Both still have someone to
    /// cover and heal, so neither degenerates into the lone-unit `HoldRange`.
    #[test]
    fn two_healers_still_cover_for_each_other() {
        let w = world(vec![healer(1, 1, 0, -10.0, 0.0), healer(2, 1, 1, -12.0, 0.0)]);
        let i = assign_intents(Stance::Hold, 1, &w);
        assert_eq!(i[&e(1)], RoleIntent::OccupyCover);
        assert_eq!(i[&e(2)], RoleIntent::OccupyCover);
    }

    /// ...but a healer genuinely alone fights instead of camping for a corpse.
    #[test]
    fn a_lone_healer_holds_range_instead_of_camping() {
        let pet = SolveUnit { is_pet: true, ..unit(9, 1, 10, -11.0, 0.0) };
        let w = world(vec![healer(1, 1, 0, -10.0, 0.0), pet, unit(2, 2, 0, 10.0, 0.0)]);
        let i = assign_intents(Stance::Hold, 1, &w);
        assert_eq!(i[&e(1)], RoleIntent::HoldRange, "a pet is not a partner");
    }

    #[test]
    fn intents_are_per_team() {
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), melee(2, 2, 0, 10.0, 0.0)]);
        let i = assign_intents(Stance::Hold, 1, &w);
        assert_eq!(i.len(), 1, "must not assign jobs to the enemy team");
    }

    // --- solve order ---

    #[test]
    fn the_focal_unit_solves_first_when_it_is_ours() {
        let w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            healer(2, 1, 1, -12.0, 0.0),
            unit(3, 1, 2, -14.0, 0.0),
        ]);
        let order = solve_order(Some(Focus::Unit(e(3))), 1, &w);
        assert_eq!(order[0], e(3), "everything is defined relative to the focus");
    }

    /// With no focal unit of ours, the healer must still precede the units whose
    /// `StackAnchor` leash is measured to it.
    #[test]
    fn the_healer_precedes_the_units_leashed_to_it() {
        let w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            unit(2, 1, 1, -14.0, 0.0),
            healer(3, 1, 2, -12.0, 0.0),
        ]);
        let order = solve_order(Some(Focus::Point(Vec2::ZERO)), 1, &w);
        assert_eq!(order[0], e(3), "healer first");
        assert_eq!(order[1..], [e(1), e(2)], "then by slot");
    }

    #[test]
    fn solve_order_excludes_pets_and_the_enemy() {
        let pet = SolveUnit { is_pet: true, ..unit(9, 1, 10, -11.0, 0.0) };
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), pet, melee(2, 2, 0, 10.0, 0.0)]);
        assert_eq!(solve_order(None, 1, &w), vec![e(1)]);
    }

    // --- constraints ---

    #[test]
    fn occupy_cover_wants_occlusion_from_ranged_enemies() {
        let mut w = world(vec![healer(1, 1, 0, -10.0, 0.0), unit(2, 2, 0, 10.0, 0.0)]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        // Directly behind the pillar from the enemy: occluded.
        assert_eq!(violations(RoleIntent::OccupyCover, Vec2::new(-10.0, 0.0), &ctx) & C_OCCLUDED, 0);
        // Off to the side with a clear line: not occluded.
        assert_ne!(
            violations(RoleIntent::OccupyCover, Vec2::new(-10.0, 30.0), &ctx) & C_OCCLUDED,
            0
        );
    }

    /// Cover is for units that shoot. A melee enemy is not deterred by a pillar,
    /// so it must not be counted when deciding whether a spot is "in cover" —
    /// otherwise no spot ever qualifies once a melee closes.
    #[test]
    fn melee_enemies_do_not_count_as_casters_to_hide_from() {
        let mut w = world(vec![healer(1, 1, 0, -10.0, 0.0), melee(2, 2, 0, 10.0, 0.0)]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_eq!(
            violations(RoleIntent::OccupyCover, Vec2::new(-10.0, 30.0), &ctx) & C_OCCLUDED,
            0,
            "a melee enemy must not make an open spot count as 'exposed'"
        );
    }

    /// PINS THE KNOWN MISCLASSIFICATION, so a future pet-aware `is_melee` fails
    /// here loudly instead of silently re-tuning the healer. A pet inherits its
    /// OWNER's class, so a Felhunter reads as `Warlock` and every Hunter pet as
    /// `Hunter` — neither is `is_melee`, so both count as casters to hide from
    /// even though both close to melee. See `enemy_casters_of` for the sweep
    /// numbers that say this is currently load-bearing.
    #[test]
    fn enemy_pets_still_count_as_casters_to_hide_from() {
        let enemy_pet = SolveUnit { is_pet: true, ..unit(2, 2, 10, 10.0, 0.0) };
        let mut w = world(vec![healer(1, 1, 0, -10.0, 0.0), enemy_pet]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_ne!(
            violations(RoleIntent::OccupyCover, Vec2::new(-10.0, 30.0), &ctx) & C_OCCLUDED,
            0,
            "documented wart: a chasing pet is treated as a caster to hide from"
        );
    }

    /// The dual constraint, and the whole reason step 4 exists: cover is not
    /// cover if the healer cannot see the ally it is covering for.
    #[test]
    fn occupy_cover_also_demands_sight_of_the_ally() {
        let mut w = world(vec![
            healer(1, 1, 0, -10.0, 0.0),
            melee(2, 1, 1, 10.0, 0.0), // ally on the far side of the pillar
            unit(3, 2, 0, 0.0, 30.0),  // enemy caster off to one side
        ]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        // Standing where the pillar blocks the line to the ally fails, even
        // though it is beautifully hidden from the enemy.
        assert_ne!(
            violations(RoleIntent::OccupyCover, Vec2::new(-10.0, 0.0), &ctx) & C_SIGHT,
            0,
            "a spot the healer cannot heal from must not satisfy OccupyCover"
        );
    }

    /// THE CASTABILITY HINGE. A spell-locked healer must stop paying for a
    /// sightline it cannot use, and start paying for distance instead.
    ///
    /// This is the fix for the seed-10 loss: the Priest was counterspelled, held
    /// its line anyway because sight is statically weighted above safety, and was
    /// feared twice through the window its partner died in.
    #[test]
    fn a_spell_locked_healer_stops_paying_for_a_sightline() {
        let mut w = world(vec![
            healer(1, 1, 0, -10.0, 0.0),
            melee(2, 1, 1, 10.0, 0.0),
            unit(3, 2, 0, 12.0, 0.0),
        ]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        // Behind the pillar: hidden from the caster, blind to the ally.
        let hidden_but_blind = Vec2::new(-10.0, 0.0);

        let locked = SolveUnit { can_cast_heal: false, ..w.units[0] };
        let ctx = SolveContext {
            world: &w,
            unit: locked,
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_eq!(
            violations(RoleIntent::OccupyCover, hidden_but_blind, &ctx) & C_SIGHT,
            0,
            "a healer that cannot cast must not be charged for losing the line"
        );

        // ...and the same spot for a healer that CAN cast is a violation.
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_ne!(
            violations(RoleIntent::OccupyCover, hidden_but_blind, &ctx) & C_SIGHT,
            0,
            "a healer that CAN cast still needs the line"
        );
    }

    /// The other half: while locked out, distance from casters becomes a real
    /// constraint. The two are mutually exclusive by construction, which is why
    /// they never compete for weight — an unconditional standoff was measured
    /// and reverted precisely because it perturbed the choice without binding.
    #[test]
    fn standoff_applies_only_while_the_healer_cannot_cast() {
        let mut w = world(vec![
            healer(1, 1, 0, 0.0, 0.0),
            melee(2, 1, 1, 40.0, 0.0),
            unit(3, 2, 0, 5.0, 0.0), // caster well inside threat_radius (12)
        ]);
        w.threat_radius = 12.0;
        w.obstacles = vec![];

        let castable = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_eq!(
            violations(RoleIntent::OccupyCover, Vec2::ZERO, &castable) & C_STANDOFF,
            0,
            "a healer mid-rotation is not charged for standing close"
        );

        let locked = SolveUnit { can_cast_heal: false, ..w.units[0] };
        let ctx = SolveContext {
            world: &w,
            unit: locked,
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_ne!(
            violations(RoleIntent::OccupyCover, Vec2::ZERO, &ctx) & C_STANDOFF,
            0,
            "a locked-out healer must be charged for staying in CC range"
        );
    }

    /// When the two halves cannot both hold, sight of the ally must win — a
    /// healer that cannot heal is worse than one that can be shot at.
    #[test]
    fn sight_of_the_ally_outranks_cover_when_both_are_impossible() {
        let mut w = world(vec![
            healer(1, 1, 0, -10.0, 0.0),
            melee(2, 1, 1, 10.0, 0.0),
            unit(3, 2, 0, 12.0, 0.0), // caster right beside the ally
        ]);
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        let hidden_but_blind = Vec2::new(-10.0, 0.0);
        let seen_but_sighted = Vec2::new(0.0, 20.0);
        assert!(
            infeasibility(RoleIntent::OccupyCover, seen_but_sighted, &ctx)
                < infeasibility(RoleIntent::OccupyCover, hidden_but_blind, &ctx),
            "the position that can actually heal must score better"
        );
    }

    #[test]
    fn occupy_cover_is_leashed_to_heal_range() {
        let mut w = world(vec![healer(1, 1, 0, 0.0, 0.0), unit(2, 1, 1, 0.0, 0.0)]);
        w.heal_range = 10.0;
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        assert_eq!(violations(RoleIntent::OccupyCover, Vec2::new(5.0, 0.0), &ctx) & C_LEASH, 0);
        assert_ne!(violations(RoleIntent::OccupyCover, Vec2::new(50.0, 0.0), &ctx) & C_LEASH, 0);
    }

    /// The constraint the additive scorer could not express: see my ally AND deny
    /// the enemy kill target its line, in ONE query.
    #[test]
    fn screen_partner_demands_sight_of_the_ally_and_none_for_the_enemy() {
        let mut w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            healer(2, 1, 1, -20.0, 0.0),
            unit(3, 2, 0, 20.0, 0.0),
        ]);
        w.kill_target = Some(e(3));
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        // Behind the pillar: sees the ally at -20, hidden from the enemy at +20.
        let v = violations(RoleIntent::ScreenPartner, Vec2::new(-10.0, 0.0), &ctx);
        assert_eq!(v & (C_SIGHT | C_OCCLUDED), 0, "this is the position that should exist");
        // Way off to the side: sees both, so it fails the denial half.
        let v = violations(RoleIntent::ScreenPartner, Vec2::new(0.0, 40.0), &ctx);
        assert_ne!(v & C_OCCLUDED, 0);
    }

    #[test]
    fn hold_range_keeps_enemies_at_arms_length_but_stays_sighted() {
        let mut w = world(vec![unit(1, 1, 0, -30.0, 0.0), melee(2, 2, 0, 0.0, 0.0)]);
        w.threat_radius = 12.0;
        w.kill_target = Some(e(2));
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::new(0.0, 0.0)),
            placed: &BTreeMap::new(),
        };
        assert_eq!(violations(RoleIntent::HoldRange, Vec2::new(-30.0, 0.0), &ctx), 0);
        assert_ne!(
            violations(RoleIntent::HoldRange, Vec2::new(-5.0, 0.0), &ctx) & C_STANDOFF,
            0,
            "inside threat radius"
        );
    }

    #[test]
    fn press_target_wants_range_and_sight_of_the_focus() {
        let mut w = world(vec![melee(1, 1, 0, -50.0, 0.0), unit(2, 2, 0, 0.0, 0.0)]);
        w.obstacles = vec![pillar_at(-25.0, 0.0)];
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::ZERO),
            placed: &BTreeMap::new(),
        };
        // Far away and behind a pillar: fails both halves.
        let v = violations(RoleIntent::PressTarget, Vec2::new(-50.0, 0.0), &ctx);
        assert_ne!(v & C_LEASH, 0);
        assert_ne!(v & C_SIGHT, 0);
        // Close and clear: satisfies.
        assert_eq!(violations(RoleIntent::PressTarget, Vec2::new(-5.0, 5.0), &ctx), 0);
    }

    // --- cohesion: the convergent/divergent hinge ---

    /// `StackAnchor` must read already-placed teammates, or "same side as the
    /// rest of the team" has no meaning.
    #[test]
    fn stack_anchor_rejects_the_far_side_of_the_focus_from_the_team() {
        let w = world(vec![melee(1, 1, 0, 10.0, 0.0), melee(2, 1, 1, 12.0, 0.0)]);
        let mut placed = BTreeMap::new();
        placed.insert(e(2), Vec2::new(12.0, 0.0)); // team is at +x of the focus
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::ZERO),
            placed: &placed,
        };
        assert_eq!(
            violations(RoleIntent::StackAnchor, Vec2::new(10.0, 0.0), &ctx) & C_COHESION,
            0,
            "same side as the team"
        );
        assert_ne!(
            violations(RoleIntent::StackAnchor, Vec2::new(-10.0, 0.0), &ctx) & C_COHESION,
            0,
            "opposite side of the focus"
        );
    }

    /// The FIRST unit to solve has no placed teammates, so cohesion must be
    /// vacuous rather than unsatisfiable — otherwise the solve deadlocks on its
    /// own first step.
    #[test]
    fn cohesion_is_vacuous_for_the_first_unit_placed() {
        let w = world(vec![melee(1, 1, 0, 10.0, 0.0)]);
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::ZERO),
            placed: &BTreeMap::new(),
        };
        assert_eq!(violations(RoleIntent::StackAnchor, Vec2::new(-10.0, 0.0), &ctx) & C_COHESION, 0);
    }

    // --- the solve itself ---

    /// The step-3 lesson, encoded: a unit already satisfying its intent must not
    /// move. Chasing a globally-best spot is what left the camped healer
    /// permanently 3.6-5.8yd behind its own orders.
    #[test]
    fn a_satisfied_unit_stands_still() {
        let mut w = world(vec![unit(1, 1, 0, -30.0, 0.0), melee(2, 2, 0, 0.0, 0.0)]);
        w.kill_target = Some(e(2));
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::ZERO),
            placed: &BTreeMap::new(),
        };
        assert_eq!(solve_unit(RoleIntent::HoldRange, &ctx), Vec2::new(-30.0, 0.0));
    }

    /// THE GRADIENT PROBE. This unit is 5yd inside a 12yd threat radius and the
    /// candidate ring is only `SOLVE_LOOKAHEAD` (2yd) wide, so **no candidate
    /// satisfies `HoldRange` at all**. An earlier draft used a satisfy-or-relax
    /// ladder here: it dropped `C_STANDOFF`, every candidate tied, and the
    /// nearest-satisfying tie-break chose standing still — the unit sat inside
    /// the threat radius indefinitely, which is the statue pathology the U6
    /// probes exist to catch. `infeasibility` is what makes the unit walk out.
    #[test]
    fn an_unsatisfied_unit_steps_toward_satisfaction() {
        let mut w = world(vec![unit(1, 1, 0, -5.0, 0.0), melee(2, 2, 0, 0.0, 0.0)]);
        w.threat_radius = 12.0;
        w.kill_target = Some(e(2));
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: Some(Vec2::ZERO),
            placed: &BTreeMap::new(),
        };
        let spot = solve_unit(RoleIntent::HoldRange, &ctx);
        assert_ne!(spot, Vec2::new(-5.0, 0.0), "must move out of threat range");
        assert!(
            spot.distance(Vec2::ZERO) > 5.0,
            "must move AWAY from the threat, got {spot:?}"
        );
    }

    /// With nothing satisfiable, the ladder must still return a position rather
    /// than panicking or returning a degenerate direction.
    #[test]
    fn an_impossible_constraint_set_falls_back_to_holding_position() {
        // Enemy caster with a clear line from every direction, no cover at all.
        let w = world(vec![healer(1, 1, 0, 0.0, 0.0), unit(2, 2, 0, 5.0, 0.0)]);
        let ctx = SolveContext {
            world: &w,
            unit: w.units[0],
            focus: None,
            placed: &BTreeMap::new(),
        };
        let spot = solve_unit(RoleIntent::OccupyCover, &ctx);
        assert!(spot.is_finite(), "must return a usable position");
    }

    #[test]
    fn solve_team_places_every_non_pet_member_once() {
        let pet = SolveUnit { is_pet: true, ..unit(9, 1, 10, -11.0, 0.0) };
        let mut w = world(vec![
            melee(1, 1, 0, -10.0, 0.0),
            healer(2, 1, 1, -12.0, 0.0),
            pet,
            unit(3, 2, 0, 10.0, 0.0),
        ]);
        w.kill_target = Some(e(3));
        let placed = solve_team(Stance::Press, None, 1, &w);
        assert_eq!(placed.len(), 2, "both team-1 combatants, no pet, no enemy");
        assert!(placed.contains_key(&e(1)) && placed.contains_key(&e(2)));
    }

    /// Determinism is not optional: the solve shares a schedule with the AI, so a
    /// run-to-run difference here would desync every seeded replay.
    #[test]
    fn solve_team_is_deterministic() {
        let mut w = world(vec![
            melee(1, 1, 0, -10.0, 3.0),
            healer(2, 1, 1, -12.0, -4.0),
            unit(3, 2, 0, 10.0, 1.0),
        ]);
        w.kill_target = Some(e(3));
        w.obstacles = vec![pillar_at(0.0, 0.0)];
        let first = solve_team(Stance::Press, None, 1, &w);
        for _ in 0..16 {
            assert_eq!(solve_team(Stance::Press, None, 1, &w), first);
        }
    }

    /// A stance with no resolvable focus must still place everyone (falling back
    /// to their own constraints) rather than returning an empty map that a
    /// consumer would read as "nobody should move".
    #[test]
    fn a_missing_focus_still_places_the_team() {
        let w = world(vec![melee(1, 1, 0, -10.0, 0.0), healer(2, 1, 1, -12.0, 0.0)]);
        let placed = solve_team(Stance::Press, None, 1, &w);
        assert_eq!(placed.len(), 2);
    }
}
