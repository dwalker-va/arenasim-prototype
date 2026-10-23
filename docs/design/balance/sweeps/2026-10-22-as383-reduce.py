#!/usr/bin/env python3
"""AS-383: reduce `2026-10-22-as383-scan.py` rows, one block per file.

    2026-10-22-as383-reduce.py rows.jsonl [rows2.jsonl ...]
"""
import json, sys
from collections import Counter, defaultdict


def q(xs, p):
    xs = sorted(xs)
    return xs[int(p * (len(xs) - 1))] if xs else None


for path in sys.argv[1:]:
    rows = [json.loads(l) for l in open(path)]
    n = len(rows)
    print(f"== {path}  n={n}")
    print(f"win {sum(r['win'] for r in rows)/n:.3f}  heal {sum(r['heal'] for r in rows)/n:.0f}")
    ev = [e for r in rows for e in r["events"]]
    for ab in ["Fear", "Polymorph", "Death Coil", "Frost Shock"]:
        es = [e for e in ev if e["ab"] == ab]
        print(f"-- {ab}: {len(es)/n:.2f}/match (n={len(es)})  begun/match "
              f"{sum(r['begun'].get(ab,0) for r in rows)/n:.2f}")
        if not es:
            continue
        print("   posture at cast start:", dict(Counter(e["post"] for e in es)))
        print("   posture 1s before   :", dict(Counter(e.get("prev_post") for e in es)))
        print("   listed (ready) at start:", dict(Counter(e["listed"] for e in es)),
              " listed 1s before:", dict(Counter(e.get("listed_prev") for e in es)))
        print("   inside inner disc:", dict(Counter(e.get("inner") for e in es)))
        print("   repair code:", dict(Counter((e["post"], (e["repair"] or "-").split(" ")[0]) for e in es)))
        print("   repair 'none' detail:", Counter(e["repair"] for e in es if e["repair"] and e["repair"].startswith("none")).most_common(6))
        dc = [e["d_caster"] for e in es if "d_caster" in e]
        print(f"   d_caster q10/50/90: {q(dc,.1):.1f} {q(dc,.5):.1f} {q(dc,.9):.1f}" if dc else "")
        dp = [e["d_prev"] for e in es if "d_prev" in e]
        if dp:
            print(f"   d_caster 1s before q10/50/90: {q(dp,.1):.1f} {q(dp,.5):.1f} {q(dp,.9):.1f}")
        cm = [e["closed_by_me"] for e in es if "closed_by_me" in e]
        cc = [e["closed_by_caster"] for e in es if "closed_by_caster" in e]
        if cm:
            print(f"   closed in last 1s by me: mean {sum(cm)/len(cm):.2f}  by caster: mean {sum(cc)/len(cc):.2f}")
        da = [e["d_ally"] for e in es if "d_ally" in e]
        ac = [e["ally_to_caster"] for e in es if "ally_to_caster" in e]
        if da:
            print(f"   d to own ally q50 {q(da,.5):.1f}; ally to caster q10/50/90 {q(ac,.1):.1f} {q(ac,.5):.1f} {q(ac,.9):.1f}")
        print("   by ally class:", dict(Counter(e.get("ally_cls") for e in es)))
        for post in ["Free", "Dip", "Pressured"]:
            sub = [e for e in es if e["post"] == post]
            if not sub:
                continue
            g = [e["goal_to_caster"] for e in sub if "goal_to_caster" in e]
            mg = [e["me_to_goal"] for e in sub if "me_to_goal" in e]
            ind = [e["in_disc_for"] for e in sub if "in_disc_for" in e]
            print(f"   [{post}] n={len(sub)} goal->caster q50 {q(g,.5)} me->goal q50 {q(mg,.5)} in-disc-for q50 {q(ind,.5)} entered in {dict(Counter(e.get('entered_post') for e in sub))}")
        print("   goal kind:", dict(Counter(e["goal"] for e in es)))
    dp = Counter()
    eps = []
    for r in rows:
        d = dict(r.get("dips", {}))
        eps += d.pop("ep", [])
        dp.update(d)
    if eps:
        print(f"dip episodes {len(eps)}: mean dur {sum(e[0] for e in eps)/len(eps):.2f}s; "
              f"ended {dict(Counter(e[1] for e in eps))}; scream at end {sum(e[2] for e in eps)}; "
              f"enemy healer feared {sum(e[3] for e in eps)}; CC/FS events on Priest in dip+5s {sum(e[4] for e in eps)}")
    print("dips per match:", {k: round(v / n, 2) for k, v in dp.items()})
    pt = defaultdict(float)
    for r in rows:
        for k, v in r["post_t"].items():
            pt[k] += v
    tot = sum(pt.values())
    print("posture time share:", {k: round(v / tot, 3) for k, v in pt.items()}, f" mean match {tot/n:.0f}s")
    print(f"time <20yd of enemy healer /match {sum(r['in_fs20'] for r in rows)/n:.1f}s; "
          f"inside an outer CC disc /match {sum(r['in_disc'] for r in rows)/n:.1f}s; "
          f"mean dist to enemy healer {sum(r['sham_d_mean'] or 0 for r in rows)/n:.1f}")
    rp = defaultdict(float)
    for r in rows:
        for k, v in r["repairs"].items():
            rp[k] += v
    print("repair-code time /match:", {k: round(v / n, 1) for k, v in sorted(rp.items(), key=lambda x: -x[1])[:10]})
