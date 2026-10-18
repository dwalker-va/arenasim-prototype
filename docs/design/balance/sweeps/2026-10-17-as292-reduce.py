#!/usr/bin/env python3
"""AS-292: how often is pressure ONLY a ranged enemy crossing its closing band?

Reduces one arm of `tests/as292_bank.rs` (the instrument in
`2026-10-17-as292-instrument.patch`): per match, the decision trace and the
instrument's pressure lines. Usage:

    2026-10-17-as292-reduce.py <AS292_DIR>

Classes of pressure, per (frame, teammate), from the instrument's flags:

- `healer-closing only`: the only thing pressuring the teammate is an enemy
  HEALER (Priest, Shaman, Druid) targeting it from inside the closing band
  (beyond its preferred range, within threat_intent_radius) — nothing inside
  danger_radius, no melee or pet within the intent radius, no closing DPS.
- `dps-closing only`: the same, with a ranged DPS (Warlock, Hunter) closing.

Counts per (profile, map):

- healer PRESSURED entries (trace movement_decision into `pressured` from
  another posture) and how many the pressure test on that frame held ONLY by a
  healer closing: under Legacy the onset test (`on`, widened band), under
  TeamPlan the plain test (`p`), as `compound_pressure_onset` reads them;
- healer frames in PRESSURED on which the plain test held only by a healer
  closing (the hold);
- `ally_in_danger` onsets (trace situation_change) whose subject was, on the
  onset frame, pressured only by a healer closing; and dying+pressured frames.
"""
import bisect
import collections
import glob
import json
import os
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def frame(t):
    return int(round(float(t) * 60))


def parse_press(path):
    """{(frame, entity): flags} — the last classification in a frame wins."""
    out = {}
    if not os.path.exists(path):
        return out
    with open(path) as f:
        for line in f:
            parts = line.split()
            t, team, ent, cls = parts[0], int(parts[1]), int(parts[2]), parts[3]
            kv = dict(p.split("=") for p in parts[4:])
            flags = {k: (v in ("1", "true")) for k, v in kv.items() if k != "prof"}
            flags["class"] = cls
            flags["team"] = team
            out[(frame(t), ent)] = flags
    return out


def only_healer(fl, onset=False):
    if fl is None:
        return False
    other = fl["prox"] or fl["mp"] or (fl["ocd"] if onset else fl["cd"])
    return (fl["och"] if onset else fl["ch"]) and not other


def only_dps(fl, onset=False):
    if fl is None:
        return False
    other = fl["prox"] or fl["mp"] or (fl["och"] if onset else fl["ch"])
    return (fl["ocd"] if onset else fl["cd"]) and not other


def main(d):
    agg = collections.defaultdict(collections.Counter)
    for tr in sorted(glob.glob(os.path.join(d, "*.trace.jsonl"))):
        stem = tr[: -len(".trace.jsonl")]
        profile, amap = os.path.basename(stem).split("_")[:2]
        key = (profile, amap)
        c = agg[key]
        c["matches"] += 1
        press = parse_press(stem + ".press.log")
        # Posture intervals per healer entity, from the trace's transitions.
        postures = collections.defaultdict(list)  # ent -> [(frame, posture)]
        onsets = []
        with open(tr) as f:
            for line in f:
                try:
                    e = json.loads(line)
                except ValueError:
                    continue
                if e.get("kind") == "movement_decision":
                    a = e["actor"]
                    if a["class"] not in HEALERS:
                        continue
                    fr = frame(e["sim_time"])
                    postures[a["entity_id"]].append((fr, e["posture"]))
                    prev = e.get("previous_posture")
                    if e["posture"] == "pressured" and prev and prev != "pressured":
                        c["entries"] += 1
                        fl = press.get((fr, a["entity_id"]))
                        onset = profile == "Legacy"
                        if fl is None:
                            c["entries_no_pressure"] += 1
                        elif only_healer(fl, onset):
                            c["entries_healer_only"] += 1
                        elif only_dps(fl, onset):
                            c["entries_dps_only"] += 1
                elif e.get("kind") == "situation_change" and e.get("name") == "ally_in_danger":
                    if e["change"] == "onset":
                        onsets.append((frame(e["sim_time"]), e["subject"]))
        # Hold frames: in PRESSURED and the plain test held only by a healer.
        by_ent = collections.defaultdict(list)
        for (fr, ent), fl in press.items():
            by_ent[ent].append((fr, fl))
        for ent, seq in postures.items():
            seq.sort(key=lambda x: x[0])
            frames = [f0 for f0, _ in seq]
            for fr, fl in by_ent.get(ent, []):
                i = bisect.bisect_right(frames, fr) - 1
                if i >= 0 and seq[i][1] == "pressured":
                    c["pressured_frames_with_pressure"] += 1
                    if only_healer(fl):
                        c["pressured_frames_healer_only"] += 1
        for (fr, ent), fl in press.items():
            if fl["class"] in HEALERS and fl["p"]:
                c["healer_p_frames"] += 1
                c["healer_p_frames_healer_only"] += only_healer(fl)
                c["healer_p_frames_dps_only"] += only_dps(fl)
            if fl["dying"] and fl["p"]:
                c["danger_frames"] += 1
                c["danger_frames_healer_only"] += only_healer(fl)
                c["danger_frames_dps_only"] += only_dps(fl)
        for fr, subj in onsets:
            c["aid_onsets"] += 1
            fl = press.get((fr, subj))
            if fl is None:
                c["aid_onsets_no_line"] += 1
            elif only_healer(fl):
                c["aid_onsets_healer_only"] += 1
            elif only_dps(fl):
                c["aid_onsets_dps_only"] += 1

    def pct(a, b):
        return f"{a} / {b} ({100.0 * a / b:.1f}%)" if b else f"{a} / 0"

    for key in sorted(agg):
        c = agg[key]
        print(f"== {key[0]} {key[1]}: {c['matches']} matches")
        print(f"  healer PRESSURED entries, healer-closing only: {pct(c['entries_healer_only'], c['entries'])}"
              f"  (dps-closing only {c['entries_dps_only']}, no pressure line {c['entries_no_pressure']})")
        print(f"  healer frames in PRESSURED with pressure, held only by a healer closing: "
              f"{pct(c['pressured_frames_healer_only'], c['pressured_frames_with_pressure'])}")
        print(f"  healer pressured frames, healer-closing only: {pct(c['healer_p_frames_healer_only'], c['healer_p_frames'])}"
              f"  (dps-closing only {c['healer_p_frames_dps_only']})")
        print(f"  ally_in_danger onsets, healer-closing only: {pct(c['aid_onsets_healer_only'], c['aid_onsets'])}"
              f"  (dps-closing only {c['aid_onsets_dps_only']}, no line {c['aid_onsets_no_line']})")
        print(f"  dying+pressured frames, healer-closing only: {pct(c['danger_frames_healer_only'], c['danger_frames'])}"
              f"  (dps-closing only {c['danger_frames_dps_only']})")


if __name__ == "__main__":
    main(sys.argv[1])
