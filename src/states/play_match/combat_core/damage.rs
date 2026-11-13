//! Damage application, absorb shields, and interrupt processing.

use super::super::abilities::SpellSchool;
use super::super::class_ai;
use super::super::components::*;
use super::super::constants::DIVINE_SHIELD_DAMAGE_PENALTY;
use super::super::utils::combat_log_id_for;
use super::get_lockout_duration_reduction;
use super::{DamageFollowUp, DamageFollowUps, TransferReceiver};
use crate::combat::log::{CombatLog, CombatLogEventType};
use bevy::prelude::*;

/// Roll a critical strike check. Returns true if the roll is a crit.
pub fn roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool {
    rng.random_f32() < crit_chance
}

/// Map a magical `SpellSchool` to its slot in `Combatant::damage_mitigated_by_resistance`.
/// Returns `None` for `SpellSchool::Physical` and `SpellSchool::None`, which have no resistance slot.
pub(crate) fn resistance_school_index(school: SpellSchool) -> Option<usize> {
    match school {
        SpellSchool::Frost => Some(0),
        SpellSchool::Holy => Some(1),
        SpellSchool::Shadow => Some(2),
        SpellSchool::Arcane => Some(3),
        SpellSchool::Fire => Some(4),
        SpellSchool::Nature => Some(5),
        SpellSchool::Physical | SpellSchool::None => None,
    }
}

/// How a hit reached its victim: what the damage pipeline's attacker-side and
/// once-per-hit steps need to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// A spell, a ranged attack, or anything else struck directly.
    Direct,
    /// A melee strike: a melee swing or a melee ability. Retaliation
    /// ([`AuraType::DamageReflect`]) answers only these.
    Melee,
    /// A periodic tick (a damage over time, a channel). Ticks do not spend a
    /// vulnerability's hit charges, and a charged vulnerability does not raise
    /// them.
    Periodic,
    /// Damage dealt back to an attacker ([`AuraType::DamageReflect`], a
    /// reflecting shield). It meets its victim's own mitigation, absorbs and
    /// saves, and begets nothing: it is never reflected or transferred again.
    Reflected,
    /// A share of another unit's damage that its receiver takes as its own
    /// (an [`AuraType::DamageTransfer`]: Blessing of Sacrifice). Its
    /// receiver's immunity, damage-taken modifiers and absorbs apply, so a
    /// Paladin under Divine Shield takes none of it. Its attacker's
    /// damage-dealt increase and its receiver's armor and resistance do not:
    /// the share met those on the unit it left. It begets nothing.
    Transferred,
    /// A share of another unit's damage that cannot be prevented (Soul Link:
    /// TBC's tooltip, spell 19028, "That damage cannot be prevented"). It
    /// skips its receiver's immunity, mitigation, modifiers and absorbs, and
    /// begets nothing.
    TransferredUnpreventable,
    /// A transferred share handed back to its holder because its receiver
    /// could not take it (dead, gone, or too low to survive it). It was
    /// mitigated on the holder already, so it skips immunity, mitigation and
    /// modifiers, but the holder's absorbs soak it, as they would have had it
    /// never left; and it begets nothing.
    Returned,
}

impl HitKind {
    /// Whether the hit is a secondary one: damage another hit caused, which
    /// is never reflected or transferred again.
    pub fn is_secondary(self) -> bool {
        matches!(
            self,
            HitKind::Reflected
                | HitKind::Transferred
                | HitKind::TransferredUnpreventable
                | HitKind::Returned
        )
    }

    /// The hit a transferred share lands as on its receiver: one its receiver
    /// may prevent, or one it may not.
    pub fn share(preventable: bool) -> Self {
        if preventable {
            HitKind::Transferred
        } else {
            HitKind::TransferredUnpreventable
        }
    }
}

/// One hit, as `apply_damage_with_absorb` lands it.
#[derive(Clone, Copy, Debug)]
pub struct Hit<'a> {
    /// The damage as its source computed it: stat scaling and crit in, no
    /// modifier yet.
    pub amount: f32,
    pub school: SpellSchool,
    /// What dealt it ("Frostbolt"): named in the stealth reveal's log line.
    pub source: &'a str,
    /// The breakdown row its dealer's damage is logged under, which a
    /// transferred share of it is credited to as well: `source`, unless its
    /// site logs it under another name ([`Hit::row`]; a channel's tick is
    /// "Drain Life (tick)").
    pub row: &'a str,
    /// The unit that dealt it, when there is one.
    pub attacker: Option<Entity>,
    /// The unit it lands on.
    pub victim: Entity,
    /// The attacker's outgoing multiplier for this school
    /// ([`damage_dealt_multiplier`] over its auras): 1.0 for none.
    pub dealt: f32,
    pub kind: HitKind,
}

impl<'a> Hit<'a> {
    /// A direct hit of `amount` `school` damage from `attacker` on `victim`,
    /// raised by the attacker's own damage-dealt auras (`attacker_auras`).
    pub fn new(
        amount: f32,
        school: SpellSchool,
        source: &'a str,
        attacker: Option<Entity>,
        attacker_auras: Option<&ActiveAuras>,
        victim: Entity,
    ) -> Self {
        Self {
            amount,
            school,
            source,
            row: source,
            attacker,
            victim,
            dealt: damage_dealt_multiplier(attacker_auras, school),
            kind: HitKind::Direct,
        }
    }

