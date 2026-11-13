#!/usr/bin/env python3
"""AS-440 reducer: rows from `tests/healer_hide_exposure.rs`.

    2026-11-13-as440-reduce.py diag rows.jsonl[.gz] ...
    2026-11-13-as440-reduce.py h2h ctl.rows.jsonl[.gz] h2h.rows.jsonl[.gz] ...
    2026-11-13-as440-reduce.py local ctl.csv[.gz] h2h.csv[.gz] [--eligible]
    2026-11-13-as440-reduce.py mech ctl.rows.jsonl ctl_logs/ h2h.rows.jsonl h2h_logs/ > mech.jsonl
    2026-11-13-as440-reduce.py mechreport mech.jsonl

`event` (round 2) is the event study: both arms are identical until the
stance arm's healer first hugs, so its windows open at that second.

    2026-11-13-as440-reduce.py event ctl.rows.jsonl np.rows.jsonl

`partner` (round 2) is the per-comp table of what changes for the partner
(rows from the harness with the partner fields), beside the 12-seed effect
read from a `local` report.

    2026-11-13-as440-reduce.py partner ctl.rows.jsonl np.rows.jsonl local.txt

`mech` (round 2) joins each side's per-frame mechanism metrics (the row's
`mech`) with its match log — damage taken and heals by source, crowd control
on the healer by ability and source, with chains — for the side running the
option and its all-shipped twin, one JSON line per pair. `mechreport` reads
those pairs: the arms' means over every pair, and over the pairs whose
outcome flipped each way.

`local` (round 2) reads the `--batch` CSVs of the same banks and splits the
paired head-to-head effect by the running side's comp, the enemy comp and the
map, with a seed-clustered bootstrap interval (resampling seeds, 2,000 draws)
and the effect per seed block, and says how much of the pooled net the top
comps carry.

`h2h` pairs every side that ran a hide rule (`h2h-<rule>-t<seat>` rows, the
seat's side) with the same side in the all-shipped twin (`ctl`), at the same
map, cell and seed: win (a draw counts half; McNemar on decisive flips), the
side's first death being its partner, and the healer's hard crowd control.

Every side is a 2v2 team of one healer and one DPS (its partner) against an
enemy that fields at least one ranged DPS. Two denominators throughout
(memory: nested designs): SIDES (one per match per qualifying team) for
rates of events, and SECONDS (both alive, an enemy ranged DPS alive) for
shares of time.
"""
import collections
import gzip
import json
import sys

CAUSES = ["pressured_locked", "pressured_cover", "pressured_hug", "pressured_cc", "free", "free_sight",
          "free_reach", "free_hug", "walk", "escape", "dip", "ccd", "none"]


def rows(paths):
    for p in paths:
        op = gzip.open if p.endswith(".gz") else open
        with op(p, "rt") as f:
            for line in f:
                if line.strip():
                    yield json.loads(line)


