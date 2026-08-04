//! Team-level strategy layer — the `TeamPlan` from
//! `design-docs/team-level-positioning-ai.md`.
//!
//! **The layer is inert under `AiProfile::Legacy`, which is the default and what
//! every recorded baseline runs.** The types, the resource, and the recompute
//! cadence all exist and run, but under `Legacy` every plan is
//! `TeamPlan::default()` (`anchor: None`), and nothing consumes a plan yet in
//! EITHER profile. That is the point — landing the scaffolding as a *provable*
//! no-op means the next step changes behaviour on purpose rather than by
//! accident, and any drift it causes is attributable to it alone.
//!
//! Under `AiProfile::TeamPlan` the planner does select a real anchor and stance
//! (see [`update_team_plans`]); that output is still read by nothing, so it is
//! observable only in tests.
//!
//! Verify the no-op with the recorded baseline, not the test suite:
//!
//! ```bash
//! scripts/behaviour_baseline.sh | diff tests/baselines/legacy_behaviour_2026-07-31.txt -
//! ```
//!
//! An empty diff means byte-identical simulation. The movement probes assert
//! *bounded* properties, so they would pass through a real behaviour change.
//!
//! ## Why three layers
//!
//! Strategy (this file) answers "what is our team trying to do", on a cadence of
//! seconds. Below it sits an obligation layer (duties owed to *another* unit —
//! peeling, screening) and then execution (the existing posture machinery). See
//! the design doc; the ordering matters because obligations are instrumental to
//! strategy, not parallel with it.
//!
//! ## Determinism
//!
//! The planner must never draw from `GameRng`. It runs inside the same schedule as
//! the AI, so consuming a random number would shift the draw order for everything
//! downstream and silently break seeded replay — the failure would look like an
//! unrelated behaviour change several systems away. Plan inputs are positions,
//! health and roster only.

use bevy::prelude::*;
use std::collections::BTreeMap;

use super::components::{Combatant, MatchCountdown};
use super::ai_profile::{AiProfile, AiProfiles};
use super::constants::PET_SLOT_BASE;
use super::map_config::ActiveMapGeometry;
use super::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT, MOVER_RADIUS};
use crate::states::match_config::CharacterClass;

/// Where a team wants the fight to happen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Index into `ActiveMapGeometry::volumes` — a pillar to fight around.
    Obstacle(usize),
    /// An arbitrary spot, for open-field plans.
    Point(Vec2),
}

/// Why a team withdrew. `Bait` and `Recover` were separate stances in an early
/// draft; they share their entire exit path (into `Press` on conversion or on
/// enemy overextension) and differ only in *why* the team retreated, so they are
/// one stance with a reason code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawReason {
    /// From strength — retreat to provoke an overcommit.
    Draw,
    /// From weakness — retreat because we must.
    Recover,
}

/// What a team is trying to do right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stance {
    /// Hold advantageous ground and make the enemy come to us.
    Hold,
    /// Press an advantage.
    #[default]
    Press,
    /// Retreat in order to counterattack.
    Withdraw(WithdrawReason),
}

/// One unit's positional job, resolved against the team's anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleIntent {
    /// Break enemy caster sight while staying in heal range of the anchor ally.
    OccupyCover,
    /// Keep sight of the partner while denying it to the enemy kill target.
    ScreenPartner,
    /// Get in ability range of the called target.
    PressTarget,
    /// Stay outside enemy threat range while keeping sight of the kill target.
    HoldRange,
    /// Converge on the team's shared anchor — the only intent where teammates
    /// deliberately stack rather than distribute.
    StackAnchor,
}

/// One team's plan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamPlan {
    /// Where this team wants the fight. `None` = open field, and is what step 2
    /// always produces.
    pub anchor: Option<Anchor>,
    pub stance: Stance,
    /// The team's called kill target. The plan owns this rather than each unit
    /// choosing independently; held constant for a whole match in v1.
    pub kill_target: Option<Entity>,
    /// Per-unit positional intent. `BTreeMap` (not `HashMap`) because plan
    /// iteration order must be stable at a fixed seed — the same determinism rule
    /// every other AI collection in this codebase follows.
    pub intents: BTreeMap<Entity, RoleIntent>,
}

/// Both teams' plans.
///
/// A fixed two-element array indexed `team - 1`, not a map: teams are always
/// exactly two, so an array is deterministic by construction and there is no
/// iteration order to get wrong.
#[derive(Resource, Clone, Debug, Default)]
pub struct TeamPlans {
    plans: [TeamPlan; 2],
    /// Roster fingerprint the current plans were computed from. Recomputing only
    /// when this changes is what makes the plan an intent that PERSISTS across
    /// seconds; a per-frame plan would just be the movement scorer with extra
    /// steps.
    roster: Vec<(u8, u8)>,
    /// Set once, per team, when the teams first meet — and never cleared.
    /// See [`teams_in_contact`] for why the latch has to outlive a replan.
    contact: [bool; 2],
    /// Number of recomputes this match. Exposed for tests and tracing — a plan
    /// layer that silently never recomputes looks identical to a working one.
    pub revisions: u32,
}

impl TeamPlans {
    /// Plan for `team` (1 or 2). Returns team 1's plan for any out-of-range value
    /// rather than panicking — a bad team id is a caller bug, not worth aborting a
    /// match over.
    pub fn for_team(&self, team: u8) -> &TeamPlan {
        &self.plans[usize::from(team.saturating_sub(1)).min(1)]
    }

    /// Whether `team` has met the enemy yet. Once true, always true.
    pub fn has_contact(&self, team: u8) -> bool {
        self.contact[Self::index(team)]
    }

    fn index(team: u8) -> usize {
        usize::from(team.saturating_sub(1)).min(1)
    }

    fn for_team_mut(&mut self, team: u8) -> &mut TeamPlan {
        &mut self.plans[Self::index(team)]
    }
}