    /// This hit, with the attacker's damage-dealt multiplier `dealt` (read
    /// ahead of time with [`DealtMods`]).
    pub fn with_dealt(mut self, dealt: f32) -> Self {
        self.dealt = dealt;
        self
    }

    /// This hit, logged under the breakdown row `row` rather than its
    /// `source`'s name.
    pub fn row(mut self, row: &'a str) -> Self {
        self.row = row;
        self
    }

    /// This hit, of `kind`.
    pub fn of_kind(mut self, kind: HitKind) -> Self {
        self.kind = kind;
        self
    }

    /// This hit, as a melee strike when `melee`.
    pub fn melee_if(self, melee: bool) -> Self {
        if melee {
            self.of_kind(HitKind::Melee)
        } else {
            self
        }
    }
}

/// The outgoing multiplier the attacker's [`AuraType::DamageDealtIncrease`]
/// auras put on a hit of `school`: the strongest one in scope (the stacking
/// simplification: same-kind effects do not compound), or 1.0 with none.
pub fn damage_dealt_multiplier(attacker_auras: Option<&ActiveAuras>, school: SpellSchool) -> f32 {
    1.0 + strongest_in_scope(attacker_auras, AuraType::DamageDealtIncrease, school, false)
        .map_or(0.0, |(_, value)| value)
}

/// An attacker's [`AuraType::DamageDealtIncrease`] auras, read out ahead of a
/// loop that borrows every unit's auras mutably (the DoT ticks): what
/// [`damage_dealt_multiplier`] reads, kept as (scope, value) pairs.
#[derive(Clone, Debug, Default)]
pub struct DealtMods(Vec<(Option<DamageScope>, f32)>);

impl DealtMods {
    /// The increases on `auras`.
    pub fn of(auras: Option<&ActiveAuras>) -> Self {
        Self::from_auras(auras.map_or(&[], |a| a.auras.as_slice()))
    }

    /// The increases among `auras` (a snapshot's copy of a unit's auras).
    pub fn from_auras(auras: &[Aura]) -> Self {
        Self(
            auras
                .iter()
                .filter(|a| a.effect_type == AuraType::DamageDealtIncrease)
                .map(|a| {
                    (
                        a.damage_mods.scope,
                        a.magnitude * f32::from(a.stacks.map_or(1, |s| s.count)),
                    )
                })
                .collect(),
        )
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The multiplier on a hit of `school`, as [`damage_dealt_multiplier`].
    pub fn multiplier(&self, school: SpellSchool) -> f32 {
        1.0 + self
            .0
            .iter()
            .filter(|(scope, _)| DamageScope::reaches(*scope, school))
            .map(|(_, v)| *v)
            .fold(0.0, f32::max)
    }
}

/// The strongest aura of `kind` on `auras` whose scope reaches `school`: its
/// index and its value (magnitude times stacks). The first wins a tie.
/// `skip_charged` passes over an aura with hit charges.
fn strongest_in_scope(
    auras: Option<&ActiveAuras>,
    kind: AuraType,
    school: SpellSchool,
    skip_charged: bool,
) -> Option<(usize, f32)> {
    auras?
        .auras
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.effect_type == kind
                && DamageScope::reaches(a.damage_mods.scope, school)
                && !(skip_charged && a.damage_mods.charges.is_some())
        })
        .map(|(i, a)| (i, a.magnitude * f32::from(a.stacks.map_or(1, |s| s.count))))
        .fold(None, |best: Option<(usize, f32)>, (i, v)| match best {
            Some((_, b)) if b >= v => best,
            _ => Some((i, v)),
        })
}

/// The strongest magnitude among `auras` of `kind`, or 0.0.
fn strongest(auras: &[Aura], kind: AuraType) -> f32 {
    auras
        .iter()
        .filter(|aura| aura.effect_type == kind)
        .map(|aura| aura.magnitude)
        .fold(0.0, f32::max)
}

/// The armor a Physical hit meets on a target: its own armor plus every flat
/// [`AuraType::ArmorIncrease`], scaled by the strongest
/// [`AuraType::ArmorPercentIncrease`] and the strongest
/// [`AuraType::ArmorPercentReduction`] (each kind its strongest only; the two
/// kinds multiply). Exactly `target.armor` + the flat sum with no percentage
/// aura. Never written into the `armor` stat.
pub fn effective_armor(target: &Combatant, auras: Option<&ActiveAuras>) -> f32 {
    armor_met(target.armor, auras.map_or(&[], |a| a.auras.as_slice()))
}

