//! Player-facing talent text, generated from the node data in `talents.ron`.
//!
//! The talent sibling of [`super::ability_text`]: ONE generator for what a
//! talent does, so its tooltip in the talent panel, its encyclopedia page and
//! its search entry say the same thing. Nothing is hand-written per talent: a
//! node's prose comes from its effects (`per_rank`), its rank, its tier and
//! its prerequisite, so talent N+1 reads correctly with no code here.
//!
//! Ranks ADD, for every effect kind (the engine's rule, `talents.rs`), so the
//! text for rank `n` is each per-rank number times `n`: two ranks of "+3%
//! maximum health" read "+6% maximum health".
//!
//! Pure functions of plain data — no egui, no Bevy — so tests assert on the
//! strings directly.

use crate::states::ability_text::{build_aura_description, next_cast_sentence};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::ability_config::{AbilityDefinitions, AuraEffect};
use crate::states::play_match::talent_choices::{
    ranked_effects, ChoiceMenu, ChoiceOption, ChoiceSlot,
};
use crate::states::play_match::talent_mechanics::TalentMechanic;
use crate::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentTrigger,
};
use crate::states::play_match::talents::{
    AbilityField, LearnBlocker, PatchOp, PatchTarget, TalentConfig, TalentDefinitions,
    TalentEffect, TalentId, TalentStat, TreeId, CAPSTONE_TIER, MAX_TALENT_POINTS, TIER_OPENS_AT,
};

/// What `n` ranks of one effect do, as a clause: "+6% maximum health".
pub fn effect_text(effect: &TalentEffect, ranks: u8, abilities: &AbilityDefinitions) -> String {
    let n = f32::from(ranks);
    match effect {
        TalentEffect::StatPercent(stat, fraction) => {
            format!("{} {}", percent(fraction * n), stat_name(*stat))
        }
        TalentEffect::PetStatPercent(stat, fraction) => {
            format!("{} pet {}", percent(fraction * n), stat_name(*stat))
        }
        TalentEffect::Patch { on, field, op } => {
            let target = match on {
                PatchTarget::Ability(ability) => abilities
                    .get(ability)
                    .map_or_else(|| format!("{ability:?}"), |c| c.name.clone()),
                PatchTarget::School(school) => format!("{school:?} abilities'"),
            };
            let change = match op {
                PatchOp::Add(value) if field_is_fraction(*field) => percent(value * n),
                PatchOp::Add(value) => signed(value * n, field_unit(*field)),
                PatchOp::Mul(m) => percent((m - 1.0) * n),
            };
            format!("{change} {target} {}", field_name(*field))
        }
        TalentEffect::Grant(ability) => {
            let name = abilities
                .get(ability)
                .map_or_else(|| format!("{ability:?}"), |c| c.name.clone());
            format!("Learn {name}")
        }
        TalentEffect::WidenChoice(option) => {
            format!(
                "Unlocks {} as {} choice",
                option_name(*option, abilities),
                menu_phrase(option.menu())
            )
        }
        TalentEffect::AddChoiceSlot(slot) => match slot {
            ChoiceSlot::OffHandPoison => "Opens a second poison slot for the off hand".to_string(),
        },
        TalentEffect::Mechanic(mechanic) => mechanic_text(mechanic.at_rank(ranks)),
        // The proc the engine resolves at this rank (`TalentProc::at_rank`):
        // its chance or its effect times the rank, as it declares.
        TalentEffect::Proc(proc) => proc_clause(&proc.at_rank(ranks), abilities),
        // The engine's own rank rule: once per rank from rank `from` on.
        TalentEffect::FromRank(from, inner) => {
            match ranked_effects(std::slice::from_ref(effect), ranks).next() {
                Some((inner, times)) => effect_text(inner, times, abilities),
                None => format!("From rank {from}: {}", effect_text(inner, 1, abilities)),
            }
        }
    }
}

/// What `rank` ranks of `talent` do, every effect joined: "+6% maximum
/// health". Rank 0 reads as rank 1, what the first point buys. An effect
/// that starts at a later rank (`FromRank`) is left out until that rank.
pub fn rank_text(talent: &TalentConfig, rank: u8, abilities: &AbilityDefinitions) -> String {
    let rank = rank.max(1);
    let clauses: Vec<String> = ranked_effects(&talent.per_rank, rank)
        .map(|(effect, times)| effect_text(effect, times, abilities))
        .collect();
    format!("{}.", clauses.join(", "))
}

