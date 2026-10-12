#!/usr/bin/env python3
"""AS-288: why `ally_in_danger` windows were short, on main before the debounce.

usage: 2026-10-12-as288-causes.py <causes.txt[.gz]>

The input is the instrument's log (`2026-10-12-as288-causes-arm.patch`, run as
`AID_OUT=causes.txt cargo test --release --test aid_causes -- --ignored`): one
`probe|<profile>|<map>|<comp>|<seed>|on|...` line per window onset and one
`...|end|<t>|<team>|<subject>|<secs>|<cause>` line per window end or switch.

For every window of two frames or fewer it prints the onset trigger against
the end cause, the attacker's class and distance where the cause was a lost
pressure, and how soon the same teammate's next window opened.
"""
import collections
import gzip
import re
import sys

FRAME = 1 / 60
SHORT = 2.5 * FRAME


def lines(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        for line in f:
            yield line.rstrip("\n").split("|")


def main(path):
    probe = collections.defaultdict(list)
    for p in lines(path):
        if p[0] == "probe":
            probe[tuple(p[1:5])].append(p[5:])

    pairs = collections.Counter()
    causes = collections.Counter()
    reopen = collections.Counter()
    lengths = collections.Counter()
    for events in probe.values():
        last_on = {}
        for i, ev in enumerate(events):
            key = (ev[2], ev[3])
            if ev[0] == "on":
                last_on[key] = ev
                continue
            secs = float(ev[4])
            lengths[min(round(secs / FRAME), 61)] += 1
            if secs > SHORT:
                continue
            on = last_on.get(key)
            trigger = on[4].split("@")[0] if on else "?"
            if ev[5].startswith("healed"):
                hp = float(re.search(r"\(([0-9.]+)\)", ev[5]).group(1))
                cause = f"healed to {hp:.2f}"
                kind = "healed back over the threshold"
            else:
                cause = re.sub(r"\(([0-9.]+)\)", r" at \1yd", ev[5])
                kind = re.sub(r":\w+ at [0-9.]+yd", "", cause)
            causes[kind] += 1
            pairs[(trigger, cause)] += 1
            gap = next(
                (
                    (float(e2[1]) - float(ev[1])) / FRAME
                    for e2 in events[i + 1 :]
                    if e2[0] == "on" and (e2[2], e2[3]) == key
                ),
                None,
            )
            reopen[
                "never" if gap is None
                else "next frame" if gap <= 1.5
                else "within 1s" if gap <= 60.5
                else "after 1s"
            ] += 1

    total = sum(causes.values())
    print(f"window lengths in frames (61 = over 60): {sorted(lengths.items())}")
    print(f"\nwindows of 2 frames or fewer: {total}")
    print("end cause:")
    for k, v in causes.most_common():
        print(f"  {v:4} ({100 * v / total:4.1f}%)  {k}")
    print("onset trigger -> end cause (attacker class and distance at the end):")
    for (trigger, cause), v in pairs.most_common():
        print(f"  {v:4}  {trigger:14} -> {cause}")
    print("the same teammate's next window opened:")
    for k, v in reopen.most_common():
        print(f"  {v:4}  {k}")


if __name__ == "__main__":
    main(sys.argv[1])
