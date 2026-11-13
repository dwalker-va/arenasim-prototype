#!/usr/bin/env python3
"""AS-481: every TBC class or pet spell whose damage is a bleed.

usage: scripts/tbc_bleeds.py [--build 2.5.6.69795] [--cache-dir DIR]
                             > tests/data/tbc_bleeds.csv

Bleeds ignore armor, as in TBC (user ruling, 2026-11-13). The sim declares a
bleed on an ability's damage over time (`mechanic: Some(Bleed)` in
`abilities.ron`); `tests/bleed_mechanic_audit.rs` holds that declared set to
this file, so an ability TBC calls a bleed cannot ship without the flag, and
one it does not cannot carry it.

A spell is a bleed here by TrinityCore's armor rule (`IsDamageReducedByArmor`):
it has a damaging effect -- a periodic damage aura (Effect 6, EffectAura 3) or
school damage (Effect 2) -- whose mechanic is Bleed (15), read from the spell's
own `SpellCategories.Mechanic` or that effect's `SpellEffect.EffectMechanic`.
So Rake's direct hit is not a bleed and its periodic effect is. A spell that
triggers a bleed (Pounce -> Pounce Bleed, Deep Wounds -> Deep Wound) is listed
under its own name too, `via` the bleed it triggers, since the sim folds a
trigger into one ability. A trigger only a spell script makes is in
SCRIPTED_TRIGGERS.

The inventory is every spell on a class's skill lines (`SkillLineAbility`,
joined by `ClassMask`, a talent with no mask by its line) or a pet's ("Pet -
<family>"), below the Season of Discovery cut, at `DifficultyID` 0. Rows are
(class, name): the sim joins an ability by its class and its RON `name`.
"""
from __future__ import annotations

import argparse
import csv
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
sys.path.insert(0, HERE)

import db2_spell_sweep as db2  # noqa: E402

TBC_BUILD = "2.5.6.69795"
ERA_CUT = 400000
BLEED = "15"
EFFECT_APPLY_AURA = "6"
AURA_PERIODIC_DAMAGE = "3"
EFFECT_SCHOOL_DAMAGE = "2"
# How deep a trigger chain is followed (Deep Wounds: talent -> proc -> bleed).
TRIGGER_DEPTH = 3
# Triggers the client data does not carry: a DUMMY effect (Effect 3) whose
# spell script casts the bleed. Deep Wounds' three procs (12162, 12850, 12868)
# cast Deep Wound (12721) from TrinityCore's `spell_warr_deep_wounds`.
SCRIPTED_TRIGGERS = {"12162": "12721", "12850": "12721", "12868": "12721"}

# `ChrClasses` -> its `ClassMask` bit.
CLASS_BIT = {
    "Warrior": 1 << 0,
    "Paladin": 1 << 1,
    "Hunter": 1 << 2,
    "Rogue": 1 << 3,
    "Priest": 1 << 4,
    "Shaman": 1 << 6,
    "Mage": 1 << 7,
    "Warlock": 1 << 8,
    "Druid": 1 << 10,
}


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--build", default=TBC_BUILD)
    p.add_argument("--cache-dir", default=None)
    args = p.parse_args(argv)
    cache = args.cache_dir or os.path.join(ROOT, ".db2-cache", args.build)

    names = {r["ID"]: r["Name_lang"] for r in db2.load("SpellName", args.build, cache)}
    lines = {r["ID"]: r["DisplayName_lang"] for r in db2.load("SkillLine", args.build, cache)}
    spell_mechanic = {}
    for r in db2.load("SpellCategories", args.build, cache):
        if r.get("DifficultyID", "0") == "0":
            spell_mechanic.setdefault(r["SpellID"], set()).add(r["Mechanic"])
    effects = {}
    for r in db2.load("SpellEffect", args.build, cache):
        if r.get("DifficultyID", "0") == "0":
            effects.setdefault(r["SpellID"], []).append(r)

    def bleeds(spell: str) -> bool:
        own = BLEED in spell_mechanic.get(spell, set())
        for e in effects.get(spell, []):
            damaging = (
                e["Effect"] == EFFECT_APPLY_AURA and e["EffectAura"] == AURA_PERIODIC_DAMAGE
            ) or e["Effect"] == EFFECT_SCHOOL_DAMAGE
            if damaging and (own or e["EffectMechanic"] == BLEED):
                return True
        return False

    def bleed_via(spell: str, depth: int = 0):
        """The bleed `spell` deals: itself, or one it triggers."""
        if bleeds(spell):
            return spell
        if depth >= TRIGGER_DEPTH:
            return None
        triggers = [e["EffectTriggerSpell"] for e in effects.get(spell, [])]
        triggers.append(SCRIPTED_TRIGGERS.get(spell, ""))
        for trig in triggers:
            if trig not in ("", "0"):
                found = bleed_via(trig, depth + 1)
                if found:
                    return found
        return None

    rows = [
        r
        for r in db2.load("SkillLineAbility", args.build, cache)
        if int(r["Spell"]) < ERA_CUT
    ]
    class_lines = {cls: set() for cls in CLASS_BIT}
    for r in rows:
        for cls, bit in CLASS_BIT.items():
            if int(r["ClassMask"] or 0) & bit:
                class_lines[cls].add(r["SkillLine"])

    found = {}  # (owner, name) -> {spell: via}
    for r in rows:
        spell = r["Spell"]
        via = bleed_via(spell)
        if via is None:
            continue
        name = names.get(spell, "")
        mask = int(r["ClassMask"] or 0)
        owners = [
            cls
            for cls, bit in CLASS_BIT.items()
            if mask & bit or (mask == 0 and r["SkillLine"] in class_lines[cls])
        ]
        line = lines.get(r["SkillLine"], "")
        if line.startswith("Pet - "):
            owners.append(line)
        for owner in owners:
            found.setdefault((owner, name), {})[spell] = via

    out = csv.writer(sys.stdout, lineterminator="\n")
    print("# Generated by scripts/tbc_bleeds.py at build %s; do not edit by hand." % args.build)
    out.writerow(["class", "name", "spells", "via"])
    for (owner, name), spells in sorted(found.items()):
        ids = sorted(spells, key=int)
        vias = sorted({names.get(v, v) for s, v in spells.items() if v != s})
        out.writerow([owner, name, " ".join(ids), " ".join(vias)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