/// A per-match option's name: the curse's ability name from the config, the
/// others' own display names.
fn option_name(option: ChoiceOption, abilities: &AbilityDefinitions) -> String {
    match option {
        ChoiceOption::RogueOpener(o) => o.name().to_string(),
        ChoiceOption::RoguePoison(p) => p.name().to_string(),
        ChoiceOption::WarlockCurse(c) => abilities
            .get(&c.ability())
            .map_or_else(|| format!("{c:?}"), |a| a.name.clone()),
        ChoiceOption::WarlockDemon(d) => d.name().to_string(),
        ChoiceOption::HunterPet(p) => p.name().to_string(),
        ChoiceOption::WarriorShout(s) => s.name().to_string(),
        ChoiceOption::MageArmor(a) => a.name().to_string(),
        ChoiceOption::PaladinAura(a) => a.name().to_string(),
        ChoiceOption::PriestForm(f) => f.name().to_string(),
    }
}

/// A per-match menu in a sentence, with its article ("a curse", "an aura"):
/// each menu carries its own, so no rule guesses one from the spelling.
fn menu_phrase(menu: ChoiceMenu) -> &'static str {
    match menu {
        ChoiceMenu::RogueOpener => "a stealth opener",
        ChoiceMenu::RoguePoison => "a weapon poison",
        ChoiceMenu::WarlockCurse => "a curse",
        ChoiceMenu::WarlockDemon => "a demon",
        ChoiceMenu::HunterPet => "a pet",
        ChoiceMenu::WarriorShout => "a shout",
        ChoiceMenu::MageArmor => "an armor",
        ChoiceMenu::PaladinAura => "an aura",
        ChoiceMenu::PriestForm => "a form",
    }
}

/// A bespoke mechanic, at the rank its numbers were scaled to.
fn mechanic_text(mechanic: TalentMechanic) -> String {
    match mechanic {
        TalentMechanic::SoulLink { share } => format!(
            "{} of the damage you take is dealt to your pet instead, while it lives",
            unsigned_percent(share)
        ),
        TalentMechanic::ReflectiveShield { share } => format!(
            "{} of the damage your absorb shields soak is dealt back to the attacker",
            unsigned_percent(share)
        ),
        TalentMechanic::CheatDeath {
            health,
            reduction,
            duration,
        } => format!(
            "The first blow that would kill you leaves you at {} health, and you take {} less damage for {}",
            unsigned_percent(health),
            unsigned_percent(reduction),
            unsigned(duration, "sec")
        ),
        TalentMechanic::MasterDemonologist { reduction } => format!(
            "You and your pet take {} less spell damage while it lives",
            unsigned_percent(reduction)
        ),
        TalentMechanic::DemonicSacrifice { shadow_damage } => format!(
            "Sacrificing your pet at the gates grants Touch of Shadow: {} Shadow damage for the \
             match",
            percent(shadow_damage)
        ),
    }
}

/// A talent proc as a clause, at the numbers it carries (a node's proc is
/// one rank's worth; [`effect_text`] hands this the proc at its rank): when
/// it fires, and what it does. "A 4% chance on each damage tick of
/// Corruption, primes your next cast: Within 10 sec, the next Shadow Bolt is
/// instant". The one generator for a talent proc's prose, the talent panel's
/// and the encyclopedia's alike ([`talent_proc_description`]).
pub fn proc_clause(proc: &TalentProc, abilities: &AbilityDefinitions) -> String {
    let names = |list: &[crate::states::play_match::AbilityType]| -> String {
        let names: Vec<String> = list
            .iter()
            .map(|a| {
                abilities
                    .get(a)
                    .map_or_else(|| format!("{a:?}"), |d| d.name.clone())
            })
            .collect();
        names.join(" or ")
    };
    let of = |what: &str| {
        if proc.abilities.is_empty() {
            what.to_string()
        } else {
            format!("{what} of {}", names(&proc.abilities))
        }
    };
    let event = match proc.trigger {
        TalentTrigger::MeleeHit if proc.crit_only => "a melee critical strike".to_string(),
        TalentTrigger::MeleeHit => "a melee hit".to_string(),
        TalentTrigger::SpellHit => of("a hit"),
        TalentTrigger::DotTick => of("each damage tick"),
        TalentTrigger::HealCrit => of("a critical heal"),
        TalentTrigger::CritTaken => "being struck by a melee or ranged critical hit".to_string(),
        TalentTrigger::StealthBreak => "leaving stealth".to_string(),
        TalentTrigger::AbilityUsed if proc.abilities.is_empty() => "using any ability".to_string(),
        TalentTrigger::AbilityUsed => format!("using {}", names(&proc.abilities)),
    };
    let chance = match proc.chance {
        ProcChance::Chance(c) if c >= 1.0 => "On".to_string(),
        ProcChance::Chance(c) => format!("A {} chance on", unsigned_percent(c)),
        ProcChance::PerMinute(r) => format!(
            "{} times a minute of melee, whatever the weapon, on",
            unsigned(r, "")
        ),
    };
    let effect = match &proc.effect {
        ProcEffect::ApplyAura {
            on,
            aura_type,
            magnitude,
            duration,
            tick_interval,
            dispel_type,
            ..
        } => {
            let whom = match on {
                ProcTarget::Owner => "you",
                ProcTarget::Other => match proc.trigger {
                    TalentTrigger::HealCrit => "the ally healed",
                    TalentTrigger::CritTaken => "the attacker",
                    _ => "the target",
                },
            };
            // The aura's own sentence, the one every ability's aura reads
            // (`build_aura_description`), so its number is in its own unit.
            let sentence = build_aura_description(&AuraEffect {
                aura_type: *aura_type,
                duration: *duration,
                magnitude: *magnitude,
                magnitude_coefficient: 0.0,
                break_on_damage: -1.0,
                tick_interval: *tick_interval,
                dr_category: None,
                scope: None,
                charges: None,
                dispel_type: *dispel_type,
                stacking: None,
                bloom: None,
                compound: None,
                purge_priority: None,
                leech: None,
                next_cast: None,
            });
            format!("{} (on {whom})", in_clause(&sentence))
        }
        ProcEffect::NextCast {
            modifier, duration, ..
        } => format!(
            "primes your next cast: {}",
            next_cast_sentence(modifier, *duration).trim_end_matches('.')
        ),
        ProcEffect::HealBack {
            fraction, duration, ..
        } => format!(
            "heals you for {} of it over {}",
            unsigned_percent(*fraction),
            unsigned(*duration, "sec")
        ),
    };
    let cooldown = if proc.internal_cooldown > 0.0 {
        format!(
            " (at most once every {})",
            unsigned(proc.internal_cooldown, "sec")
        )
    } else {
        String::new()
    };
    format!("{chance} {event}, {effect}{cooldown}")
}