/// A team's composition, reduced to what plan selection needs.
///
/// Deliberately coarse. The distinction that matters for a pillar camp is whether
/// a team has something that must CLOSE to deal damage (melee) alongside a healer
/// worth protecting — such a team gains by making the enemy walk to it. A team
/// that can already deal damage at range gains nothing from camping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompProfile {
    pub has_melee: bool,
    pub has_healer: bool,
    pub has_ranged: bool,
}

impl CompProfile {
    /// A melee/healer pair — the archetype that benefits from holding cover and
    /// forcing the approach.
    pub fn wants_to_camp(&self) -> bool {
        self.has_melee && self.has_healer
    }
}

/// Classify a roster of classes. Pure, so plan selection is testable without a
/// `World`.
pub fn classify_comp(classes: &[CharacterClass]) -> CompProfile {
    CompProfile {
        has_melee: classes.iter().any(|c| c.is_melee()),
        has_healer: classes.iter().any(|c| c.is_healer()),
        // A healer that is also melee (Paladin) does not make a team "ranged".
        has_ranged: classes.iter().any(|c| !c.is_melee() && !c.is_healer()),
    }
}

/// Pick the obstacle a team should hold, given its side of the arena.
///
/// Chooses the cover nearest the team's own spawn: camping the far side would
/// mean crossing the arena first, which is the opposite of making the enemy come
/// to you. Ties break on the lowest index so the choice is deterministic — the
/// four Nagrand pillars are symmetric, so ties are the normal case, not an edge.
///
/// `spawn_x` must be the team's ACTUAL spawn abscissa (`ArenaBounds::team_spawn_x`,
/// signed by side) — not a `±1.0` side sentinel. With a sentinel the ranking
/// inverts: `|center.x - ±1|` is smallest for the pillar nearest the arena
/// CENTRE, i.e. the one furthest from the gate. That is invisible on Nagrand only
/// because its two same-side pillars share an `x` and therefore tie.
///
/// Side membership uses a strict product test rather than `signum`, because
/// `(0.0f32).signum() == 1.0` and `(-0.0f32).signum() == -1.0` — an obstacle
/// sitting exactly on the centre line would otherwise be silently handed to one
/// team based on the sign of a zero. A centre-line obstacle belongs to neither
/// side and is skipped by both.
///
/// Returns `None` when the map has no obstacles, which is what keeps this inert
/// on BasicArena without a separate map check.
pub fn choose_anchor(volumes: &[ObstacleVolume], spawn_x: f32) -> Option<Anchor> {
    let mut best: Option<(f32, usize)> = None;
    for (i, v) in volumes.iter().enumerate() {
        let (center, _) = v.footprint_disc();
        // Same side of the arena as our gate, and nearest to it.
        if center.x * spawn_x <= 0.0 {
            continue;
        }
        let d = (center.x - spawn_x).abs();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| Anchor::Obstacle(i))
}


/// Standoff beyond the anchor's own footprint when holding it, so a camper stands
/// *beside* cover rather than inside its collision skin.
pub const CAMP_STANDOFF: f32 = 2.0;

/// How close an enemy must come before a camp releases into normal combat.
///
/// Set above `danger_radius` (12) so the release fires slightly BEFORE a healer
/// would flip to PRESSURED — a camp that outlived the posture change would have
/// the unit holding ground while its own AI thinks it is under threat.
pub const CAMP_ENGAGE_RADIUS: f32 = 15.0;

/// Distance within which a camper is considered arrived and stops adjusting.
/// Prevents jitter around the exact hold point.
pub const CAMP_ARRIVAL_EPSILON: f32 = 0.5;

/// Where a unit should stand to hold `anchor` against an enemy approaching from
/// `threat_from`.
///
/// The spot is on the far side of the obstacle from the approach, so the cover
/// sits between the camper and the incoming team — the whole point of taking a
/// pillar before contact. Returns `None` if the anchor index is stale or the
/// approach direction is degenerate.
///
/// Pure, so the geometry is testable without a match. The caller must still check
/// the result is in bounds and unblocked; this only picks the direction.
pub fn hold_position(
    volumes: &[ObstacleVolume],
    anchor: Anchor,
    // EVERY living enemy, not just the nearest. Hiding from one of two casters
    // leaves the other with a clear line — measured at 69.8% exposure to the
    // enemy Warlock while nominally "in cover", which is how a camped healer got
    // Mana Burned from 274 to 6 and lost the match.
    threats: &[Vec2],
    // The ally this unit must keep in sight — a healer's partner. `None` for
    // units with no such obligation (melee), which take the best-hidden spot.
    keep_sighted: Option<Vec2>,
) -> Option<Vec2> {
    let Anchor::Obstacle(i) = anchor else {
        if let Anchor::Point(p) = anchor {
            return Some(p);
        }
        return None;
    };
    let (center, radius) = volumes.get(i)?.footprint_disc();
    if threats.is_empty() {
        return None;
    }

    // Bearing away from the threat CENTROID seeds the search; with one threat
    // this is the old shadow point exactly.
    let centroid = threats.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / threats.len() as f32;
    let away = (center - centroid).normalize_or_zero();
    if away == Vec2::ZERO {
        return None;
    }
    let ring = radius + MOVER_RADIUS + CAMP_STANDOFF;
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);

    // Score every spot on the ring by how many threats it breaks sight to, and
    // take the best. Maximising the COUNT is the fix: the previous version took
    // the first spot hiding from a single threat and stopped looking.
    //
    // Sight to the ally is a hard requirement when there is one — a healer that
    // cannot heal is worse than one that can be shot at — so it filters
    // candidates rather than scoring them. If nothing satisfies it, the loop
    // below relaxes to sight-only.
    const STEPS: usize = 24;
    let candidates = |require_sight: bool| -> Option<(usize, Vec2)> {
        let mut best: Option<(usize, Vec2)> = None;
        for k in 0..STEPS {
            // Sweep from the centroid-shadow bearing so ties resolve toward the
            // most intuitive spot, deterministically.
            let ang = away.to_angle() + (k as f32) * std::f32::consts::TAU / STEPS as f32;
            let cand = center + Vec2::from_angle(ang) * ring;
            if require_sight {
                if let Some(ally) = keep_sighted {
                    if !has_line_of_sight(volumes, eye(cand), eye(ally)) {
                        continue;
                    }
                }
            }
            let hidden = threats
                .iter()
                .filter(|t| !has_line_of_sight(volumes, eye(cand), eye(**t)))
                .count();
            // Strictly greater keeps the first (lowest-k) spot on ties.
            if best.is_none_or(|(b, _)| hidden > b) {
                best = Some((hidden, cand));
            }
        }
        best
    };

    candidates(true)
        .or_else(|| candidates(false))
        .map(|(_, spot)| spot)
}


