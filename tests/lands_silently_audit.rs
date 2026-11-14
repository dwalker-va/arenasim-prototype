//! Lands-silently audits: every ability in `abilities.ron` that lands
//! something — damage, a DoT, a crowd-control effect, a buff or debuff, a
//! dispel, an interrupt, a heal, health into mana — must put SOME visual in
//! the scene when it does, or be named on a known-silent list with the card
//! that will fix it.
//!
//! # Why this exists
//!
//! Immolate's 15-second burn rendered nothing, and every test was green. The
//! DoT visuals were routed by exact string match with a silent fallthrough,
//! and only heals and projectile landings had a sweep that failed when a
//! member landed with no visual. This file is the sweep for every family.
//!
//! # The shape of each sweep — two claims, two tests
//!
//! Each family SCANS the config for its subjects, so each has two claims,
//! written as separate tests because they fail for different reasons and the
//! second is worth nothing without the first:
//!
//! 1. **`<family>_finds_every_member`** — the scan finds exactly the NAMED
//!    members (set equality, never a count or a floor). A new ability joining
//!    the family must be added by name; a predicate that breaks and finds
//!    nothing fails instead of passing vacuously.
//! 2. **`<family>_lands_nothing_silently`** — the family's judge finds a visual
//!    for every member except exactly the `KNOWN_SILENT` ones. Set equality in
//!    BOTH directions: a new silent member fails, AND a known-silent member
//!    that has since gained a visual fails — so the list can only shrink and
//!    can never go stale. Each entry names the card that clears it — or, for
//!    a member the design rules silent, is a permanent [`SilentByDesign`]
//!    entry citing the design doc that ruled it (which must exist).
//!
//! Every list a family keeps — its members, its known-silent and by-design
//! entries — is in name order, one row per member (`check_sorted` fails it
//! otherwise), so two changes that each add a member add it in different
//! places instead of both appending to the end, where they would conflict.
//!
//! On top of the families, `every_ability_belongs_to_a_family` makes the
//! families a cover of the whole config: an ability that no family scans is
//! a failure unless it is named in `NO_FAMILY` with the visual that covers it.
//! Two families are not an ability's: an item proc's EXTRA SWING (Hand of
//! Justice) lands an attack too, so `extra_swing_*` scans `items.ron` for it;
//! and a talent's `Mechanic` lands at its own site (Soul Link's transfer, the
//! Demonic Sacrifice), so `talent_mechanic_*` scans `talents.ron`; and a
//! talent's `Proc` lands its aura from the proc resolver, so `talent_proc_*`
//! scans `talents.ron` for those.
//! The harness's own failure modes are proved by planted offenders at the
//! bottom of the file.
//!
//! # What a judge accepts as "a visual"
//!
//! Something in world space that the ability's landing routes to — through a
//! router that names the ability or its aura type (`SchoolImpact::anchor_for`,
//! `hunter_shot_for`, `bolt_kind_for`, `InstantAbilityFired::is_spawned_for`,
//! `DotStateVisual::for_dot`, `HealImpact::kind_for`, the curse table), or an
//! explicitly named bespoke branch. GENERIC visuals count (a stock
//! school-impact row is a visual); a channel every ability shares regardless
//! of which one was used (the auto-attack swing, floating combat text, the
//! team-frame icon, the casting-orb sputter) does not, because it cannot tell
//! the player the ability landed. Heroic Strike was the example until its
//! empowered swing was marked (`HeroicStrikeSwing`) and dressed: its bonus
//! rides the ordinary swing, which alone would change nothing on screen.

use std::collections::BTreeSet;

use bevy::prelude::Entity;

// One `use` per item, never a braced group: rustfmt keeps separate `use`s
// separate and sorts them, so an import a change adds is one new line at its
// own place, where a braced group reflows and two changes that each add an
// import conflict in it.
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::AbilityConfig;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::bolt_kind_for;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::client_landing_style;
use arenasim::states::play_match::combat_core::ExtraSwingSource;
use arenasim::states::play_match::components::Aura;
use arenasim::states::play_match::components::AuraApplyRoute;
use arenasim::states::play_match::components::AuraLandingKind;
use arenasim::states::play_match::components::AuraType;
use arenasim::states::play_match::components::CurseKind;
use arenasim::states::play_match::components::HealImpact;
use arenasim::states::play_match::components::HealImpactKind;
use arenasim::states::play_match::components::HotVisual;
use arenasim::states::play_match::components::InstantAbilityFired;
use arenasim::states::play_match::components::LifeTapGlow;
use arenasim::states::play_match::components::PaladinOneShotKind;
use arenasim::states::play_match::components::RootStyle;
use arenasim::states::play_match::components::SchoolImpact;
use arenasim::states::play_match::components::ShieldKind;
use arenasim::states::play_match::components::TotemElement;
use arenasim::states::play_match::curse_spec;
use arenasim::states::play_match::equipment::load_item_definitions;
use arenasim::states::play_match::equipment::ItemDefinitions;
use arenasim::states::play_match::equipment::ItemId;
use arenasim::states::play_match::hunter_shot_for;
use arenasim::states::play_match::is_live_shield;
use arenasim::states::play_match::lands_poison_cloud;
use arenasim::states::play_match::on_use_items::OnUseEffect;
use arenasim::states::play_match::oneshot_visual;
use arenasim::states::play_match::proc_trinkets::ProcEffect;
use arenasim::states::play_match::root_style_for;
use arenasim::states::play_match::talent_mechanics::MechanicKind;
use arenasim::states::play_match::talent_procs::effect_aura;
use arenasim::states::play_match::talent_procs::ProcEvent;
use arenasim::states::play_match::talent_procs::TalentProc;
use arenasim::states::play_match::talent_procs::TalentProcSlot;
use arenasim::states::play_match::talents::TalentDefinitions;
use arenasim::states::play_match::talents::TalentEffect;
use arenasim::states::play_match::talents::TalentId;
use arenasim::states::play_match::wears_guardian_spirit;
use arenasim::states::play_match::wears_shadowform_look;
use arenasim::states::play_match::DotStateVisual;
use arenasim::states::play_match::InterruptLanding;
use arenasim::states::play_match::SlowTint;
use arenasim::states::play_match::DIVINE_SHIELD_AURA_NAME;
use arenasim::states::play_match::ICE_BARRIER_AURA_NAME;
use arenasim::states::play_match::PW_SHIELD_AURA_NAME;

use AbilityType::*;

// ── the harness ─────────────────────────────────────────────────────────────

/// What every weapon poison lands with (`lands_poison_cloud`): one shared
/// cloud, replayed on each application. It is the whole of a poison's visual
/// in the direct-damage, DoT and aura-application families.
const POISON_CLOUD: &str = "weapon poison cloud on every application (poison_landing.rs)";

/// A judge: the visual an ability's landing reaches in this family, or `None`
/// if it lands silently.
type Judge = fn(AbilityType, &AbilityConfig) -> Option<&'static str>;

/// Every list a family keeps is in name order, one row per member, so two
/// changes that each add a member add it in different places rather than
/// both at the end of the list, where they would conflict.
fn check_sorted<T: std::fmt::Debug>(
    family: &str,
    list: &str,
    keys: impl IntoIterator<Item = T>,
) -> Result<(), String> {
    let names: Vec<String> = keys.into_iter().map(|key| format!("{key:?}")).collect();
    let mut sorted = names.clone();
    sorted.sort();
    if names == sorted {
        Ok(())
    } else {
        Err(format!(
            "the {family} {list} is not in name order — keep it sorted, one row per member:\n  \
             {}",
            sorted.join(", ")
        ))
    }
}