/// One rank of a talent's proc, named `talent`, as a sentence: the
/// encyclopedia's Buffs & Debuffs entry for the aura it applies. Its numbers
/// are the first rank's, and it says what each further rank adds to.
pub fn talent_proc_description(
    talent: &str,
    proc: &TalentProc,
    abilities: &AbilityDefinitions,
) -> String {
    let rank = match proc.ranks_add {
        ProcRankScale::Chance => " The chance is per rank.",
        ProcRankScale::Effect => " Its effect is per rank.",
    };
    format!("{talent}: {}.{rank}", proc_clause(proc, abilities))
}

/// A sentence made a clause: its first letter lower-cased, its full stops
/// turned to semicolons. "Roots the target for 4 sec. Breaks after 80
/// damage." reads "roots the target for 4 sec; breaks after 80 damage".
fn in_clause(sentence: &str) -> String {
    let body = sentence.trim().trim_end_matches('.').replace(". ", "; ");
    let mut chars = body.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Every rank's text, one line each: "Rank 1: +3% maximum health." A
/// one-rank node gives one line with no rank prefix.
pub fn rank_lines(talent: &TalentConfig, abilities: &AbilityDefinitions) -> Vec<String> {
    if talent.max_rank == 1 {
        return vec![rank_text(talent, 1, abilities)];
    }
    (1..=talent.max_rank)
        .map(|rank| format!("Rank {rank}: {}", rank_text(talent, rank, abilities)))
        .collect()
}

/// Where the talent sits: "Demonology, tier 1 · 2 ranks". A capstone says
/// so.
pub fn placement(talents: &TalentDefinitions, talent: &TalentConfig) -> String {
    let tier = if talent.tier == CAPSTONE_TIER {
        "capstone".to_string()
    } else {
        format!("tier {}", talent.tier)
    };
    let ranks = if talent.max_rank == 1 {
        "1 rank".to_string()
    } else {
        format!("{} ranks", talent.max_rank)
    };
    format!(
        "{}, {tier} \u{b7} {ranks}",
        tree_name(talents, talent.class, talent.tree)
    )
}

/// What the talent needs before its first rank, one line each: its tier's
/// gate and its prerequisite. Empty for a tier-1 node with none.
pub fn requirement_lines(talents: &TalentDefinitions, talent: &TalentConfig) -> Vec<String> {
    let mut lines = Vec::new();
    let gate = TIER_OPENS_AT[usize::from(talent.tier.clamp(1, CAPSTONE_TIER) - 1)];
    if gate > 0 {
        lines.push(format!(
            "Requires {gate} points in {}",
            tree_name(talents, talent.class, talent.tree)
        ));
    }
    if let Some((required, rank)) = talent.requires {
        lines.push(requires_line(talents, required, rank));
    }
    lines
}

/// Why the talent cannot take another rank, as the panel says it.
pub fn blocker_text(
    talents: &TalentDefinitions,
    class: CharacterClass,
    blocker: LearnBlocker,
) -> String {
    match blocker {
        LearnBlocker::NotThisClass => format!("Not a {} talent", class.name()),
        LearnBlocker::MaxRank => "Every rank learned".to_string(),
        LearnBlocker::NoPoints => format!("All {MAX_TALENT_POINTS} points spent"),
        LearnBlocker::TierClosed { tree, needed, has } => format!(
            "Requires {needed} points in {} ({has} spent)",
            tree_name(talents, class, tree)
        ),
        LearnBlocker::Requires { talent, rank } => requires_line(talents, talent, rank),
    }
}

/// The talent's name.
pub fn talent_name(talents: &TalentDefinitions, talent: TalentId) -> String {
    talents
        .get(talent)
        .map_or_else(|| format!("{talent:?}"), |t| t.name.clone())
}

/// A tree's display name.
pub fn tree_name(talents: &TalentDefinitions, class: CharacterClass, tree: TreeId) -> String {
    talents
        .trees(class)
        .iter()
        .find(|t| t.id == tree)
        .map_or_else(|| format!("{tree:?}"), |t| t.name.clone())
}

fn requires_line(talents: &TalentDefinitions, required: TalentId, rank: u8) -> String {
    let max = talents.get(required).map_or(rank, |t| t.max_rank);
    format!("Requires {} {rank}/{max}", talent_name(talents, required))
}

/// "+3%", to one decimal only when it needs one.
fn percent(fraction: f32) -> String {
    signed(fraction * 100.0, "%")
}

/// "30%", with no sign: a share, not a change.
fn unsigned_percent(fraction: f32) -> String {
    unsigned(fraction * 100.0, "%")
}

/// A signed number with its unit: "+3%", "-0.25 sec", "+1.5%".
fn signed(value: f32, unit: &str) -> String {
    let sign = if value < 0.0 { "\u{2212}" } else { "+" };
    format!("{sign}{}", unsigned(value.abs(), unit))
}

/// A number with its unit and no sign: "3%", "0.25 sec", "8 sec".
fn unsigned(value: f32, unit: &str) -> String {
    let magnitude = value.abs();
    let rounded = (magnitude * 100.0).round() / 100.0;
    let number = if (rounded - rounded.round()).abs() < f32::EPSILON {
        format!("{rounded:.0}")
    } else {
        let two = format!("{rounded:.2}");
        two.trim_end_matches('0').to_string()
    };
    let gap = if unit == "%" || unit.is_empty() {
        ""
    } else {
        " "
    };
    format!("{number}{gap}{unit}")
}

/// A stat's name in a sentence.
fn stat_name(stat: TalentStat) -> &'static str {
    match stat {
        TalentStat::MaxHealth => "maximum health",
        TalentStat::MaxMana => "maximum mana",
        TalentStat::ManaRegen => "mana regeneration",
        TalentStat::AttackPower => "attack power",
        TalentStat::SpellPower => "spell power",
        TalentStat::CritChance => "critical strike chance",
        TalentStat::MovementSpeed => "movement speed",
        TalentStat::Armor => "armor",
        TalentStat::FireResistance => "fire resistance",
        TalentStat::FrostResistance => "frost resistance",
        TalentStat::ShadowResistance => "shadow resistance",
        TalentStat::ArcaneResistance => "arcane resistance",
        TalentStat::NatureResistance => "nature resistance",
        TalentStat::HolyResistance => "holy resistance",
        TalentStat::WeaponDamage => "weapon damage",
        TalentStat::OffHandDamage => "off-hand weapon damage",
        TalentStat::DodgeChance => "dodge chance",
        TalentStat::ParryChance => "parry chance",
        TalentStat::MeleeAttackSpeed => "melee attack speed",
    }
}