/// [`effective_armor`] from an armor stat and the auras its holder wears: the
/// one rule, for a caller that holds a snapshot of the unit rather than its
/// `Combatant` (an AI judging what a Physical hit would meet).
pub fn armor_met(armor: f32, auras: &[Aura]) -> f32 {
    let flat = armor
        + auras
            .iter()
            .filter(|a| a.effect_type == AuraType::ArmorIncrease)
            .map(|a| a.magnitude)
            .sum::<f32>();
    let up = strongest(auras, AuraType::ArmorPercentIncrease);
    let down = strongest(auras, AuraType::ArmorPercentReduction);
    if up == 0.0 && down == 0.0 {
        return flat;
    }
    flat * (1.0 + up) * (1.0 - down).max(0.0)
}

/// The share of a Physical hit that `armor` stops: `armor / (armor + 5500)`.
pub fn armor_reduction(armor: f32) -> f32 {
    armor / (armor + 5500.0)
}

/// Apply damage to a combatant: the one damage pipeline. Returns
/// `(actual_damage_to_health, damage_absorbed)`.
///
/// The steps, in order:
/// 0. **Immunity** (Divine Shield, Cyclone) blocks the hit outright.
/// 1. **Damage dealt**: the attacker's strongest in-scope
///    [`AuraType::DamageDealtIncrease`] ([`Hit::dealt`]).
/// 2. **Armor** for Physical ([`effective_armor`]), **resistance** for magic.
/// 3. **Damage taken**: the strongest in-scope
///    [`AuraType::DamageTakenReduction`], then the strongest in-scope
///    [`AuraType::DamageTakenIncrease`]; a charged one spends a charge on a
///    direct hit. Each kind applies its strongest only (the stacking
///    simplification); the two kinds multiply.
/// 4. **Transfer**: the strongest [`AuraType::DamageTransfer`] (or the
///    holder's Soul Link) sends its share to its receiver (`follow_ups`,
///    resolved by `process_damage_follow_ups`).
/// 5. **Absorbs** soak what is left; a reflecting shield sends its share of
///    what it soaked back to the attacker.
/// 6. **Stealth**: damage that reaches health reveals the target.
/// 7. **The killing blow**: a blow that would kill a holder of a save
///    ([`AuraType::KillingBlowSave`], or an unspent Cheat Death) is prevented.
/// 8. **Health**.
/// 9. **Retaliation**: a melee hit on a holder of [`AuraType::DamageReflect`]
///    returns its strongest one's damage to the attacker.
///
/// A [`HitKind::Transferred`] share (Blessing of Sacrifice) skips 1-2, which
/// it met on the unit it left, 4 and 9: its receiver's immunity, damage-taken
/// modifiers and absorbs apply, though a charged increase spends no charge on
/// it. A [`HitKind::TransferredUnpreventable`] share (Soul Link) skips 0-5
/// and 9. A [`HitKind::Returned`] share skips 0-4 and 9, and meets its
/// holder's absorbs. A secondary hit (reflected, transferred or returned)
/// never transfers or reflects. Every follow-up a hit makes is pushed to
/// `follow_ups` and lands later in the frame; with no modifier aura and no
/// talent, nothing is pushed and the pipeline is the one that was here before
/// it.
///
/// This is the one site every damage source funnels through — direct hits,
/// auto-attacks, projectiles, DoT ticks, area effects, backlash, follow-ups —
/// so every rule above lives here rather than at each caller. Damage a shield
/// fully absorbs, or that immunity blocks, never touches health and leaves
/// stealth intact.
///
/// # Panics (debug only)
/// Panics if damage is negative (damage should always be >= 0).
pub fn apply_damage_with_absorb(
    hit: Hit<'_>,
    target: &mut Combatant,
    mut active_auras: Option<&mut ActiveAuras>,
    follow_ups: &mut DamageFollowUps,
    combat_log: &mut CombatLog,
) -> (f32, f32) {
    let Hit {
        amount: damage,
        school: spell_school,
        source,
        ..
    } = hit;
    // Invariant: damage should never be negative
    debug_assert!(
        damage >= 0.0,
        "apply_damage_with_absorb: damage cannot be negative, got {}",
        damage
    );

    // Invariant: target health should be valid before we modify it
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: target health already negative ({})",
        target.current_health
    );

    // A share that met everything up to the transfer on its holder: one handed
    // back to it, or one its receiver cannot prevent (Soul Link).
    let premitigated = matches!(
        hit.kind,
        HitKind::Returned | HitKind::TransferredUnpreventable
    );
    // A share its receiver takes as its own (Blessing of Sacrifice): it met
    // the attacker's increase, armor and resistance on its holder, but this
    // unit's immunity, damage-taken modifiers and absorbs apply.
    let shared = hit.kind == HitKind::Transferred;

    // Check for damage immunity (Divine Shield, or a Cyclone) — blocks all
    // incoming damage
    if !premitigated && blocks_all_damage(active_auras.as_deref()) {
        return (0.0, 0.0);
    }

    let mut remaining_damage = damage;
    let mut total_absorbed = 0.0;

    if !premitigated {
        if !shared {
            mitigate(target, active_auras.as_deref(), &hit, &mut remaining_damage);
        }

        // Damage taken reduction (Devotion Aura, Pain Suppression, Cheat
        // Death's cut): the strongest in scope only — the stacking
        // simplification, as for the increase below. A single reduction
        // reduces exactly as it always has.
        if remaining_damage > 0.0 {
            if let Some((_, value)) = strongest_in_scope(
                active_auras.as_deref(),
                AuraType::DamageTakenReduction,
                spell_school,
                false,
            ) {
                remaining_damage *= 1.0 - value;
            }
        }

        // Damage taken increase: the strongest in scope. A charged one raises
        // direct hits only, and each spends a charge.
        if let Some(auras) = active_auras.as_deref_mut() {
            take_damage_increase(auras, &hit, &mut remaining_damage);
        }

        // Transfer: a share leaves for its receiver after the holder's
        // mitigation and before its absorbs.
        if !hit.kind.is_secondary() && remaining_damage > 0.0 {
            transfer_share(
                target,
                active_auras.as_deref(),
                &hit,
                &mut remaining_damage,
                follow_ups,
            );
        }
    }

    // Absorbs soak everything but a share that cannot be prevented: a share
    // its receiver takes, and a returned one, included.
    if hit.kind != HitKind::TransferredUnpreventable {
        // Check for absorb shields and consume them
        if let Some(auras) = active_auras.as_deref_mut() {
            for aura in auras.auras.iter_mut() {
                if aura.effect_type == AuraType::Absorb && remaining_damage > 0.0 {
                    // Invariant: absorb shield magnitude should be positive
                    debug_assert!(
                        aura.magnitude >= 0.0,
                        "apply_damage_with_absorb: absorb shield has negative magnitude ({})",
                        aura.magnitude
                    );

                    let absorb_amount = aura.magnitude.min(remaining_damage);
                    aura.magnitude -= absorb_amount;
                    remaining_damage -= absorb_amount;
                    total_absorbed += absorb_amount;
                    // A reflecting shield returns its share of what it soaked.
                    if let (Some(share), Some(attacker), false) = (
                        aura.damage_mods.reflect_absorbed,
                        hit.attacker,
                        hit.kind.is_secondary(),
                    ) {
                        if absorb_amount > 0.0 {
                            follow_ups.push(DamageFollowUp::Reflect {
                                to: attacker,
                                from: hit.victim,
                                dealer: aura.caster,
                                amount: absorb_amount * share,
                                school: aura.spell_school.unwrap_or(SpellSchool::Holy),
                                source: aura.ability_name.clone(),
                            });
                        }
                    }
                }
            }
            // Remove depleted absorb shields
            auras
                .auras
                .retain(|a| !(a.effect_type == AuraType::Absorb && a.magnitude <= 0.0));
        }
    }

    if remaining_damage > 0.0 {
        super::reveal_stealthed(target, source, combat_log);
    }

    // The killing blow: a save prevents it.
    if remaining_damage >= target.current_health && target.current_health > 0.0 {
        prevent_killing_blow(
            target,
            active_auras.as_deref_mut(),
            &hit,
            &mut remaining_damage,
            follow_ups,
        );
    }

    // Apply remaining damage to health
    let actual_damage = remaining_damage.min(target.current_health);
    target.current_health = (target.current_health - remaining_damage).max(0.0);
    target.damage_taken += actual_damage;

    // Retaliation: a melee strike on a holder of a reflect aura.
    if hit.kind == HitKind::Melee {
        if let Some(attacker) = hit.attacker {
            retaliate(active_auras.as_deref(), &hit, attacker, follow_ups);
        }
    }

    // Post-condition: health should still be valid
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: health went negative after damage"
    );

    (actual_damage, total_absorbed)
}