/// Ally HP fraction at or below which a camped healer breaks cover to get line of
/// sight for a heal.
///
/// Deliberately higher than `urgency_hp_threshold` (0.5, the "someone is dying"
/// mark): a healer that only pokes out at half health has already lost the race,
/// because it must then chain-cast while exposed. Topping up earlier means
/// shorter exposures.
pub const CAMP_POKE_HP: f32 = 0.85;

/// Whether a camped healer should break cover for line of sight this tick.
///
/// This is the LINE-OF-SIGHT CYCLE, and it is why a camp cannot be a fixed point.
/// Sight to an ally and occlusion from that ally's attackers are near-opposite
/// demands — the partner fights in roughly the same direction as the enemies
/// shooting at it, so one position cannot satisfy both. Measured: pushing enemy
/// exposure from 70% down to 52% dragged ally sight from 92% to 70%.
///
/// Real play resolves this in TIME rather than space: poke out to heal, duck back
/// while the heal lands and the enemy casts. Casting units are already planted
/// (they `continue` above the camp branch), so the cycle needs no state machine —
/// only the question "do I need sight right now?", asked per tick.
pub fn should_break_cover(worst_ally_hp_fraction: Option<f32>) -> bool {
    match worst_ally_hp_fraction {
        // Nobody to heal: stay hidden. This is the DUCK half, and it is the half
        // that was missing — the healer used to hold a sight-line permanently.
        None => false,
        Some(hp) => hp <= CAMP_POKE_HP,
    }
}

/// Whether a camping unit should still be holding, given the nearest enemy.
///
/// A camp that never releases is a unit refusing to fight — the team would hold
/// position until the match hit its duration cap. Once the enemy has committed
/// (come within `engage_radius`), the camp has done its job: it made them cross
/// the arena, and normal combat takes over from there.
///
/// This is the PER-UNIT half of the release, and on its own it is not enough:
/// see [`teams_in_contact`], which is what actually ends the camp.
pub fn should_hold(nearest_enemy_distance: Option<f32>, engage_radius: f32) -> bool {
    match nearest_enemy_distance {
        // Nobody near: keep holding, this is the pre-contact camp.
        None => true,
        Some(d) => d > engage_radius,
    }
}

/// Have the two teams met? True once ANY enemy is within `engage_radius` of ANY
/// living member of the camping team.
///
/// **This is what ends a camp, and asking it per-unit instead was the bug.**
/// `should_hold` releases a unit when an enemy walks into *its own* bubble. A
/// healer facing a ranged comp never has that happen: the enemy stops at 30-40yd
/// to cast, so the healer's personal bubble stays empty for the entire match and
/// its camp never released. Measured on `Warrior+Priest` vs `Warlock+Priest` over
/// seeds 7/11/12: the teams met 18.9s after the gates, and the healer was still
/// camping for 71-79% of every frame after that — welded to a ring 8.5yd around
/// its pillar while the fight moved 20-30yd away, with its posture AI suppressed
/// (the camp branch removes `MovementDirective`). It could not see the ally it
/// was there to heal on 60-94% of the frames where it was actively trying to, and
/// every TeamPlan loss in the 12-seed sweep delivered exactly zero healing.
///
/// A camp is an OPENER — `design-docs/team-level-positioning-ai.md` step 3, "the
/// team takes the pillar *before contact*". In-fight positioning around cover is
/// step 4's focal-rooted team solve, and until that lands the tuned posture layer
/// (heal range, `cover_pull`, `medic_chase`) is strictly better at it than a
/// fixed ring around a fixed pillar.
///
/// Pets count as enemies — a Felhunter in your face is contact — but only living
/// units on either side are considered.
pub fn teams_in_contact(own: &[Vec2], enemies: &[Vec2], engage_radius: f32) -> bool {
    own.iter()
        .any(|a| enemies.iter().any(|e| a.distance(*e) <= engage_radius))
}