/// A patchable field's name in a sentence ("Shadow Bolt cast time").
fn field_name(field: AbilityField) -> &'static str {
    match field {
        AbilityField::CastTime => "cast time",
        AbilityField::Range => "range",
        AbilityField::MinRange => "minimum range",
        AbilityField::ManaCost => "mana cost",
        AbilityField::Cooldown => "cooldown",
        AbilityField::DamageBaseMin => "minimum damage",
        AbilityField::DamageBaseMax => "maximum damage",
        AbilityField::DamageCoefficient => "damage from spell power",
        AbilityField::HealingBaseMin => "minimum healing",
        AbilityField::HealingBaseMax => "maximum healing",
        AbilityField::HealingCoefficient => "healing from spell power",
        AbilityField::ApplicationChance => "chance to apply",
        AbilityField::ProjectileSpeed => "projectile speed",
        AbilityField::LockoutDuration => "lockout",
        AbilityField::ChannelDuration => "channel duration",
        AbilityField::ChannelTickInterval => "time between ticks",
        AbilityField::ChannelHealingPerTick => "healing per tick",
        AbilityField::ManaBurnAmount => "mana burned",
        AbilityField::AuraDuration => "effect duration",
        AbilityField::AuraMagnitude => "effect magnitude",
        AbilityField::AuraMagnitudeCoefficient => "effect strength from spell power",
        AbilityField::AuraBreakOnDamage => "damage to break",
        AbilityField::AuraTickInterval => "time between ticks",
        AbilityField::Damage => "damage",
        AbilityField::CritChance => "critical strike chance",
        AbilityField::AuraStrength => "effect strength",
    }
}