/// The damage a hit credits its dealer with: what reached health plus what
/// absorbs soaked, as WoW's damage meters count it. Damage an immunity
/// blocked reached neither and is credited nothing.
///
/// The one rule for "damage dealt", at every damage site: direct hits,
/// periodic ticks, channels, the dispel backlash, reflects and transferred
/// shares alike. A site credits its dealer through [`credit_damage_dealt`]
/// and logs its combat-log damage row (`CombatLog::log_damage`, the Results
/// screen's per-ability breakdown) with this amount, so a combatant's Results
/// total is the sum of its breakdown rows. `tests/damage_site_audit.rs` holds
/// every site to it; `tests/results_damage_totals.rs` plays it.
pub fn credited_damage(landed: f32, absorbed: f32) -> f32 {
    landed + absorbed
}

/// Credit `dealer` with one hit, under [`credited_damage`]'s rule: the only
/// write to `Combatant::damage_dealt`.
pub fn credit_damage_dealt(dealer: &mut Combatant, landed: f32, absorbed: f32) {
    dealer.damage_dealt += credited_damage(landed, absorbed);
}

/// Step 0: whether `auras` block every hit — a damage immunity (Divine
/// Shield) or a Cyclone.
pub fn blocks_all_damage(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.is_cycloned()
            || a.auras
                .iter()
                .any(|aura| aura.effect_type == AuraType::DamageImmunity)
    })
}

