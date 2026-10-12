#!/usr/bin/env python3
"""AS-287: which casts in the book can an interrupt take, by TBC's rules?

usage: 2026-10-12-as287-tbc-interruptibility.py [--build 2.5.6.69795]
                                                [--cache-dir .db2-cache/<build>]

An interrupt (Kick, Pummel, Spell Lock, Wind Shear) takes a cast only if the
cast's `SpellCategories.PreventionType` is 1 (Silence). A cast with
PreventionType 2 (Pacify) -- the physical shots -- has no cast bar an interrupt
can cut: its bar is grey. (TrinityCore's interrupt effect checks exactly this,
plus `SpellInterrupts.InterruptFlags` bit 0x8; both columns are printed.)

For every ability in `assets/config/abilities.ron` with a cast bar or a
channel, the script joins its name to the player-class ranks of the TBC client
(spells taught on a class skill line, `SkillLineAbility.ClassMask != 0`, below
the Season of Discovery cut), and prints the distinct (PreventionType,
InterruptFlags) values across ranks. It exits non-zero if a name splits across
ranks, if a name does not join, or if the RON's `uninterruptible` flag
disagrees with PreventionType 2 -- so a new cast-time ability is checked by
re-running it, not by reading a list.
"""
from __future__ import annotations

import argparse
import csv
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "scripts"))

import db2_spell_sweep as db2  # noqa: E402

TBC_BUILD = "2.5.6.69795"
ERA_CUT = 400000
SILENCE, PACIFY = "1", "2"
INTERRUPT_FLAG = 0x8


def ron_casts(path: str):
    """(ability, display name, cast_time, channel?, uninterruptible?) for every
    ability with a cast bar or a channel."""
    text = open(path, encoding="utf-8").read()
    out = []
    for m in re.finditer(r"\n        (\w+): \(\n(.*?)\n        \),", text, re.S):
        body = m.group(2)
        name = re.search(r'\bname: "([^"]+)"', body).group(1)
        cast = re.search(r"\bcast_time: ([\d.]+)", body)
        cast_time = float(cast.group(1)) if cast else 0.0
        channel = "channel_duration: Some" in body
        flagged = re.search(r"\buninterruptible: true", body) is not None
        if cast_time > 0.0 or channel:
            out.append((m.group(1), name, cast_time, channel, flagged))
    return out


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--build", default=TBC_BUILD)
    p.add_argument("--cache-dir", default=None)
    args = p.parse_args(argv)
    cache = args.cache_dir or os.path.join(ROOT, ".db2-cache", args.build)

    names = {r["ID"]: r["Name_lang"] for r in db2.load("SpellName", args.build, cache)}
    player = {
        r["Spell"]
        for r in db2.load("SkillLineAbility", args.build, cache)
        if r["ClassMask"] not in ("", "0") and int(r["Spell"]) < ERA_CUT
    }
    prevention = {}
    for r in db2.load("SpellCategories", args.build, cache):
        if r["SpellID"] in player:
            prevention.setdefault(r["SpellID"], set()).add(r["PreventionType"])
    flags = {}
    for r in db2.load("SpellInterrupts", args.build, cache):
        if r["SpellID"] in player:
            flags.setdefault(r["SpellID"], set()).add(int(r["InterruptFlags"]))

    by_name = {}
    for spell in player:
        by_name.setdefault(names.get(spell), set()).add(spell)

    ron = os.path.join(ROOT, "assets", "config", "abilities.ron")
    failures = []
    print(f"build {args.build}: PreventionType 1 = Silence (interruptible), 2 = Pacify (not)")
    print(f"{'ability':<22}{'cast':>6}  {'PreventionType':<16}{'InterruptFlags':<18}{'RON flag':<10}verdict")
    for ability, name, cast_time, channel, flagged in ron_casts(ron):
        spells = by_name.get(name, set())
        if not spells:
            failures.append(f"{ability}: no player spell named {name!r}")
            print(f"{ability:<22}{'':>6}  UNRESOLVED")
            continue
        prev = set().union(*(prevention.get(s, set()) for s in spells))
        intr = set().union(*(flags.get(s, set()) for s in spells))
        cast = "chan" if channel else f"{cast_time:.1f}"
        if len(prev) != 1:
            failures.append(f"{ability}: PreventionType splits across ranks: {sorted(prev)}")
        uninterruptible = prev == {PACIFY}
        verdict = "uninterruptible" if uninterruptible else "interruptible"
        if uninterruptible != flagged:
            failures.append(
                f"{ability}: TBC says {verdict}, abilities.ron says "
                f"uninterruptible: {str(flagged).lower()}"
            )
            verdict += "  MISMATCH"
        print(
            f"{ability:<22}{cast:>6}  {','.join(sorted(prev)):<16}"
            f"{','.join(hex(f) for f in sorted(intr)):<18}{str(flagged).lower():<10}{verdict}"
        )
    for f in failures:
        print("FAIL:", f)
    print("HELD" if not failures else f"{len(failures)} FAILURES")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