/// Whether `field` holds a fraction, so an `Add` patch of it reads as
/// percentage points ("+3% Shadow Bolt critical strike chance").
fn field_is_fraction(field: AbilityField) -> bool {
    match field {
        AbilityField::CritChance | AbilityField::ApplicationChance => true,
        AbilityField::CastTime
        | AbilityField::Range
        | AbilityField::MinRange
        | AbilityField::ManaCost
        | AbilityField::Cooldown
        | AbilityField::DamageBaseMin
        | AbilityField::DamageBaseMax
        | AbilityField::DamageCoefficient
        | AbilityField::HealingBaseMin
        | AbilityField::HealingBaseMax
        | AbilityField::HealingCoefficient
        | AbilityField::ProjectileSpeed
        | AbilityField::LockoutDuration
        | AbilityField::ChannelDuration
        | AbilityField::ChannelTickInterval
        | AbilityField::ChannelHealingPerTick
        | AbilityField::ManaBurnAmount
        | AbilityField::AuraDuration
        | AbilityField::AuraMagnitude
        | AbilityField::AuraMagnitudeCoefficient
        | AbilityField::AuraBreakOnDamage
        | AbilityField::AuraTickInterval
        | AbilityField::AuraStrength
        // Takes only a Mul, which is written as a percentage.
        | AbilityField::Damage => false,
    }
}