/// Steps 1-2: the attacker's damage-dealt increase, then the target's armor
/// (Physical) or resistance (magic). A transferred share skips them on its
/// receiver: it met them on the unit it left.
fn mitigate(target: &mut Combatant, auras: Option<&ActiveAuras>, hit: &Hit, damage: &mut f32) {
    let spell_school = hit.school;
    // The attacker's damage-dealt increase.
    if hit.dealt != 1.0 {
        *damage *= hit.dealt;
    }

    // Apply armor reduction for Physical damage. Equipment armor plus any
    // `ArmorIncrease` aura (Mark of the Wild), scaled by the percentage armor
    // auras — exactly `target.armor` when there is none.
    let armor = effective_armor(target, auras);
    if spell_school == SpellSchool::Physical && armor > 0.0 {
        let pre_armor = *damage;
        let reduction = armor_reduction(armor);
        *damage *= 1.0 - reduction;
        target.damage_mitigated_by_armor += pre_armor - *damage;
    }

    // Apply spell resistance for magical damage
    if spell_school != SpellSchool::Physical && spell_school != SpellSchool::None {
        let mut resistance = target.get_resistance(spell_school);

        // Sum SpellResistanceBuff auras matching this school
        if let Some(auras) = auras {
            for aura in auras.auras.iter() {
                if aura.effect_type == AuraType::SpellResistanceBuff
                    && aura.spell_school == Some(spell_school)
                {
                    resistance += aura.magnitude;
                }
            }
        }

        if resistance > 0.0 {
            let pre_resist = *damage;
            let reduction = resistance / (resistance * 5.0 / 3.0 + 300.0);
            *damage *= 1.0 - reduction;
            if let Some(slot) = resistance_school_index(spell_school) {
                target.damage_mitigated_by_resistance[slot] += pre_resist - *damage;
            }
        }
    }
}

/// What a transferred share of `amount` `school` damage, landing as `kind`
/// ([`HitKind::share`]), would put on the health of a receiver wearing
/// `auras`: exactly what [`apply_damage_with_absorb`] takes from it, read
/// without spending anything. A share that cannot be prevented is all of
/// `amount`; one its receiver takes as its own meets the receiver's immunity,
/// its damage-taken modifiers (an uncharged increase only, as the funnel
/// applies) and its absorbs. `process_damage_follow_ups` judges by it whether
/// the receiver can bear the share.
pub fn share_to_health(
    kind: HitKind,
    amount: f32,
    school: SpellSchool,
    auras: Option<&ActiveAuras>,
) -> f32 {
    if kind != HitKind::Transferred {
        return amount;
    }
    if blocks_all_damage(auras) {
        return 0.0;
    }
    let mut damage = amount;
    if damage > 0.0 {
        if let Some((_, value)) =
            strongest_in_scope(auras, AuraType::DamageTakenReduction, school, false)
        {
            damage *= 1.0 - value;
        }
    }
    if damage > 0.0 {
        if let Some((_, value)) =
            strongest_in_scope(auras, AuraType::DamageTakenIncrease, school, true)
        {
            damage *= 1.0 + value;
        }
    }
    for aura in auras.map_or(&[][..], |a| a.auras.as_slice()) {
        if aura.effect_type == AuraType::Absorb && damage > 0.0 {
            damage -= aura.magnitude.min(damage);
        }
    }
    damage
}

/// Step 9: the strongest [`AuraType::DamageReflect`] on the holder returns its
/// damage to the melee attacker, credited to the aura's caster.
fn retaliate(
    auras: Option<&ActiveAuras>,
    hit: &Hit,
    attacker: Entity,
    follow_ups: &mut DamageFollowUps,
) {
    let Some(auras) = auras else {
        return;
    };
    let Some(aura) = auras
        .auras
        .iter()
        .filter(|a| a.effect_type == AuraType::DamageReflect)
        .fold(None, |best: Option<&Aura>, a| match best {
            Some(b) if b.magnitude >= a.magnitude => best,
            _ => Some(a),
        })
    else {
        return;
    };
    if aura.magnitude <= 0.0 {
        return;
    }
    follow_ups.push(DamageFollowUp::Reflect {
        to: attacker,
        from: hit.victim,
        dealer: aura.caster,
        amount: aura.magnitude,
        school: aura.spell_school.unwrap_or(SpellSchool::Physical),
        source: aura.ability_name.clone(),
    });
}

/// Step 3's increase: the strongest [`AuraType::DamageTakenIncrease`] in scope
/// raises the hit. A charged one raises only direct hits (not periodic ones,
/// nor a transferred share), spends a charge each time, and ends at zero
/// charges.
fn take_damage_increase(auras: &mut ActiveAuras, hit: &Hit, damage: &mut f32) {
    if *damage <= 0.0 {
        return;
    }
    let uncharged_only = matches!(hit.kind, HitKind::Periodic | HitKind::Transferred);
    let Some((i, value)) = strongest_in_scope(
        Some(auras),
        AuraType::DamageTakenIncrease,
        hit.school,
        uncharged_only,
    ) else {
        return;
    };
    *damage *= 1.0 + value;
    let aura = &mut auras.auras[i];
    if let Some(charges) = aura.damage_mods.charges.as_mut() {
        *charges = charges.saturating_sub(1);
        if *charges == 0 {
            auras.auras.remove(i);
        }
    }
}

