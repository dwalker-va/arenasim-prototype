#!/usr/bin/env python3
"""Inventory-driven WoW Classic DB2 spell-visual sweep.

The shared join script for client-data research cards (AS-9, AS-15, AS-19,
AS-37, ...). Every one of those sessions re-wrote an ad-hoc join in a
scratchpad, and twice the result was a table with a row that had never
actually been joined:

  * AS-15's `as15_chain.py` hardcoded three spells; the remaining rows of its
    kit table were filled in from an assumed shadow-school default. Curse of
    Weakness was wrong for months.
  * AS-37 round 1 corrected that row and then asserted a completeness claim
    over a hand-curated 41-entry spell-ID list, which provably omitted era
    Warlock spells (Curse of Exhaustion, Soul Link, the stone-creation
    family).

Both failures are the same hazard: a CURATED LIST cannot be audited for what
is missing from it. This script exists to make that failure structurally
hard, via four load-bearing properties:

  1. The spell set is an INVENTORY, derived from `SkillLineAbility` for the
     skill lines you name. There is no spell-ID list to forget an entry from.
  2. It prints the three counts that make an omission visible -- inventory
     names, names with no `SpellXSpellVisual` row, and joined name x visual
     rows -- as a checkable sum (the AS-37 Warlock book: 114 = 42 + 72).
  3. The build-era cut is an EXPLICIT parameter (`--era-cut`). This build
     carries Season of Discovery runes on the same skill lines; the cut that
     separates them is an assumption that can rot, so it is stated and
     overridable, and the script proves it is a clean partition.
  4. It ASSERTS that every inventory name resolves to exactly one
     `SpellVisual` at Probability 1, and exits non-zero when one splits. Rank
     collapse is the assumption every one of these research docs rests on;
     failing loudly when it breaks is worth more than the join itself.

Resolution is INVENTORY-SCOPED, never name-scoped globally. "Immolate" has
five visuals across the whole `SpellName` table, but all eight *Warlock-line*
Immolate ranks resolve to visual 46. Scoping by skill line is what makes
property 4 a real signal instead of a false alarm.

Every output row carries a provenance marker, so an unjoinable row reads as
UNRESOLVED in the output instead of being quietly absent:

  RESOLVED    name -> exactly one SpellVisual, all rows Probability 1
  NO-VISUAL   name has SpellIDs but no JOINABLE `SpellXSpellVisual` row --
              either no row at all, or only rows pointing at SpellVisualID 0,
              which is the client's own "no visual" (passive talents, *Effect*
              helper spells). Nothing to join either way.
  SPLIT       name resolves to >1 SpellVisual -- rank collapse BROKEN (fatal)
  LOW-PROB    a `SpellXSpellVisual` row with Probability != 1 (fatal)
  UNRESOLVED  a real gap in the data rather than an omission, in one of two
              severities -- the marker is shared, the consequence is not:
                * a SpellID with no `SpellName` row: REPORTED, not fatal. The
                  name is unknown; it gets its own section in the output.
                  Property 4 still judges the ROW: splits, off-Probability
                  rows and dangling visual ids are selected by SHAPE, not by
                  this marker, so an unnamed SpellID that also splits is still
                  fatal. (The marker is a ladder and this branch sits at the
                  top of it, so a marker-string check would have missed that.)
                * a SpellVisualID absent from `SpellVisual`: FATAL. The join
                  produced a visual id that does not exist, so property 4
                  cannot vouch for that row. `--allow-split` does NOT downgrade
                  this one -- see the flag's help.

Usage
-----
    # The AS-37 Warlock book: Demonology / Affliction / Destruction.
    scripts/db2_spell_sweep.py --skill-line 354 355 593

    # ...with the caster-side body-animation chain (the AS-37 section 5 join).
    scripts/db2_spell_sweep.py --skill-line 354 355 593 --caster-anims

    # Discover skill-line ids without hardcoding them anywhere.
    scripts/db2_spell_sweep.py --list-skill-lines | grep -i affliction

    # Spot-check one spell -- still inventory-scoped, so it cannot pick up
    # same-named spells from other classes.
    scripts/db2_spell_sweep.py --skill-line 354 355 593 --name Immolate --events

Data source: wago.tools DB2 CSV exports, cached under `--cache-dir`. Fetched
with `curl` and a browser User-Agent -- Python's `urllib` gets a 403 from
wago.tools, which has cost more than one session an hour.
"""

