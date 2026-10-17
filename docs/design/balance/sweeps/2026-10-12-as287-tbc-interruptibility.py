#!/usr/bin/env python3
"""AS-287 / AS-291: every ability's `prevention`, checked against TBC.

usage: 2026-10-12-as287-tbc-interruptibility.py [--build 2.5.6.69795]
                                                [--cache-dir .db2-cache/<build>]

`abilities.ron` gives every ability a `prevention` -- TBC's
`SpellCategories.PreventionType`: 1 Silence, 2 Pacify, 0 None. It decides two
things in the sim. A silence stops only a Silence-prevented ability, whether
it is being begun or is already being cast. The sim adds one condition of its
own: a silence stops only an ability that costs mana, so the `silence` column
reads "no" for a free one. An interrupt (Kick, Pummel, Spell Lock, Wind Shear)
takes only a Silence-prevented cast bar; a Pacify one (Aimed Shot) is grey. TrinityCore's silence handler, its `CheckCasterAuras` and its
interrupt effect all read this column; the interrupt also reads
`SpellInterrupts.InterruptFlags` bit 0x8, printed for a cast bar.

Each ability is joined by name to its ranks in the TBC client. A class ability
joins on its own class's skill lines (`SkillLineAbility.ClassMask` has the
class's bit, or is a talent with no mask on one of the class's lines), so the
Hunter's "Charge" that teaches a pet never reads as the Warrior's. A pet ability joins on its pet's skill line ("Pet - Felhunter"). The
spells are taken below the Season of Discovery cut, and a missing
`SpellCategories` row reads as PreventionType 0, the column's default.

An ability with no TBC spell under its name is in EXPLICIT, which names the
spells it is read from, the client build, and why. Those spells are read too,
so an explicit value is checked like any other, and an EXPLICIT ability that
starts resolving by name fails as stale.

The script exits non-zero if a name does not join, if a name splits across
ranks, if an explicit row is stale or reads a different value, or if a RON
`prevention` disagrees with the client -- so a new ability is checked by
re-running it, not by reading a list.
"""
from __future__ import annotations

import argparse
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "scripts"))

import db2_spell_sweep as db2  # noqa: E402

TBC_BUILD = "2.5.6.69795"
# Wrath of the Lich King Classic ("wow_classic_titan"): the first client that
# has Master's Call and Wind Shear at all.
WRATH_BUILD = "3.80.2.70315"
ERA_CUT = 400000
INTERRUPT_FLAG = 0x8
PREVENTION = {"0": "None", "1": "Silence", "2": "Pacify"}

# `ChrClasses` id -> its `ClassMask` bit.
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

# ability -> (build, spell ids, why). Every ability whose own name has no TBC
# spell; the value is still read from the client, from these spells.
EXPLICIT = {
    "SpiderWeb": (
        TBC_BUILD,
        ["4167"],
        "TBC Hunter pets have no Web; 4167 is the spider Web they learn in Wrath, "
        "and it is in the TBC client already",
    ),
    "BoarCharge": (
        TBC_BUILD,
        ["7371", "26177", "26178", "26179", "26201", "27685"],
        'the client names it "Charge", on the Pet - Boar skill line',
    ),
    "MastersCall": (
        WRATH_BUILD,
        ["53271"],
        "Master's Call arrives in Wrath; that client is the first to have it",
    ),
    "WindShear": (
        WRATH_BUILD,
        ["57994"],
        "Wind Shear arrives in Wrath (TBC's interrupt, Earth Shock, is also Silence)",
    ),
}


def ron_abilities(path: str):
    """(ability, name, class, pet, cast_time, channel?, mana_cost, prevention)
    for every ability in the book."""
    text = open(path, encoding="utf-8").read()
    out = []
    for m in re.finditer(r"\n        (\w+): \(\n(.*?)\n        \),", text, re.S):
        body = m.group(2)
        name = re.search(r'\bname: "([^"]+)"', body).group(1)
        cls = re.search(r"\bclass: (\w+)", body).group(1)
        pet = re.search(r"\bpet: Some\((\w+)\)", body)
        cast = re.search(r"\bcast_time: ([\d.]+)", body)
        mana = re.search(r"\bmana_cost: ([\d.]+)", body)
        prevention = re.search(r"\bprevention: (\w+)", body)
        out.append(
            (
                m.group(1),
                name,
                cls,
                pet.group(1) if pet else None,
                float(cast.group(1)) if cast else 0.0,
                "channel_duration: Some" in body,
                float(mana.group(1)) if mana else 0.0,
                prevention.group(1) if prevention else None,
            )
        )
    return out