/// Step 4: the strongest transfer on the holder — a [`AuraType::DamageTransfer`]
/// (to its caster) or the holder's Soul Link (to its pet) — takes its share
/// of the hit for its receiver. A transfer aura's share is one its receiver
/// may prevent; Soul Link's is not ([`HitKind::TransferredUnpreventable`]).
fn transfer_share(
    target: &Combatant,
    auras: Option<&ActiveAuras>,
    hit: &Hit,
    damage: &mut f32,
    follow_ups: &mut DamageFollowUps,
) {
    let aura = auras.and_then(|a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::DamageTransfer)
            .filter_map(|aura| {
                aura.caster.map(|c| Share {
                    receiver: TransferReceiver::Unit(c),
                    share: aura.magnitude,
                    by: aura.ability_name.as_str(),
                    preventable: true,
                })
            })
            .fold(None, |best: Option<Share>, t| match best {
                Some(b) if b.share >= t.share => best,
                _ => Some(t),
            })
    });
    // A severed link (its pet is dead) takes no share.
    let link = target
        .talent_mechanics
        .soul_link()
        .filter(|_| !target.soul_link_severed)
        .map(|share| Share {
            receiver: TransferReceiver::OwnPet,
            share,
            by: SOUL_LINK_NAME,
            // TBC's tooltip (spell 19028): "That damage cannot be prevented."
            preventable: false,
        });
    let best = match (aura, link) {
        (Some(a), Some(l)) => Some(if l.share > a.share { l } else { a }),
        (a, l) => a.or(l),
    };
    let Some(Share {
        receiver,
        share,
        by,
        preventable,
    }) = best
    else {
        return;
    };
    let amount = *damage * share.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return;
    }
    *damage -= amount;
    follow_ups.push(DamageFollowUp::Transfer {
        holder: hit.victim,
        receiver,
        amount,
        school: hit.school,
        preventable,
        dealer: hit.attacker,
        ability: hit.row.to_string(),
        by: by.to_string(),
    });
}

/// One transfer a holder carries, as step 4 weighs it.
#[derive(Clone, Copy)]
struct Share<'a> {
    receiver: TransferReceiver,
    share: f32,
    by: &'a str,
    preventable: bool,
}

/// The name Soul Link's transfers log.
pub const SOUL_LINK_NAME: &str = "Soul Link";

/// Step 7: a blow that would kill the holder is prevented by its save — the
/// first [`AuraType::KillingBlowSave`] on it (spent, with the rest of its
/// compound when it is one — Guardian Spirit; the holder's health is
/// left where it stood, and a follow-up heals it through `apply_healing`), or
/// else an unspent Cheat Death (the holder is left at its share of maximum
/// health, or where it stood if lower, and takes less damage for a while).
fn prevent_killing_blow(
    target: &mut Combatant,
    auras: Option<&mut ActiveAuras>,
    hit: &Hit,
    damage: &mut f32,
    follow_ups: &mut DamageFollowUps,
) {
    if let Some(auras) = auras {
        if let Some(i) = auras
            .auras
            .iter()
            .position(|a| a.effect_type == AuraType::KillingBlowSave)
        {
            // A save that belongs to a compound (Guardian Spirit) spends the
            // whole buff, its healing increase included.
            let save = auras.remove_debuff_at(i);
            *damage = 0.0;
            follow_ups.push(DamageFollowUp::Saved {
                holder: hit.victim,
                by: save.ability_name,
                caster: save.caster,
                heal_fraction: Some(save.magnitude),
                aura: None,
            });
            return;
        }
        if let Some(aura) = cheat_death(target, damage) {
            let by = aura.ability_name.clone();
            auras.auras.push(aura);
            follow_ups.push(DamageFollowUp::Saved {
                holder: hit.victim,
                by,
                caster: None,
                heal_fraction: None,
                aura: None,
            });
        }
        return;
    }
    if let Some(aura) = cheat_death(target, damage) {
        follow_ups.push(DamageFollowUp::Saved {
            holder: hit.victim,
            by: aura.ability_name.clone(),
            caster: None,
            heal_fraction: None,
            aura: Some((aura.magnitude, aura.duration)),
        });
    }
}

/// The name a Cheat Death save logs, and the name of its damage cut.
pub const CHEAT_DEATH_NAME: &str = "Cheat Death";

/// Spend the holder's Cheat Death on a killing blow, when it has one unspent:
/// the blow leaves it at its share of maximum health (or where it stood, if
/// lower), and the damage cut it then takes is returned.
fn cheat_death(target: &mut Combatant, damage: &mut f32) -> Option<Aura> {
    if target.killing_blow_save_spent {
        return None;
    }
    let (health, reduction, duration) = target.talent_mechanics.cheat_death()?;
    target.killing_blow_save_spent = true;
    let floor = (target.max_health * health).min(target.current_health);
    *damage = (target.current_health - floor).max(0.0);
    Some(cheat_death_aura(reduction, duration))
}