def diag(paths):
    groups = collections.defaultdict(list)
    for r in rows(paths):
        if r["enemy_ranged"] < 1:
            continue
        k = "H+M" if r["partner_melee"] else "H+R"
        groups[k].append(r)
        groups["all"].append(r)
    for k in ("H+M", "H+R", "all"):
        g = groups[k]
        n = len(g)
        both = sum(r["both_secs"] for r in g)
        hid = sum(r["hidden_secs"] for r in g)
        exp = sum(r["exposing_secs"] for r in g)
        link = sum(r["exposing_link_secs"] for r in g)
        moves = [m for r in g for m in r["focus"]]
        sides_with = sum(1 for r in g if r["focus"])
        print(f"\n== {k}: {n} sides, {both/60:.0f} min both alive ==")
        print(f"  healer hidden from every enemy ranged DPS: {hid/both:.1%} of seconds")
        print(f"  exposing (hidden, partner shot in sight): {exp/both:.1%} of seconds"
              f" = {exp/n:.1f}s per side; healer keeps heal link on {link/max(exp,1e-9):.1%}")
        print(f"  partner loss while exposing: {sum(r['partner_loss_exposing'] for r in g)/max(sum(r['partner_loss'] for r in g),1e-9):.1%} of all partner health lost")
        print(f"  focus moves healer->partner: {len(moves)} in {sides_with} sides ({sides_with/n:.1%});"
              f" link at onset {sum(m['link'] for m in moves)/max(len(moves),1):.1%};"
              f" pressured at onset {sum(m['pressured'] for m in moves)/max(len(moves),1):.1%}")
        if moves:
            nm = len(moves)
            print(f"  at onset: sight of partner {sum(m['sight'] for m in moves)/nm:.1%}, within 40yd {sum(m['dist']<=40 for m in moves)/nm:.1%},"
                  f" enemy melee within 10yd of healer {sum(m['melee_near'] for m in moves)/nm:.1%},"
                  f" partner on an enemy ranged DPS {sum(m['partner_on_ranged'] for m in moves)/nm:.1%}")
            def phase(m):
                if m.get("hid_by") == "never_seen":
                    return "opener, never seen"
                return "opener, lost <10s" if m["t"] - m.get("blind_for", 0) < 10 else "mid-fight"
            for ph in ("opener, never seen", "opener, lost <10s", "mid-fight"):
                sub = [m for m in moves if phase(m) == ph]
                if not sub:
                    continue
                key = "cause" if ph == "opener, never seen" else "break_cause"
                causes = collections.Counter(m.get(key) for m in sub)
                print(f"  PHASE {ph}: {len(sub)/nm:.1%} of moves ({len(sub)/n:.2f}/side), link at onset "
                      f"{sum(m['link'] for m in sub)/len(sub):.0%}; {key}: "
                      + ", ".join(f"{k} {v/len(sub):.0%}" for k, v in causes.most_common(7)))
            br = collections.Counter(m.get("broke", "?") for m in moves)
            hb = collections.Counter(m.get("hid_by", "?") for m in moves)
            print("  whose step hid the healer from the DPS on it: " + ", ".join(f"{k} {v/nm:.1%}" for k, v in hb.most_common()))
            bl = sorted(m.get("blind_for", 0) for m in moves)
            print(f"  DPS blind to the healer before the move: median {bl[nm//2]:.1f}s, p25 {bl[nm//4]:.1f}s, p75 {bl[3*nm//4]:.1f}s")
            bc = collections.Counter(m.get("break_cause", "?") for m in moves)
            print("  healer's cause when the DPS lost it: " + ", ".join(f"{k} {v/nm:.1%}" for k, v in bc.most_common(8)))
            for who in ("healer", "enemy", "joint", "both"):
                sub = collections.Counter(m.get("break_cause") for m in moves if m.get("hid_by") == who)
                if sub:
                    print(f"    {who:6} step: " + ", ".join(f"{k} {v}" for k, v in sub.most_common(6)))
            print("  partner sight at onset: " + ", ".join(f"{k} {v/nm:.1%}" for k, v in br.most_common()))
            print(f"  while held: healer keeps link {sum(m['held_link']*m['held'] for m in moves)/sum(m['held'] for m in moves):.1%},"
                  f" PRESSURED {sum(m['held_pressured']*m['held'] for m in moves)/sum(m['held'] for m in moves):.1%}")
            held = sum(m["held"] for m in moves)
            print(f"  mean held {held/len(moves):.1f}s; healer hidden {sum(m['held_hidden'] for m in moves)/held:.1%} of it;"
                  f" partner dies under it {sum(m['partner_died'] for m in moves)/len(moves):.1%};"
                  f" mean partner loss {sum(m['partner_loss'] for m in moves)/len(moves):.1%}")
        ec = collections.Counter()
        hc = collections.Counter()
        lc = collections.Counter()
        un = collections.Counter()
        for r in g:
            for c, s in r.get("exposing_link_cause", {}).items():
                lc[c] += s
            for c, s in r.get("exposing_unlinked", {}).items():
                un[c] += s
            for c, s in r["exposing_cause"].items():
                ec[c] += s
            for c, s in r["hidden_cause"].items():
                hc[c] += s
        oc = collections.Counter(m["cause"] for m in moves)
        tot_un = sum(un.values())
        if tot_un:
            print("  exposing without link, why: " + ", ".join(f"{k} {v/tot_un:.0%}" for k, v in un.most_common()))
        ol = collections.Counter(m["cause"] for m in moves if m["link"])
        print(f"  {'cause':16} {'exposing s':>10} {'share':>6} {'link':>6} {'hidden':>6} {'onsets':>6} {'share':>6} {'link':>6}")
        for c in CAUSES:
            if ec[c] or oc[c] or hc[c]:
                print(f"  {c:16} {ec[c]:10.0f} {ec[c]/max(exp,1e-9):6.1%} {lc[c]/max(ec[c],1e-9):6.1%} {hc[c]/max(hid,1e-9):6.1%} {oc[c]:6d} {oc[c]/max(len(moves),1):6.1%} {ol[c]/max(oc[c],1):6.1%}")
        fd = collections.Counter(r["first_death"] for r in g)
        res = collections.Counter(r["result"] for r in g)
        print(f"  first death: partner {fd['partner']/n:.1%}, healer {fd['healer']/n:.1%}; "
              f"win {res['win']/n:.1%} loss {res['loss']/n:.1%} draw {res['draw']/n:.1%}; "
              f"healer hard CC {sum(r['healer_hard_cc_secs'] for r in g)/n:.1f}s/side")
    # per healer class, H+M
    print("\n== by healer, exposing share / onsets per side / link at onset ==")
    for k in ("H+M", "H+R"):
        byh = collections.defaultdict(list)
        for r in groups[k]:
            byh[r["healer"]].append(r)
        for h, g in sorted(byh.items()):
            both = sum(r["both_secs"] for r in g)
            exp = sum(r["exposing_secs"] for r in g)
            moves = [m for r in g for m in r["focus"]]
            print(f"  {k} {h:8} n={len(g):4} exposing {exp/both:5.1%} onsets/side {len(moves)/len(g):.2f}"
                  f" link {sum(m['link'] for m in moves)/max(len(moves),1):5.1%}"
                  f" partner-first {sum(r['first_death']=='partner' for r in g)/len(g):5.1%}")