/// Claim 1: the scan found exactly the named members.
fn check_finds<T: Ord + Copy + std::fmt::Debug>(
    family: &str,
    scanned: &BTreeSet<T>,
    named: &[T],
) -> Result<(), String> {
    check_sorted(family, "member list", named.iter())?;
    let named: BTreeSet<T> = named.iter().copied().collect();
    let mut problems = Vec::new();
    for lost in named.difference(scanned) {
        problems.push(format!(
            "the {family} scan no longer finds {lost:?} — the scan predicate broke, or \
             {lost:?} left the family (then remove it from the {family} member list)"
        ));
    }
    for new in scanned.difference(&named) {
        problems.push(format!(
            "{new:?} is now a {family} member — add it to the {family} member list \
             (and check the {family} judge draws it)"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Claim 2: the judge draws every member except exactly the known-silent
/// ones.
fn check_judged<T: Ord + Copy + std::fmt::Debug>(
    family: &str,
    members: &BTreeSet<T>,
    judge: impl Fn(T) -> Option<&'static str>,
    known_silent: &[(T, &str)],
) -> Result<(), String> {
    check_sorted(
        family,
        "known-silent list",
        known_silent.iter().map(|(member, _)| member),
    )?;
    let mut problems = Vec::new();
    let mut listed = BTreeSet::new();
    for (ability, card) in known_silent {
        if !listed.insert(*ability) {
            problems.push(format!(
                "{ability:?} is on the {family} known-silent list twice — remove one"
            ));
        }
        if !members.contains(ability) {
            problems.push(format!(
                "{ability:?} is on the {family} known-silent list ({card}) but is not a \
                 {family} member — remove it from that list"
            ));
        }
    }
    for ability in members {
        match (judge(*ability), listed.contains(ability)) {
            (None, false) => problems.push(format!(
                "{ability:?} lands with no {family} visual — give it one, or add \
                 ({ability:?}, \"<the card that will fix it>\") to the {family} \
                 known-silent list"
            )),
            (Some(visual), true) => problems.push(format!(
                "{ability:?} now draws `{visual}` — remove it from the {family} \
                 known-silent list"
            )),
            _ => {}
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// A member that lands with no visual in its family BY RULING, not pending a
/// card: the client gives it none and a design doc rules it stays that way.
/// Permanent where a known-silent entry is temporary, so it names the doc that
/// ruled it instead of a card — and the doc must exist.
struct SilentByDesign<T = AbilityType> {
    ability: T,
    /// Repo-relative path of the design doc that rules it silent.
    doc: &'static str,
    reason: &'static str,
}

/// A by-design entry must cite a design doc that exists, and give a reason.
fn check_rulings<T: std::fmt::Debug>(
    family: &str,
    rulings: &[SilentByDesign<T>],
) -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut problems = Vec::new();
    for r in rulings {
        if !r.doc.starts_with("docs/") || !root.join(r.doc).is_file() {
            problems.push(format!(
                "{:?} is silent by design in {family}, but its ruling `{}` is not a design \
                 doc in docs/ — cite the doc that rules it, or list it as known-silent with \
                 the card that will draw it",
                r.ability, r.doc
            ));
        }
        if r.reason.trim().is_empty() {
            problems.push(format!(
                "{:?} is silent by design in {family} with no reason",
                r.ability
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn scan(pred: impl Fn(AbilityType, &AbilityConfig) -> bool) -> BTreeSet<AbilityType> {
    AbilityDefinitions::default()
        .iter()
        .filter(|(a, c)| pred(**a, c))
        .map(|(a, _)| *a)
        .collect()
}

/// Run a family's two checks against the real config.
fn assert_finds(
    family: &str,
    pred: fn(AbilityType, &AbilityConfig) -> bool,
    named: &[AbilityType],
) {
    if let Err(e) = check_finds(family, &scan(pred), named) {
        panic!("{e}");
    }
}

fn assert_judged(
    family: &str,
    pred: fn(AbilityType, &AbilityConfig) -> bool,
    judge: Judge,
    known_silent: &[(AbilityType, &str)],
) {
    let defs = AbilityDefinitions::default();
    let members = scan(pred);
    let judged = check_judged(
        family,
        &members,
        |a| judge(a, defs.get(&a).expect("member is in the config")),
        known_silent,
    );
    if let Err(e) = judged {
        panic!("{e}");
    }
}

/// [`assert_judged`] for a family with members silent by ruling: they count
/// as listed (so one that gains a visual fails the same way), and each must
/// cite its doc.
fn assert_judged_with_rulings(
    family: &str,
    pred: fn(AbilityType, &AbilityConfig) -> bool,
    judge: Judge,
    known_silent: &[(AbilityType, &str)],
    rulings: &[SilentByDesign],
) {
    if let Err(e) = check_rulings(family, rulings)
        .and(check_sorted(
            family,
            "known-silent list",
            known_silent.iter().map(|(member, _)| member),
        ))
        .and(check_sorted(
            family,
            "silent-by-design list",
            rulings.iter().map(|r| r.ability),
        ))
    {
        panic!("{e}");
    }
    let mut listed: Vec<(AbilityType, &str)> = known_silent.to_vec();
    listed.extend(rulings.iter().map(|r| (r.ability, r.doc)));
    listed.sort_by_key(|(member, _)| format!("{member:?}"));
    assert_judged(family, pred, judge, &listed);
}

// ── aura classification ─────────────────────────────────────────────────────

/// Which sweep an aura type belongs to. Exhaustive with no wildcard, on
/// purpose: a new `AuraType` does not compile until someone decides which
/// family judges it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AuraFamily {
    /// Damage over time — judged by the DoT router.
    Dot,
    /// Takes control away (stuns, roots, fears, incapacitates, silences,
    /// slows) — judged by the crowd-control sweep.
    Control,
    /// Every other buff or debuff — judged by the aura-application sweep.
    Status,
}

fn aura_family(t: AuraType) -> AuraFamily {
    match t {
        AuraType::DamageOverTime => AuraFamily::Dot,
        AuraType::Stun
        | AuraType::Root
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Cyclone
        | AuraType::Silence
        | AuraType::SpellSchoolLockout
        | AuraType::MovementSpeedSlow
        | AuraType::AttackSpeedSlow => AuraFamily::Control,
        AuraType::MaxHealthIncrease
        | AuraType::HealingReduction
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::SpellResistanceBuff
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::FearImmunity
        | AuraType::ArmorIncrease
        // No ability grants haste yet: the Dragonspine Trophy's proc does,
        // and its band cue draws it until AS-433 gives it its own one-shot.
        | AuraType::AttackSpeedIncrease
        | AuraType::NextCast
        // The damage-pipeline modifiers (AS-408): stat-shaped buffs and
        // debuffs, judged by the aura-application sweep (the band cue draws
        // each until its user's visual card gives it its own).
        | AuraType::DamageTakenIncrease
        | AuraType::DamageDealtIncrease
        | AuraType::ArmorPercentIncrease
        | AuraType::ArmorPercentReduction
        | AuraType::DamageTransfer
        | AuraType::KillingBlowSave
        // Guardian Spirit's face (AS-415).
        | AuraType::HealingReceivedIncrease
        | AuraType::DamageReflect
        // Sacred Shield's arming buff (AS-421): a buff like any other.
        | AuraType::TriggeredAbsorb
        // Aura Mastery's immunity (AS-420): a buff like any other.
        | AuraType::SilenceInterruptImmunity
        // Shadow Dance's window (AS-419): a buff like any other.
        | AuraType::ShadowDance
        // Vampiric Embrace's leech (AS-416): a debuff like any other.
        | AuraType::LeechMark
        | AuraType::TravelForm => AuraFamily::Status,
    }
}

/// Every aura an ability grants: its RON `applies_aura`, plus — for a totem,
/// whose buff is pulsed by code rather than declared in the RON — the aura
/// its `totem_spec` pulses (the same data gameplay reads).
fn granted_auras(ability: AbilityType, config: &AbilityConfig) -> Vec<AuraType> {
    let mut auras: Vec<AuraType> = config.applies_aura.iter().map(|a| a.aura_type).collect();
    for element in TotemElement::ALL {
        let (totem_ability, aura, _, _) = totem_spec(element);
        if totem_ability == ability {
            auras.push(aura);
        }
    }
    auras
}

fn grants(ability: AbilityType, config: &AbilityConfig, family: AuraFamily) -> bool {
    granted_auras(ability, config)
        .into_iter()
        .any(|t| aura_family(t) == family)
}

// ── family: heals ───────────────────────────────────────────────────────────

fn is_heal(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_heal()
}

const HEAL_MEMBERS: &[AbilityType] = &[
    FlashHeal,
    FlashOfLight,
    HolyLight,
    HolyNova,
    HolyShock,
    LesserHealingWave,
    Swiftmend,
];

/// Scoped by AS-335 (the Priest talents' visuals).
const HEAL_KNOWN_SILENT: &[(AbilityType, &str)] = &[(
    HolyNova,
    "AS-355 (Holy Nova, the nova on the field; scoped by AS-335)",
)];

fn heal_landing(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    HealImpact::kind_for(a).map(|_| "heal landing (heal_impact.rs)")
}

#[test]
fn heal_finds_every_member() {
    assert_finds("heal", is_heal, HEAL_MEMBERS);
}

#[test]
fn heal_lands_nothing_silently() {
    assert_judged("heal", is_heal, heal_landing, HEAL_KNOWN_SILENT);
}

// ── family: direct damage ───────────────────────────────────────────────────

/// Abilities whose damage is resolved in class AI rather than declared in the
/// config's damage fields, so no config predicate can see it.
const CODE_RESOLVED_DAMAGE: &[AbilityType] = &[HeroicStrike];

/// A damage shield: an aura whose damage lands on each attacker that hits its
/// bearer, from the aura itself (Retribution Aura's `DamageReflect`). No
/// damage field declares it, so the config predicate below asks the aura.
fn is_damage_shield(c: &AbilityConfig) -> bool {
    c.applies_aura
        .iter()
        .any(|a| a.aura_type == AuraType::DamageReflect)
}

fn is_direct_damage(a: AbilityType, c: &AbilityConfig) -> bool {
    c.is_damage()
        || c.mana_burn_amount > 0.0
        || CODE_RESOLVED_DAMAGE.contains(&a)
        || is_damage_shield(c)
}

const DIRECT_DAMAGE_MEMBERS: &[AbilityType] = &[
    AimedShot,
    Ambush,
    ArcaneShot,
    Conflagrate,
    CrusaderStrike,
    DeathCoil,
    DrainLife,
    FrostNova,
    FrostShock,
    Frostbolt,
    Hemorrhage,
    HeroicStrike,
    HolyNova,
    HolyShock,
    Immolate,
    LightningBolt,
    ManaBurn,
    MindBlast,
    MindFlay,
    Moonfire,
    MortalStrike,
    RetributionAura,
    Shadowbolt,
    Shadowburn,
    SinisterStrike,
    WoundPoison,
];

fn damage_landing(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    if bolt_kind_for(a).is_some() {
        return Some("bespoke bolt impact (spell_bolts.rs)");
    }
    // The client's Hunter shot landing, played through the shared impact's
    // `landing_style` override — so it needs the anchor too.
    if hunter_shot_for(a).is_some() && SchoolImpact::anchor_for(a).is_some() {
        return Some("client Hunter shot landing (hunter_shots.rs)");
    }
    if SchoolImpact::anchor_for(a).is_some() {
        return Some("school impact (school_impact.rs)");
    }
    if InstantAbilityFired::is_spawned_for(a) {
        return Some("instant-ability stroke (instant_ability.rs)");
    }
    // A poison's hit rides the weapon swing that procced it; what lands is
    // the poison cloud, on every application.
    if lands_poison_cloud(a) {
        return Some(POISON_CLOUD);
    }
    // Bespoke landings spawned by an `ability ==` branch at the resolution
    // site. A new ability cannot reach these arms without being named here.
    match a {
        DeathCoil => Some("DeathCoilBurst (projectiles.rs → death_coil.rs)"),
        LightningBolt => Some("forked-arc strike (casting.rs → lightning_bolt.rs)"),
        Immolate => Some("landing flame burst (casting.rs → flame.rs)"),
        DrainLife => Some("drain beam (drain_life.rs, keyed on the channel's ability)"),
        Moonfire => Some("moon orb + beam landing (casting.rs → moonfire.rs)"),
        // The empowered swing carries `HeroicStrikeSwing` (auto_attack.rs);
        // heroic_strike.rs lays its trail and lands it through the client
        // landing router, so the router must answer for it.
        HeroicStrike if client_landing_style(a).is_some() => {
            Some("trail + decisivestrike landing (HeroicStrikeSwing → heroic_strike.rs)")
        }
        _ => None,
    }
}

const DIRECT_DAMAGE_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    (
        Conflagrate,
        "AS-348 (Shadowburn and Conflagrate, the Destruction casts; AS-334's talent visuals)",
    ),
    (
        CrusaderStrike,
        "AS-379 (Crusader Strike, the Retribution capstone's strike)",
    ),
    (
        Hemorrhage,
        "AS-336 (Hemorrhage, the Subtlety strike and its wound; AS-367 builds it)",
    ),
    (
        HolyNova,
        "AS-355 (Holy Nova, the nova on the field; scoped by AS-335)",
    ),
    (
        MindFlay,
        "AS-356 (Mind Flay, the channel and its slow; scoped by AS-335)",
    ),
    (
        Shadowburn,
        "AS-348 (Shadowburn and Conflagrate, the Destruction casts; AS-334's talent visuals)",
    ),
];

#[test]
fn direct_damage_finds_every_member() {
    assert_finds("direct-damage", is_direct_damage, DIRECT_DAMAGE_MEMBERS);
}

/// Direct damage that lands with no visual by ruling.
const DIRECT_DAMAGE_SILENT_BY_DESIGN: &[SilentByDesign] = &[SilentByDesign {
    ability: RetributionAura,
    doc: "docs/design/2026-10-18-paladin-talent-client-data.md",
    reason: "the damage shield's hit is dealt by the aura itself, with no spell of its own, \
             so the client draws only combat text (§8, finding 2); the user ruled it silent \
             (Rulings, 2026-10-18) — the rune disc as the aura goes up is its whole visual",
}];

#[test]
fn direct_damage_lands_nothing_silently() {
    assert_judged_with_rulings(
        "direct-damage",
        is_direct_damage,
        damage_landing,
        DIRECT_DAMAGE_KNOWN_SILENT,
        DIRECT_DAMAGE_SILENT_BY_DESIGN,
    );
}

/// Moonfire lands through its own orb-and-beam landing, never the shared
/// school impact as well — the two would play over each other.
#[test]
fn moonfire_lands_through_its_own_landing_only() {
    assert_eq!(SchoolImpact::anchor_for(Moonfire), None);
    assert!(bolt_kind_for(Moonfire).is_none());
}

/// The four Hunter shots land through the client's landing, not their school
/// rows — including the two the direct-damage sweep does not reach (Serpent
/// Sting is judged as a DoT and Concussive Shot as control, on their STATES).
/// A pin on the router, so one dropping back to a school row fails here.
#[test]
fn the_hunter_shots_land_through_the_client_landing() {
    for a in [AimedShot, ArcaneShot, ConcussiveShot, SerpentSting] {
        assert!(
            hunter_shot_for(a).is_some() && SchoolImpact::anchor_for(a).is_some(),
            "{a:?} must reach the client Hunter shot landing"
        );
    }
}

// ── family: damage over time ────────────────────────────────────────────────

fn is_dot(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Dot)
}

const DOT_MEMBERS: &[AbilityType] = &[
    Corruption,
    CurseOfAgony,
    DeadlyPoison,
    Immolate,
    Moonfire,
    Rend,
    Rupture,
    SerpentSting,
    ShadowWordPain,
    SiphonLife,
    UnstableAffliction,
];

/// DoTs that land with nothing drawn yet.
const DOT_KNOWN_SILENT: &[(AbilityType, &str)] = &[(
    SiphonLife,
    "AS-347 (Siphon Life, the Affliction drain; its heal-back is icon-only by ruling)",
)];

/// The DoT STATE a member's aura routes to. The aura carries the ability's
/// RON `name:`, so this sweeps the config name through the very router every
/// DoT renderer asks.
///
/// Deadly Poison has no state — the client draws none for any Rogue poison,
/// and nothing per tick — so its visual is the poison cloud that lands with
/// every application. It must not ALSO route to a state.
fn dot_state(a: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    if lands_poison_cloud(a) {
        assert_eq!(DotStateVisual::for_dot(&c.name), None, "{a:?}");
        return Some(POISON_CLOUD);
    }
    DotStateVisual::for_dot(&c.name).map(|v| match v {
        DotStateVisual::Drip(_) => "affliction drips (affliction.rs)",
        DotStateVisual::CorruptionShroud => "Corruption shroud (warlock_dots.rs)",
        DotStateVisual::UnstableAfflictionState => "UA glow + crackle (warlock_dots.rs)",
        DotStateVisual::ImmolateBurn => "Immolate burn (immolate.rs)",
        DotStateVisual::ApplyApparitionOnly(_) => "curse apply apparition (warlock_dots.rs)",
    })
}

#[test]
fn dot_finds_every_member() {
    assert_finds("DoT", is_dot, DOT_MEMBERS);
}

/// DoTs that draw no state by ruling. The landing is the whole visual and the
/// aura icon carries the DoT, as with Curse of Tongues.
const DOT_SILENT_BY_DESIGN: &[SilentByDesign] = &[
    SilentByDesign {
        ability: Moonfire,
        doc: "docs/design/2026-10-03-druid-client-data.md",
        reason: "client has no state; aura icon carries it (ruling 2; the Curse of Tongues \
                 precedent) — the orb-and-beam landing is the whole visual",
    },
    SilentByDesign {
        ability: ShadowWordPain,
        doc: "docs/design/2026-10-17-baseline-kit-client-data.md",
        reason: "client has no state and no tick visual (§3); aura icon carries it, by the \
                 Moonfire ruling — the shadowwordpain_chest landing is the whole visual",
    },
];

/// Shadow Word: Pain's landing is its whole visual, so it must be routed: the
/// aura its RON name lands marks the hit (`apply_pending_auras` →
/// `shadow_word_pain.rs`).
#[test]
fn shadow_word_pain_lands_its_hit() {
    let defs = AbilityDefinitions::default();
    let name = &defs.get(&ShadowWordPain).unwrap().name;
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::DamageOverTime, name),
        Some(AuraLandingKind::ShadowWordPainHit)
    );
}

#[test]
fn dot_lands_nothing_silently() {
    assert_judged_with_rulings(
        "DoT",
        is_dot,
        dot_state,
        DOT_KNOWN_SILENT,
        DOT_SILENT_BY_DESIGN,
    );
}

/// The router names each state it draws — a pin on the judge, so a wrong
/// route (Immolate drawn as a bleed, say) fails even though it is not silent.
#[test]
fn the_dot_router_names_each_members_state() {
    use arenasim::states::play_match::components::DripKind;
    let defs = AbilityDefinitions::default();
    let route = |a: AbilityType| DotStateVisual::for_dot(&defs.get(&a).unwrap().name);
    assert_eq!(route(Rend), Some(DotStateVisual::Drip(DripKind::Bleed)));
    // Rupture has no client state; it bleeds as Rend does (AS-336, AS-365).
    assert_eq!(route(Rupture), Some(DotStateVisual::Drip(DripKind::Bleed)));
    assert_eq!(
        route(SerpentSting),
        Some(DotStateVisual::Drip(DripKind::Poison))
    );
    assert_eq!(route(Corruption), Some(DotStateVisual::CorruptionShroud));
    assert_eq!(
        route(UnstableAffliction),
        Some(DotStateVisual::UnstableAfflictionState)
    );
    assert_eq!(route(Immolate), Some(DotStateVisual::ImmolateBurn));
    assert_eq!(
        route(CurseOfAgony),
        Some(DotStateVisual::ApplyApparitionOnly(CurseKind::Agony))
    );
    // A DoT nothing draws routes nowhere — the fallthrough is `None`, which
    // the sweep above fails on, never a default visual.
    assert_eq!(DotStateVisual::for_dot("Shadow Word: Pain"), None);
    // The curse table only answers for DoT curses: Weakness is not a DoT.
    assert_eq!(
        DotStateVisual::for_dot(curse_spec(CurseKind::Weakness).aura_name),
        None
    );
}

// ── family: crowd control ───────────────────────────────────────────────────

fn is_control(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Control)
}

const CONTROL_MEMBERS: &[AbilityType] = &[
    BoarCharge,
    CheapShot,
    ConcussiveShot,
    CripplingPoison,
    CurseOfExhaustion,
    Cyclone,
    DeathCoil,
    EntanglingRoots,
    Fear,
    FreezingTrap,
    FrostNova,
    FrostShock,
    Frostbolt,
    HammerOfJustice,
    KidneyShot,
    MindFlay,
    Polymorph,
    PsychicScream,
    Repentance,
    Silence,
    SpiderWeb,
];

/// The victim treatment of a member's control aura. Keyed on the aura TYPE
/// (the renderers poll `ActiveAuras` for it), except Incapacitate, whose ice
/// block is spawned by the trap trigger rather than by the aura, a slow,
/// whose ring is tinted through `SlowTint::for_slow` by the aura's RON
/// `name:` — so a slow with no tint arm lands silently — and a root, whose
/// restraint `root_style_for` picks from its name and school.
fn control_treatment(a: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    let mut treatment = None;
    for t in granted_auras(a, c) {
        if aura_family(t) != AuraFamily::Control {
            continue;
        }
        // Every control aura must be drawn for the member to pass.
        treatment = Some(control_visual(
            t,
            &c.name,
            Some(c.spell_school),
            a == FreezingTrap,
        )?);
    }
    treatment
}

/// The victim treatment one control aura draws: its type, its name (`name`:
/// the slow tint and the root style route by it), its school where it has one
/// (the root style), and whether Freezing Trap's trigger spawned it (the only
/// incapacitate drawn as the ice block). Shared by the crowd-control family
/// and the talent-proc family.
fn control_visual(
    t: AuraType,
    name: &str,
    school: Option<SpellSchool>,
    freezing_trap: bool,
) -> Option<&'static str> {
    match t {
        AuraType::Stun => Some("stun whirl (hard_cc.rs)"),
        AuraType::Root => Some(match root_style_for(name, school) {
            RootStyle::Roots => "bark roots (druid_control.rs)",
            RootStyle::Ice | RootStyle::Web => "root restraint (hard_cc.rs)",
        }),
        AuraType::Cyclone => Some("cyclone funnel + lifted victim (druid_control.rs)"),
        AuraType::Fear => Some("fear shroud (fear.rs)"),
        AuraType::Polymorph => Some("sheep swap (polymorph.rs)"),
        AuraType::Incapacitate if freezing_trap => Some("ice block (traps.rs)"),
        AuraType::MovementSpeedSlow => {
            SlowTint::for_slow(name).map(|_| "slow bind ring + scuff (slow_ring.rs)")
        }
        _ => None,
    }
}

#[test]
fn control_finds_every_member() {
    assert_finds("crowd-control", is_control, CONTROL_MEMBERS);
}

/// Repentance is an `Incapacitate`, which draws the ice block only for the
/// Freezing Trap that spawns it: it lands silently until AS-378 gives it its
/// own visual.
const CONTROL_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    (
        CurseOfExhaustion,
        "AS-334 (the talent visuals: Curse of Exhaustion's slow)",
    ),
    (
        MindFlay,
        "AS-356 (Mind Flay, the channel and its slow; scoped by AS-335)",
    ),
    (
        Repentance,
        "AS-378 (Repentance, the Retribution incapacitate; never Freezing Trap's ice block)",
    ),
    (
        Silence,
        "AS-357 (Silence on the target, and the Silence aura's routing; scoped by AS-335)",
    ),
];

#[test]
fn control_lands_nothing_silently() {
    assert_judged(
        "crowd-control",
        is_control,
        control_treatment,
        CONTROL_KNOWN_SILENT,
    );
}

// ── family: buff / debuff application ───────────────────────────────────────

fn is_status(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Status)
}

const STATUS_MEMBERS: &[AbilityType] = &[
    AimedShot,
    AirTotem,
    AmplifyCurse,
    ArcaneIntellect,
    AuraMastery,
    BattleShout,
    BerserkerRage,
    BlessingOfSacrifice,
    CommandingShout,
    ConcentrationAura,
    CurseOfTongues,
    CurseOfWeakness,
    DemoralizingShout,
    DevotionAura,
    DivineFavor,
    DivineProtection,
    DivineShield,
    EarthTotem,
    ExposeArmor,
    FireTotem,
    FrostArmor,
    GuardianSpirit,
    Hemorrhage,
    IceBarrier,
    InnerFocus,
    Innervate,
    Lifebloom,
    MageArmorSpell,
    MarkOfTheWild,
    MindNumbingPoison,
    MoltenArmor,
    MortalStrike,
    PainSuppression,
    PowerInfusion,
    PowerWordFortitude,
    PowerWordShield,
    Rejuvenation,
    RetributionAura,
    SacredShield,
    ShadowDance,
    ShadowResistanceAura,
    Shadowform,
    TravelForm,
    VampiricEmbrace,
    WaterTotem,
    WoundPoison,
];

/// The family-wide aura-application cue — what draws a status aura that no
/// bespoke effect owns. Nothing does today, which is the AS-134 gap. When
/// AS-134's `AuraApplyRoute` lands, this body becomes
/// `(AuraApplyRoute::for_aura(t) == AuraApplyRoute::Band).then_some("…")` and the AS-134
/// entries below come off the known-silent list in the same diff (the sweep
/// fails until they do).
fn family_application_cue(_t: AuraType) -> Option<&'static str> {
    None
}

fn status_visual(a: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    let mut visual = None;
    for t in granted_auras(a, c) {
        if aura_family(t) != AuraFamily::Status {
            continue;
        }
        let curse = CurseKind::ALL.into_iter().any(|k| {
            let spec = curse_spec(k);
            spec.aura_type == t && spec.aura_name == c.name
        });
        let this = match t {
            _ if curse => Some("curse apply apparition (warlock_dots.rs)"),
            // A non-heal the client lands with a heal's kit (Innervate).
            _ if HealImpact::kind_for(a).is_some() => {
                Some("heal-kit landing on cast (heal_impact.rs)")
            }
            // Each client shield wears its model's cards, keyed on the aura's
            // name. An absorb with no model of its own draws nothing.
            AuraType::Absorb if c.name == PW_SHIELD_AURA_NAME => {
                Some("client shield cards (shield.rs)")
            }
            AuraType::DamageImmunity if c.name == DIVINE_SHIELD_AURA_NAME => {
                Some("client Divine Shield cards and sparkles (shield.rs)")
            }
            AuraType::Absorb if c.name == ICE_BARRIER_AURA_NAME => {
                Some("client Ice Barrier cards, icicles and mist (shield.rs)")
            }
            AuraType::HealingReduction => Some("heal-refused tell (mortal_wounds.rs)"),
            // A heal over time routes by its name: a per-tick pulse (Healing
            // Stream, the tick site's `kind_for_hot_tick`), a landing swirl
            // (Rejuvenation, `AuraLandingKind` at `apply_pending_auras`), or a
            // sustained pulse (Lifebloom, detected off `ActiveAuras`).
            AuraType::HealingOverTime => HotVisual::for_hot(&c.name).map(|v| match v {
                HotVisual::TickPulse(_) => {
                    assert_eq!(
                        HealImpact::kind_for_hot_tick(t, &c.name),
                        Some(HealImpactKind::TotemPulse)
                    );
                    "per-tick heal pulse (heal_impact.rs)"
                }
                HotVisual::LandingSwirl => {
                    assert_eq!(
                        AuraLandingKind::for_aura(t, &c.name),
                        Some(AuraLandingKind::RejuvenationSwirl)
                    );
                    "Rejuvenation ribbon swirl on landing (druid_heals.rs)"
                }
                HotVisual::SustainedPulse => "Lifebloom head pulse while it lives (druid_heals.rs)",
            }),
            AuraType::FearImmunity if a == BerserkerRage => {
                Some("berserk mask (effects/berserker_rage.rs → berserk.rs)")
            }
            AuraType::TravelForm => {
                Some("pill on all fours + shift puff (TravelFormVisual → shapeshift.rs)")
            }
            _ if wears_shadowform_look(c.applies_aura.as_ref().and_then(|a| a.compound)) => {
                Some("violet translucent body + feet shroud while the form holds (shadowform.rs)")
            }
            _ if wears_guardian_spirit(c.applies_aura.as_ref().and_then(|a| a.compound)) => Some(
                "halo and wings over the shoulders while the spirit holds (guardian_spirit.rs)",
            ),
            _ if AuraLandingKind::for_aura(t, &c.name)
                == Some(AuraLandingKind::MarkOfTheWildGlyph) =>
            {
                Some("Mark of the Wild glyph on landing (druid_heals.rs)")
            }
            // The Paladin talent one-shots (AS-374), each routed by its
            // aura's name: the landing marks it, the band does not.
            _ if paladin_oneshot(t, &c.name).is_some() => paladin_oneshot(t, &c.name),
            // Mind-numbing Poison's cast slow: the poison cloud on every
            // application. (Wound Poison's reduction is the heal-refused tell
            // above, which keys on the type.)
            _ if lands_poison_cloud(a) => Some(POISON_CLOUD),
            _ => family_application_cue(t),
        };
        // Every status aura must be drawn for the member to pass.
        visual = Some(this?);
    }
    visual
}

/// The Paladin talent one-shot an aura's landing plays, routed by its name.
fn paladin_oneshot(t: AuraType, name: &str) -> Option<&'static str> {
    AuraLandingKind::for_aura(t, name)
        .and_then(PaladinOneShotKind::for_landing)
        .map(oneshot_visual)
}