/// The damage cut Cheat Death leaves on the Rogue it saves: untyped (TBC gives
/// it no dispel type), so nothing removes it.
pub fn cheat_death_aura(reduction: f32, duration: f32) -> Aura {
    Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration,
        magnitude: reduction,
        break_on_damage_threshold: -1.0,
        ability_name: CHEAT_DEATH_NAME.to_string(),
        dispel_type: DispelType::Untyped,
        ..Default::default()
    }
}

/// Check if a combatant has an absorb shield active
pub fn has_absorb_shield(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::Absorb)
    })
}

/// Check if a combatant has Weakened Soul (cannot receive Power Word: Shield)
pub fn has_weakened_soul(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::WeakenedSoul)
    })
}

/// Get the physical damage reduction multiplier from DamageReduction auras on the attacker.
/// Used by Curse of Weakness to reduce outgoing physical damage by a percentage.
/// Returns the percentage reduction (0.2 = 20% less damage).
/// Multiple reductions stack additively (two 20% reductions = 40% total).
pub fn get_physical_damage_reduction(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::DamageReduction)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Check if a combatant has damage immunity (Divine Shield active)
pub fn has_damage_immunity(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::DamageImmunity)
    })
}

/// Returns the outgoing damage multiplier for the caster.
/// If caster has DamageImmunity (Divine Shield), returns DIVINE_SHIELD_DAMAGE_PENALTY (0.5).
/// Otherwise returns 1.0 (no penalty).
pub fn get_divine_shield_damage_penalty(auras: Option<&ActiveAuras>) -> f32 {
    if has_damage_immunity(auras) {
        DIVINE_SHIELD_DAMAGE_PENALTY
    } else {
        1.0
    }
}

/// Process interrupt attempts: interrupt target's cast or channel and apply spell school lockout.
pub fn process_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    interrupts: Query<(Entity, &InterruptPending)>,
    mut casting_targets: Query<(&mut CastingState, &Combatant), Without<ChannelingState>>,
    mut channeling_targets: Query<(&mut ChannelingState, &Combatant), Without<CastingState>>,
    combatants: Query<&Combatant>,
    pet_query: Query<&Pet>,
    celebration: Option<Res<VictoryCelebration>>,
    auras_query: Query<&ActiveAuras>,
    positions: Query<&Transform>,
    // `Option` so a world with no map (the Animation Sandbox's staged
    // interrupts) resolves as an open field.
    map_geometry: Option<Res<super::super::map_config::ActiveMapGeometry>>,
) {
    // Don't process interrupts during victory celebration
    if celebration.is_some() {
        return;
    }

    for (interrupt_entity, interrupt) in interrupts.iter() {
        // No interrupt lands on a caster its interrupter cannot see: it was
        // spent (cooldown and cost, at its use site) and does nothing. Every
        // interrupter's decision already asks for sight
        // (`class_ai::interrupts`), and nothing moves between the decision and
        // here; this keeps any other path that queues an `InterruptPending` to
        // the same rule.
        let blind = map_geometry.as_ref().is_some_and(|map| {
            match (
                positions.get(interrupt.caster),
                positions.get(interrupt.target),
            ) {
                (Ok(from), Ok(to)) => !super::super::map_geometry::has_line_of_sight(
                    &map.volumes,
                    from.translation,
                    to.translation,
                ),
                _ => false,
            }
        });
        if blind {
            commands.entity(interrupt_entity).despawn();
            continue;
        }
        let mut interrupted = false;

        // Check if target is casting. A cast already interrupted, or an
        // instant routed through a zero-length cast, is no interrupt target:
        // the interrupt was spent (cooldown and cost, at its use site) and does
        // nothing — no cut, no lockout. The AI never queues one at an instant
        // (`class_ai::cast_is_interruptible`); this keeps every other path that
        // queues an `InterruptPending` to the same rule.
        if let Ok((mut cast_state, target_combatant)) = casting_targets.get_mut(interrupt.target) {
            // The cast is read from its caster's own book.
            let book = target_combatant.book();
            if class_ai::cast_is_interruptible(&cast_state, book) {
                // Get the spell school of the interrupted spell
                let interrupted_ability_def = book.get_unchecked(&cast_state.ability);
                let interrupted_school = interrupted_ability_def.spell_school;
                let interrupted_spell_name = &interrupted_ability_def.name;

                // Mark cast as interrupted
                cast_state.interrupted = true;
                cast_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds
                commands.spawn((
                    CastEnding {
                        caster: interrupt.target,
                        kind: CastEndingKind::Interrupted,
                    },
                    // Names the interrupt, so its landing plays on the victim
                    // (graphical only; inert in headless).
                    InterruptedBy {
                        ability: interrupt.ability,
                        interrupter: interrupt.caster,
                    },
                    PlayMatchEntity,
                ));

                // Mark the ability cast as interrupted in the combat log (for timeline visualization)
                let interrupted_caster_id =
                    combat_log_id_for(target_combatant, pet_query.get(interrupt.target).ok());
                combat_log.mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                // Check for lockout duration reduction (Concentration Aura)
                let lockout_reduction =
                    get_lockout_duration_reduction(auras_query.get(interrupt.target).ok());

                // Apply lockout and log
                apply_interrupt_lockout(
                    &mut commands,
                    &mut combat_log,
                    interrupt,
                    &combatants,
                    &pet_query,
                    target_combatant,
                    interrupted_school,
                    interrupted_spell_name,
                    lockout_reduction,
                );

                interrupted = true;
            }
        }

        // Check if target is channeling (if not already interrupted a cast)
        if !interrupted {
            if let Ok((mut channel_state, target_combatant)) =
                channeling_targets.get_mut(interrupt.target)
            {
                // Don't interrupt if already interrupted
                if !channel_state.interrupted {
                    // Get the spell school of the interrupted channel
                    let interrupted_ability_def = target_combatant
                        .book()
                        .get_unchecked(&channel_state.ability);
                    let interrupted_school = interrupted_ability_def.spell_school;
                    let interrupted_spell_name = &interrupted_ability_def.name;

                    // Mark channel as interrupted
                    channel_state.interrupted = true;
                    channel_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds
                    commands.spawn((
                        CastEnding {
                            caster: interrupt.target,
                            kind: CastEndingKind::Interrupted,
                        },
                        InterruptedBy {
                            ability: interrupt.ability,
                            interrupter: interrupt.caster,
                        },
                        PlayMatchEntity,
                    ));

                    // Mark the ability as interrupted in the combat log (for timeline visualization)
                    let interrupted_caster_id =
                        combat_log_id_for(target_combatant, pet_query.get(interrupt.target).ok());
                    combat_log
                        .mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                    // Check for lockout duration reduction (Concentration Aura)
                    let lockout_reduction =
                        get_lockout_duration_reduction(auras_query.get(interrupt.target).ok());

                    // Apply lockout and log
                    apply_interrupt_lockout(
                        &mut commands,
                        &mut combat_log,
                        interrupt,
                        &combatants,
                        &pet_query,
                        target_combatant,
                        interrupted_school,
                        interrupted_spell_name,
                        lockout_reduction,
                    );
                }
            }
        }

        // Despawn the interrupt entity
        commands.entity(interrupt_entity).despawn();
    }
}