def mcnemar(b, c):
    return (b - c) / (b + c) ** 0.5 if b + c else 0.0


def h2h(paths):
    ctl = {}
    for r in rows([paths[0]]):
        _, m, a, b, seed = r["label"].split("|")
        ctl[(m, a, b, seed, r["side"])] = r
    arms = collections.defaultdict(list)
    for r in rows(paths[1:]):
        bank, m, a, b, seed = r["label"].split("|")
        rule, seat = bank.split("-")[1], int(bank.split("-t")[-1])
        if r["side"] != seat:
            continue
        c = ctl.get((m, a, b, seed, seat))
        if c is None:
            continue
        arms[rule].append((m, seed, r, c))

    def win(r):
        return {"win": 1.0, "draw": 0.5, "loss": 0.0}[r["result"]]

    def report(name, pairs):
        n = len(pairs)
        if not n:
            return
        w0 = sum(win(c) for _, _, _, c in pairs) / n
        w1 = sum(win(r) for _, _, r, _ in pairs) / n
        b = sum(1 for _, _, r, c in pairs if win(r) > win(c))
        cc = sum(1 for _, _, r, c in pairs if win(r) < win(c))
        pf0 = sum(c["first_death"] == "partner" for _, _, _, c in pairs) / n
        pf1 = sum(r["first_death"] == "partner" for _, _, r, _ in pairs) / n
        pb = sum(1 for _, _, r, c in pairs if (r["first_death"] == "partner") and c["first_death"] != "partner")
        pc = sum(1 for _, _, r, c in pairs if (r["first_death"] != "partner") and c["first_death"] == "partner")
        hf0 = sum(c["first_death"] == "healer" for _, _, _, c in pairs) / n
        hf1 = sum(r["first_death"] == "healer" for _, _, r, _ in pairs) / n
        cc0 = sum(c["healer_hard_cc_secs"] for _, _, _, c in pairs) / n
        cc1 = sum(r["healer_hard_cc_secs"] for _, _, r, _ in pairs) / n
        on0 = sum(c["healer_cc_onsets"] for _, _, _, c in pairs) / n
        on1 = sum(r["healer_cc_onsets"] for _, _, r, _ in pairs) / n
        blocks = collections.defaultdict(list)
        for p in pairs:
            blocks[p[1]].append(p)
        deltas = [100 * (sum(win(r) for _, _, r, _ in v) - sum(win(c) for _, _, _, c in v)) / len(v)
                  for _, v in sorted(blocks.items())]
        per = " / ".join(f"{d:+.1f}" for d in deltas)
        z = mcnemar(b, cc)
        same_sign = all(d > 0 for d in deltas) or all(d < 0 for d in deltas)
        star = "*" if abs(z) >= 1.96 and same_sign and len(deltas) == 3 else ""
        print(f"  {name:22} n={n:5} win {100*w0:5.1f} -> {100*w1:5.1f} ({100*(w1-w0):+5.1f}{star}, z {z:+.1f}; blocks {per})"
              f" | partner first {100*pf0:4.1f} -> {100*pf1:4.1f} (z {mcnemar(pb, pc):+.1f})"
              f" | healer first {100*hf0:4.1f} -> {100*hf1:4.1f}"
              f" | healer hard CC {cc0:4.2f} -> {cc1:4.2f}s ({on0:.2f} -> {on1:.2f} onsets)")

    for rule, pairs in sorted(arms.items()):
        print(f"\n== {rule} (side running it vs its all-shipped twin) ==")
        report("all", pairs)
        for shape, melee in (("H+M", True), ("H+R", False)):
            sub = [p for p in pairs if p[2]["partner_melee"] == melee]
            report(shape, sub)
            for m in ("PillaredArena", "TwinPillars"):
                report(f"  {shape} {m[:8]}", [p for p in sub if p[0] == m])
        for h in ("Priest", "Paladin", "Shaman", "Druid"):
            report(f"healer {h}", [p for p in pairs if p[2]["healer"] == h])
        for shape, melee in (("H+M", True), ("H+R", False)):
            for partner in sorted({p[2]["partner"] for p in pairs if p[2]["partner_melee"] == melee}):
                report(f"{shape} partner {partner}", [p for p in pairs if p[2]["partner"] == partner])


