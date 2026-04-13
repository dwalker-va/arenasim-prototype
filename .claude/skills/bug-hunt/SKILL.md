---
name: bug-hunt
description: Launch a swarm of agents to run headless arena matches concurrently, analyze combat logs for suspected bugs, cross-reference against known issues, and produce a consolidated bug report.
---

# Bug Hunt — Concurrent Match Analysis

Run a swarm of parallel agents that each execute headless arena matches, analyze combat logs for anomalies and suspected bugs, then compile a deduplicated bug report after filtering out known issues.

## Overview

This skill orchestrates a multi-agent bug hunt:

1. **Generate match configs** — Create diverse scenarios covering all class combos, team sizes, maps, and edge cases
2. **Launch agent swarm** — Spawn 4-6 agents in parallel, each running a batch of matches
3. **Analyze logs** — Each agent reads its match logs and flags suspected bugs
4. **Cross-reference known issues** — Filter findings against `docs/known-issues.md`
5. **Compile report** — Aggregate all findings into a single deduplicated bug report

## Execution Steps

### Step 1: Build the project

```bash
cargo build --release
```

If the build fails, stop and report the error. Do not proceed with broken code.

### Step 2: Generate match configs

Create a diverse set of 20-30 match configs in `/tmp/bug-hunt/`. Cover:

**1v1 matchups** (7 classes = 21 unique pairings + 7 mirrors = 28 total):
- Pick 8-10 diverse 1v1 pairings (prioritize cross-role: melee vs healer, caster vs melee, healer vs healer)

**2v2 matchups** (8-10 configs):
- DPS+Healer vs DPS+Healer
- Double DPS vs DPS+Healer
- Double healer vs DPS+Healer (edge case)
- Include varied kill targets and CC targets

**3v3 matchups** (6-8 configs):
- Standard comps (e.g., Warrior/Mage/Priest vs Rogue/Warlock/Paladin)
- Triple DPS, double healer, all-melee, all-caster edge cases

**Edge case configs** (2-4 configs):
- Mirror matches (identical teams)
- All same class (e.g., 3x Warrior vs 3x Warrior)
- Max size disparity if supported

Each config should use a unique `random_seed` for reproducibility. Use sequential seeds starting from a random base (e.g., 5000-5030).

```bash
mkdir -p /tmp/bug-hunt
# Example config generation
echo '{"team1":["Warrior"],"team2":["Mage"],"random_seed":5001}' > /tmp/bug-hunt/m01_1v1_war_mage.json
echo '{"team1":["Rogue","Priest"],"team2":["Mage","Paladin"],"random_seed":5010,"team1_kill_target":0,"team2_kill_target":0}' > /tmp/bug-hunt/m10_2v2_rp_mp.json
# ... etc for all configs
```

### Step 3: Launch agent swarm

Spawn 4-6 agents using the Agent tool. Each agent receives:
- A batch of 5-7 match config paths to run
- Instructions to run each match, read the log, and analyze for bugs
- The bug detection checklist (below)
- Instructions to return structured findings

**IMPORTANT:** Launch all agents in a single message so they run concurrently.

Each agent should be given this prompt template (customize the config paths per agent):

---

**Agent prompt template:**

You are a combat bug hunter. Run each assigned headless match, read the resulting match log, and analyze it for bugs.

**Your match configs:** [list of /tmp/bug-hunt/mXX_*.json paths]

**For each match, run:**
```bash
cargo run --release -- --headless <config_path>
```

Then read the latest match log:
```bash
ls -t match_logs/match_*.txt | head -1
```

Read the full log file and analyze it against the bug detection checklist below.

**Bug Detection Checklist — scan every match log for these anomalies:**

1. **CC bypass** — Any `[CAST]` or `[DMG]` from a combatant who has an active `[CC]` (stun/fear/poly/incapacitate). Cross-reference timestamps: if a CC applies at T and a cast starts at T+X where X < CC duration, that's a bug.