const STATUS_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    // A totem's OBJECT is drawn; the buff it pulses onto allies is not.
    (AirTotem, "AS-134 (aura application — totem pulse)"),
    (
        AmplifyCurse,
        "AS-346 (Amplify Curse and Shadow Trance, the primed casts)",
    ),
    (ArcaneIntellect, "AS-134 (aura application)"),
    (BattleShout, "AS-134 (aura application)"),
    (CommandingShout, "AS-134 (aura application)"),
    (ConcentrationAura, "AS-134 (aura application)"),
    (DemoralizingShout, "AS-134 (aura application)"),
    (DevotionAura, "AS-134 (aura application)"),
    (DivineProtection, "AS-376 (Divine Protection globe)"),
    (EarthTotem, "AS-134 (aura application — totem pulse)"),
    (
        ExposeArmor,
        "AS-366 (Expose Armor, the Combat capstone's armor rend)",
    ),
    (FireTotem, "AS-134 (aura application — totem pulse)"),
    (FrostArmor, "AS-134 (aura application)"),
    (
        Hemorrhage,
        "AS-336 (Hemorrhage, the Subtlety strike and its wound; AS-367 builds it)",
    ),
    (InnerFocus, "AS-352 (Inner Focus, the focus on the Priest)"),
    (MageArmorSpell, "AS-134 (aura application)"),
    (MoltenArmor, "AS-134 (aura application)"),
    (
        PainSuppression,
        "AS-353 (Pain Suppression, the save on the ally)",
    ),
    (
        PowerInfusion,
        "AS-353 (Power Infusion, the infusion on the ally)",
    ),
    (PowerWordFortitude, "AS-134 (aura application)"),
    (SacredShield, "AS-375 (Sacred Shield disc and absorb card)"),
    (
        ShadowDance,
        "AS-336 (Shadow Dance, the Subtlety capstone's window; AS-368 builds it)",
    ),
    (ShadowResistanceAura, "AS-134 (aura application)"),
    (
        VampiricEmbrace,
        "AS-357 (Vampiric Embrace on the target, and its heals; scoped by AS-335)",
    ),
];