import re

LOGLINE = re.compile(r"^\[\s*([0-9.]+)s\] \[(\w+)\] (.*)$")
UNIT = r"Team [12] \w+ #\d"


def parse_log(path, healer, partner):
    """Damage, heals and crowd control from one match log, seconds after the
    gates. `healer` / `partner` are log names (`Team 1 Druid #2`)."""
    out = {"cc": [], "dmg_h": collections.Counter(), "dmg_p": collections.Counter(),
           "heal_by_h": collections.Counter(), "heals_on_p": collections.Counter(),
           "dmg_by_p": 0, "dmg_by_h": 0}
    gate = 0.0
    last_src = {}
    try:
        lines = open(path).read().splitlines()
    except OSError:
        return None
    for line in lines:
        m = LOGLINE.match(line)
        if not m:
            continue
        t, kind, body = float(m.group(1)), m.group(2), m.group(3)
        if "Gates open" in body:
            gate = t
        if kind == "DMG":
            mm = re.match(rf"({UNIT})'s (.+?) (?:hits|CRITS|crits) ({UNIT}) for (\d+)", body)
            tick = re.match(rf"({UNIT})'s (.+?) ticks for (\d+) damage on ({UNIT})", body)
            if mm or tick:
                if mm:
                    src, abil, tgt, amt = mm.group(1), mm.group(2), mm.group(3), int(mm.group(4))
                else:
                    src, abil, amt, tgt = tick.group(1), tick.group(2), int(tick.group(3)), tick.group(4)
                cls = src.split()[2] + ("" if src.split()[1] != healer.split()[1] else "(own)")
                if src == partner and tgt.split()[1] != partner.split()[1]:
                    out["dmg_by_p"] += amt
                if src == healer and tgt.split()[1] != healer.split()[1]:
                    out["dmg_by_h"] += amt
                if tgt == healer:
                    out["dmg_h"][cls] += amt
                elif tgt == partner:
                    out["dmg_p"][cls] += amt
        elif kind == "HEAL":
            mm = re.match(rf"({UNIT})'s (.+?) heals ({UNIT}) for (\d+)", body)
            if mm:
                src, abil, tgt, amt = mm.group(1), mm.group(2), mm.group(3), int(mm.group(4))
                if src == healer:
                    out["heal_by_h"]["partner" if tgt == partner else ("self" if tgt == healer else "other")] += amt
                if tgt == partner:
                    out["heals_on_p"]["healer" if src == healer else "other"] += amt
        elif kind in ("CC", "CAST"):
            mm = re.match(rf"({UNIT}) (?:uses|casts|begins casting) (.+?) on ({UNIT})", body)
            if mm:
                last_src[(mm.group(2), mm.group(3))] = mm.group(1)
            mm = re.match(rf"(.+?) on ({UNIT}) \(([0-9.]+)s", body) if kind == "CC" else None
            if mm and mm.group(2) == healer and " uses " not in body:
                abil = mm.group(1)
                src = last_src.get((abil, healer), "")
                out["cc"].append({"t": round(t - gate, 2), "ability": abil, "dur": float(mm.group(3)),
                                  "src": src.split()[2] if src else "?"})
    return out


