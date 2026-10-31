#!/usr/bin/env python3
"""AS-399 mechanism: what the target focus changes inside a match.

    2026-10-31-as399-mechanism.py run <arenasim> <bank.jsonl> <out.rows.jsonl> [workers]
    2026-10-31-as399-mechanism.py report <rows.jsonl>...

`run` plays every config of a bank (`2026-10-31-as399-gen.py`) as a single
`--headless` match, with the decision trace on, reduces each match's log and
trace to one JSON row, and deletes them. A row carries:

- the winner, the duration, and every death (team, class, time);
- the damage each attacker class dealt to each victim class, by team;
- every `target_focus` situation change (team, kind, time, the subject's and
  the old target's class).

`report` pairs the focus-on rows with the focus-off rows at the same cell and
seed (labels `uni-TT` / `focus-off`) and prints:

- who the focus moves off and onto (counts and seconds held);
- by side composition, the share of damage the enemy's ranged DPS put on each
  of the side's units, focus on and off;
- by side composition, who on the side dies first, and how often the side
  loses, focus on and off.

Composition kinds: H+M (a healer and a melee DPS), H+R (a healer and a ranged
DPS), R+R, M+R, M+M (no healer).
"""
import collections
import json
import os
import re
import subprocess
import sys
import tempfile
from multiprocessing import Pool

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MELEE = {"Warrior", "Rogue"}
RANGED = {"Mage", "Hunter", "Warlock"}

DMG = re.compile(r"^\[\s*([\d.]+)s\] \[DMG\] Team (\d) (\w+) #\d's .*? hits Team (\d) (\w+) #\d for (\d+) damage")
DEATH = re.compile(r"^\[\s*([\d.]+)s\] \[DEATH\] Team (\d) (\w+) #\d has been eliminated")
WIN = re.compile(r"^Winner: (.*)$")
DUR = re.compile(r"^Duration: ([\d.]+)s")


def kind(team):
    cls = team.split("+")
    h = sum(c in HEALERS for c in cls)
    m = sum(c in MELEE for c in cls)
    r = sum(c in RANGED for c in cls)
    if h == 1:
        return "H+M" if m else "H+R"
    if h == 0:
        return {(2, 0): "M+M", (0, 2): "R+R", (1, 1): "M+R"}[(m, r)]
    return "H+H"


def reduce_one(args):
    exe, cfg, trace = args
    with tempfile.TemporaryDirectory(dir=os.environ.get("AS399_TMP")) as d:
        cpath = os.path.join(d, "c.json")
        with open(cpath, "w") as f:
            json.dump(cfg, f)
        log = os.path.join(d, "m.txt")
        subprocess.run([exe, "--headless", cpath, "--output", log, "--trace-mode",
                        "on" if trace else "off"], check=True, capture_output=True)
        row = {"label": cfg["label"], "deaths": [], "dmg": collections.Counter(), "focus": []}
        with open(log) as f:
            for line in f:
                m = DMG.match(line)
                if m:
                    t, at, ac, vt, vc, n = m.groups()
                    row["dmg"][f"{at}|{ac}|{vc}"] += int(n)
                    continue
                m = DEATH.match(line)
                if m:
                    row["deaths"].append([int(m.group(2)), m.group(3), float(m.group(1)) - 10.0])
                    continue
                m = WIN.match(line)
                if m:
                    w = m.group(1).strip()
                    row["winner"] = {"Team 1": 1, "Team 2": 2}.get(w, 0)
                    continue
                m = DUR.match(line)
                if m:
                    row["duration"] = float(m.group(1)) - 10.0
        if trace:
            cls = {}
            tpath = log[:-4] + "_trace.jsonl"
            with open(tpath) as f:
                for line in f:
                    if '"actor"' in line and len(cls) < 6:
                        try:
                            e = json.loads(line)
                        except ValueError:
                            continue
                        a = e.get("actor") or {}
                        if "entity_id" in a:
                            cls.setdefault(a["entity_id"], a["class"])
                    if '"target_focus"' in line and '"situation_change"' in line:
                        e = json.loads(line)
                        row["focus"].append([e["team"], e["change"], e["sim_time"],
                                             e.get("subject"), e.get("from")])
            row["focus"] = [[t, k, s, cls.get(a, "?"), cls.get(b, "?")]
                            for t, k, s, a, b in row["focus"]]
        row["dmg"] = dict(row["dmg"])
        return row


def cmd_run(exe, bank, out, workers=1):
    cfgs = [json.loads(l) for l in open(bank) if l.strip()]
    jobs = [(exe, c, c["label"].startswith("uni-TT")) for c in cfgs]
    with open(out, "w") as f, Pool(workers) as p:
        for i, row in enumerate(p.imap(reduce_one, jobs, chunksize=4)):
            f.write(json.dumps(row) + "\n")
            if i % 200 == 0:
                f.flush()
                print(f"{i}/{len(jobs)}", file=sys.stderr)


