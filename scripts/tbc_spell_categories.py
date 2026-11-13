#!/usr/bin/env python3
"""AS-478: every TBC class or pet spell's `SpellCategories` values, by name.

usage: scripts/tbc_spell_categories.py [--cache-root .db2-cache]
                                       > tests/data/tbc_spell_categories.csv

`abilities.ron` gives every ability a `prevention`: TBC's
`SpellCategories.PreventionType`, 1 Silence, 2 Pacify, 0 None.
`tests/prevention_tbc_audit.rs` holds every ability's value to this file,
joined by the ability's class (a pet ability's "Pet - <pet>") and its RON
`name`, so an ability cannot ship with a value TBC does not give it, or with
no client spell behind it at all.

The inventory is every spell on a class's skill lines (`SkillLineAbility`,
joined by `ClassMask`, a talent with no mask by its line) or a pet's ("Pet -
<family>") -- the lines of `SkillLine.CategoryID` 7 -- below the Season of
Discovery cut, at `DifficultyID` 0, at TBC build 2.5.6.69795. Rows are (owner, name), every rank of the name together. A
spell with no `SpellCategories` row reads as PreventionType 0, the column's
default. A name whose ranks disagree is written with both values
("None/Silence"), and the audit fails on any ability that joins it.

Two tables say what the inventory cannot:

* EXPLICIT names the client spells an ability is read from when TBC has no
  spell under its own (owner, name): a pet ability the client names
  differently, or a spell that arrives in a later client. Its row is written
  under the sim's own (owner, name) with source `explicit`. An EXPLICIT key
  that the inventory starts joining by name is stale, and fails.
* EXCLUDED drops a spell that sits on a class's skill line under the same name
  as the spell the class begins, without being it (a hidden companion effect).
  An EXCLUDED spell the inventory no longer holds is stale, and fails.

Any failure prints to stderr and exits 1 before a row is written, so the
fixture is never regenerated half-checked.
"""
from __future__ import annotations

import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
sys.path.insert(0, HERE)

import db2_spell_sweep as db2  # noqa: E402

TBC_BUILD = "2.5.6.69795"
# Wrath of the Lich King Classic ("wow_classic_titan"): the first client that
# has Master's Call, Wind Shear, Guardian Spirit and Sacred Shield at all.
WRATH_BUILD = "3.80.2.70315"
ERA_CUT = 400000
PREVENTION = {"0": "None", "1": "Silence", "2": "Pacify"}
HEADER = "owner,source,build,spells,prevention,name"
# `SkillLine.CategoryID` of a class's or a pet's skill lines. The others
# (professions, weapon skills, racials) teach spells every class can have.
CLASS_SKILL_CATEGORY = "7"

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

# (owner, the sim's name) -> (build, spell ids, why). Every ability TBC has no
# spell for under its own name; the value is still read from the client.
EXPLICIT = {
    ("Pet - Spider", "Web"): (
        TBC_BUILD,
        ["4167"],
        "TBC Hunter pets have no Web; 4167 is the spider Web they learn in Wrath, "
        "and it is in the TBC client already",
    ),
    ("Pet - Boar", "Boar Charge"): (
        TBC_BUILD,
        ["7371", "26177", "26178", "26179", "26201", "27685"],
        'the client names it "Charge", on the Pet - Boar skill line',
    ),
    ("Pet - Bird", "Master's Call"): (
        WRATH_BUILD,
        ["53271"],
        "Master's Call arrives in Wrath. 53271 is the spell the Hunter begins; "
        "its triggered effects (54216, 62305) read Pacify, but prevention gates "
        "the spell begun",
    ),
    ("Shaman", "Wind Shear"): (
        WRATH_BUILD,
        ["57994"],
        "Wind Shear arrives in Wrath (TBC's interrupt, Earth Shock, is also Silence)",
    ),
    ("Priest", "Guardian Spirit"): (
        WRATH_BUILD,
        ["47788"],
        "Guardian Spirit arrives in Wrath. 47788 is the spell the Priest begins; "
        "48153, the heal it triggers on the save, is not",
    ),
    ("Paladin", "Sacred Shield"): (
        WRATH_BUILD,
        ["53601"],
        "Sacred Shield arrives in Wrath. 53601 is the spell the Paladin begins; "
        "58597, the absorb it triggers (PreventionType 0), is not",
    ),
}