#[test]
fn status_finds_every_member() {
    assert_finds("aura-application", is_status, STATUS_MEMBERS);
}

#[test]
fn status_lands_nothing_silently() {
    assert_judged(
        "aura-application",
        is_status,
        status_visual,
        STATUS_KNOWN_SILENT,
    );
}

// ── family: dispels ─────────────────────────────────────────────────────────

fn is_dispel(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_dispel
}

const DISPEL_MEMBERS: &[AbilityType] =
    &[DevourMagic, DispelMagic, MastersCall, PaladinCleanse, Purge];

/// The ribbon is spawned by `process_dispels` (effects/dispels.rs) for every
/// successful `DispelPending`, so a dispel is drawn exactly when its
/// resolution site spawns one. That is a per-ability fact about the producer,
/// so it is named per ability: a new dispel is judged silent until someone
/// confirms its producer and adds it here.
fn dispel_visual(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    match a {
        // class_ai/priest.rs, class_ai/paladin.rs, class_ai/mod.rs (Purge)
        DispelMagic | PaladinCleanse | Purge => Some("dispel ribbon (DispelPending → dispels.rs)"),
        // class_ai/pet_ai.rs
        DevourMagic => Some("dispel ribbon (DispelPending → dispels.rs)"),
        // class_ai/pet_ai.rs — also a DispelBurst at the dispatch site.
        MastersCall => Some("dispel ribbon + DispelBurst (pet_ai.rs)"),
        _ => None,
    }
}

#[test]
fn dispel_finds_every_member() {
    assert_finds("dispel", is_dispel, DISPEL_MEMBERS);
}

#[test]
fn dispel_lands_nothing_silently() {
    assert_judged("dispel", is_dispel, dispel_visual, &[]);
}

// ── family: interrupts ──────────────────────────────────────────────────────

fn is_interrupt(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_interrupt
}

const INTERRUPT_MEMBERS: &[AbilityType] = &[Kick, Pummel, SpellLock, WindShear];

