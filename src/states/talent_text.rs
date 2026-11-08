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

use crate::states::match_config::CharacterClass;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::talent_choices::{
    ranked_effects, ChoiceMenu, ChoiceOption, ChoiceSlot,
};
use crate::states::play_match::talent_mechanics::TalentMechanic;
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
        ChoiceOption::HunterPet(p) => p.name().to_string(),
        ChoiceOption::WarriorShout(s) => s.name().to_string(),
        ChoiceOption::MageArmor(a) => a.name().to_string(),
        ChoiceOption::PaladinAura(a) => a.name().to_string(),
    }
}

/// A per-match menu in a sentence, with its article ("a curse", "an aura"):
/// each menu carries its own, so no rule guesses one from the spelling.
fn menu_phrase(menu: ChoiceMenu) -> &'static str {
    match menu {
        ChoiceMenu::RogueOpener => "a stealth opener",
        ChoiceMenu::RoguePoison => "a weapon poison",
        ChoiceMenu::WarlockCurse => "a curse",
        ChoiceMenu::HunterPet => "a pet",
        ChoiceMenu::WarriorShout => "a shout",
        ChoiceMenu::MageArmor => "an armor",
        ChoiceMenu::PaladinAura => "an aura",
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
        AbilityField::AuraMagnitude => "effect strength",
        AbilityField::AuraMagnitudeCoefficient => "effect strength from spell power",
        AbilityField::AuraBreakOnDamage => "damage to break",
        AbilityField::AuraTickInterval => "time between ticks",
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
        | AbilityField::AuraBreakOnDamage => "",
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
        // not its default, all seven named.
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
                "Unlocks Bird as a pet choice",
                "Unlocks Commanding Shout as a shout choice",
                "Unlocks Molten Armor as an armor choice",
                "Unlocks Concentration Aura as an aura choice",
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