# spell id -> why. On a class's skill line under the name of the spell the
# class begins, but not that spell.
EXCLUDED = {
    "44416": "Pain Suppression's hidden threat reduction (Effect 125, "
    "Attributes 0x80), which 33206 triggers; the Priest begins 33206",
}


class Client:
    """One build's PreventionType and spell names, by spell id."""

    def __init__(self, build: str, cache: str):
        self.prevention = {}
        for r in db2.load("SpellCategories", build, cache):
            if r.get("DifficultyID", "0") == "0":
                self.prevention.setdefault(r["SpellID"], set()).add(r["PreventionType"])
        self.names = {r["ID"]: r["Name_lang"] for r in db2.load("SpellName", build, cache)}

    def prevention_of(self, spells) -> str:
        values = set().union(*(self.prevention.get(s, {"0"}) for s in spells))
        return "/".join(sorted(PREVENTION.get(v, v) for v in values))


def inventory(build: str, cache: str, names: dict) -> dict:
    """(owner, name) -> spell ids, for every class and pet spell below the cut."""
    lines = {}
    for r in db2.load("SkillLine", build, cache):
        if r["CategoryID"] == CLASS_SKILL_CATEGORY:
            lines[r["ID"]] = r["DisplayName_lang"]
    rows = [
        r
        for r in db2.load("SkillLineAbility", build, cache)
        if int(r["Spell"]) < ERA_CUT and r["SkillLine"] in lines
    ]
    # A class's skill lines: every line teaching a spell with the class's bit.
    # A row on one of them with no mask at all is a talent (Swiftmend).
    class_lines = {cls: set() for cls in CLASS_BIT}
    for r in rows:
        for cls, bit in CLASS_BIT.items():
            if int(r["ClassMask"] or 0) & bit:
                class_lines[cls].add(r["SkillLine"])
    found = {}
    for r in rows:
        name = names.get(r["Spell"])
        if not name:
            continue
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
            found.setdefault((owner, name), set()).add(r["Spell"])
    return found


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument(
        "--cache-root",
        default=os.path.join(ROOT, ".db2-cache"),
        help="holds one directory per client build (default: .db2-cache)",
    )
    args = p.parse_args(argv)

    clients = {}

    def client(build):
        if build not in clients:
            clients[build] = Client(build, os.path.join(args.cache_root, build))
        return clients[build]

    tbc = client(TBC_BUILD)
    found = inventory(TBC_BUILD, os.path.join(args.cache_root, TBC_BUILD), tbc.names)
    failures = []

    held = set().union(*found.values()) if found else set()
    for spell, why in EXCLUDED.items():
        if spell not in held:
            failures.append(f"EXCLUDED {spell} is stale: no class or pet row holds it ({why})")
    rows = {}
    for key, spells in found.items():
        kept = spells - set(EXCLUDED)
        if kept:
            rows[key] = ("name", TBC_BUILD, kept)
    for key, (build, ids, why) in EXPLICIT.items():
        if key in found:
            failures.append(
                f"EXPLICIT {key} is stale: TBC now has {key[1]!r} for {key[0]} by name"
            )
            continue
        missing = [s for s in ids if s not in client(build).names]
        if missing:
            failures.append(f"EXPLICIT {key}: spells {missing} are not in build {build}")
            continue
        rows[key] = ("explicit", build, set(ids))

    for (owner, name) in rows:
        if "," in owner or "\n" in name or "\n" in owner:
            failures.append(f"{(owner, name)}: an owner holds a comma, or a field a newline")
    if failures:
        for f in failures:
            sys.stderr.write(f"FAIL: {f}\n")
        return 1

    print(f"# Generated by scripts/tbc_spell_categories.py from builds {TBC_BUILD} "
          f"and {WRATH_BUILD}; do not edit by hand.")
    print("# `name` is the last column and is read to the end of the line, so it "
          "may hold a comma or a quote.")
    print(HEADER)
    for (owner, name), (source, build, spells) in sorted(rows.items()):
        ids = " ".join(sorted(spells, key=int))
        prevention = client(build).prevention_of(spells)
        print(f"{owner},{source},{build},{ids},{prevention},{name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
