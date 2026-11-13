#!/usr/bin/env python3
"""AS-440 reducer: rows from `tests/healer_hide_exposure.rs`.

    2026-11-13-as440-reduce.py diag rows.jsonl[.gz] ...
    2026-11-13-as440-reduce.py h2h ctl.rows.jsonl[.gz] h2h.rows.jsonl[.gz] ...

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


if __name__ == "__main__":
    {"diag": diag, "h2h": h2h}[sys.argv[1]](sys.argv[2:])