def mech(paths):
    ctl_rows, ctl_logs, arm_rows, arm_logs = paths
    ctl = {}
    for r in rows([ctl_rows]):
        _, m, a, b, seed = r["label"].split("|")
        ctl[(m, a, b, seed, r["side"])] = r

    def names(r, label):
        _, m, a, b, seed = label.split("|")
        team = (a, b)[r["side"] - 1].split("+")
        h = f"Team {r['side']} {r['healer']} #{team.index(r['healer']) + 1}"
        p = f"Team {r['side']} {r['partner']} #{team.index(r['partner']) + 1}"
        return h, p

    for r in rows([arm_rows]):
        bank, m, a, b, seed = r["label"].split("|")
        seat = int(bank.rsplit("-t", 1)[1])
        if r["side"] != seat:
            continue
        c = ctl.get((m, a, b, seed, seat))
        if c is None:
            continue
        h, p = names(r, r["label"])
        la = parse_log(f"{arm_logs}/match_{r['line']:06d}.txt", h, p)
        lc = parse_log(f"{ctl_logs}/match_{c['line']:06d}.txt", h, p)
        if la is None or lc is None:
            continue
        keep = lambda x: {k: x[k] for k in ("result", "first_death", "duration", "healer_hard_cc_secs", "mech")}
        print(json.dumps({"label": r["label"], "map": m, "side": f"{r['partner']}+{r['healer']}",
                          "healer": r["healer"], "partner": r["partner"], "melee": r["partner_melee"],
                          "enemy": (b, a)[seat - 1] if False else (b if seat == 1 else a),
                          "arm": dict(keep(r), log=la), "ctl": dict(keep(c), log=lc)}))


def mechreport(paths):
    pairs = list(rows(paths))
    W = {"win": 1.0, "draw": 0.5, "loss": 0.0}
    HARD = ("Polymorph", "Fear", "Psychic Scream", "Kidney Shot", "Cheap Shot", "Hammer of Justice",
            "Freezing Trap", "Cyclone", "Death Coil", "Intimidating Shout", "Gouge", "Blind", "Sap",
            "Concussion Blow", "Charge", "Intercept", "Howl of Terror", "Scatter Shot")

    def hard(cc):
        return [e for e in cc if e["ability"] in HARD]

    def chains(cc):
        ev = sorted(cc, key=lambda e: e["t"])
        n, end = 0, -9.0
        for e in ev:
            if e["t"] <= end + 0.5:
                n += 1
            end = max(end, e["t"] + e["dur"])
        return n

    def summarize(name, sel):
        if not sel:
            return
        n = len(sel)
        print(f"\n== {name}: {n} pairs")
        rowsout = []
        for arm in ("ctl", "arm"):
            g = [p[arm] for p in sel]
            m = lambda k: sum(x["mech"][k] for x in g) / n
            alive = sum(x["mech"]["alive_secs"] for x in g)
            both = sum(x["mech"]["both_secs"] for x in g) or 1
            rowsout.append((arm, {
                "win": sum(W[x["result"]] for x in g) / n,
                "healer dies first": sum(x["first_death"] == "healer" for x in g) / n,
                "partner dies first": sum(x["first_death"] == "partner" for x in g) / n,
                "healer alive s": m("alive_secs"),
                "hug s": m("hug_secs"),
                "PRESSURED s": m("pressured_secs"),
                "within 4yd of a pillar, share of life": sum(x["mech"]["at_pillar_secs"] for x in g) / alive,
                "yd from pillar skin": sum(x["mech"]["pillar_skin"] for x in g) / alive,
                "yd from partner": sum(x["mech"]["partner_dist"] for x in g) / both,
                "heal link, share of both-alive": sum(x["mech"]["link_secs"] for x in g) / both,
                "enemy melee within 10yd, share": sum(x["mech"]["melee_near_secs"] for x in g) / alive,
                "seen by an enemy caster, share": sum(x["mech"]["seen_secs"] for x in g) / alive,
                "heal on partner (bars)": m("heal_partner"),
                "heal on self (bars)": m("heal_self"),
                "dmg taken healer (bars)": m("dmg_healer"),
                "dmg taken partner (bars)": m("dmg_partner"),
                "mana at 20s": sum(max(x["mech"]["mana20"], 0) for x in g) / max(1, sum(x["mech"]["mana20"] >= 0 for x in g)),
                "mana at 40s": sum(max(x["mech"]["mana40"], 0) for x in g) / max(1, sum(x["mech"]["mana40"] >= 0 for x in g)),
                "OOM s": m("oom_secs"),
                "healer hard CC s": sum(x["healer_hard_cc_secs"] for x in g) / n,
                "hard CC landings on healer (log)": sum(len(hard(x["log"]["cc"])) for x in g) / n,
                "of them chained (within 0.5s of the last end)": sum(chains(hard(x["log"]["cc"])) for x in g) / n,
                "roots/slows on healer (log)": sum(len(x["log"]["cc"]) - len(hard(x["log"]["cc"])) for x in g) / n,
                "healer heals on partner (log)": sum(x["log"]["heal_by_h"].get("partner", 0) for x in g) / n,
                "healer heals on self (log)": sum(x["log"]["heal_by_h"].get("self", 0) for x in g) / n,
                "dmg to healer (log)": sum(sum(x["log"]["dmg_h"].values()) for x in g) / n,
                "dmg dealt by partner (log)": sum(x["log"]["dmg_by_p"] for x in g) / n,
                "dmg dealt by healer (log)": sum(x["log"]["dmg_by_h"] for x in g) / n,
                "dmg to partner (log)": sum(sum(x["log"]["dmg_p"].values()) for x in g) / n,
            }))
        keys = list(rowsout[0][1])
        print(f"  {'':48} {'stance':>9} {'no stance':>10} {'delta':>8}")
        for k in keys:
            a, b = rowsout[0][1][k], rowsout[1][1][k]
            fmt = (lambda v: f"{100*v:8.1f}%") if (k.startswith("win") or "first" in k or "share" in k) else (lambda v: f"{v:9.2f}")
            print(f"  {k:48} {fmt(a)} {fmt(b)} {b-a:+8.2f}")
        for arm in ("ctl", "arm"):
            src = collections.Counter()
            for p in sel:
                for e in hard(p[arm]["log"]["cc"]):
                    src[f"{e['ability']} ({e['src']})"] += 1
            print(f"  CC on healer, {arm}: " + ", ".join(f"{k} {v/n:.2f}" for k, v in src.most_common(7)))
        for who in ("dmg_h", "dmg_p"):
            for arm in ("ctl", "arm"):
                src = collections.Counter()
                for p in sel:
                    for k, v in p[arm]["log"][who].items():
                        src[k] += v
                print(f"  {'damage to healer' if who == 'dmg_h' else 'damage to partner'} by source, {arm}: "
                      + ", ".join(f"{k} {v/n:.0f}" for k, v in src.most_common(5)))

    melee = [p for p in pairs if p["melee"]]
    rng_ = [p for p in pairs if not p["melee"]]
    for nm, grp in (("H+M", melee), ("H+R", rng_)):
        hug = [p for p in grp if p["ctl"]["mech"]["hug_secs"] > 0]
        summarize(f"{nm}, all pairs", grp)
        summarize(f"{nm}, healer hugged in the stance arm", hug)
        summarize(f"{nm}, stance lost -> no stance won", [p for p in grp if p["ctl"]["result"] == "loss" and p["arm"]["result"] == "win"])
        summarize(f"{nm}, stance won -> no stance lost", [p for p in grp if p["ctl"]["result"] == "win" and p["arm"]["result"] == "loss"])


