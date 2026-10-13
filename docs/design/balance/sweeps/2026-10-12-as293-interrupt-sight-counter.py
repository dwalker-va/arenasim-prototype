#!/usr/bin/env python3
"""Instrument a checkout to COUNT interrupts by line of sight.

AS-293 measurement arm. Run against a checkout (either side of the fix), then
build it: every interrupt the AI commits to prints one line to stderr, naming
the interrupt and whether the interrupter could see its target at that
instant:

    AS293 INT <Kick|Pummel|Wind Shear|Spell Lock> in_sight=<true|false>

The counter only READS (the map's obstacle volumes, the two positions) and
writes to stderr, so it cannot move a match: an instrumented binary produces
the same batch CSV as the clean one (checked on every map in the AS-293
findings note).

Two sites, the same on both sides of the fix:
- `check_interrupts` (Kick, Pummel, Wind Shear): at its `// Use the interrupt!`
  line, where both arms hold `my_pos` and `target_pos`. The map's geometry is
  read through an added read-only `Option<Res<ActiveMapGeometry>>` parameter.
- the Felhunter's `try_spell_lock`: before each `execute_spell_lock(` call,
  where both arms hold `ctx`, `my_pos` and `target_entity`.

Usage:
    2026-10-12-as293-interrupt-sight-counter.py <checkout>
    cargo build --release --manifest-path <checkout>/Cargo.toml
    <checkout>/target/release/arenasim --batch sweep.jsonl --out arm.csv \\
        --jobs 4 --trace-mode off 2> arm.stderr
    grep -c 'in_sight=false' arm.stderr
"""

import re
import sys

COMBAT_AI = "src/states/play_match/combat_ai.rs"
PET_AI = "src/states/play_match/class_ai/pet_ai.rs"

GEO_PARAM = (
    "    as293_geo: Option<Res<crate::states::play_match::map_config::ActiveMapGeometry>>,\n"
)

INT_PRINT = """        // AS-293 measurement counter (instrumentation only).
        eprintln!(
            "AS293 INT {} in_sight={}",
            ability_def.name,
            as293_geo.as_ref().map_or(true, |g| {
                crate::states::play_match::map_geometry::has_line_of_sight(
                    &g.volumes, my_pos, target_pos,
                )
            })
        );
"""

LOCK_PRINT = """{indent}// AS-293 measurement counter (instrumentation only).
{indent}eprintln!(
{indent}    "AS293 INT Spell Lock in_sight={{}}",
{indent}    ctx.combatants.get(&target_entity).map_or(true, |t| {{
{indent}        crate::states::play_match::map_geometry::has_line_of_sight(
{indent}            ctx.obstacles, my_pos, t.position,
{indent}        )
{indent}    }})
{indent});
"""


def patch_combat_ai(src: str) -> str:
    start = src.index("pub fn check_interrupts(")
    body = src.index(") {\n", start)
    src = src[:body] + GEO_PARAM + src[body:]
    anchor = src.index("        // Use the interrupt!\n", start)
    return src[:anchor] + INT_PRINT + src[anchor:]


def patch_pet_ai(src: str) -> str:
    start = src.index("fn try_spell_lock(")
    end = src.index("pub(crate) fn execute_spell_lock(", start)
    head, body, tail = src[:start], src[start:end], src[end:]
    calls = list(re.finditer(r"(?m)^(?P<indent>[ \t]*)execute_spell_lock\(", body))
    if not calls:
        raise SystemExit("no execute_spell_lock( call in try_spell_lock")
    for m in reversed(calls):
        body = (
            body[: m.start()]
            + LOCK_PRINT.format(indent=m.group("indent"))
            + body[m.start() :]
        )
    return head + body + tail


def main() -> int:
    root = sys.argv[1]
    for rel, patch in ((COMBAT_AI, patch_combat_ai), (PET_AI, patch_pet_ai)):
        path = f"{root}/{rel}"
        src = open(path).read()
        if "AS293 INT" in src:
            print(f"{rel}: already instrumented", file=sys.stderr)
            return 1
        open(path, "w").write(patch(src))
        print(f"{rel}: instrumented", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
