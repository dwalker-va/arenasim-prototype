"""The Hunter directional bank: from the E0 `uni` 2v2 banks, every cell with a
Hunter on either side, plus 8 control cells without one (digest-ordered,
covering every other class on both sides), on all three maps under both
profiles, at the 0.8 seeds (three blocks).

    2026-10-30-as303-hunter-bank.py <dir>

reads `<dir>/banks/uni2-<map>-<profile>.jsonl`, written by
`2026-10-29-as303-gen.py uni <map> <profile> 2 801-804,811-814,821-824`, and
writes `<dir>/banks/hunter.jsonl` (37,656 matches)."""
import json, glob, hashlib, sys

D = sys.argv[1]
out = []
for f in sorted(glob.glob(f"{D}/banks/uni2-*.jsonl")):
    rows = [json.loads(l) for l in open(f)]
    cells = {}
    for r in rows:
        cells.setdefault((tuple(r["team1"]), tuple(r["team2"])), []).append(r)
    hunter = [c for c in cells if "Hunter" in c[0] + c[1]]
    rest = sorted((c for c in cells if c not in hunter),
                  key=lambda c: hashlib.sha256(repr(c).encode()).hexdigest())
    others = {"Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Shaman", "Druid"}
    control, s1, s2 = [], set(), set()
    for c in rest:
        if len(control) >= 8 and s1 >= others and s2 >= others:
            break
        if len(control) < 8 or not (set(c[0]) <= s1 and set(c[1]) <= s2):
            control.append(c)
            s1 |= set(c[0]); s2 |= set(c[1])
    assert s1 >= others and s2 >= others, (f, s1, s2)
    for c in hunter + control:
        out.extend(cells[c])
    print(f, len(hunter), len(control), file=sys.stderr)
with open(f"{D}/banks/hunter.jsonl", "w") as w:
    for r in out:
        w.write(json.dumps(r) + "\n")
print(len(out), file=sys.stderr)