def load(paths, want):
    out = {}
    for path in paths:
        for line in open(path):
            try:
                r = json.loads(line)
            except ValueError:
                continue  # a partial last line
            bank, m, t1, t2, s = r["label"].split("|")
            if bank == want:
                out[(m, t1, t2, int(s))] = r
    return out


def pct(a, b):
    return 100.0 * a / b if b else float("nan")


def cmd_report(paths):
    on, off = load(paths, "uni-TT"), load(paths, "focus-off")
    keys = sorted(set(on) & set(off))
    print(f"paired matches: {len(keys)}")

    # 1. Who the focus moves off and onto.
    onset = collections.Counter()
    held = collections.Counter()
    role = lambda c: "healer" if c in HEALERS else ("melee" if c in MELEE else "ranged")
    matches_with = 0
    for k in keys:
        r = on[k]
        if any(f[1] == "onset" for f in r["focus"]):
            matches_with += 1
        open_ = {}
        for team, kd, t, subj, frm in r["focus"]:
            if kd == "onset":
                if team in open_:
                    s0, a, b = open_[team]
                    held[(role(b), role(a))] += t - s0
                onset[(role(frm), role(subj))] += 1
                onset[("class", frm, subj)] += 1
                open_[team] = (t, subj, frm)
            elif kd == "end" and team in open_:
                s0, a, b = open_.pop(team)
                held[(role(b), role(a))] += t - s0
        for team, (s0, a, b) in open_.items():
            held[(role(b), role(a))] += r.get("duration", s0) - s0
    print(f"\n## Focus onsets ({matches_with} of {len(keys)} matches have one)")
    tot = sum(v for k, v in onset.items() if k[0] != "class")
    tsec = sum(held.values())
    print("| off (hidden target) | onto (subject) | onsets | share | seconds held | share |\n|---|---|---|---|---|---|")
    for k, v in sorted(((k, v) for k, v in onset.items() if k[0] != "class"), key=lambda x: -x[1]):
        print(f"| {k[0]} | {k[1]} | {v} | {pct(v, tot):.1f}% | {held[k]:.0f} | {pct(held[k], tsec):.1f}% |")
    print("\nTop class pairs (off -> onto):")
    for k, v in sorted(((k, v) for k, v in onset.items() if k[0] == "class"), key=lambda x: -x[1])[:15]:
        print(f"  {k[1]} -> {k[2]}: {v}")

    # 2. Damage share taken, by side composition and opponent composition.
    print("\n## Damage the enemy's ranged DPS deal to each unit of a side (share of their damage)")
    dmg = collections.defaultdict(lambda: [collections.Counter(), collections.Counter()])
    for k in keys:
        m, t1, t2, s = k
        for arm, r in ((0, off[k]), (1, on[k])):
            for key, n in r["dmg"].items():
                at, ac, vc = key.split("|")
                if ac not in RANGED:
                    continue
                side = t2 if at == "1" else t1
                opp = t1 if at == "1" else t2
                if vc not in side.split("+"):
                    continue  # a pet or a totem
                vrole = role(vc)
                dmg[(kind(side), kind(opp))][arm][vrole] += n
                dmg[(kind(side), "any")][arm][vrole] += n
    print("| side | vs | healer off/on | melee off/on | ranged off/on | total dmg off/on |\n|---|---|---|---|---|---|")
    for (sk, ok), (a0, a1) in sorted(dmg.items()):
        t0, t1_ = sum(a0.values()), sum(a1.values())
        cells = [f"{pct(a0[r], t0):.0f} / {pct(a1[r], t1_):.0f}" for r in ("healer", "melee", "ranged")]
        print(f"| {sk} | {ok} | " + " | ".join(cells) + f" | {t0} / {t1_} |")

    # 3. First death and losses, by side composition.
    print("\n## First death on the side, and losses, by side composition (vs any)")
    fd = collections.defaultdict(lambda: [collections.Counter(), collections.Counter()])
    for k in keys:
        m, t1, t2, s = k
        for arm, r in ((0, off[k]), (1, on[k])):
            for seat, side, opp in ((1, t1, t2), (2, t2, t1)):
                for kk in ((kind(side), "any"), (kind(side), kind(opp))):
                    c = fd[kk][arm]
                    c["n"] += 1
                    c["loss"] += r.get("winner") == 3 - seat
                    first = next((d for d in r["deaths"]), None)
                    if first and first[0] == seat:
                        c["first " + role(first[1])] += 1
                    elif first is None:
                        c["no death"] += 1
    print("| side | vs | n | losses off/on | first death healer off/on | melee off/on | ranged off/on |\n|---|---|---|---|---|---|---|")
    for (sk, ok), (a0, a1) in sorted(fd.items()):
        n = a0["n"]
        cells = [f"{pct(a0[x], n):.1f} / {pct(a1[x], n):.1f}"
                 for x in ("loss", "first healer", "first melee", "first ranged")]
        print(f"| {sk} | {ok} | {n} | " + " | ".join(cells) + " |")


def main(argv):
    if argv[0] == "run":
        cmd_run(argv[1], argv[2], argv[3], int(argv[4]) if len(argv) > 4 else 1)
    elif argv[0] == "report":
        cmd_report(argv[1:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