POS_COMPS = {"Warrior+Druid", "Warlock+Shaman", "Warlock+Priest", "Rogue+Shaman", "Rogue+Priest"}
NEG_COMPS = {"Hunter+Druid", "Warlock+Druid", "Warrior+Priest", "Mage+Shaman"}


def paired_rows(ctl_path, arm_path):
    ctl = {}
    for r in rows([ctl_path]):
        _, m, a, b, s = r["label"].split("|")
        ctl[(m, a, b, s, r["side"])] = r
    for r in rows([arm_path]):
        bank, m, a, b, s = r["label"].split("|")
        seat = int(bank.rsplit("-t", 1)[1])
        if r["side"] != seat:
            continue
        c = ctl.get((m, a, b, s, seat))
        if c is not None:
            yield r, c


def event(paths):
    F = ["healP", "healS", "dmgH", "dmgP", "hardCC", "link", "seen", "hug", "press", "atPillar"]
    acc = collections.defaultdict(lambda: collections.defaultdict(float))
    ident = [0, 0]
    for r, c in paired_rows(paths[0], paths[1]):
        side = f"{r['partner']}+{r['healer']}"
        grp = "helped" if side in POS_COMPS else ("hurt" if side in NEG_COMPS else None)
        if grp is None:
            continue
        cs, ns = c["mech"]["series"], r["mech"]["series"]
        t0 = next((i for i, x in enumerate(cs) if x[7] > 0), None)
        key = (grp, r["partner_melee"])
        acc[key]["pairs"] += 1
        if t0 is None:
            continue
        acc[key]["hug"] += 1
        acc[key]["t0"] += t0
        ident[0] += 1
        ident[1] += cs[:t0] == ns[:t0]
        for arm, ser in (("ctl", cs), ("np", ns)):
            for w in (10, 20):
                win = ser[t0:t0 + w]
                for k, f in enumerate(F):
                    acc[key][f"{arm}{w}_{f}"] += sum(x[k] for x in win)
                end = ser[min(t0 + w, len(ser)) - 1]
                acc[key][f"{arm}{w}_mana"] += end[10]
                acc[key][f"{arm}{w}_pHP"] += end[11]
                acc[key][f"{arm}{w}_hHP"] += end[12]
        W_ = {"win": 1, "draw": .5, "loss": 0}
        acc[key]["ctl_win"] += W_[c["result"]]
        acc[key]["np_win"] += W_[r["result"]]
    print(f"identical series before the first hug: {ident[1]}/{ident[0]}")
    for key in sorted(acc):
        a = acc[key]
        n = a["hug"]
        print(f"\n== {key[0]} comps, partner {'melee' if key[1] else 'ranged'}: {int(a['pairs'])} pairs, "
              f"{int(n)} with a hug, first hug {a['t0']/n:.1f}s after the gates; win {a['ctl_win']/n:.3f} -> {a['np_win']/n:.3f}")
        for w in (10, 20):
            print(f"   first {w}s from the hug: " + "  ".join(
                f"{f} {a[f'ctl{w}_{f}']/n:.2f}->{a[f'np{w}_{f}']/n:.2f}" for f in F + ["mana", "pHP", "hHP"]))