from __future__ import annotations

import argparse
import collections
import csv
import os
import subprocess
import sys

# The build every prior client-data session used, cmp-verified byte-identical
# across four separate fetches. Overridable; it is a premise, not a constant.
DEFAULT_BUILD = "1.15.9.69547"

# Season of Discovery runes ride the same skill lines as the era book in this
# build. AS-37 round 2 verified this is a CLEAN partition at 1.15.9.69547 --
# max era-side SpellID 28610, min cut-side 403501, nothing between. The script
# re-proves that gap on every run (see `report_era_cut`), because a threshold
# that silently starts splitting a family is exactly the premise that rots.
DEFAULT_ERA_CUT = 400000

WAGO_CSV = "https://wago.tools/db2/{table}/csv?build={build}"
USER_AGENT = "curl/8.7.1"

# Caster-side body animations, from AS-11's mapping. `AnimationData` at
# 1.15.9.69547 carries no name column, so only these four can be named.
ANIM_NAMES = {
    51: "ReadySpellDirected",
    52: "ReadySpellOmni",
    53: "SpellCastDirected",
    54: "SpellCastOmni",
}

# SpellVisualEvent (StartEvent, EndEvent) pairs, vocabulary established across
# AS-9 and AS-15. TargetType 1 is the caster, 2 the victim.
EVENT_PAIRS = {
    ("1", "2"): "precast loop",
    ("3", "13"): "cast one-shot",
    ("6", "13"): "impact",
    ("7", "8"): "aura state",
    ("9", "10"): "area",
    ("11", "12"): "channel target",
    ("4", "5"): "positioner",
}

PRECAST = ("1", "2")
CAST = ("3", "13")

# SpellVisualKitEffect.EffectType 6 is the body animation (Effect joins
# SpellVisualAnim.ID). EffectType 2 is the attached model, which this script
# does not follow -- that chain continues into SpellVisualKitModelAttach and
# CASC M2 parsing, and lives in the per-card scripts.
EFFECT_ANIM = "6"


# --------------------------------------------------------------------------
# fetching / loading
# --------------------------------------------------------------------------