class Client:
    """One build's PreventionType and InterruptFlags, by spell id."""

    def __init__(self, build: str, cache: str):
        self.prevention = {}
        for r in db2.load("SpellCategories", build, cache):
            if r.get("DifficultyID", "0") == "0":
                self.prevention.setdefault(r["SpellID"], set()).add(r["PreventionType"])
        self.flags = {}
        for r in db2.load("SpellInterrupts", build, cache):
            self.flags.setdefault(r["SpellID"], set()).add(int(r["InterruptFlags"]))
        self.names = {r["ID"]: r["Name_lang"] for r in db2.load("SpellName", build, cache)}

    def read(self, spells):
        prev = set().union(*(self.prevention.get(s, {"0"}) for s in spells))
        intr = set().union(*(self.flags.get(s, set()) for s in spells))
        return prev, intr


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--build", default=TBC_BUILD)
    p.add_argument("--cache-dir", default=None)
    args = p.parse_args(argv)

    def cache(build):
        if build == args.build and args.cache_dir:
            return args.cache_dir
        return os.path.join(ROOT, ".db2-cache", build)

    tbc = Client(args.build, cache(args.build))
    lines = {r["ID"]: r["DisplayName_lang"] for r in db2.load("SkillLine", args.build, cache(args.build))}
    rows = [
        r
        for r in db2.load("SkillLineAbility", args.build, cache(args.build))
        if int(r["Spell"]) < ERA_CUT
    ]
    # A class's skill lines: every line teaching a spell with the class's bit.
    # A row on one of them with no mask at all is a talent (Swiftmend).
    class_lines = {cls: set() for cls in CLASS_BIT}
    for r in rows:
        for cls, bit in CLASS_BIT.items():
            if int(r["ClassMask"] or 0) & bit:
                class_lines[cls].add(r["SkillLine"])
    by_class = {}  # (class, name) -> spells
    by_pet = {}  # (pet skill line name, name) -> spells
    for r in rows:
        name = tbc.names.get(r["Spell"])
        mask = int(r["ClassMask"] or 0)
        for cls, bit in CLASS_BIT.items():
            if mask & bit or (mask == 0 and r["SkillLine"] in class_lines[cls]):
                by_class.setdefault((cls, name), set()).add(r["Spell"])
        line = lines.get(r["SkillLine"], "")
        if line.startswith("Pet - "):
            by_pet.setdefault((line, name), set()).add(r["Spell"])
    clients = {args.build: tbc}

    ron = os.path.join(ROOT, "assets", "config", "abilities.ron")
    failures = []
    print(f"build {args.build}: PreventionType 1 = Silence, 2 = Pacify, 0 = None")
    print(
        f"{'ability':<22}{'cast':>5}  {'TBC':<9}{'RON':<9}{'InterruptFlags':<16}"
        f"{'silence':<9}{'interrupt':<11}source"
    )
    for ability, name, cls, pet, cast_time, channel, mana, ron_value in ron_abilities(ron):
        if pet:
            spells = set()
            for (line, n), s in by_pet.items():
                if n == name and line.startswith(f"Pet - {pet}"):
                    spells |= s
        else:
            spells = by_class.get((cls, name), set())
        source = "name"
        client = tbc
        if ability in EXPLICIT:
            build, ids, why = EXPLICIT[ability]
            if spells:
                failures.append(f"{ability}: EXPLICIT is stale, {name!r} now joins by name")
            if build not in clients:
                clients[build] = Client(build, cache(build))
            client = clients[build]
            missing = [s for s in ids if s not in client.names]
            if missing:
                failures.append(f"{ability}: EXPLICIT spells {missing} are not in build {build}")
            spells = set(ids)
            source = f"{build} {','.join(ids)}: {why}"
        if not spells:
            failures.append(f"{ability}: no {pet or cls} spell named {name!r}")
            print(f"{ability:<22}{'':>5}  UNRESOLVED")
            continue
        prev, intr = client.read(spells)
        tbc_value = "/".join(PREVENTION.get(v, v) for v in sorted(prev))
        if len(prev) != 1:
            failures.append(f"{ability}: PreventionType splits across ranks: {sorted(prev)}")
        if ron_value is None:
            failures.append(f"{ability}: abilities.ron declares no `prevention`")
        elif ron_value != tbc_value:
            failures.append(f"{ability}: TBC says {tbc_value}, abilities.ron says {ron_value}")
            source = "MISMATCH  " + source
        cast_bar = cast_time > 0.0 or channel
        cast = "chan" if channel else (f"{cast_time:.1f}" if cast_time > 0.0 else "")
        # The sim's own condition on top of the column: a free ability is not
        # silenced (`AbilityConfig::silence_stops`).
        silence = "stops" if tbc_value == "Silence" and mana > 0.0 else "no"
        interrupt = ("takes" if tbc_value == "Silence" else "grey") if cast_bar else ""
        flags = ",".join(hex(f) for f in sorted(intr)) if cast_bar else ""
        print(
            f"{ability:<22}{cast:>5}  {tbc_value:<9}{str(ron_value):<9}{flags:<16}"
            f"{silence:<9}{interrupt:<11}{source}"
        )
    for f in failures:
        print("FAIL:", f)
    print("HELD" if not failures else f"{len(failures)} FAILURES")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