/// Recompute both teams' plans when the roster changes.
///
/// **Under `AiProfile::Legacy` (the default) this produces `TeamPlan::default()`
/// for every comp, and nothing reads the result in either profile.** The cadence
/// and the wiring are real so that step 3 only has to add the consumer. Under
/// `AiProfile::TeamPlan` a camping comp on a map with cover gets a real
/// `Anchor::Obstacle` and `Stance::Hold`.
///
/// Cadence is roster-driven rather than per-frame: gates opening and any
/// combatant dying are the events that invalidate a plan. Pets are excluded — a
/// pet dying does not change what a team is trying to do.
pub fn update_team_plans(
    countdown: Res<MatchCountdown>,
    combatants: Query<(&Combatant, &Transform)>,
    geometry: Option<Res<ActiveMapGeometry>>,
    profile: Option<Res<AiProfiles>>,
    mut plans: ResMut<TeamPlans>,
) {
    if !countdown.gates_opened {
        return;
    }

    // CONTACT — checked every frame, ABOVE the roster gate, because the event
    // that ends a camp is the teams meeting, not the roster changing. Latching it
    // here (rather than releasing per-unit down in the movement system) is what
    // makes the release survive a replan: any death recomputes the plan, and a
    // recomputed plan would otherwise hand a camping comp `Stance::Hold` again and
    // send its healer back to the pillar with the fight 30yd away.
    //
    // Positions are read live rather than from the roster fingerprint, so this
    // costs one O(n*m) pass over at most six units per frame.
    let living: Vec<(u8, Vec2)> = combatants
        .iter()
        .filter(|(c, _)| c.is_alive())
        .map(|(c, t)| (c.team, Vec2::new(t.translation.x, t.translation.z)))
        .collect();
    for team in [1u8, 2u8] {
        // Guard before touching `plans` mutably: an unconditional write would mark
        // the resource changed every frame and defeat change detection.
        if plans.has_contact(team) {
            continue;
        }
        let own: Vec<Vec2> = living.iter().filter(|(t, _)| *t == team).map(|(_, p)| *p).collect();
        let foes: Vec<Vec2> = living.iter().filter(|(t, _)| *t != team).map(|(_, p)| *p).collect();
        if teams_in_contact(&own, &foes, CAMP_ENGAGE_RADIUS) {
            plans.contact[TeamPlans::index(team)] = true;
            // The camp is over. Fall back to today's behaviour — the posture layer
            // owns in-fight positioning until step 4's team solve lands.
            if plans.for_team(team).stance == Stance::Hold {
                plans.for_team_mut(team).stance = Stance::Press;
            }
        }
    }

    // (team, slot) rather than Entity: stable across the match, and meaningful in
    // a debugger. Sorted so the fingerprint does not depend on query order.
    let mut roster: Vec<(u8, u8)> = combatants
        .iter()
        .map(|(c, _)| c)
        .filter(|c| c.is_alive() && c.slot < PET_SLOT_BASE)
        .map(|c| (c.team, c.slot))
        .collect();
    roster.sort_unstable();

    // Read through Deref — taking ResMut mutably here would mark the resource
    // changed every frame and defeat Bevy's change detection for consumers.
    if roster == plans.roster {
        return;
    }

    plans.roster = roster;
    plans.revisions += 1;

    // Legacy keeps the inert plan, so every recorded baseline stays reproducible.
    // Only the TeamPlan profile selects a real plan.
    // Per team: a head-to-head match has one side planning and the other not.
    let profiles = profile.map(|p| *p).unwrap_or_default();
    let volumes: &[ObstacleVolume] = geometry
        .as_ref()
        .map(|g| g.volumes.as_slice())
        .unwrap_or(&[]);
    // `choose_anchor` ranks by distance to the GATE, so it needs the real spawn
    // abscissa, not a side sentinel — see its doc comment. Without geometry there
    // are no volumes either, so the fallback magnitude is never actually used.
    let spawn_mag = geometry
        .as_ref()
        .map(|g| g.bounds.team_spawn_x())
        .unwrap_or(1.0);

    for team in [1u8, 2u8] {
        if !profiles.for_team(team).is_team_plan() {
            *plans.for_team_mut(team) = TeamPlan::default();
            continue;
        }

        let classes: Vec<CharacterClass> = combatants
            .iter()
            .map(|(c, _)| c)
            .filter(|c| c.team == team && c.is_alive() && c.slot < PET_SLOT_BASE)
            .map(|c| c.class)
            .collect();
        let comp = classify_comp(&classes);

        // Team 1 spawns at -x, team 2 at +x. Camping the cover nearest our own
        // gate is what makes the enemy walk to us; camping theirs would mean
        // crossing the arena first.
        let spawn_x = if team == 1 { -spawn_mag } else { spawn_mag };

        // A camp is only meaningful for a comp that must close to deal damage,
        // and only where there is cover to hold. `choose_anchor` returns None on
        // an obstacle-free map, so BasicArena needs no separate guard.
        //
        // ...and only BEFORE contact. Without the latch, the commonest replan
        // trigger (a death) lands mid-fight and would re-arm the camp, marching a
        // healer back to its pillar at the worst possible moment.
        let anchor = (comp.wants_to_camp() && !plans.has_contact(team))
            .then(|| choose_anchor(volumes, spawn_x))
            .flatten();

        let plan = plans.for_team_mut(team);
        plan.anchor = anchor;
        // Hold ground and make them come; without an anchor there is nothing to
        // hold, so fall back to today's behaviour.
        plan.stance = if anchor.is_some() { Stance::Hold } else { Stance::Press };
        // Cleared, not carried: the commonest replan trigger is a death, and the
        // dead unit is exactly the one most likely to be the stale `kill_target`.
        // Leaving it would hand step 3's consumer a despawned `Entity`.
        plan.kill_target = None;
        plan.intents.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spawn-side position for `team`, far enough apart that the two sides are
    /// NOT in contact. The planner reads live positions to latch contact, so a
    /// helper that stacked everyone on the origin would report instant contact
    /// and no test could ever observe a camp.
    fn spawn_pos(team: u8) -> Transform {
        Transform::from_xyz(if team == 1 { -60.0 } else { 60.0 }, 0.0, 0.0)
    }

    /// Drive `update_team_plans` over a real `World`, so the cadence is tested as
    /// wired rather than as intended.
    fn run_planner(gates_open: bool, roster: &[(u8, u8, bool)]) -> TeamPlans {
        let mut app = App::new();
        app.insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: gates_open,
        });
        app.insert_resource(TeamPlans::default());
        for &(team, slot, alive) in roster {
            let mut c = Combatant::new(team, slot, CharacterClass::Warrior);
            if !alive {
                c.current_health = 0.0;
            }
            app.world_mut().spawn((c, spawn_pos(team)));
        }
        app.add_systems(Update, update_team_plans);
        app.update();
        app.world().resource::<TeamPlans>().clone()
    }

    /// As `run_planner`, but with an `AiProfile` and Nagrand's geometry present,
    /// so the plan-SELECTION branch runs rather than being skipped for want of
    /// resources.
    fn run_planner_with_profile(
        profile: AiProfile,
        roster: &[(u8, u8, CharacterClass)],
    ) -> TeamPlans {
        let mut app = App::new();
        app.insert_resource(MatchCountdown { time_remaining: 0.0, gates_opened: true });
        app.insert_resource(TeamPlans::default());
        app.insert_resource(AiProfiles::uniform(profile));
        app.insert_resource(ActiveMapGeometry {
            bounds: super::super::arena_bounds::ArenaBounds::Bowl {
                semi_x: 59.72,
                semi_z: 59.72,
                alcove_depth: 10.0,
                alcove_half_width: 8.0,
            },
            volumes: nagrand(),
            cover_anchors: Vec::new(),
        });
        for &(team, slot, class) in roster {
            app.world_mut().spawn((Combatant::new(team, slot, class), spawn_pos(team)));
        }
        app.add_systems(Update, update_team_plans);
        app.update();
        app.world().resource::<TeamPlans>().clone()
    }

    /// The planner must NOT run before gates open — a plan formed during the
    /// countdown would be based on starting positions nobody fights from.
    #[test]
    fn does_not_plan_before_gates_open() {
        let plans = run_planner(false, &[(1, 0, true), (2, 0, true)]);
        assert_eq!(plans.revisions, 0);
        assert!(plans.roster.is_empty());
    }

    /// The load-bearing check behind the step-2 no-op claim: the planner really
    /// DOES run. A system that silently never executes would also produce a
    /// byte-identical baseline, so "nothing changed" only means something once
    /// this passes.
    #[test]
    fn plans_once_gates_are_open() {
        let plans = run_planner(true, &[(1, 0, true), (1, 1, true), (2, 0, true)]);
        assert_eq!(plans.revisions, 1, "planner should have recomputed exactly once");
        assert_eq!(plans.roster, vec![(1, 0), (1, 1), (2, 0)]);
        // ...and with no `AiProfile` resource the output stays inert.
        for team in [1u8, 2u8] {
            assert_eq!(plans.for_team(team).anchor, None);
        }
    }

    /// `Legacy` is the default and what every recorded baseline runs — it must
    /// keep producing the inert plan even on a map with cover and a camping comp,
    /// or the baselines stop meaning anything.
    #[test]
    fn legacy_profile_stays_inert_on_a_map_with_cover() {
        let plans = run_planner_with_profile(
            AiProfile::Legacy,
            &[(1, 0, CharacterClass::Warrior), (1, 1, CharacterClass::Priest), (2, 0, CharacterClass::Mage)],
        );
        for team in [1u8, 2u8] {
            assert_eq!(plans.for_team(team).anchor, None, "Legacy must not select an anchor");
            assert_eq!(plans.for_team(team).stance, Stance::Press);
        }
    }

    /// The plan-selection branch itself, driven through a real `World` — without
    /// this the entire `TeamPlan`-profile path is covered only by the pure
    /// helpers, and the wiring between them is untested.
    #[test]
    fn team_plan_profile_selects_a_same_side_anchor_for_a_camping_comp() {
        let plans = run_planner_with_profile(
            AiProfile::TeamPlan,
            &[(1, 0, CharacterClass::Warrior), (1, 1, CharacterClass::Priest), (2, 0, CharacterClass::Mage)],
        );

        // Team 1 is melee + healer on a map with cover: it camps, on a -x pillar.
        let p1 = plans.for_team(1);
        let Some(Anchor::Obstacle(i)) = p1.anchor else {
            panic!("melee + healer on a map with cover should anchor, got {:?}", p1.anchor)
        };
        assert!(matches!(i, 0 | 1), "team 1 must anchor on a -x pillar, got {i}");
        assert_eq!(p1.stance, Stance::Hold);

        // Team 2 is a lone Mage: nothing to camp with, so today's behaviour.
        let p2 = plans.for_team(2);
        assert_eq!(p2.anchor, None, "a pure-ranged comp gains nothing from camping");
        assert_eq!(p2.stance, Stance::Press);
    }

    /// Dead combatants leave the roster, so a death triggers a replan — that is
    /// the cadence, not a per-frame recompute.
    #[test]
    fn dead_combatants_are_excluded_from_the_roster() {
        let plans = run_planner(true, &[(1, 0, true), (1, 1, false), (2, 0, true)]);
        assert_eq!(plans.roster, vec![(1, 0), (2, 0)], "dead slot 1 should be gone");
    }

    /// Pets must not drive replanning — a pet dying does not change what a team is
    /// trying to do, and letting it churn the plan would make the cadence noise.
    #[test]
    fn pets_do_not_enter_the_roster() {
        // Real pets get `PET_SLOT_BASE + owner_slot`, so exercise actual pet
        // slots, not just the boundary. An earlier version of this test spawned a
        // LOCAL `PET_SLOT_BASE` that disagreed with the project constant by 10x,
        // so it asserted the filter against its own wrong value and could never
        // fail while real pets sailed straight into the roster.
        let plans = run_planner(
            true,
            &[
                (1, 0, true),
                (1, 1, true),
                (1, PET_SLOT_BASE, true),     // team 1 slot-0's pet
                (1, PET_SLOT_BASE + 1, true), // team 1 slot-1's pet
                (2, 0, true),
                (2, PET_SLOT_BASE, true),
            ],
        );
        assert_eq!(
            plans.roster,
            vec![(1, 0), (1, 1), (2, 0)],
            "pets must not enter the roster — a pet dying does not change what a team wants"
        );
    }

    /// A stable roster must not recompute — the plan is an intent that persists,
    /// not a per-frame derivation.
    #[test]
    fn stable_roster_does_not_replan() {
        let mut app = App::new();
        app.insert_resource(MatchCountdown { time_remaining: 0.0, gates_opened: true });
        app.insert_resource(TeamPlans::default());
        app.world_mut().spawn((Combatant::new(1, 0, CharacterClass::Warrior), spawn_pos(1)));
        app.world_mut().spawn((Combatant::new(2, 0, CharacterClass::Warrior), spawn_pos(2)));
        app.add_systems(Update, update_team_plans);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<TeamPlans>().revisions,
            1,
            "five frames with an unchanged roster should yield ONE recompute"
        );
    }

    /// Nagrand's real spawn abscissa (`Bowl { semi_x: 59.72, alcove_depth: 10.0 }`
    /// -> 59.72 + 5.0). `choose_anchor` ranks by distance to the GATE, so probing
    /// it with a `±1.0` side sentinel would exercise the inverted ordering.
    const NAGRAND_SPAWN_X: f32 = 64.72;

    fn pillar(x: f32, z: f32) -> ObstacleVolume {
        ObstacleVolume::Prism {
            center_xz: Vec2::new(x, z),
            circumradius: 6.0,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        }
    }

    /// The four Nagrand pillars, in maps.ron declaration order.
    fn nagrand() -> Vec<ObstacleVolume> {
        vec![
            pillar(-40.0, -20.0),
            pillar(-40.0, 20.0),
            pillar(40.0, -20.0),
            pillar(40.0, 20.0),
        ]
    }


    /// The hold spot must put the pillar BETWEEN the camper and the approach —
    /// that is the entire point of taking cover before contact.
    #[test]
    fn hold_position_is_on_the_far_side_from_the_threat() {
        let v = nagrand();
        let anchor = Anchor::Obstacle(0); // pillar at (-40, -20)
        // Enemy approaching from the arena centre.
        let spot = hold_position(&v, anchor, &[Vec2::ZERO], None).expect("a far side exists");
        let (center, _) = v[0].footprint_disc();

        // The pillar centre must lie between the threat and the hold spot.
        let to_spot = (spot - Vec2::ZERO).normalize();
        let to_center = (center - Vec2::ZERO).normalize();
        assert!(
            to_spot.dot(to_center) > 0.99,
            "hold spot {spot:?} should be directly beyond the pillar from the threat"
        );
        assert!(
            spot.distance(center) > center.distance(Vec2::ZERO) * 0.0 + 6.0,
            "hold spot must be clear of the pillar footprint"
        );
    }


    /// The healer-pinning case from the design doc: a spot that hides from the
    /// threat but ALSO keeps the partner in sight. A pure shadow point satisfies
    /// only the first, which is how the Priest ended up behind its pillar unable
    /// to heal while its partner died.
    #[test]
    fn hold_position_keeps_the_ally_in_sight() {
        let v = nagrand();
        let (center, _) = v[0].footprint_disc();
        let threat = Vec2::ZERO;
        // Ally on the same side as the threat — the hard case, where the pure
        // shadow point would put the pillar between healer and ally.
        let ally = center + (threat - center).normalize() * 20.0;

        let spot = hold_position(&v, Anchor::Obstacle(0), &[threat], Some(ally))
            .expect("a hold spot exists");
        let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
        assert!(
            has_line_of_sight(&v, eye(spot), eye(ally)),
            "a healer must be able to see the ally it is holding cover for; spot {spot:?}"
        );
    }

    /// With no sight obligation (melee), the pure shadow point is still used —
    /// the dual constraint must not change behaviour for units that do not have it.
    #[test]
    fn hold_position_without_an_ally_is_the_plain_shadow_point() {
        let v = nagrand();
        let (center, radius) = v[0].footprint_disc();
        let spot = hold_position(&v, Anchor::Obstacle(0), &[Vec2::ZERO], None).unwrap();
        let expected = center + (center - Vec2::ZERO).normalize() * (radius + MOVER_RADIUS + CAMP_STANDOFF);
        assert!(spot.distance(expected) < 1e-3, "melee should take the plain shadow point");
    }

    /// Standing on the pillar is not holding it — the spot must clear the
    /// footprint plus the mover's own radius.
    #[test]
    fn hold_position_clears_the_footprint() {
        let v = nagrand();
        let (center, radius) = v[0].footprint_disc();
        let spot = hold_position(&v, Anchor::Obstacle(0), &[Vec2::ZERO], None).unwrap();
        assert!(
            spot.distance(center) >= radius + MOVER_RADIUS,
            "hold spot is inside the collision skin"
        );
    }

    /// The approach direction decides the side, so a threat from the opposite
    /// quarter must flip the hold spot.
    #[test]
    fn hold_position_follows_the_approach() {
        let v = nagrand();
        let from_centre = hold_position(&v, Anchor::Obstacle(0), &[Vec2::ZERO], None).unwrap();
        let from_behind = hold_position(&v, Anchor::Obstacle(0), &[Vec2::new(-80.0, -40.0)], None).unwrap();
        assert!(
            from_centre.distance(from_behind) > 6.0,
            "opposite approaches should yield opposite sides of the pillar"
        );
    }

    /// A stale anchor index must not panic a live match.
    #[test]
    fn hold_position_tolerates_a_stale_anchor() {
        assert_eq!(hold_position(&nagrand(), Anchor::Obstacle(99), &[Vec2::ZERO], None), None);
        assert_eq!(hold_position(&[], Anchor::Obstacle(0), &[Vec2::ZERO], None), None);
    }


    /// The DUCK half — the one that was missing. With nobody hurt, a camped healer
    /// must NOT hold a sight-line; holding one permanently is what left the Priest
    /// exposed to the enemy Warlock 70% of the match.
    #[test]
    fn healthy_team_means_stay_hidden() {
        assert!(!should_break_cover(None), "no ally to heal: stay in cover");
        assert!(!should_break_cover(Some(1.0)), "full HP: stay in cover");
        assert!(!should_break_cover(Some(0.95)), "a scratch is not worth exposure");
    }

    /// The POKE half — break cover while there is still a race to win.
    #[test]
    fn injured_ally_means_break_cover() {
        assert!(should_break_cover(Some(CAMP_POKE_HP)), "at the threshold, poke");
        assert!(should_break_cover(Some(0.5)), "badly hurt: definitely poke");
        assert!(should_break_cover(Some(0.05)), "nearly dead: poke");
    }

    /// The poke threshold must sit ABOVE the "someone is dying" mark. A healer that
    /// only emerges at half health has to chain-cast while exposed, which is the
    /// losing shape — topping up earlier keeps each exposure short.
    #[test]
    fn poke_threshold_is_above_the_urgency_mark() {
        assert!(
            CAMP_POKE_HP > 0.5,
            "CAMP_POKE_HP {CAMP_POKE_HP} must exceed urgency_hp_threshold (0.5)"
        );
    }

    /// Contact is a TEAM event. The per-unit test (`should_hold`) is not enough:
    /// a healer facing a ranged comp never has an enemy in its own bubble, so its
    /// camp never released and it stayed welded to the pillar all match.
    #[test]
    fn contact_is_a_team_event_not_a_personal_one() {
        // Healer at the pillar, partner out fighting, enemy engaging the partner.
        let healer = Vec2::new(-40.0, -20.0);
        let partner = Vec2::new(-10.0, 0.0);
        let enemy = Vec2::new(-2.0, 0.0);

        // The healer's own bubble is empty — 40yd of clear air.
        assert!(
            should_hold(Some(healer.distance(enemy)), CAMP_ENGAGE_RADIUS),
            "the per-unit test sees nothing, which is exactly the bug"
        );
        // ...but the teams have plainly met.
        assert!(teams_in_contact(&[healer, partner], &[enemy], CAMP_ENGAGE_RADIUS));
    }

    /// Two teams still crossing the arena are not in contact — the opener must
    /// survive the approach or it buys nothing.
    #[test]
    fn no_contact_while_the_teams_are_still_closing() {
        assert!(!teams_in_contact(
            &[Vec2::new(-60.0, 0.0), Vec2::new(-40.0, -20.0)],
            &[Vec2::new(60.0, 0.0), Vec2::new(55.0, 10.0)],
            CAMP_ENGAGE_RADIUS
        ));
    }

    /// Degenerate rosters must not report contact — an empty side has met nobody.
    #[test]
    fn contact_needs_units_on_both_sides() {
        assert!(!teams_in_contact(&[], &[Vec2::ZERO], CAMP_ENGAGE_RADIUS));
        assert!(!teams_in_contact(&[Vec2::ZERO], &[], CAMP_ENGAGE_RADIUS));
    }

    /// Drive the planner over a `World` where the teams ARE touching, so the
    /// contact latch and the stance downgrade are tested as wired.
    fn run_planner_in_contact() -> TeamPlans {
        let mut app = App::new();
        app.insert_resource(MatchCountdown { time_remaining: 0.0, gates_opened: true });
        app.insert_resource(TeamPlans::default());
        app.insert_resource(AiProfiles::uniform(AiProfile::TeamPlan));
        app.insert_resource(ActiveMapGeometry {
            bounds: super::super::arena_bounds::ArenaBounds::Bowl {
                semi_x: 59.72,
                semi_z: 59.72,
                alcove_depth: 10.0,
                alcove_half_width: 8.0,
            },
            volumes: nagrand(),
            cover_anchors: Vec::new(),
        });
        // A camping comp (Warrior + Priest) standing right on top of a lone Mage.
        app.world_mut()
            .spawn((Combatant::new(1, 0, CharacterClass::Warrior), Transform::from_xyz(0.0, 0.0, 0.0)));
        app.world_mut()
            .spawn((Combatant::new(1, 1, CharacterClass::Priest), Transform::from_xyz(-2.0, 0.0, 0.0)));
        app.world_mut()
            .spawn((Combatant::new(2, 0, CharacterClass::Mage), Transform::from_xyz(3.0, 0.0, 0.0)));
        app.add_systems(Update, update_team_plans);
        app.update();
        app.world().resource::<TeamPlans>().clone()
    }

    /// A camping comp that is ALREADY in contact must never take the camp — the
    /// opener's window has passed.
    #[test]
    fn a_comp_already_in_contact_does_not_camp() {
        let plans = run_planner_in_contact();
        assert!(plans.has_contact(1), "touching units must register contact");
        assert_eq!(plans.for_team(1).anchor, None, "no camp once the teams have met");
        assert_eq!(plans.for_team(1).stance, Stance::Press);
    }

    /// THE REGRESSION THIS LATCH EXISTS FOR. A death is the commonest replan
    /// trigger and it lands mid-fight; without the latch outliving the replan, the
    /// recomputed plan hands the comp `Stance::Hold` again and marches its healer
    /// back to the pillar with the fight 30yd away.
    #[test]
    fn a_replan_after_contact_does_not_re_arm_the_camp() {
        let mut app = App::new();
        app.insert_resource(MatchCountdown { time_remaining: 0.0, gates_opened: true });
        app.insert_resource(TeamPlans::default());
        app.insert_resource(AiProfiles::uniform(AiProfile::TeamPlan));
        app.insert_resource(ActiveMapGeometry {
            bounds: super::super::arena_bounds::ArenaBounds::Bowl {
                semi_x: 59.72,
                semi_z: 59.72,
                alcove_depth: 10.0,
                alcove_half_width: 8.0,
            },
            volumes: nagrand(),
            cover_anchors: Vec::new(),
        });
        // Frame 1: apart. Team 1 is a camping comp, so it takes the pillar.
        let w = app
            .world_mut()
            .spawn((Combatant::new(1, 0, CharacterClass::Warrior), spawn_pos(1)))
            .id();
        app.world_mut()
            .spawn((Combatant::new(1, 1, CharacterClass::Priest), spawn_pos(1)));
        let m = app
            .world_mut()
            .spawn((Combatant::new(2, 0, CharacterClass::Mage), spawn_pos(2)))
            .id();
        app.world_mut()
            .spawn((Combatant::new(2, 1, CharacterClass::Warlock), spawn_pos(2)));
        app.add_systems(Update, update_team_plans);
        app.update();
        assert!(
            app.world().resource::<TeamPlans>().for_team(1).anchor.is_some(),
            "pre-contact, a melee+healer comp should camp"
        );

        // Frame 2: the teams meet.
        app.world_mut().entity_mut(m).insert(Transform::from_xyz(-58.0, 0.0, 0.0));
        app.update();
        assert!(app.world().resource::<TeamPlans>().has_contact(1));
        assert_eq!(app.world().resource::<TeamPlans>().for_team(1).stance, Stance::Press);

        // Frame 3: the Warrior dies — a replan, mid-fight, with the roster changed.
        app.world_mut().entity_mut(w).get_mut::<Combatant>().unwrap().current_health = 0.0;
        app.update();
        let plans = app.world().resource::<TeamPlans>();
        assert!(plans.revisions >= 2, "the death should have triggered a replan");
        assert_eq!(
            plans.for_team(1).anchor,
            None,
            "a mid-fight replan must NOT re-arm the camp"
        );
        assert_eq!(plans.for_team(1).stance, Stance::Press);
    }

    /// A camp that never releases is a unit refusing to fight — the match would
    /// run to its duration cap. Holding ends when the enemy commits.
    #[test]
    fn camp_releases_once_the_enemy_commits() {
        assert!(should_hold(None, 15.0), "pre-contact: keep holding");
        assert!(should_hold(Some(40.0), 15.0), "still far: keep holding");
        assert!(!should_hold(Some(14.0), 15.0), "committed: release and fight");
        assert!(!should_hold(Some(0.0), 15.0), "in melee: definitely release");
    }

    #[test]
    fn melee_plus_healer_wants_to_camp() {
        let comp = classify_comp(&[CharacterClass::Warrior, CharacterClass::Priest]);
        assert!(comp.wants_to_camp(), "melee + healer is the camping archetype");
    }

    /// A team that already deals damage at range gains nothing from making the
    /// enemy walk to it.
    #[test]
    fn double_ranged_does_not_camp() {
        let comp = classify_comp(&[CharacterClass::Mage, CharacterClass::Warlock]);
        assert!(!comp.wants_to_camp());
        assert!(comp.has_ranged);
    }

    /// A Paladin is both melee and healer, so it satisfies both halves alone —
    /// and must NOT be counted as ranged.
    #[test]
    fn paladin_counts_as_melee_and_healer_but_not_ranged() {
        let comp = classify_comp(&[CharacterClass::Paladin]);
        assert!(comp.has_melee && comp.has_healer);
        assert!(!comp.has_ranged, "a melee healer must not make the team ranged");
        assert!(comp.wants_to_camp());
    }

    /// Each team camps cover on ITS OWN side. Camping the far pillars would mean
    /// crossing the arena first, which defeats the purpose.
    #[test]
    fn anchor_is_on_the_teams_own_side() {
        let v = nagrand();
        // Team 1 spawns at -x.
        let a1 = choose_anchor(&v, -NAGRAND_SPAWN_X).expect("a -x pillar exists");
        let Anchor::Obstacle(i1) = a1 else { panic!("expected an obstacle anchor") };
        assert!(matches!(i1, 0 | 1), "team 1 must anchor on a -x pillar, got {i1}");

        let a2 = choose_anchor(&v, NAGRAND_SPAWN_X).expect("a +x pillar exists");
        let Anchor::Obstacle(i2) = a2 else { panic!("expected an obstacle anchor") };
        assert!(matches!(i2, 2 | 3), "team 2 must anchor on a +x pillar, got {i2}");
    }

    /// Symmetric pillars make ties the NORMAL case here, not an edge case, so the
    /// tie-break has to be deterministic or plans differ run to run.
    #[test]
    fn anchor_choice_is_deterministic_under_ties() {
        let v = nagrand();
        let first = choose_anchor(&v, -NAGRAND_SPAWN_X);
        for _ in 0..16 {
            assert_eq!(choose_anchor(&v, -NAGRAND_SPAWN_X), first);
        }
    }

    /// The ranking is distance to the GATE, not to the arena centre. Pinning this
    /// on an ASYMMETRIC same-side pair is the only way to catch it: Nagrand's two
    /// -x pillars tie, so a fully inverted comparison passes there.
    #[test]
    fn anchor_is_the_pillar_nearest_our_gate() {
        // Index 0 is deep in our half (near the gate); index 1 sits near centre.
        let v = vec![pillar(-40.0, 0.0), pillar(-10.0, 0.0)];
        assert_eq!(
            choose_anchor(&v, -NAGRAND_SPAWN_X),
            Some(Anchor::Obstacle(0)),
            "must pick the pillar nearest our own gate, not the one nearest centre"
        );
    }

    /// A pillar exactly on the centre line belongs to NEITHER side. `signum` would
    /// hand it to a team based on the sign of a zero (`(0.0f32).signum() == 1.0`).
    #[test]
    fn centre_line_obstacles_belong_to_neither_side() {
        let v = vec![pillar(0.0, 0.0), pillar(-0.0, 5.0)];
        assert_eq!(choose_anchor(&v, -NAGRAND_SPAWN_X), None);
        assert_eq!(choose_anchor(&v, NAGRAND_SPAWN_X), None);
    }

    /// No obstacles means no anchor — which is what keeps this inert on
    /// BasicArena without a separate map check.
    #[test]
    fn no_obstacles_yields_no_anchor() {
        assert_eq!(choose_anchor(&[], -NAGRAND_SPAWN_X), None);
        assert_eq!(choose_anchor(&[], NAGRAND_SPAWN_X), None);
    }

    #[test]
    fn defaults_are_an_inert_open_field_plan() {
        let plans = TeamPlans::default();
        for team in [1u8, 2u8] {
            let p = plans.for_team(team);
            assert_eq!(p.anchor, None, "the default plan must carry no anchor");
            assert_eq!(p.stance, Stance::Press);
            assert_eq!(p.kill_target, None);
            assert!(p.intents.is_empty());
        }
        assert_eq!(plans.revisions, 0);
    }

    /// Teams index independently — writing one must not alias the other.
    #[test]
    fn teams_index_independently() {
        let mut plans = TeamPlans::default();
        plans.for_team_mut(1).stance = Stance::Hold;
        assert_eq!(plans.for_team(1).stance, Stance::Hold);
        assert_eq!(plans.for_team(2).stance, Stance::Press);
    }

    /// A bad team id must not panic mid-match. 0 and 3+ clamp into range.
    #[test]
    fn out_of_range_team_ids_clamp_instead_of_panicking() {
        let plans = TeamPlans::default();
        for team in [0u8, 1, 2, 3, 255] {
            let _ = plans.for_team(team);
        }
    }

    /// `Withdraw` carries its reason, so `Draw` and `Recover` stay distinguishable
    /// despite sharing a stance and an exit path.
    #[test]
    fn withdraw_reasons_are_distinct() {
        assert_ne!(
            Stance::Withdraw(WithdrawReason::Draw),
            Stance::Withdraw(WithdrawReason::Recover)
        );
        assert_ne!(Stance::Withdraw(WithdrawReason::Recover), Stance::Press);
    }
}