def partner(paths):
    import statistics
    win = {}
    on = False
    for line in open(paths[2]):
        if line.startswith("-- by side comp"):
            on = True
            continue
        if on and "top 1" in line:
            break
        mm = re.match(r"\s+(\S+)\s+n=\s*(\d+)\s+([+-][0-9.]+)", line)
        if on and mm:
            win[mm.group(1)] = float(mm.group(3))
    g = collections.defaultdict(list)
    for r, c in paired_rows(paths[0], paths[1]):
        g[f"{r['partner']}+{r['healer']}"].append((r, c))
    out = []
    for comp, ps in g.items():
        def share(i, k, den):
            return sum(x[i]["mech"][k] for x in ps) / max(1e-9, sum(x[i]["mech"][den] for x in ps))

        def mean(i, k):
            return sum(x[i]["mech"][k] for x in ps) / len(ps)
        out.append([win.get(comp, 0.0), comp,
                    100 * (share(0, "partner_uptime_secs", "partner_alive_secs") - share(1, "partner_uptime_secs", "partner_alive_secs")),
                    mean(0, "partner_cc_secs") - mean(1, "partner_cc_secs"),
                    mean(0, "partner_alive_secs") - mean(1, "partner_alive_secs"),
                    mean(0, "heal_partner") - mean(1, "heal_partner"),
                    mean(0, "dmg_partner") - mean(1, "dmg_partner"),
                    mean(0, "healer_hard_cc_secs") - mean(1, "healer_hard_cc_secs") if False else
                    sum(x[0]["healer_hard_cc_secs"] - x[1]["healer_hard_cc_secs"] for x in ps) / len(ps),
                    100 * sum((x[0]["first_death"] == "partner") - (x[1]["first_death"] == "partner") for x in ps) / len(ps)])
    names = ["uptime pt", "partner CC s", "partner alive s", "heal on partner", "dmg to partner", "healer CC s", "partner dies 1st pt"]
    print(f"{'side comp':16}{'12-seed':>8}" + "".join(f"{nm:>17}" for nm in names))
    for r in sorted(out, reverse=True):
        print(f"{r[1]:16}{r[0]:+8.1f}" + "".join(f"{v:+17.2f}" for v in r[2:]))
    for k, nm in enumerate(names):
        print(f"corr(12-seed effect, {nm}) = {statistics.correlation([r[0] for r in out], [r[k + 2] for r in out]):+.2f}")


def read_csv(path):
    import csv
    op = gzip.open if path.endswith(".gz") else open
    with op(path, "rt") as f:
        yield from csv.DictReader(f)


NO_STANCE_ENEMIES = {"Warrior+Rogue", "Warrior+Priest", "Warrior+Hunter", "Warrior+Shaman",
                     "Rogue+Priest", "Rogue+Hunter", "Rogue+Shaman", "Priest+Hunter", "Hunter+Shaman"}


