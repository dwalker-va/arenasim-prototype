#!/usr/bin/env python3
"""Instrument a checkout to COUNT Hammer of Justice casts by line of sight.

AS-279 measurement arm. Run against a checkout (either side of the fix), then
build it: every Hammer of Justice the Paladin AI commits to prints one line to
stderr, naming the path (rotation / dip / peel) and whether the caster could
see its target at that instant:

    AS279 HOJ in_sight=<true|false> path=<rotation|dip|peel>

The counter only READS the decision's snapshot (`ctx`, the caster's position)
and writes to stderr, so it cannot move a match: an instrumented binary
produces the same batch CSV as the clean one (checked on both maps in the
AS-279 findings note).

It works on both sides of the fix because it anchors on the shared
`cast_hammer_of_justice(` call shape — commands, combat_log, entity, def,
abilities, combatant, <target> — and reads the target expression from the
seventh argument, whatever the caller names it.

Usage:
    2026-10-12-as279-hoj-sight-counter.py <checkout>
    cargo build --release --manifest-path <checkout>/Cargo.toml
    <checkout>/target/release/arenasim --batch sweep.jsonl --out arm.csv \
        --jobs 4 --trace-mode off 2> arm.stderr
    grep -c 'in_sight=false' arm.stderr
"""

import re
import sys

PATH = "src/states/play_match/class_ai/paladin.rs"

CALL = re.compile(
    r"(?P<indent>[ \t]*)cast_hammer_of_justice\(\s*commands,\s*combat_log,\s*entity,"
    r"\s*def,\s*abilities,\s*combatant,\s*(?P<target>[^,]+),"
)

HELPER = '''
/// AS-279 measurement counter (instrumentation only; see
/// docs/design/balance/sweeps/2026-10-12-as279-hoj-sight-counter.py).
fn as279_count_hoj(ctx: &CombatContext, my_pos: Vec3, target: Entity, path: &str) {
    if let Some(t) = ctx.combatants.get(&target) {
        let in_sight = crate::states::play_match::map_geometry::has_line_of_sight(
            ctx.obstacles,
            my_pos,
            t.position,
        );
        eprintln!("AS279 HOJ in_sight={in_sight} path={path}");
    }
}
'''


def enclosing_fn(src: str, at: int) -> str:
    return re.findall(r"fn (\w+)", src[:at])[-1]


def main() -> int:
    path = f"{sys.argv[1]}/{PATH}"
    src = open(path).read()
    if "as279_count_hoj" in src:
        print("already instrumented", file=sys.stderr)
        return 1
    out = []
    last = 0
    n = 0
    for m in CALL.finditer(src):
        fn = enclosing_fn(src, m.start())
        if fn == "cast_hammer_of_justice":
            continue
        label = {
            "try_hammer_of_justice": "rotation",
            "try_dip_hammer_of_justice": "dip",
            "try_peel_hammer_of_justice": "peel",
        }[fn]
        target = m.group("target").strip()
        out.append(src[last:m.start()])
        out.append(
            f'{m.group("indent")}as279_count_hoj(ctx, my_pos, {target}, "{label}");\n'
        )
        last = m.start()
        n += 1
    out.append(src[last:])
    if n != 3:
        print(f"expected 3 call sites, found {n}", file=sys.stderr)
        return 1
    open(path, "w").write("".join(out) + HELPER)
    print(f"instrumented {n} call sites in {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