/// The unit an `Add` patch of `field` is written in.
fn field_unit(field: AbilityField) -> &'static str {
    match field {
        AbilityField::CastTime
        | AbilityField::Cooldown
        | AbilityField::LockoutDuration
        | AbilityField::ChannelDuration
        | AbilityField::ChannelTickInterval
        | AbilityField::AuraDuration
        | AbilityField::AuraTickInterval => "sec",
        AbilityField::Range | AbilityField::MinRange => "yd",
        AbilityField::ManaCost | AbilityField::ManaBurnAmount => "mana",
        AbilityField::DamageBaseMin
        | AbilityField::DamageBaseMax
        | AbilityField::DamageCoefficient
        | AbilityField::HealingBaseMin
        | AbilityField::HealingBaseMax
        | AbilityField::HealingCoefficient
        | AbilityField::ApplicationChance
        | AbilityField::ProjectileSpeed
        | AbilityField::ChannelHealingPerTick
        | AbilityField::AuraMagnitude
        | AbilityField::AuraMagnitudeCoefficient
        | AbilityField::AuraBreakOnDamage
        | AbilityField::CritChance
        | AbilityField::AuraStrength
        // Takes only a Mul, which is written as a percentage.
        | AbilityField::Damage => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::AbilityType;

    fn abilities() -> &'static AbilityDefinitions {
        AbilityDefinitions::shipped()
    }

    #[test]
    fn ranks_add_in_the_text_as_in_the_engine() {
        let effect = TalentEffect::StatPercent(TalentStat::MaxHealth, 0.03);
        assert_eq!(effect_text(&effect, 1, abilities()), "+3% maximum health");
        assert_eq!(effect_text(&effect, 2, abilities()), "+6% maximum health");
        let pet = TalentEffect::PetStatPercent(TalentStat::WeaponDamage, 0.10);
        assert_eq!(effect_text(&pet, 2, abilities()), "+20% pet weapon damage");
    }

    /// The Combat tree's nodes read as Classic writes them: a raised chance
    /// or haste in points, and Aggression as Sinister Strike's damage.
    #[test]
    fn the_combat_tree_reads_as_classic_writes_it() {
        let talents = TalentDefinitions::shipped();
        let text = |id: TalentId, rank: u8| rank_text(talents.get(id).unwrap(), rank, abilities());
        assert_eq!(text(TalentId::LightningReflexes, 2), "+8% dodge chance.");
        assert_eq!(text(TalentId::Deflection, 1), "+4% parry chance.");
        assert_eq!(
            text(TalentId::DualWieldSpecialization, 2),
            "+20% off-hand weapon damage."
        );
        assert_eq!(
            text(TalentId::Aggression, 2),
            "+12% Sinister Strike damage."
        );
        assert_eq!(text(TalentId::SwiftBlades, 1), "+10% melee attack speed.");
        assert_eq!(text(TalentId::ExposeArmor, 1), "Learn Expose Armor.");
    }

    #[test]
    fn patches_and_grants_name_their_ability() {
        let bane = TalentEffect::Patch {
            on: PatchTarget::Ability(AbilityType::Shadowbolt),
            field: AbilityField::CastTime,
            op: PatchOp::Add(-0.25),
        };
        assert_eq!(
            effect_text(&bane, 2, abilities()),
            "\u{2212}0.5 sec Shadow Bolt cast time"
        );
        let drain = TalentEffect::Patch {
            on: PatchTarget::Ability(AbilityType::DrainLife),
            field: AbilityField::ChannelHealingPerTick,
            op: PatchOp::Mul(1.15),
        };
        assert_eq!(
            effect_text(&drain, 1, abilities()),
            "+15% Drain Life healing per tick"
        );
        let grant = TalentEffect::Grant(AbilityType::DeathCoil);
        assert_eq!(effect_text(&grant, 1, abilities()), "Learn Death Coil");
    }

    #[test]
    fn choices_and_mechanics_say_what_they_do() {
        use crate::states::match_config::{RoguePoison, WarlockCurse};
        let tongues = TalentEffect::WidenChoice(ChoiceOption::WarlockCurse(WarlockCurse::Tongues));
        assert_eq!(
            effect_text(&tongues, 1, abilities()),
            "Unlocks Curse of Tongues as a curse choice"
        );
        let wound = TalentEffect::WidenChoice(ChoiceOption::RoguePoison(RoguePoison::Wound));
        assert_eq!(
            effect_text(&wound, 1, abilities()),
            "Unlocks Wound Poison as a weapon poison choice"
        );
        // Every menu reads with its own article: the last option that is
        // not its default, all nine named.
        let unlocks: Vec<String> = ChoiceMenu::ALL
            .iter()
            .map(|menu| {
                let option = menu
                    .options()
                    .into_iter()
                    .rev()
                    .find(|o| !o.is_default())
                    .expect("a menu has a non-default option");
                effect_text(&TalentEffect::WidenChoice(option), 1, abilities())
            })
            .collect();
        assert_eq!(
            unlocks,
            vec![
                "Unlocks Ambush as a stealth opener choice",
                "Unlocks Mind-numbing Poison as a weapon poison choice",
                "Unlocks Curse of Tongues as a curse choice",
                "Unlocks Sacrifice as a demon choice",
                "Unlocks Bird as a pet choice",
                "Unlocks Commanding Shout as a shout choice",
                "Unlocks Molten Armor as an armor choice",
                "Unlocks Concentration Aura as an aura choice",
                "Unlocks Shadowform as a form choice",
            ]
        );
        let slot = TalentEffect::AddChoiceSlot(ChoiceSlot::OffHandPoison);
        assert_eq!(
            effect_text(&slot, 1, abilities()),
            "Opens a second poison slot for the off hand"
        );
        // A share scales by the rank, as `at_rank` scales it in the engine.
        let link = TalentEffect::Mechanic(TalentMechanic::SoulLink { share: 0.1 });
        assert_eq!(
            effect_text(&link, 2, abilities()),
            "20% of the damage you take is dealt to your pet instead, while it lives"
        );
        let shield = TalentEffect::Mechanic(TalentMechanic::ReflectiveShield { share: 0.25 });
        assert_eq!(
            effect_text(&shield, 1, abilities()),
            "25% of the damage your absorb shields soak is dealt back to the attacker"
        );
        let cheat = TalentEffect::Mechanic(TalentMechanic::CheatDeath {
            health: 0.1,
            reduction: 0.9,
            duration: 3.0,
        });
        assert_eq!(
            effect_text(&cheat, 1, abilities()),
            "The first blow that would kill you leaves you at 10% health, \
             and you take 90% less damage for 3 sec"
        );
        let master = TalentEffect::Mechanic(TalentMechanic::MasterDemonologist { reduction: 0.04 });
        assert_eq!(
            effect_text(&master, 1, abilities()),
            "You and your pet take 4% less spell damage while it lives"
        );
        let sacrifice = TalentEffect::Mechanic(TalentMechanic::DemonicSacrifice {
            shadow_damage: 0.15,
        });
        assert_eq!(
            effect_text(&sacrifice, 1, abilities()),
            "Sacrificing your pet at the gates grants Touch of Shadow: +15% Shadow damage for \
             the match"
        );
    }

    /// A proc that rolls `chance` a rank on `trigger`, giving its owner
    /// 10 attack power for 6 sec.
    fn proc_on(trigger: TalentTrigger, chance: ProcChance, ranks_add: ProcRankScale) -> TalentProc {
        TalentProc {
            trigger,
            abilities: vec![],
            crit_only: false,
            chance,
            ranks_add,
            effect: ProcEffect::ApplyAura {
                on: ProcTarget::Owner,
                aura_type: crate::states::play_match::components::AuraType::AttackPowerIncrease,
                magnitude: 10.0,
                duration: 6.0,
                tick_interval: 0.0,
                dispel_type: crate::states::play_match::components::DispelType::Physical,
                name: None,
            },
            internal_cooldown: 0.0,
        }
    }

    /// A proc's ranks add to its CHANCE when it says so: each rank's line
    /// reads the chance the engine rolls at that rank, and the effect is the
    /// same at every rank.
    #[test]
    fn a_chance_scaled_proc_reads_its_chance_at_each_rank() {
        let mut nightfall = proc_on(
            TalentTrigger::DotTick,
            ProcChance::Chance(0.02),
            ProcRankScale::Chance,
        );
        nightfall.abilities = vec![AbilityType::Corruption, AbilityType::DrainLife];
        let effect = TalentEffect::Proc(nightfall);
        assert_eq!(
            effect_text(&effect, 1, abilities()),
            "A 2% chance on each damage tick of Corruption or Drain Life, \
             increases attack power by 10 for 6 sec (on you)"
        );
        assert_eq!(
            effect_text(&effect, 3, abilities()),
            "A 6% chance on each damage tick of Corruption or Drain Life, \
             increases attack power by 10 for 6 sec (on you)"
        );
        // A per-minute rate is a chance too: The Art of War's 4.8 a rank.
        let art = TalentEffect::Proc(proc_on(
            TalentTrigger::MeleeHit,
            ProcChance::PerMinute(4.8),
            ProcRankScale::Chance,
        ));
        assert_eq!(
            effect_text(&art, 2, abilities()),
            "9.6 times a minute of melee, whatever the weapon, on a melee hit, \
             increases attack power by 10 for 6 sec (on you)"
        );
    }

    /// A proc's ranks add to its EFFECT when it says so: the aura's
    /// magnitude, the heal-back's share and the next-cast modifier's numbers
    /// each read at the rank, and a certain proc reads "On".
    #[test]
    fn an_effect_scaled_proc_reads_its_effect_at_each_rank() {
        let mut vengeance = proc_on(
            TalentTrigger::MeleeHit,
            ProcChance::Chance(1.0),
            ProcRankScale::Effect,
        );
        vengeance.crit_only = true;
        assert_eq!(
            effect_text(&TalentEffect::Proc(vengeance), 3, abilities()),
            "On a melee critical strike, increases attack power by 30 for 6 sec (on you)"
        );

        let mut recovery = proc_on(
            TalentTrigger::CritTaken,
            ProcChance::Chance(1.0),
            ProcRankScale::Effect,
        );
        recovery.effect = ProcEffect::HealBack {
            fraction: 0.25,
            duration: 6.0,
            tick_interval: 2.0,
            name: None,
        };
        assert_eq!(
            effect_text(&TalentEffect::Proc(recovery), 2, abilities()),
            "On being struck by a melee or ranged critical hit, heals you for 50% of it \
             over 6 sec"
        );

        let mut initiative = proc_on(
            TalentTrigger::AbilityUsed,
            ProcChance::Chance(1.0),
            ProcRankScale::Effect,
        );
        initiative.effect = ProcEffect::NextCast {
            modifier: crate::states::play_match::next_cast::NextCastModifier {
                applies_to: crate::states::play_match::next_cast::NextCastScope::Any,
                instant: false,
                free: false,
                crit_chance: 0.0,
                cast_time_reduction: 0.0,
                cost_reduction: 5.0,
                power: 0.0,
            },
            duration: 10.0,
            dispel_type: crate::states::play_match::components::DispelType::Physical,
            name: None,
        };
        assert_eq!(
            effect_text(&TalentEffect::Proc(initiative), 3, abilities()),
            "On using any ability, primes your next cast: Within 10 sec, the next \
             ability costs 15 less"
        );
    }

    /// An aura on the event's other unit names who it lands on, and an
    /// internal cooldown is stated.
    #[test]
    fn a_proc_names_its_aura_target_and_its_cooldown() {
        let mut blackout = proc_on(
            TalentTrigger::SpellHit,
            ProcChance::Chance(0.02),
            ProcRankScale::Chance,
        );
        blackout.effect = ProcEffect::ApplyAura {
            on: ProcTarget::Other,
            aura_type: crate::states::play_match::components::AuraType::Stun,
            magnitude: 1.0,
            duration: 3.0,
            tick_interval: 0.0,
            dispel_type: crate::states::play_match::components::DispelType::Auto,
            name: None,
        };
        blackout.internal_cooldown = 8.0;
        assert_eq!(
            effect_text(&TalentEffect::Proc(blackout.clone()), 1, abilities()),
            "A 2% chance on a hit, stuns the target for 3 sec (on the target) \
             (at most once every 8 sec)"
        );
        blackout.trigger = TalentTrigger::CritTaken;
        assert!(effect_text(&TalentEffect::Proc(blackout), 1, abilities())
            .contains("(on the attacker)"));
    }

    /// A rank's whole line, proc included, as the panel shows it.
    #[test]
    fn a_proc_node_reads_every_rank() {
        let talent = TalentConfig {
            per_rank: vec![TalentEffect::Proc(proc_on(
                TalentTrigger::StealthBreak,
                ProcChance::Chance(1.0),
                ProcRankScale::Effect,
            ))],
            max_rank: 2,
            ..TalentDefinitions::shipped()
                .get(TalentId::DemonicEmbrace)
                .expect("shipped")
                .clone()
        };
        assert_eq!(
            rank_lines(&talent, abilities()),
            vec![
                "Rank 1: On leaving stealth, increases attack power by 10 for 6 sec (on you)."
                    .to_string(),
                "Rank 2: On leaving stealth, increases attack power by 20 for 6 sec (on you)."
                    .to_string(),
            ]
        );
    }

    /// The ability fields this card adds: a crit chance reads in percentage
    /// points, an aura's strength as a percentage of it, and an application
    /// chance (a fraction too) in points.
    #[test]
    fn crit_chance_strength_and_application_chance_read_in_their_units() {
        let conviction = TalentEffect::Patch {
            on: PatchTarget::Ability(AbilityType::MortalStrike),
            field: AbilityField::CritChance,
            op: PatchOp::Add(0.01),
        };
        assert_eq!(
            effect_text(&conviction, 3, abilities()),
            "+3% Mortal Strike critical strike chance"
        );
        let vile = TalentEffect::Patch {
            on: PatchTarget::Ability(AbilityType::Shadowbolt),
            field: AbilityField::AuraStrength,
            op: PatchOp::Mul(1.04),
        };
        assert_eq!(
            effect_text(&vile, 5, abilities()),
            "+20% Shadow Bolt effect strength"
        );
        let improved = TalentEffect::Patch {
            on: PatchTarget::Ability(AbilityType::Shadowbolt),
            field: AbilityField::ApplicationChance,
            op: PatchOp::Add(0.02),
        };
        assert_eq!(
            effect_text(&improved, 5, abilities()),
            "+10% Shadow Bolt chance to apply"
        );
    }

    #[test]
    fn a_from_rank_effect_reads_from_its_rank_on() {
        let talent = TalentConfig {
            per_rank: vec![
                TalentEffect::StatPercent(TalentStat::MaxHealth, 0.03),
                TalentEffect::FromRank(
                    2,
                    Box::new(TalentEffect::StatPercent(TalentStat::Armor, 0.05)),
                ),
            ],
            max_rank: 3,
            ..TalentDefinitions::shipped()
                .get(TalentId::DemonicEmbrace)
                .expect("shipped")
                .clone()
        };
        assert_eq!(rank_text(&talent, 1, abilities()), "+3% maximum health.");
        assert_eq!(
            rank_text(&talent, 2, abilities()),
            "+6% maximum health, +5% armor."
        );
        assert_eq!(
            rank_text(&talent, 3, abilities()),
            "+9% maximum health, +10% armor."
        );
        let late = &talent.per_rank[1];
        assert_eq!(effect_text(late, 1, abilities()), "From rank 2: +5% armor");
        assert_eq!(effect_text(late, 3, abilities()), "+10% armor");
    }

    #[test]
    fn every_shipped_talent_reads_every_rank() {
        let talents = TalentDefinitions::shipped();
        for (id, talent) in talents.iter() {
            let lines = rank_lines(talent, abilities());
            assert_eq!(lines.len(), usize::from(talent.max_rank), "{id:?}");
            for line in lines {
                assert!(line.ends_with('.') && line.len() > 4, "{id:?}: {line:?}");
            }
            assert!(!placement(talents, talent).is_empty());
        }
    }

    #[test]
    fn a_gated_node_names_its_gate_and_a_tier_one_node_needs_nothing() {
        let talents = TalentDefinitions::shipped();
        let embrace = talents.get(TalentId::DemonicEmbrace).expect("shipped");
        assert!(requirement_lines(talents, embrace).is_empty());
        let unholy = talents.get(TalentId::UnholyPower).expect("shipped");
        assert_eq!(
            requirement_lines(talents, unholy),
            vec!["Requires 2 points in Demonology".to_string()]
        );
    }
}