2. **Actions while dead** — Any `[CAST]`, `[DMG]`, or `[HEAL]` from a combatant after their `[DEATH]` entry. Projectiles already in flight landing after caster death are acceptable (note but don't flag). New casts after death are bugs.

3. **Double death** — A combatant with two `[DEATH]` entries.

4. **Missing combatants** — Config specifies N combatants per team but the log shows fewer in the composition section.

5. **Damage to dead targets** — `[DMG]` entries on a target after that target's `[DEATH]` entry. DoT ticks on dead targets count.

6. **CC on dead targets** — `[CC]` entries applied to targets after their `[DEATH]`.

7. **Duplicate buff stacking** — Same `[BUFF]` applied twice at `0.00s` to the same target (e.g., double Battle Shout, double Devotion Aura).

8. **Friendly fire breaking own CC** — Damage from Team X breaking a CC that Team X applied (e.g., Team 2 Felhunter breaking Team 2 Mage's Polymorph).

9. **CC through immunity** — CC applied to a target with active Divine Shield, Ice Block, or other immunity effects.

10. **Impossible timing** — Cast completing faster than its stated cast time, or instant abilities showing cast bars.

11. **Resource anomalies** — Negative mana/rage/energy, or mana exceeding max without an Arcane Intellect buff.

12. **Match timeout** — If the match hits max_duration_secs (300s default), note the game state. Timeouts in dampening/OOM situations are acceptable; timeouts with both teams at high HP suggest stuck AI.

13. **Zero damage dealt** — A DPS class dealing 0 total damage suggests it was permanently CC'd or bugged AI.

14. **Healing when no healer** — `[HEAL]` events from non-healer classes (except Warlock drain/healthstone or Paladin self-heals).

15. **Spell school lockout violations** — A caster using a spell from a locked school during an active lockout period.

**For each suspected bug, record:**
```
### BUG: [short title]
- **Match:** [config filename]
- **Seed:** [random_seed]
- **Severity:** P0 (critical) / P1 (high) / P2 (medium) / P3 (low/cosmetic)
- **Evidence:** [exact log lines with timestamps]
- **Description:** [what happened and why it's wrong]
- **Reproduction:** [the exact JSON config to reproduce]
```

After running all your matches, return ALL findings as a structured list. If a match has no bugs, note it as clean.

---

### Step 4: Collect and cross-reference

Once all agents complete:

1. **Read known issues:** Read `docs/known-issues.md`
2. **Filter findings:** Remove any bugs that match a known issue entry (by bug category or description)
3. **Deduplicate:** Merge identical bugs found across multiple matches into a single entry with multiple reproduction cases
4. **Count frequency:** Note how many matches each bug appeared in

### Step 5: Write the bug report

Save the report to `docs/reports/YYYY-MM-DD-headless-match-bug-report.md` using today's date.

**Report format:**

```markdown
# Headless Match Bug Report — YYYY-MM-DD

**Matches run:** N (X x 1v1, Y x 2v2, Z x 3v3)
**New bugs found:** N distinct categories
**Known issues confirmed:** N (see docs/known-issues.md)
**Agents used:** N parallel runners

---

## New Bugs

### BUG-N: [title] (Severity)

**Frequency:** N/M matches (list match IDs)
**Severity:** P0/P1/P2/P3

[Description of the bug]

**Examples:**
- [Match ID]: [specific evidence with timestamps]

**Root cause hypothesis:** [theory about what's wrong]

**Reproduction:**
```json
[exact config JSON]
```

---

## Known Issues Confirmed

List any known issues that were observed, with match counts.

## Clean Matches

List matches with no bugs detected.

## Match Results Summary

| # | Format | Teams | Winner | Duration | Bugs |
|---|--------|-------|--------|----------|------|
| ... |

## Observations (Not Bugs)

AI behavior notes, balance concerns, or patterns worth investigating.
```

### Step 6: Report to user

Summarize:
- How many matches ran
- How many new bugs found (vs known issues)
- Top 3 most critical findings
- Link to the full report file

## Known Issues Doc

The cross-reference file is at `docs/known-issues.md`. If it doesn't exist, warn the user but continue — just skip the filtering step and flag all findings.

## Tips

- If a match hangs or times out, kill it after 120s and note the timeout
- Use `--release` builds for speed — debug builds are 10x slower
- Each agent should run matches sequentially (not concurrently) to avoid port/file conflicts
- Agents should use unique output_path per match to avoid log file overwrites:
  ```json
  {"output_path": "/tmp/bug-hunt/log_m01.txt", ...}
  ```
- If cargo build fails, all agents will fail — build once before spawning agents