/// The VICTIM's side: the mark the interrupt lands on the chest of the unit
/// whose cast it cut (`InterruptedBy` → `client_landings.rs`). That is what
/// tells an interrupt from a fizzle — the casting-orb sputter, which both
/// share, cannot — so it is the landing every member must reach. Routed
/// through `InterruptLanding::for_ability`, and the landing router must then
/// actually draw it.
fn interrupt_visual(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    let landing = InterruptLanding::for_ability(a)?;
    client_landing_style(a)?;
    Some(match landing {
        InterruptLanding::Shockwave => "shockwave on the victim's chest (client_landings.rs)",
        InterruptLanding::Seal => "closing rune seal on the victim's chest (client_landings.rs)",
        InterruptLanding::WindShockwave => {
            "wind-blue shockwave on the victim's chest (client_landings.rs)"
        }
    })
}

const INTERRUPT_KNOWN_SILENT: &[(AbilityType, &str)] = &[];

#[test]
fn interrupt_finds_every_member() {
    assert_finds("interrupt", is_interrupt, INTERRUPT_MEMBERS);
}

#[test]
fn interrupt_lands_nothing_silently() {
    assert_judged(
        "interrupt",
        is_interrupt,
        interrupt_visual,
        INTERRUPT_KNOWN_SILENT,
    );
}

/// The user's per-interrupt decisions, pinned on the router: Kick and Pummel
/// share the client's shockwave, Spell Lock takes Counterspell's seal, and
/// Wind Shear the shockwave in wind-blue. A wrong route fails here even
/// though it is not silent.
#[test]
fn each_interrupt_lands_its_decided_mark() {
    assert_eq!(
        InterruptLanding::for_ability(Kick),
        Some(InterruptLanding::Shockwave)
    );
    assert_eq!(
        InterruptLanding::for_ability(Pummel),
        Some(InterruptLanding::Shockwave)
    );
    assert_eq!(
        InterruptLanding::for_ability(SpellLock),
        Some(InterruptLanding::Seal)
    );
    assert_eq!(
        InterruptLanding::for_ability(WindShear),
        Some(InterruptLanding::WindShockwave)
    );
    // Kick and Pummel keep their weapon strokes on the interrupter's side.
    for a in [Kick, Pummel] {
        assert!(
            InstantAbilityFired::is_spawned_for(a),
            "{a:?} lost its stroke"
        );
    }
}

// ── family: health into mana ────────────────────────────────────────────────

fn is_health_to_mana(_: AbilityType, c: &AbilityConfig) -> bool {
    c.health_to_mana.is_some()
}

const HEALTH_TO_MANA_MEMBERS: &[AbilityType] = &[LifeTap];

/// A conversion lands on its caster: the Warlock AI spawns the
/// `InstantAbilityFired` marker, and the instant-ability router plays the
/// Life Tap glow off it (`lifetap.m2`, AS-340).
fn health_to_mana_visual(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    (InstantAbilityFired::is_spawned_for(a) && LifeTapGlow::plays_for(a))
        .then_some("Life Tap glow on the caster's chest (life_tap.rs)")
}

const HEALTH_TO_MANA_KNOWN_SILENT: &[(AbilityType, &str)] = &[];

#[test]
fn health_to_mana_finds_every_member() {
    assert_finds("health-to-mana", is_health_to_mana, HEALTH_TO_MANA_MEMBERS);
}

#[test]
fn health_to_mana_lands_nothing_silently() {
    assert_judged(
        "health-to-mana",
        is_health_to_mana,
        health_to_mana_visual,
        HEALTH_TO_MANA_KNOWN_SILENT,
    );
}

// ── family: on-use items ────────────────────────────────────────────────────
//
// An on-use item's effect lands through `process_on_use_items`
// (effects/on_use.rs), not through an ability, so this family scans
// `items.ron` rather than `abilities.ron` and names its members by item.

const ON_USE_MEMBERS: &[ItemId] = &[
    ItemId::DiamondFlask,
    ItemId::Earthstrike,
    ItemId::KissOfTheSpider,
    ItemId::MedallionOfTheAlliance,
    ItemId::TalismanOfEphemeralPower,
];

/// What a buff on-use's landing draws when nothing bespoke owns it: the
/// family's aura band. A label of its own, so the debt it stands for can be
/// told apart from a real visual ([`ON_USE_GENERIC_UNTIL`]).
const GENERIC_BAND: &str = "aura band on landing (aura_band.rs) — the generic cue";

/// The visual an on-use's landing reaches, by its effect kind. The
/// Medallion's break draws nothing yet (a self-cast gold dispel ribbon, the
/// Holy launch hands, the broken CC's own visual scattering is AS-432's).
///
/// A buff lands through `apply_pending_auras`, built by the constructor
/// `process_on_use_items` lands it with, so the family's aura band draws it
/// wherever the router sends that aura to the band — and with it the heal
/// over time a buff may carry. That is [`GENERIC_BAND`], and every member it
/// is all of is named in [`ON_USE_GENERIC_UNTIL`] or [`ON_USE_BAND_BY_RULING`].
fn on_use_visual(items: &ItemDefinitions, id: ItemId) -> Option<&'static str> {
    let item = items.get_unchecked(&id);
    let on_use = item.on_use.as_ref().expect("a member has an on-use");
    match on_use.effect {
        OnUseEffect::CcBreak => None,
        OnUseEffect::Buff(_) => {
            let buff = on_use.aura(id, &item.name);
            (AuraApplyRoute::for_instance(buff.effect_type, &buff.ability_name)
                == AuraApplyRoute::Band)
                .then_some(GENERIC_BAND)
        }
    }
}

/// On-use buffs the generic band draws only UNTIL their card gives each its
/// own one-shot: (item, card). Shrink-only, like a known-silent list — the
/// set of members drawn by the bare band must equal this list plus
/// [`ON_USE_BAND_BY_RULING`], so a new buff on the bare band fails until it
/// is listed, and an entry whose item gains a one-shot fails until removed.
const ON_USE_GENERIC_UNTIL: &[(ItemId, &str)] = &[
    (ItemId::Earthstrike, "AS-433"),
    (ItemId::KissOfTheSpider, "AS-433"),
    (ItemId::TalismanOfEphemeralPower, "AS-433"),
];

/// On-use buffs that keep the generic band by ruling, not pending a card:
/// (item, the card that ruled it, why). No design doc records these rulings,
/// so each cites its card.
const ON_USE_BAND_BY_RULING: &[(ItemId, &str, &str)] = &[(
    ItemId::DiamondFlask,
    "AS-338",
    "the user ruled the Flask keeps the aura band rather than a one-shot of its own",
)];