def fetch(table: str, build: str, cache_dir: str, refresh: bool = False) -> str:
    """Return the local path to `table`.csv, downloading it if needed."""
    os.makedirs(cache_dir, exist_ok=True)
    path = os.path.join(cache_dir, table + ".csv")
    if os.path.exists(path) and os.path.getsize(path) > 0 and not refresh:
        return path
    url = WAGO_CSV.format(table=table, build=build)
    sys.stderr.write("fetching %s ... " % url)
    sys.stderr.flush()
    tmp = path + ".part"
    # curl, not urllib: wago.tools 403s a default Python User-Agent.
    proc = subprocess.run(
        ["curl", "-sS", "-f", "-A", USER_AGENT, "-o", tmp, url],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        if os.path.exists(tmp):
            os.remove(tmp)
        sys.stderr.write("FAILED\n")
        raise SystemExit(
            "could not fetch %s at build %s: %s"
            % (table, build, proc.stderr.strip() or "curl exit %d" % proc.returncode)
        )
    os.replace(tmp, path)
    sys.stderr.write("%d bytes\n" % os.path.getsize(path))
    return path


def load(table: str, build: str, cache_dir: str, refresh: bool = False) -> list[dict]:
    with open(fetch(table, build, cache_dir, refresh), newline="", encoding="utf-8") as f:
        return list(csv.DictReader(f))


def index(rows: list[dict], key: str) -> dict[int, dict]:
    return {int(r[key]): r for r in rows}


def group(rows: list[dict], key: str) -> dict[int, list[dict]]:
    out: dict[int, list[dict]] = collections.defaultdict(list)
    for r in rows:
        out[int(r[key])].append(r)
    return out


# --------------------------------------------------------------------------
# the inventory
# --------------------------------------------------------------------------


class Sweep:
    def __init__(self, args):
        self.args = args
        self.build = args.build
        self.cache = args.cache_dir
        self._refresh = args.refresh

        self.name_of = {int(r["ID"]): r["Name_lang"] for r in self.table("SpellName")}
        self.xsv = group(self.table("SpellXSpellVisual"), "SpellID")
        self.visual_ids = {int(r["ID"]) for r in self.table("SpellVisual")}

        self._events = None
        self._kit_effects = None
        self._anims = None

    def table(self, name: str) -> list[dict]:
        """Load one DB2 table as a list of dict rows (fetching/caching it)."""
        return load(name, self.build, self.cache, self._refresh)

    # lazily loaded -- only the chains that were asked for cost a fetch
    @property
    def events(self):
        if self._events is None:
            self._events = group(self.table("SpellVisualEvent"), "SpellVisualID")
        return self._events

    @property
    def kit_effects(self):
        if self._kit_effects is None:
            self._kit_effects = group(self.table("SpellVisualKitEffect"), "ParentSpellVisualKitID")
        return self._kit_effects

    @property
    def anims(self):
        if self._anims is None:
            self._anims = index(self.table("SpellVisualAnim"), "ID")
        return self._anims

    # ---------------------------------------------------------------- inventory

    def inventory(self, skill_lines: set[str], era_cut: int):
        """Every spell on the named skill lines, bucketed name -> {SpellID}.

        Returns (era, cut, unnamed) where `era` and `cut` are name -> id-set
        maps either side of the era cut, and `unnamed` lists SpellIDs with no
        `SpellName` row (a real gap; reported, never dropped).
        """
        era: dict[str, set[int]] = collections.defaultdict(set)
        cut: dict[str, set[int]] = collections.defaultdict(set)
        unnamed: list[int] = []
        for r in self.table("SkillLineAbility"):
            if r["SkillLine"] not in skill_lines:
                continue
            sid = int(r["Spell"])
            name = self.name_of.get(sid)
            if name is None:
                unnamed.append(sid)
                # Keep it in the inventory under a marker name so it is
                # counted and shown as UNRESOLVED rather than disappearing.
                name = "<no SpellName row for id %d>" % sid
            bucket = cut if (era_cut and sid >= era_cut) else era
            bucket[name].add(sid)
        return era, cut, sorted(set(unnamed))

    # ---------------------------------------------------------------- resolving

    def resolve(self, name: str, spell_ids: set[int]):
        """Resolve one inventory name to its SpellVisual(s).

        Returns a dict with the provenance marker, the visuals found (as
        visual_id -> sorted contributing SpellIDs), any off-probability rows,
        and the SpellIDs that carried no `SpellXSpellVisual` row at all.
        """
        visuals: dict[int, list[int]] = collections.defaultdict(list)
        low_prob: list[tuple[int, int, str]] = []
        bare_ids: list[int] = []
        dangling: list[int] = []
        for sid in sorted(spell_ids):
            rows = self.xsv.get(sid, [])
            if not rows:
                bare_ids.append(sid)
                continue
            for r in rows:
                vid = int(r["SpellVisualID"])
                if vid == 0:
                    # A row that points at visual 0 is "no visual", not a join.
                    continue
                visuals[vid].append(sid)
                prob = r["Probability"]
                try:
                    is_certain = float(prob) == 1.0
                except ValueError:
                    # An unparseable Probability is itself a gap worth seeing.
                    is_certain = False
                if not is_certain:
                    low_prob.append((sid, vid, prob or "<empty>"))
                if vid not in self.visual_ids:
                    dangling.append(vid)

        # A SpellID the inventory carries but `SpellName` does not know is a
        # real gap in the data, and outranks every other marker.
        if any(sid not in self.name_of for sid in spell_ids):
            marker = "UNRESOLVED"
        elif dangling:
            marker = "UNRESOLVED"
        elif not visuals:
            marker = "NO-VISUAL"
        elif len(visuals) > 1:
            marker = "SPLIT"
        elif low_prob:
            marker = "LOW-PROB"
        else:
            marker = "RESOLVED"

        return {
            "name": name,
            "spell_ids": sorted(spell_ids),
            "visuals": {v: sorted(set(s)) for v, s in sorted(visuals.items())},
            "low_prob": low_prob,
            "bare_ids": bare_ids,
            "dangling": sorted(set(dangling)),
            "marker": marker,
        }

    # ------------------------------------------------------------------ chains

    def loop_anims(self, kit: int | None):
        """SpellVisualKit -> EffectType 6 -> SpellVisualAnim.LoopAnimID."""
        if kit is None:
            return None
        out = []
        for e in self.kit_effects.get(kit, []):
            if e["EffectType"] != EFFECT_ANIM:
                continue
            anim = self.anims.get(int(e["Effect"]))
            out.append(int(anim["LoopAnimID"]) if anim else None)
        return out

    def caster_pair(self, visual: int):
        """The caster-side (1,2) precast and (3,13) cast kit ids for a visual."""
        pre = cast = None
        for e in self.events.get(visual, []):
            if e["TargetType"] != "1":
                continue
            se = (e["StartEvent"], e["EndEvent"])
            if se == PRECAST:
                pre = int(e["SpellVisualKitID"])
            elif se == CAST:
                cast = int(e["SpellVisualKitID"])
        return pre, cast


# --------------------------------------------------------------------------
# formatting
# --------------------------------------------------------------------------


def abbrev(xs, n=8):
    xs = list(xs)
    if len(xs) <= n:
        return str(xs)
    return "%s...(+%d)" % (xs[:n], len(xs) - n)


def fmt_kit(sweep: Sweep, kit):
    if kit is None:
        return "none"
    anims = sweep.loop_anims(kit)
    if not anims:
        return "%d->no-anim" % kit
    return "%d->%s" % (
        kit,
        "/".join("%s %s" % (a, ANIM_NAMES.get(a, "?")) for a in anims),
    )


def report_era_cut(era, cut, era_cut):
    """Prove the era cut is a clean partition rather than asserting it."""
    print("### build-era cut")
    if not era_cut:
        print("  DISABLED (--era-cut 0): every skill-line spell is in the inventory.")
        print()
        return
    era_ids = [i for ids in era.values() for i in ids]
    cut_ids = [i for ids in cut.values() for i in ids]
    print("  cut at SpellID >= %d  (an explicit parameter, not a constant)" % era_cut)
    # There is no partition to prove when the inventory sits entirely on one
    # side of the cut, and computing one tracebacks on the empty side. Say
    # which side it is: an all-cut-side inventory leaves NOTHING to sweep,
    # which is a very different result from a clean era book.
    if not cut_ids:
        print("  nothing above the cut -- the inventory is entirely era-side, unaffected.")
        print()
        return
    if not era_ids:
        print(
            "  EVERY inventory spell is above the cut (%d name(s), ids %d..%d)."
            % (len(cut), min(cut_ids), max(cut_ids))
        )
        print("  The era-side inventory is EMPTY -- this sweep has nothing to join.")
        print(
            "  Nothing below is a statement about this skill line's era book. "
            "If you\n  expected era spells here, the cut or the skill-line id is wrong."
        )
        print()
        return
    hi, lo = max(era_ids), min(cut_ids)
    print(
        "  partition: max era-side id %d | min cut-side id %d | gap %d"
        % (hi, lo, lo - hi)
    )
    print(
        "  %d name(s) excluded above the cut: %s"
        % (len(cut), ", ".join(sorted(cut)))
    )
    if hi >= era_cut or lo < era_cut:
        print("  !! the cut does not partition cleanly -- re-verify it")
    # A name on BOTH sides is the cut doing real work (a rune that reuses an
    # era spell's name) -- and the shape that would make the cut start eating
    # era rows if the threshold ever drifted. Name it rather than let it hide
    # inside the excluded list, where it reads as "this spell was excluded".
    straddle = sorted(set(era) & set(cut))
    if straddle:
        print("  name(s) on BOTH sides of the cut -- era ranks KEPT, cut ids dropped:")
        for nm in straddle:
            print(
                "    %-28s era %s | cut %s"
                % (nm, sorted(era[nm]), sorted(cut[nm]))
            )
    print()


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Inventory-driven DB2 spell-visual sweep (see module docstring).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--skill-line",
        nargs="+",
        metavar="ID",
        help="SkillLine ids defining the inventory (Warlock: 354 355 593). "
        "The spell set is derived from these -- there is no spell-ID list.",
    )
    p.add_argument(
        "--list-skill-lines",
        action="store_true",
        help="print every SkillLine id and display name, then exit "
        "(so skill-line ids never have to be hardcoded either)",
    )
    p.add_argument(
        "--era-cut",
        type=int,
        default=DEFAULT_ERA_CUT,
        metavar="ID",
        help="exclude SpellIDs >= this as non-era (Season of Discovery runes "
        "ride the same skill lines in this build). 0 disables. Default %d." % DEFAULT_ERA_CUT,
    )
    p.add_argument("--build", default=DEFAULT_BUILD, help="wago.tools build (default %(default)s)")
    p.add_argument(
        "--cache-dir",
        default=os.environ.get("DB2_CACHE_DIR", ".db2-cache"),
        help="where fetched CSVs are cached; a per-build subdirectory is used "
        "(default %(default)s, or $DB2_CACHE_DIR)",
    )
    p.add_argument("--refresh", action="store_true", help="re-fetch CSVs even when cached")
    p.add_argument(
        "--name",
        nargs="+",
        metavar="SUBSTR",
        help="show only inventory names matching these (case-insensitive "
        "substring). Still INVENTORY-SCOPED -- it filters the skill-line "
        "inventory, it does not search SpellName globally.",
    )
    p.add_argument(
        "--caster-anims",
        action="store_true",
        help="add the caster-side chain: (1,2)/(3,13) kits -> EffectType 6 -> "
        "SpellVisualAnim.LoopAnimID (the AS-37 section 5 join)",
    )
    p.add_argument(
        "--events",
        action="store_true",
        help="dump every SpellVisualEvent row per resolved visual",
    )
    p.add_argument(
        "--group-by-anims",
        action="store_true",
        help="with --caster-anims, group the inventory by (precast, cast) "
        "animation signature instead of listing per name",
    )
    p.add_argument(
        "--allow-split",
        action="store_true",
        help="downgrade the property-4 assertion (exactly one SpellVisual per "
        "name, Probability 1) from fatal to a warning. Use only when you have "
        "read the split rows and know why they split. It does NOT cover a "
        "dangling SpellVisualID -- a visual id with no `SpellVisual` row has "
        "no rows to read, so it stays fatal.",
    )
    args = p.parse_args(argv)

    if args.group_by_anims and not args.caster_anims:
        p.error("--group-by-anims requires --caster-anims")

    args.cache_dir = os.path.join(args.cache_dir, args.build)

    if args.list_skill_lines:
        sweep_rows = load("SkillLine", args.build, args.cache_dir, args.refresh)
        for r in sorted(sweep_rows, key=lambda r: int(r["ID"])):
            print("%6s  %s" % (r["ID"], r["DisplayName_lang"]))
        return 0

    if not args.skill_line:
        p.error("--skill-line is required (or use --list-skill-lines to find ids)")

    sweep = Sweep(args)
    lines = set(args.skill_line)

    # Self-check: print the skill lines back, resolved by name from SkillLine,
    # so a typo'd id is visible in the output rather than silently sweeping a
    # different class's book.
    skill_names = {r["ID"]: r["DisplayName_lang"] for r in sweep.table("SkillLine")}
    print("=" * 78)
    print("DB2 spell-visual sweep -- build %s" % args.build)
    print("=" * 78)
    print()
    print("### skill lines (the inventory source -- NOT a spell-ID list)")
    for sl in sorted(lines, key=lambda s: int(s)):
        print("  %-6s %s" % (sl, skill_names.get(sl, "!! no SkillLine row -- check this id")))
    print()

    era, cut, unnamed = sweep.inventory(lines, args.era_cut)
    if not era and not cut:
        # An empty inventory proves nothing, and reads exactly like a clean
        # sweep. Fail instead -- a mistyped skill-line id is the likely cause.
        raise SystemExit(
            "no SkillLineAbility rows for skill line(s) %s -- check the ids "
            "with --list-skill-lines" % ", ".join(sorted(lines, key=int))
        )
    report_era_cut(era, cut, args.era_cut)

    selected = sorted(era)
    if args.name:
        needles = [n.lower() for n in args.name]
        selected = [n for n in selected if any(x in n.lower() for x in needles)]

    results = [sweep.resolve(n, era[n]) for n in selected]

    # ---- property 2: the three counts, as a checkable sum ----------------
    # Partition on whether a visual was actually found, NOT on the marker
    # string -- an UNRESOLVED name must still land in exactly one bucket, or
    # the sum silently stops adding up (which is the one thing this block
    # exists to prevent).
    no_visual = [r for r in results if not r["visuals"]]
    with_visual = [r for r in results if r["visuals"]]
    joined_rows = sum(len(r["visuals"]) for r in results)
    print("### counts (property 2 -- an omission has to show up here)")
    print("  inventory names .......................... %d" % len(results))
    print("  names with NO SpellXSpellVisual row ...... %d" % len(no_visual))
    print("  names with at least one visual ........... %d" % len(with_visual))
    print("  joined name x visual rows ................ %d" % joined_rows)
    print(
        "  check: %d = %d + %d  %s"
        % (
            len(results),
            len(no_visual),
            len(with_visual),
            "OK" if len(results) == len(no_visual) + len(with_visual) else "!! MISMATCH",
        )
    )
    if joined_rows != len(with_visual):
        print(
            "  note: %d joined rows > %d names -- %d name(s) resolve to more than "
            "one visual; see the property-4 block."
            % (joined_rows, len(with_visual), joined_rows - len(with_visual))
        )
    if args.name:
        print("  (filtered by --name; counts are over the filtered subset)")
    print()

    # ---- the join, with a provenance marker on every row -----------------
    if args.group_by_anims and args.caster_anims:
        groups: dict[tuple, list[str]] = collections.defaultdict(list)
        for r in results:
            if not r["visuals"]:
                continue
            for vid in r["visuals"]:
                pre, cast = sweep.caster_pair(vid)
                key = (
                    tuple(sweep.loop_anims(pre) or ()) if pre is not None else None,
                    tuple(sweep.loop_anims(cast) or ()) if cast is not None else None,
                )
                groups[key].append("%s (vis %d: kits %s/%s)" % (r["name"], vid, pre, cast))
        print("### grouped by (precast anim, cast anim) -- %d signatures" % len(groups))
        for key in sorted(groups, key=lambda k: (-len(groups[k]), str(k))):
            print("\n--- precast %s / cast %s   (%d)" % (key[0], key[1], len(groups[key])))
            for s in groups[key]:
                print("      %s" % s)
        print()
    else:
        print("### join (every row carries a provenance marker)")
        for r in results:
            print(
                "%-11s %-34s ids %s"
                % (r["marker"], r["name"], abbrev(r["spell_ids"]))
            )
            for sid, vid, prob in r["low_prob"]:
                print("            !! spell %d -> visual %d at Probability %s" % (sid, vid, prob))
            for vid in r["dangling"]:
                print("            !! visual %d has no SpellVisual row" % vid)
            if r["bare_ids"] and r["visuals"]:
                print(
                    "            note: ranks with no SpellXSpellVisual row: %s"
                    % abbrev(r["bare_ids"])
                )
            for vid, sids in r["visuals"].items():
                line = "      visual %-7d from spells %s" % (vid, abbrev(sids))
                if args.caster_anims:
                    pre, cast = sweep.caster_pair(vid)
                    line += "\n            precast %-28s cast %s" % (
                        fmt_kit(sweep, pre),
                        fmt_kit(sweep, cast),
                    )
                print(line)
                if args.events:
                    for e in sorted(
                        sweep.events.get(vid, []),
                        key=lambda e: (e["TargetType"], e["StartEvent"], e["EndEvent"]),
                    ):
                        pair = (e["StartEvent"], e["EndEvent"])
                        print(
                            "            event (%s,%s) %-15s target %s kit %s"
                            % (
                                pair[0],
                                pair[1],
                                EVENT_PAIRS.get(pair, "?"),
                                e["TargetType"],
                                e["SpellVisualKitID"],
                            )
                        )
        print()

    if no_visual:
        print("### names with no visual (nothing to join -- passives, *Effect* helpers)")
        for r in no_visual:
            print("  %-11s %-36s %s" % (r["marker"], r["name"], abbrev(r["spell_ids"])))
        print()

    if unnamed:
        print("### UNRESOLVED: skill-line SpellIDs with no SpellName row")
        print("  %s" % abbrev(unnamed, 20))
        print()

    # ---- property 4: the assertion --------------------------------------
    # Select fatals BY SHAPE, never by marker string. The marker is a LADDER
    # (`resolve()`), so at most one condition survives into it -- an unnamed
    # SpellID that also splits reads UNRESOLVED, and a marker-string check
    # would miss the split. Meanwhile the HELD population below is chosen by
    # shape (`r["visuals"]`), so the two criteria could disagree and count the
    # same row as both clean and unchecked: HELD, exit 0, over a row that
    # splits. Nothing in the DATA at 1.15.9.69547 reaches that -- no unnamed
    # skill-line SpellID carries a visual -- which is exactly why it has to be
    # closed in the SCRIPT. `tests/db2_spell_sweep_fixtures` builds the world
    # that reaches it.
    splits = [r for r in results if len(r["visuals"]) > 1]
    low = [r for r in results if r["low_prob"]]
    dangle = [r for r in results if r["dangling"]]
    print("### property 4 -- one SpellVisual per name, at Probability 1")
    # The safety claim has to carry its own scope. This is the line a person
    # transcribing constants reads, often the only line; a filter mentioned
    # three blocks earlier does not travel with it, and "HELD over a subset"
    # read as "HELD over the book" is exactly the false completeness claim
    # (AS-37 round 1) this whole script exists to make structurally hard.
    #
    # Both knobs that narrow the inventory get a clause, in the order they
    # apply: the era cut first, then --name. "filtered INVENTORY name(s)" is
    # deliberate -- the HELD line two lines up counts names WITH A VISUAL, a
    # different and usually smaller population, and two bare totals that close
    # together read as a contradiction rather than as two facts.
    scopes = []
    if cut:
        scopes.append(
            "  SCOPE: --era-cut %d excluded %d inventory name(s) above the cut (all\n"
            "         listed under 'build-era cut' above). This covers the era-side\n"
            "         inventory ONLY -- the excluded name(s) were never joined and\n"
            "         may split. Re-run with --era-cut 0 to judge the whole skill\n"
            "         line." % (args.era_cut, len(cut))
        )
    if args.name:
        scopes.append(
            "  SCOPE: --name %s is in effect. This covers the %d filtered\n"
            "         inventory name(s) ONLY -- NOT the skill-line inventory, which\n"
            "         may still split elsewhere. Re-run without --name before\n"
            "         transcribing anything as complete." % (" ".join(args.name), len(results))
        )

    def print_scopes():
        for s in scopes:
            print(s)

    if not with_visual:
        # Vacuously true is not HELD. An empty result set (an all-above-the-cut
        # inventory, or a --name that matched nothing) must not print a safety
        # claim a reader can take for a clean book.
        print("  VACUOUS: no name in scope resolved to a visual -- this run asserts NOTHING.")
        print_scopes()
        return 0

    if not splits and not low and not dangle:
        print(
            "  HELD: all %d name(s) with a visual resolve to exactly one "
            "SpellVisual at Probability 1." % len(with_visual)
        )
        print("  (Rank collapse is intact; a per-name visual is safe to transcribe.)")
        print_scopes()
        return 0

    for r in splits:
        print(
            "  SPLIT   %s -> %d visuals: %s"
            % (r["name"], len(r["visuals"]), ", ".join(str(v) for v in r["visuals"]))
        )
        for vid, sids in r["visuals"].items():
            print("            visual %-7d from spells %s" % (vid, abbrev(sids)))
    for r in low:
        print("  LOW-PROB %s: %s" % (r["name"], r["low_prob"]))
    for r in dangle:
        print("  UNRESOLVED %s: visual(s) %s absent from SpellVisual" % (r["name"], r["dangling"]))

    print()
    print(
        "  The rank-collapse assumption this sweep -- and every client-data doc "
        "built on one -- rests on does NOT hold for the rows above."
    )
    print("  Do not transcribe a single visual for them. Join each rank.")
    print_scopes()
    if args.allow_split:
        # A split and an off-Probability row are verdicts a person can OVERRULE
        # by reading the rows -- that is what this flag is for. A dangling
        # SpellVisualID is not that: the referenced `SpellVisual` row does not
        # exist, so there is nothing to read and no reading that makes the row
        # safe. Usually it means the wrong build or a truncated cache, and
        # downgrading it would hand back a clean exit over a join the script
        # cannot vouch for at all.
        if dangle:
            print(
                "  (--allow-split given, but a dangling SpellVisualID is not a split: "
                "there is\n   no row to read, so it stays fatal. Exiting 1.)"
            )
            return 1
        print("  (--allow-split given: exiting 0 anyway.)")
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
