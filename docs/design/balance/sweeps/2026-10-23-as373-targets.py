#!/usr/bin/env python3
"""The AS-373 mechanism targets from harness rows, per map and format.

    2026-10-23-as373-targets.py [--no-paladin] rows.jsonl [...]
"""
import collections
import gzip
import json
import sys

HUG = 4.0
LO = {"00-02": 0, "02-04": 2, "04-06": 4, "06-10": 6, "10-15": 10, "15-25": 15, "25+": 25}
args = sys.argv[1:]
no_pal = "--no-paladin" in args
args = [a for a in args if not a.startswith("--")]
rows = []
for p in args:
    with (gzip.open(p, "rt") if p.endswith(".gz") else open(p)) as f:
        rows += [json.loads(l) for l in f if l.strip()]
if no_pal:
    rows = [r for r in rows if r["class"] != "Paladin"]


def fmt(team):
    return "3v3" if team.count("+") == 2 else "2v2"


g = collections.defaultdict(list)
for r in rows:
    g[(r["profile"], r["map"], fmt(r["team"]))].append(r)
    g[(r["profile"], r["map"], "all")].append(r)

FIELDS = ["secs", "hard_cc", "blind_anchor", "dying", "blind_dying", "exposed"]

print("GEOMETRIC hug (within 4yd of the skin)" + (" — Paladin excluded" if no_pal else ""))
print()
print("| profile | map | fmt | hug share | blind anchor hug / open | blind dying hug / open | PRESSURED hardCC hug / open | FREE hardCC hug / open | blind eps>=1s /hm | pillar deaths /100 | win% |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for key in sorted(g):
    rs = g[key]
    acc = collections.defaultdict(lambda: collections.Counter())
    for r in rs:
        for k, o in r["occ"].items():
            ph, p, b = k.split("|")
            if ph != "post":
                continue
            mode = "hug" if LO[b] < HUG else "open"
            for post in ["all", p]:
                a = acc[(post, mode)]
                for f in FIELDS:
                    a[f] += o[f]

    def r_(post, mode, num, den="secs"):
        a = acc[(post, mode)]
        return 100 * a[num] / a[den] if a[den] else float("nan")

    tot = acc[("all", "hug")]["secs"] + acc[("all", "open")]["secs"]
    hug = 100 * acc[("all", "hug")]["secs"] / tot if tot else 0
    eps = sum(1 for r in rs for b in r["blind"] if b["secs"] >= 1.0) / len(rs)
    pd = 0
    for r in rs:
        for d in r["deaths"]:
            if d["final_blind_secs"] >= 2.0 and d["healer_cc_5s"] < 0.5:
                pd += 1
    wins = sum(r["result"] == "win" for r in rs) / len(rs) * 100
    print(
        f"| {key[0]} | {key[1]} | {key[2]} | {hug:.1f}% | {r_('all','hug','blind_anchor'):.1f} / {r_('all','open','blind_anchor'):.1f} | "
        f"{r_('all','hug','blind_dying','dying'):.1f} / {r_('all','open','blind_dying','dying'):.1f} | "
        f"{r_('pressured','hug','hard_cc'):.1f} / {r_('pressured','open','hard_cc'):.1f} | "
        f"{r_('free','hug','hard_cc'):.1f} / {r_('free','open','hard_cc'):.1f} | {eps:.2f} | {100*pd/len(rs):.1f} ({pd}) | {wins:.1f} |"
    )

if any("modes" in r for r in rows):
    print()
    print("TRACED mode (PillarModeEnter / Leave)")
    print()
    print("| profile | map | fmt | posture | mode | share | hard CC'd | in CC reach | blind anchor | blind dying (of dying) | changes /hm | median dwell s |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for key in sorted(g):
        rs = g[key]
        acc = collections.defaultdict(lambda: collections.Counter())
        for r in rs:
            for k, o in r.get("modes", {}).items():
                p, m = k.split("|")
                for post in ["all", p]:
                    for f in FIELDS:
                        acc[(post, m)][f] += o[f]
        ch = sum(r.get("mode_changes", 0) for r in rs) / len(rs)
        dw = sorted(d for r in rs for d in r.get("mode_dwells", []))
        med = dw[len(dw) // 2] if dw else float("nan")
        for post in ["all", "free", "pressured"]:
            tot = acc[(post, "hug")]["secs"] + acc[(post, "open")]["secs"]
            for m in ["hug", "open"]:
                a = acc[(post, m)]
                if not a["secs"]:
                    continue
                pc = lambda n, d="secs": f"{100*a[n]/a[d]:.1f}%" if a[d] else "-"
                print(f"| {key[0]} | {key[1]} | {key[2]} | {post} | {m} | {100*a['secs']/tot:.1f}% | {pc('hard_cc')} | {pc('exposed')} | {pc('blind_anchor')} | {pc('blind_dying','dying')} | {ch:.2f} | {med:.1f} |")
