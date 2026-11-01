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
    }
}

/// What `rank` ranks of `talent` do, every effect joined: "+6% maximum
/// health". Rank 0 reads as rank 1, what the first point buys.
pub fn rank_text(talent: &TalentConfig, rank: u8, abilities: &AbilityDefinitions) -> String {
    let rank = rank.max(1);
    let clauses: Vec<String> = talent
        .per_rank
        .iter()
        .map(|effect| effect_text(effect, rank, abilities))
        .collect();
    format!("{}.", clauses.join(", "))
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

/// A signed number with its unit: "+3%", "-0.25 sec", "+1.5%".
fn signed(value: f32, unit: &str) -> String {
    let sign = if value < 0.0 { "\u{2212}" } else { "+" };
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
    format!("{sign}{number}{gap}{unit}")
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