/// The generic-band debt: the members `judge` draws with [`GENERIC_BAND`]
/// alone are exactly `until` plus `ruled`, which overlap nowhere, and every
/// entry names its card (and a ruling its reason).
fn check_generic_band(
    members: &BTreeSet<ItemId>,
    judge: impl Fn(ItemId) -> Option<&'static str>,
    until: &[(ItemId, &str)],
    ruled: &[(ItemId, &str, &str)],
) -> Result<(), String> {
    check_sorted(
        "on-use",
        "generic-band debt",
        until.iter().map(|(id, _)| id),
    )?;
    check_sorted(
        "on-use",
        "band-by-ruling list",
        ruled.iter().map(|(id, _, _)| id),
    )?;
    let mut problems = Vec::new();
    let generic: BTreeSet<ItemId> = members
        .iter()
        .copied()
        .filter(|id| judge(*id) == Some(GENERIC_BAND))
        .collect();
    let owed: BTreeSet<ItemId> = until.iter().map(|(id, _)| *id).collect();
    let kept: BTreeSet<ItemId> = ruled.iter().map(|(id, _, _)| *id).collect();
    for id in owed.intersection(&kept) {
        problems.push(format!(
            "{id:?} is both owed a one-shot and ruled to keep the band — pick one"
        ));
    }
    let listed: BTreeSet<ItemId> = owed.union(&kept).copied().collect();
    for id in generic.difference(&listed) {
        problems.push(format!(
            "{id:?} is drawn only by the generic aura band — give it its own visual, or add \
             ({id:?}, \"<the card that will>\") to ON_USE_GENERIC_UNTIL"
        ));
    }
    for id in listed.difference(&generic) {
        problems.push(format!(
            "{id:?} is no longer drawn by the generic band alone — remove it from \
             ON_USE_GENERIC_UNTIL / ON_USE_BAND_BY_RULING"
        ));
    }
    for (id, card) in until {
        if !card.starts_with("AS-") {
            problems.push(format!("{id:?} names no card ({card:?})"));
        }
    }
    for (id, card, why) in ruled {
        if !card.starts_with("AS-") || why.trim().is_empty() {
            problems.push(format!("{id:?}'s ruling names no card or reason"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn on_use_generic_band_debt_is_listed() {
    let (members, items) = scan_on_use();
    if let Err(e) = check_generic_band(
        &members,
        |id| on_use_visual(&items, id),
        ON_USE_GENERIC_UNTIL,
        ON_USE_BAND_BY_RULING,
    ) {
        panic!("{e}");
    }
}

/// The guard fails on each way the debt can drift: a buff on the bare band
/// that nobody listed, an entry whose item gained its own visual, and an item
/// both owed and ruled.
#[test]
fn the_generic_band_guard_catches_planted_offenders() {
    use ItemId::{DiamondFlask, Earthstrike, KissOfTheSpider};
    let members: BTreeSet<ItemId> = [Earthstrike, KissOfTheSpider, DiamondFlask]
        .into_iter()
        .collect();
    let all_generic = |_| Some(GENERIC_BAND);
    let ruled: &[(ItemId, &str, &str)] = &[(DiamondFlask, "AS-338", "ruled")];
    let until: &[(ItemId, &str)] = &[(Earthstrike, "AS-433"), (KissOfTheSpider, "AS-433")];
    assert_eq!(
        check_generic_band(&members, all_generic, until, ruled),
        Ok(())
    );
    // An unlisted buff on the bare band.
    let err = check_generic_band(&members, all_generic, &until[..1], ruled).unwrap_err();
    assert!(
        err.contains("KissOfTheSpider is drawn only by the generic"),
        "{err}"
    );
    // An entry whose item now draws its own one-shot.
    let bespoke = |id| {
        (id != Earthstrike)
            .then_some(GENERIC_BAND)
            .or(Some("one-shot"))
    };
    let err = check_generic_band(&members, bespoke, until, ruled).unwrap_err();
    assert!(err.contains("Earthstrike is no longer drawn"), "{err}");
    // Owed and ruled at once.
    let both: &[(ItemId, &str, &str)] = &[
        (DiamondFlask, "AS-338", "ruled"),
        (Earthstrike, "AS-338", "x"),
    ];
    let err = check_generic_band(&members, all_generic, until, both).unwrap_err();
    assert!(err.contains("Earthstrike is both owed"), "{err}");
}

const ON_USE_KNOWN_SILENT: &[(ItemId, &str)] = &[(ItemId::MedallionOfTheAlliance, "AS-432")];

fn scan_on_use() -> (BTreeSet<ItemId>, ItemDefinitions) {
    let items = load_item_definitions().expect("items.ron loads");
    let members = items
        .iter()
        .filter(|(_, item)| item.on_use.is_some())
        .map(|(id, _)| *id)
        .collect();
    (members, items)
}

#[test]
fn on_use_finds_every_member() {
    let (members, _) = scan_on_use();
    if let Err(e) = check_finds("on-use", &members, ON_USE_MEMBERS) {
        panic!("{e}");
    }
}

#[test]
fn on_use_lands_nothing_silently() {
    let (members, items) = scan_on_use();
    let judged = check_judged(
        "on-use",
        &members,
        |id| on_use_visual(&items, id),
        ON_USE_KNOWN_SILENT,
    );
    if let Err(e) = judged {
        panic!("{e}");
    }
}

// ── family: triggered absorbs (items) ───────────────────────────────────────
//
// A triggered absorb's shield lands through `fire_triggered_absorbs`
// (triggered_absorbs.rs), not through an ability, so this family scans
// `items.ron` and names its members by item.

const TRIGGERED_ABSORB_MEMBERS: &[ItemId] = &[ItemId::AegisOfTheLastStand];

/// The visual a triggered absorb's shield reaches: a client shield rig keyed on
/// its aura's name (`is_live_shield`), built through the constructor the
/// simulation raises it with.
fn triggered_absorb_visual(items: &ItemDefinitions, id: ItemId) -> Option<&'static str> {
    let item = items.get_unchecked(&id);
    let absorb = item.triggered_absorb.as_ref().expect("a member has one");
    let shield = absorb.absorb.shield_aura(
        &item.name,
        arenasim::states::play_match::triggered_absorbs::ShieldFrom::Item(id),
        1.0,
    );
    ShieldKind::ALL
        .into_iter()
        .any(|kind| is_live_shield(kind, &shield))
        .then_some("client shield cards (shield.rs)")
}

const TRIGGERED_ABSORB_KNOWN_SILENT: &[(ItemId, &str)] = &[];

fn scan_triggered_absorbs() -> (BTreeSet<ItemId>, ItemDefinitions) {
    let items = load_item_definitions().expect("items.ron loads");
    let members = items
        .iter()
        .filter(|(_, item)| item.triggered_absorb.is_some())
        .map(|(id, _)| *id)
        .collect();
    (members, items)
}

#[test]
fn triggered_absorbs_find_every_member() {
    let (members, _) = scan_triggered_absorbs();
    if let Err(e) = check_finds("triggered-absorb", &members, TRIGGERED_ABSORB_MEMBERS) {
        panic!("{e}");
    }
}

#[test]
fn triggered_absorbs_land_nothing_silently() {
    let (members, items) = scan_triggered_absorbs();
    let judged = check_judged(
        "triggered-absorb",
        &members,
        |id| triggered_absorb_visual(&items, id),
        TRIGGERED_ABSORB_KNOWN_SILENT,
    );
    if let Err(e) = judged {
        panic!("{e}");
    }
}

// ── the families cover the config ───────────────────────────────────────────

/// Abilities no family scans, each with the visual that covers it. Anything
/// else outside every family fails `every_ability_belongs_to_a_family`.
const NO_FAMILY: &[(AbilityType, &str)] = &[
    (Charge, "a dash — charge trail (movement_trails.rs)"),
    (Disengage, "a leap — disengage trail (movement_trails.rs)"),
    (
        Flare,
        "a placement — the client's flare burning on the ground in a pulsing pool of \
         light, a rim at its radius (flare.rs), for as long as the Flare burns; it \
         lands nothing on anyone",
    ),
    (
        FrostTrap,
        "a placement — trap, trigger burst and slow-zone decal (traps.rs, ice_block.rs); \
         its slow is applied by the zone, not declared in the RON, and a body it slows \
         carries the bind ring (slow_ring.rs)",
    ),
];

// ── family: extra swings (item procs) ───────────────────────────────────────

/// An item proc that lands an extra swing (`ProcEffect::ExtraSwing`) lands an
/// attack, so it is a landing like any ability's — but it is no ability, so
/// this family scans `items.ron` and is keyed by item. Its swing takes Windfury
/// Totem's path, and its only possible visual on that path is the totem's
/// tornado, which a trinket must NOT draw (`ExtraSwingSource`): so the judge
/// reports what the shared path would draw for the item, and every member is
/// silent until its own visual lands.
fn extra_swing_items() -> BTreeSet<&'static str> {
    let items = load_item_definitions().expect("items.ron loads");
    ItemId::all()
        .iter()
        .filter(|id| {
            items
                .get(id)
                .and_then(|item| item.proc.as_ref())
                .is_some_and(|p| p.effect == ProcEffect::ExtraSwing)
        })
        .map(|id| id.as_str())
        .collect()
}

fn extra_swing_judge(name: &'static str) -> Option<&'static str> {
    let item = *ItemId::all()
        .iter()
        .find(|id| id.as_str() == name)
        .expect("a member is an item");
    ExtraSwingSource::Trinket(item)
        .spawns_windfury_tornado()
        .then_some("WindfuryTornado (Windfury Totem's visual — a trinket must not draw it)")
}

const EXTRA_SWING_MEMBERS: &[&str] = &["Gressil", "HandOfJustice"];

/// Both visuals are AS-434's (Hand of Justice's own is the client's Cheap Shot
/// crescents).
const EXTRA_SWING_KNOWN_SILENT: &[(&str, &str)] =
    &[("Gressil", "AS-434"), ("HandOfJustice", "AS-434")];

#[test]
fn extra_swing_finds_every_member() {
    if let Err(e) = check_finds("extra-swing", &extra_swing_items(), EXTRA_SWING_MEMBERS) {
        panic!("{e}");
    }
}

#[test]
fn extra_swing_lands_nothing_silently() {
    if let Err(e) = check_judged(
        "extra-swing",
        &extra_swing_items(),
        extra_swing_judge,
        EXTRA_SWING_KNOWN_SILENT,
    ) {
        panic!("{e}");
    }
}

// ── family: talent mechanics ────────────────────────────────────────────────
//
// A talent's `Mechanic` lands at its own code site (`talent_mechanics`), not
// through an ability, so this family scans `talents.ron` and names its members
// by node. The judge asks for the mechanic's OWN visual: what the generic
// channels every aura shares (the band as an aura lands, its buff-bar icon)
// and a unit's ordinary death animation draw does not tell the player this
// talent fired.

/// Every node of `talents.ron` carrying a `Mechanic`.
fn talent_mechanic_nodes() -> BTreeSet<TalentId> {
    TalentDefinitions::shipped()
        .iter()
        .filter(|(_, node)| node.per_rank.iter().any(|e| mechanic_of(e).is_some()))
        .map(|(id, _)| id)
        .collect()
}

fn mechanic_of(effect: &TalentEffect) -> Option<MechanicKind> {
    match effect {
        TalentEffect::Mechanic(m) => Some(m.kind()),
        TalentEffect::FromRank(_, inner) => mechanic_of(inner),
        _ => None,
    }
}

/// The bespoke visual a mechanic lands with. Exhaustive, so mechanic N+1
/// must answer it.
fn mechanic_visual(kind: MechanicKind) -> Option<&'static str> {
    match kind {
        MechanicKind::SoulLink
        | MechanicKind::ReflectiveShield
        | MechanicKind::CheatDeath
        | MechanicKind::MasterDemonologist
        | MechanicKind::DemonicSacrifice
        | MechanicKind::UnbreakableWill => None,
    }
}

/// A node draws when every mechanic it carries does.
fn talent_mechanic_judge(talent: TalentId) -> Option<&'static str> {
    let node = TalentDefinitions::shipped().get(talent)?;
    let mut visual = None;
    for kind in node.per_rank.iter().filter_map(mechanic_of) {
        visual = Some(mechanic_visual(kind)?);
    }
    visual
}

const TALENT_MECHANIC_MEMBERS: &[TalentId] = &[
    TalentId::CheatDeath,
    TalentId::DemonicSacrifice,
    TalentId::MasterDemonologist,
    TalentId::ReflectiveShield,
    TalentId::SoulLink,
    TalentId::UnbreakableWill,
];

const TALENT_MECHANIC_KNOWN_SILENT: &[(TalentId, &str)] = &[
    (
        TalentId::CheatDeath,
        "AS-336 (Cheat Death's save; AS-369 builds it on AS-354's shared Saved cue)",
    ),
    (
        TalentId::DemonicSacrifice,
        "AS-350 (the sacrifice triad; the Felhunter's death is the ordinary one)",
    ),
    (
        TalentId::ReflectiveShield,
        "AS-355 (Reflective Shield's reflect; it shows only as the attacker's damage taken)",
    ),
    (
        TalentId::SoulLink,
        "AS-350 (the Soul Link rune column; the transfer shows only on the Felhunter's bar)",
    ),
];

const TALENT_MECHANIC_SILENT_BY_DESIGN: &[SilentByDesign<TalentId>] = &[
    SilentByDesign {
        ability: TalentId::MasterDemonologist,
        doc: "docs/design/proposals/2026-10-11-talents-and-gauntlet.md",
        reason: "icon-only by ruling: its footprint is the aura icon on the Warlock and the \
                 Felhunter (§5.1)",
    },
    SilentByDesign {
        ability: TalentId::UnbreakableWill,
        doc: "docs/design/proposals/2026-10-11-talents-and-gauntlet.md",
        reason: "its footprint is the shorter crowd-control bar and the logged reduction \
                 (§5.2): the crowd control it shortens draws as it always has",
    },
];

#[test]
fn talent_mechanic_finds_every_member() {
    if let Err(e) = check_finds(
        "talent-mechanic",
        &talent_mechanic_nodes(),
        TALENT_MECHANIC_MEMBERS,
    ) {
        panic!("{e}");
    }
}

#[test]
fn talent_mechanic_lands_nothing_silently() {
    if let Err(e) = check_rulings("talent-mechanic", TALENT_MECHANIC_SILENT_BY_DESIGN) {
        panic!("{e}");
    }
    if let Err(e) = check_sorted(
        "talent-mechanic",
        "known-silent list",
        TALENT_MECHANIC_KNOWN_SILENT
            .iter()
            .map(|(member, _)| member),
    ) {
        panic!("{e}");
    }
    let mut listed: Vec<(TalentId, &str)> = TALENT_MECHANIC_KNOWN_SILENT.to_vec();
    listed.extend(
        TALENT_MECHANIC_SILENT_BY_DESIGN
            .iter()
            .map(|r| (r.ability, r.doc)),
    );
    listed.sort_by_key(|(member, _)| format!("{member:?}"));
    if let Err(e) = check_judged(
        "talent-mechanic",
        &talent_mechanic_nodes(),
        talent_mechanic_judge,
        &listed,
    ) {
        panic!("{e}");
    }
}

// ── family: talent proc auras ───────────────────────────────────────────────
//
// A talent's `Proc` lands an aura through `resolve_talent_procs`, not through
// an ability, so this family scans `talents.ron` for every node carrying one
// (its `FromRank` wrappers unwrapped) and names its members by node. Each
// aura is built by the engine's own `talent_procs::effect_aura`, so the judge
// sees the type and name the renderers will.
//
// The judge asks for a treatment of the proc's OWN aura: one its type or its
// name routes to (the stun whirl, a tinted slow ring, a named landing, a heal
// over time's visual, a client shield). The generic aura band does NOT count,
// as for talent mechanics: it sweeps every buff alike, so it cannot tell the
// player this talent fired. Neither does the buff-bar icon every proc wears
// (`source_talent`). A proc ruled to need no visual of its own, the band
// and its icon being enough, is named in
// [`TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING`] with the card that ruled it.

/// One talent proc as a node declares it: its id, its name and the proc.
type NodeProc = (TalentId, String, TalentProc);

/// Every proc in `defs`, each node's `FromRank` wrappers unwrapped.
fn talent_procs_in(defs: &TalentDefinitions) -> Vec<NodeProc> {
    fn procs_in(effect: &TalentEffect, out: &mut Vec<TalentProc>) {
        match effect {
            TalentEffect::Proc(p) => out.push(p.clone()),
            TalentEffect::FromRank(_, inner) => procs_in(inner, out),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for (id, node) in defs.iter() {
        let mut procs = Vec::new();
        for effect in &node.per_rank {
            procs_in(effect, &mut procs);
        }
        all.extend(procs.into_iter().map(|p| (id, node.name.clone(), p)));
    }
    all
}

/// Every node of `defs` carrying a `Proc`.
fn talent_proc_nodes(defs: &TalentDefinitions) -> BTreeSet<TalentId> {
    talent_procs_in(defs)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
}

/// The aura a proc lands, through the engine's own constructor, on an event
/// with a unit on its other end and an amount (every effect's needs).
fn talent_proc_aura((id, name, proc): &NodeProc) -> Aura {
    let slot = TalentProcSlot::new(*id, name, proc.clone());
    let (owner, other) = (Entity::from_raw(1), Entity::from_raw(2));
    let event = ProcEvent::new(owner, slot.proc.trigger)
        .amount(50.0)
        .other(other);
    effect_aura(&slot, owner, &event)
        .unwrap_or_else(|| panic!("{id:?}'s proc lands no aura"))
        .aura
}

/// The treatment a proc's aura reaches, by its family. Exhaustive over
/// [`AuraFamily`], so a family N+1 must answer here too. A proc has no school
/// and is never Freezing Trap's.
fn talent_proc_aura_visual(aura: &Aura) -> Option<&'static str> {
    let (t, name) = (aura.effect_type, aura.ability_name.as_str());
    match aura_family(t) {
        AuraFamily::Control => control_visual(t, name, None, false),
        AuraFamily::Dot => DotStateVisual::for_dot(name).map(|_| "DoT state (dot_state.rs)"),
        AuraFamily::Status => {
            if AuraLandingKind::for_aura(t, name).is_some() {
                Some("named landing one-shot (AuraLandingKind)")
            } else if t == AuraType::HealingOverTime && HotVisual::for_hot(name).is_some() {
                Some("heal-over-time visual (HotVisual::for_hot)")
            } else if ShieldKind::ALL
                .into_iter()
                .any(|kind| is_live_shield(kind, aura))
            {
                Some("client shield cards (shield.rs)")
            } else {
                // The aura band (`AuraApplyRoute::Band`) is not this proc's
                // own visual: see the family's header.
                None
            }
        }
    }
}

/// A node draws when every aura its procs land does (Vengeance lands its
/// buff from two procs).
fn talent_proc_judge(defs: &TalentDefinitions, talent: TalentId) -> Option<&'static str> {
    let mut visual = None;
    for proc in talent_procs_in(defs)
        .iter()
        .filter(|(id, _, _)| *id == talent)
    {
        visual = Some(talent_proc_aura_visual(&talent_proc_aura(proc))?);
    }
    visual
}

const TALENT_PROC_MEMBERS: &[TalentId] = &[
    TalentId::Aftermath,
    TalentId::Blackout,
    TalentId::BlessedRecovery,
    TalentId::ImprovedShadowBolt,
    TalentId::InfusionOfLight,
    TalentId::Initiative,
    TalentId::Inspiration,
    TalentId::LightsGrace,
    TalentId::MasterOfSubtlety,
    TalentId::Nightfall,
    TalentId::TheArtOfWar,
    TalentId::Vengeance,
];

/// Procs whose aura lands with no treatment of its own yet, each with the
/// card that builds one. Blackout is not here: its stun draws the stun whirl
/// every stun does (AS-335 asked whether it would).
const TALENT_PROC_KNOWN_SILENT: &[(TalentId, &str)] = &[
    (
        TalentId::Aftermath,
        "AS-349 (Aftermath: the client fire hit, and the slow ring with a Fire tint)",
    ),
    (
        TalentId::BlessedRecovery,
        "AS-352 (Blessed Recovery's palm glow and Renew's amber rings)",
    ),
    (TalentId::Nightfall, "AS-346 (Shadow Trance's speck halo)"),
    (
        TalentId::Vengeance,
        "AS-377 (Vengeance: both hands glow pale blue while the buff lasts)",
    ),
];

/// Procs ruled to need no bespoke visual: the generic aura band draws each as
/// it lands, and its buff-bar icon carries it while it lasts. (node, the card
/// that ruled it, why.) No design doc on main records these rulings, so each
/// cites its card, as `ON_USE_BAND_BY_RULING` does. They count as listed, so
/// one that gains a treatment fails until it is removed; and each must be one
/// the band draws (`AuraApplyRoute::Band`), so the list cannot hold a proc
/// that lands with nothing at all.
const TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING: &[(TalentId, &str, &str)] = &[
    (
        TalentId::ImprovedShadowBolt,
        "AS-334",
        "Shadow Vulnerability needs no visual of its own (AS-411's Visuals line); the \
         proposal's footprint is the debuff icon on the target (§5.1)",
    ),
    (
        TalentId::Initiative,
        "AS-479",
        "user ruling 2026-11-13: \"no it doesnt need a visual, just a buff icon\"",
    ),
    (
        TalentId::Inspiration,
        "AS-335",
        "Inspiration is the generic buff ring; it needs no card",
    ),
    (
        TalentId::LightsGrace,
        "AS-337",
        "Light's Grace has no client visual at any build: the icon carries it (AS-374 item 8, \
         AS-420)",
    ),
    (
        TalentId::MasterOfSubtlety,
        "AS-336",
        "the generic buff band draws Master of Subtlety; it needs no visual of its own \
         (AS-419's Visuals line)",
    ),
];

/// A by-ruling entry names a card (`AS-<n>`) and gives a reason.
fn check_proc_rulings(rulings: &[(TalentId, &str, &str)]) -> Result<(), String> {
    check_sorted(
        "talent-proc",
        "no-bespoke-visual-by-ruling list",
        rulings.iter().map(|(talent, _, _)| talent),
    )?;
    let mut problems = Vec::new();
    for (talent, card, reason) in rulings {
        let numbered = card
            .strip_prefix("AS-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !numbered {
            problems.push(format!(
                "{talent:?} needs no bespoke visual by ruling, but `{card}` is not a card id \
                 (AS-<n>)"
            ));
        }
        if reason.trim().is_empty() {
            problems.push(format!(
                "{talent:?} needs no bespoke visual by ruling, with no reason"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Every aura a by-ruling proc lands is one the generic aura band draws: the
/// ruling is that the band and the icon are enough, not that nothing draws.
fn check_rulings_draw_the_band(
    defs: &TalentDefinitions,
    rulings: &[(TalentId, &str, &str)],
) -> Result<(), String> {
    let mut problems = Vec::new();
    for proc in talent_procs_in(defs) {
        if !rulings.iter().any(|(talent, _, _)| *talent == proc.0) {
            continue;
        }
        let aura = talent_proc_aura(&proc);
        let route = AuraApplyRoute::for_instance(aura.effect_type, &aura.ability_name);
        if route != AuraApplyRoute::Band {
            problems.push(format!(
                "{:?} needs no bespoke visual by ruling, but its {:?} aura routes to \
                 {route:?}, not the aura band — list it as known-silent with its card",
                proc.0, aura.effect_type
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// The family's two claims against `defs`, with `known_silent` and `rulings`
/// as its lists: the planted-offender test runs the same function on a
/// planted table.
fn check_talent_procs(
    defs: &TalentDefinitions,
    members: &[TalentId],
    known_silent: &[(TalentId, &str)],
    rulings: &[(TalentId, &str, &str)],
) -> Result<(), String> {
    let scanned = talent_proc_nodes(defs);
    check_finds("talent-proc", &scanned, members)?;
    check_proc_rulings(rulings)?;
    check_rulings_draw_the_band(defs, rulings)?;
    check_sorted(
        "talent-proc",
        "known-silent list",
        known_silent.iter().map(|(member, _)| member),
    )?;
    let mut listed: Vec<(TalentId, &str)> = known_silent.to_vec();
    listed.extend(rulings.iter().map(|(talent, card, _)| (*talent, *card)));
    listed.sort_by_key(|(member, _)| format!("{member:?}"));
    check_judged(
        "talent-proc",
        &scanned,
        |talent| talent_proc_judge(defs, talent),
        &listed,
    )
}

#[test]
fn talent_proc_finds_every_member() {
    if let Err(e) = check_finds(
        "talent-proc",
        &talent_proc_nodes(TalentDefinitions::shipped()),
        TALENT_PROC_MEMBERS,
    ) {
        panic!("{e}");
    }
}

#[test]
fn talent_proc_lands_nothing_silently() {
    if let Err(e) = check_talent_procs(
        TalentDefinitions::shipped(),
        TALENT_PROC_MEMBERS,
        TALENT_PROC_KNOWN_SILENT,
        TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING,
    ) {
        panic!("{e}");
    }
}

/// The judge names what the one drawn member draws, so a wrong route fails
/// even though it is not silent; and the generic band is not a visual.
#[test]
fn the_talent_proc_judge_names_each_drawn_members_treatment() {
    let defs = TalentDefinitions::shipped();
    assert_eq!(
        talent_proc_judge(defs, TalentId::Blackout),
        Some("stun whirl (hard_cc.rs)")
    );
    // Inspiration's armor buff routes to the band, which is not its own.
    let inspiration = talent_procs_in(defs)
        .into_iter()
        .find(|(id, _, _)| *id == TalentId::Inspiration)
        .expect("Inspiration is a proc");
    let aura = talent_proc_aura(&inspiration);
    assert_eq!(
        AuraApplyRoute::for_instance(aura.effect_type, &aura.ability_name),
        AuraApplyRoute::Band
    );
    assert_eq!(talent_proc_aura_visual(&aura), None);
}

/// A talent proc planted on a node that has none (Improved Ambush), with no
/// tag, and behind a `FromRank` (no shipped proc is, so the plant is what
/// holds the scan to unwrapping one): the family derives its members from
/// the table, so the scan finds it unnamed, and with it named the judge finds
/// it silent and unlisted.
#[test]
fn the_talent_proc_family_catches_a_planted_untagged_proc() {
    let shipped = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/config/talents.ron"),
    )
    .expect("talents.ron reads");
    let anchor = "per_rank: [Patch(on: Ability(Ambush), field: CritChance, op: Add(0.15))],";
    assert_eq!(shipped.matches(anchor).count(), 1, "the anchor is unique");
    let planted = shipped.replace(
        anchor,
        "per_rank: [
                Patch(on: Ability(Ambush), field: CritChance, op: Add(0.15)),
                FromRank(2, Proc((
                    trigger: MeleeHit,
                    chance: Chance(0.1),
                    ranks_add: Chance,
                    effect: ApplyAura(
                        on: Owner,
                        aura_type: DamageDealtIncrease,
                        magnitude: 0.05,
                        duration: 6.0,
                        dispel_type: Untyped,
                        name: Some(\"Planted Fury\"),
                    ),
                ))),
            ],",
    );
    let defs = TalentDefinitions::from_ron(&planted, &AbilityDefinitions::default())
        .expect("the planted table loads");
    assert!(talent_proc_nodes(&defs).contains(&TalentId::ImprovedAmbush));

    // Unnamed: claim 1 fails on it.
    let err = check_talent_procs(
        &defs,
        TALENT_PROC_MEMBERS,
        TALENT_PROC_KNOWN_SILENT,
        TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING,
    )
    .unwrap_err();
    assert!(
        err.contains("ImprovedAmbush is now a talent-proc member"),
        "{err}"
    );

    // Named but untagged: claim 2 fails on it, and only on it.
    let mut members = TALENT_PROC_MEMBERS.to_vec();
    members.push(TalentId::ImprovedAmbush);
    members.sort_by_key(|id| format!("{id:?}"));
    let err = check_talent_procs(
        &defs,
        &members,
        TALENT_PROC_KNOWN_SILENT,
        TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING,
    )
    .unwrap_err();
    assert_eq!(err.lines().count(), 1, "only the planted proc fails: {err}");
    assert!(
        err.contains("ImprovedAmbush lands with no talent-proc visual"),
        "{err}"
    );

    // Tagged with its card: the family passes.
    let mut tagged = TALENT_PROC_KNOWN_SILENT.to_vec();
    tagged.push((TalentId::ImprovedAmbush, "AS-0"));
    tagged.sort_by_key(|(id, _)| format!("{id:?}"));
    assert_eq!(
        check_talent_procs(
            &defs,
            &members,
            &tagged,
            TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING
        ),
        Ok(())
    );
}

/// A ruling on a proc the band does not draw (Aftermath's slow routes to the
/// slow ring, which has no tint for it) fails: the ruling list holds only
/// procs the band draws.
#[test]
fn a_ruling_on_a_proc_the_band_does_not_draw_fails() {
    let defs = TalentDefinitions::shipped();
    let silent: Vec<(TalentId, &str)> = TALENT_PROC_KNOWN_SILENT
        .iter()
        .copied()
        .filter(|(id, _)| *id != TalentId::Aftermath)
        .collect();
    let mut rulings = TALENT_PROC_NO_BESPOKE_VISUAL_BY_RULING.to_vec();
    rulings.push((TalentId::Aftermath, "AS-0", "planted"));
    rulings.sort_by_key(|(id, _, _)| format!("{id:?}"));
    let err = check_talent_procs(defs, TALENT_PROC_MEMBERS, &silent, &rulings).unwrap_err();
    assert_eq!(err.lines().count(), 1, "only Aftermath fails: {err}");
    assert!(
        err.contains("Aftermath needs no bespoke visual by ruling, but its MovementSpeedSlow"),
        "{err}"
    );
}

#[test]
fn harness_holds_a_proc_ruling_to_a_card_and_a_reason() {
    assert_eq!(
        check_proc_rulings(&[(TalentId::Nightfall, "AS-1", "planted")]),
        Ok(())
    );
    for bad in ["AS-", "docs/x.md", "AS-1a"] {
        let err = check_proc_rulings(&[(TalentId::Nightfall, bad, "planted")]).unwrap_err();
        assert!(err.contains("is not a card id"), "{err}");
    }
    let err = check_proc_rulings(&[(TalentId::Nightfall, "AS-1", " ")]).unwrap_err();
    assert!(err.contains("no reason"), "{err}");
}

const FAMILIES: &[fn(AbilityType, &AbilityConfig) -> bool] = &[
    is_heal,
    is_direct_damage,
    is_dot,
    is_control,
    is_status,
    is_dispel,
    is_interrupt,
    is_health_to_mana,
];

#[test]
fn every_ability_belongs_to_a_family() {
    let outside = scan(|a, c| !FAMILIES.iter().any(|f| f(a, c)));
    let named: Vec<AbilityType> = NO_FAMILY.iter().map(|(a, _)| *a).collect();
    if let Err(e) = check_finds("no-family", &outside, &named) {
        panic!(
            "{e}\nAn ability outside every family is outside every lands-silently \
             sweep. Widen a family's scan to cover it, or name it in NO_FAMILY with \
             the visual that covers it."
        );
    }
}

// ── the harness catches what it claims to catch (planted offenders) ─────────

fn set(xs: &[AbilityType]) -> BTreeSet<AbilityType> {
    xs.iter().copied().collect()
}

fn draws_all_but(silent: &'static [AbilityType]) -> impl Fn(AbilityType) -> Option<&'static str> {
    move |a| (!silent.contains(&a)).then_some("planted visual")
}

#[test]
fn harness_passes_a_clean_family() {
    let members = set(&[Frostbolt, Fear]);
    assert_eq!(check_finds("planted", &members, &[Fear, Frostbolt]), Ok(()));
    assert_eq!(
        check_judged(
            "planted",
            &members,
            draws_all_but(&[Fear]),
            &[(Fear, "AS-0")]
        ),
        Ok(())
    );
}

#[test]
fn harness_fails_a_silent_member_that_is_not_listed() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt, Fear]),
        draws_all_but(&[Fear]),
        &[],
    )
    .unwrap_err();
    assert!(err.contains("Fear lands with no planted visual"), "{err}");
    assert!(
        err.contains("add (Fear,"),
        "the message must say what to add: {err}"
    );
}

#[test]
fn harness_fails_a_listed_member_that_now_draws() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt, Fear]),
        draws_all_but(&[]),
        &[(Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(err.contains("Fear now draws `planted visual`"), "{err}");
    assert!(
        err.contains("remove it from the planted known-silent list"),
        "{err}"
    );
}

#[test]
fn harness_fails_a_listed_ability_that_is_not_a_member() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt]),
        draws_all_but(&[]),
        &[(Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(
        err.contains("Fear is on the planted known-silent list (AS-0) but is not"),
        "{err}"
    );
}

#[test]
fn harness_fails_a_duplicated_listing() {
    let err = check_judged(
        "planted",
        &set(&[Fear]),
        draws_all_but(&[Fear]),
        &[(Fear, "AS-0"), (Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(err.contains("twice"), "{err}");
}

#[test]
fn harness_holds_a_ruling_to_a_doc_that_exists() {
    let ruled = |doc| SilentByDesign {
        ability: Fear,
        doc,
        reason: "planted",
    };
    assert_eq!(
        check_rulings(
            "planted",
            &[ruled("docs/design/2026-10-03-druid-client-data.md")]
        ),
        Ok(())
    );
    for bad in ["AS-0", "docs/design/no-such-ruling.md"] {
        let err = check_rulings("planted", &[ruled(bad)]).unwrap_err();
        assert!(err.contains("is not a design doc"), "{err}");
    }
    let unreasoned = SilentByDesign {
        reason: " ",
        ..ruled("docs/design/2026-10-03-druid-client-data.md")
    };
    let err = check_rulings("planted", &[unreasoned]).unwrap_err();
    assert!(err.contains("no reason"), "{err}");
}

#[test]
fn harness_fails_an_unsorted_list() {
    let err = check_finds("planted", &set(&[Fear, Frostbolt]), &[Frostbolt, Fear]).unwrap_err();
    assert!(err.contains("member list is not in name order"), "{err}");
    let err = check_judged(
        "planted",
        &set(&[Fear, Frostbolt]),
        |_| None,
        &[(Frostbolt, "AS-1"), (Fear, "AS-1")],
    )
    .unwrap_err();
    assert!(
        err.contains("known-silent list is not in name order"),
        "{err}"
    );
}

#[test]
fn harness_fails_a_scan_that_lost_a_member() {
    let err = check_finds("planted", &set(&[Frostbolt]), &[Fear, Frostbolt]).unwrap_err();
    assert!(err.contains("no longer finds Fear"), "{err}");
}

#[test]
fn harness_fails_a_scan_that_found_an_unnamed_member() {
    let err = check_finds("planted", &set(&[Frostbolt, Fear]), &[Frostbolt]).unwrap_err();
    assert!(err.contains("Fear is now a planted member"), "{err}");
}

/// The harness checks run against the real config see the real config: a
/// scan that finds nothing would pass claim 2 vacuously, so claim 1 must
/// fail on it.
#[test]
fn harness_fails_an_empty_scan() {
    let err = check_finds("planted", &BTreeSet::new(), DOT_MEMBERS).unwrap_err();
    for m in DOT_MEMBERS {
        assert!(err.contains(&format!("no longer finds {m:?}")), "{err}");
    }
}