/// Helper function to apply spell school lockout and log the interrupt.
fn apply_interrupt_lockout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    interrupt: &InterruptPending,
    combatants: &Query<&Combatant>,
    pet_query: &Query<&Pet>,
    target_combatant: &Combatant,
    interrupted_school: SpellSchool,
    interrupted_spell_name: &str,
    lockout_reduction: f32,
) {
    // Pet-aware interrupter id (Pummel/Wind Shear can come from a pet).
    let interrupter = combatants.get(interrupt.caster).ok();
    let caster_id = interrupter
        .map(|caster| combat_log_id_for(caster, pet_query.get(interrupt.caster).ok()))
        .unwrap_or_else(|| "Unknown".to_string());
    // The interrupt is named from its interrupter's own book.
    let interrupt_name = interrupter.map_or_else(
        || format!("{:?}", interrupt.ability),
        |caster| caster.book().get_unchecked(&interrupt.ability).name.clone(),
    );
    // The interrupted combatant's id (target_combatant == interrupt.target).
    let target_id = combat_log_id_for(target_combatant, pet_query.get(interrupt.target).ok());

    // Apply spell school lockout aura. The locked school is stored in the aura's
    // `magnitude` (the lockout aura has no dedicated school slot); the AI decodes
    // it via `SpellSchool::from_lockout_magnitude`.
    let locked_school_value = interrupted_school.to_lockout_magnitude();

    // Apply lockout duration reduction (e.g., Concentration Aura reduces by 50%)
    let effective_lockout = interrupt.lockout_duration * (1.0 - lockout_reduction);

    commands.spawn(AuraPending {
        target: interrupt.target,
        aura: Aura {
            effect_type: AuraType::SpellSchoolLockout,
            duration: effective_lockout,
            magnitude: locked_school_value,
            break_on_damage_threshold: -1.0, // Never breaks on damage
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(interrupt.caster),
            damage_mods: Default::default(),
            ability_name: interrupt_name,
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None, // Lockouts are not dispellable
            applied_this_frame: false,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Auto,
            compound: None,
            source_item: None,
            source_talent: None,
            next_cast: None,
            leech: None,
            stacks: None,
            bloom: None,
        },
    });

    // Log the interrupt
    let school_name = match interrupted_school {
        SpellSchool::Physical => "Physical",
        SpellSchool::Frost => "Frost",
        SpellSchool::Holy => "Holy",
        SpellSchool::Shadow => "Shadow",
        SpellSchool::Arcane => "Arcane",
        SpellSchool::Fire => "Fire",
        SpellSchool::Nature => "Nature",
        SpellSchool::None => "None",
    };

    let message = format!(
        "{} interrupts {}'s {} - {} school locked for {:.1}s",
        caster_id, target_id, interrupted_spell_name, school_name, effective_lockout
    );
    combat_log.log(CombatLogEventType::AbilityUsed, message);

    info!(
        "Team {} {} interrupted! {} school locked for {:.1}s",
        target_combatant.team,
        target_combatant.class.name(),
        school_name,
        effective_lockout
    );
}