def local(paths):
    import random
    eligible = "--eligible" in paths
    paths = [p for p in paths if p != "--eligible"]
    ctl = {}
    for r in read_csv(paths[0]):
        _, m, a, b, seed = r["label"].split("|")
        ctl[(m, a, b, seed)] = r["winner"]
    pairs = []
    HEAL = {"Priest", "Paladin", "Shaman", "Druid"}
    MELEE = {"Warrior", "Rogue"}

    def score(w, seat):
        return 0.5 if w == "draw" else (1.0 if w == f"team{seat}" else 0.0)

    for r in read_csv(paths[1]):
        bank, m, a, b, seed = r["label"].split("|")
        seat = int(bank.rsplit("-t", 1)[1])
        c = ctl.get((m, a, b, seed))
        if c is None:
            continue
        side, enemy = (a, b) if seat == 1 else (b, a)
        if eligible and ("Paladin" in side or enemy in NO_STANCE_ENEMIES):
            continue
        cls = side.split("+")
        healer = next(x for x in cls if x in HEAL)
        dps = next(x for x in cls if x not in HEAL)
        ecls = enemy.split("+")
        eshape = ("H" if any(x in HEAL for x in ecls) else "") + "".join(
            sorted("M" if x in MELEE else "R" for x in ecls if x not in HEAL))
        pairs.append(dict(map=m, seed=int(seed), block=int(seed) // 10 * 10, side=f"{dps}+{healer}",
                          healer=healer, dps=dps, shape="H+M" if dps in MELEE else "H+R",
                          enemy=enemy, eshape=eshape, d=score(r["winner"], seat) - score(c, seat)))
    rng = random.Random(440)
    seeds = sorted({p["seed"] for p in pairs})
    total_net = sum(p["d"] for p in pairs)

    def stats(sub):
        n = len(sub)
        eff = sum(p["d"] for p in sub) / n
        by = collections.defaultdict(list)
        for p in sub:
            by[p["seed"]].append(p["d"])
        boots = []
        keys = list(by)
        for _ in range(2000):
            draw = [rng.choice(keys) for _ in keys]
            num = sum(sum(by[k]) for k in draw)
            den = sum(len(by[k]) for k in draw)
            boots.append(num / den)
        boots.sort()
        lo, hi = boots[50], boots[1949]
        blocks = collections.defaultdict(list)
        for p in sub:
            blocks[p["block"]].append(p["d"])
        per = [sum(v) / len(v) for _, v in sorted(blocks.items())]
        b = sum(1 for p in sub if p["d"] > 0)
        c = sum(1 for p in sub if p["d"] < 0)
        return n, eff, lo, hi, per, b, c

    def line(name, sub):
        n, eff, lo, hi, per, b, c = stats(sub)
        agree = all(x > 0 for x in per) or all(x < 0 for x in per)
        sig = "*" if (lo > 0 or hi < 0) and agree else (" " if not (lo > 0 or hi < 0) else "~")
        share = 100 * sum(p["d"] for p in sub) / total_net if total_net else 0
        print(f"  {name:28} n={n:5} {100*eff:+5.1f}{sig} [{100*lo:+5.1f}, {100*hi:+5.1f}]"
              f" blocks {' / '.join(f'{100*x:+.1f}' for x in per)}  flips +{b}/-{c}  share of net {share:+5.0f}%")

    print(f"{len(pairs)} paired sides, {len(seeds)} seeds; net {total_net:+.1f} wins ({100*total_net/len(pairs):+.2f}pt)")
    print("(* : 95% seed-clustered interval excludes 0 AND all blocks agree; ~ : interval excludes 0, blocks disagree)")
    line("pooled", pairs)
    for key, title in (("map", "map"), ("shape", "side shape"), ("healer", "healer"), ("dps", "partner"),
                       ("eshape", "enemy shape")):
        print(f"\n-- by {title}")
        for v in sorted({p[key] for p in pairs}):
            line(f"{v}", [p for p in pairs if p[key] == v])
    for key, title in (("side", "side comp"), ("enemy", "enemy comp")):
        print(f"\n-- by {title} (sorted by share of the pooled net)")
        groups = collections.defaultdict(list)
        for p in pairs:
            groups[p[key]].append(p)
        for v, sub in sorted(groups.items(), key=lambda kv: -sum(p["d"] for p in kv[1])):
            line(v, sub)
        nets = sorted((sum(p["d"] for p in sub) for sub in groups.values()), reverse=True)
        cum = 0
        for k, x in enumerate(nets[:5], 1):
            cum += x
            print(f"     top {k}: {100*cum/total_net:.0f}% of the net" if total_net else "")
    print("\n-- side comp x map")
    groups = collections.defaultdict(list)
    for p in pairs:
        groups[(p["side"], p["map"])].append(p)
    for (v, m), sub in sorted(groups.items(), key=lambda kv: -sum(p["d"] for p in kv[1])):
        line(f"{v} {m[:8]}", sub)


if __name__ == "__main__":
    {"diag": diag, "h2h": h2h, "local": local, "mech": mech, "mechreport": mechreport,
     "event": event, "partner": partner}[sys.argv[1]](sys.argv[2:])
