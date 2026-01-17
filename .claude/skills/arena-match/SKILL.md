---
name: arena-match
description: Run a headless arena match simulation to test combat system changes. Use when you need to verify balance changes, test new abilities, compare team compositions, or validate bug fixes without launching the graphical client.
---

# Arena Match Simulation

Run headless arena matches to test combat mechanics without the graphical client.

## Commands

### Single Match

Run a single match with custom teams:

```bash
# Create config
echo '{"team1":["Warrior"],"team2":["Mage"],"random_seed":42}' > /tmp/match.json

# Run simulation
cargo run --release -- --headless /tmp/match.json

# View latest log
cat match_logs/$(ls -t match_logs | head -1)
```

### Regression Test Suite

Run the full regression test suite to verify combat system changes:

```bash
# Run all tests (18 scenarios)
./scripts/run_combat_tests.sh

# Run with more parallelism
./scripts/run_combat_tests.sh -j 8

# Run quietly (summary only)
./scripts/run_combat_tests.sh -q

# Compare against a baseline
./scripts/run_combat_tests.sh -b match_logs/regression_baseline
```

**Test Suite Options:**
- `-j, --jobs N` - Number of concurrent tests (default: 4)
- `-t, --timeout N` - Timeout per test in seconds (default: 60)
- `-q, --quiet` - Only show summary
- `-b, --baseline DIR` - Compare results against baseline

## JSON Config Format

```json
{
  "team1": ["Warrior", "Priest"],
  "team2": ["Mage", "Rogue"],
  "map": "BasicArena",
  "random_seed": 42,
  "team1_kill_target": 0,
  "team2_kill_target": 1,
  "max_duration_secs": 120
}
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `team1` | Yes | Array of 1-3 class names for team 1 |
| `team2` | Yes | Array of 1-3 class names for team 2 |
| `map` | No | `"BasicArena"` (default) or `"PillaredArena"` |
| `random_seed` | No | RNG seed for deterministic matches |
| `team1_kill_target` | No | Index (0-based) of enemy to prioritize |
| `team2_kill_target` | No | Index (0-based) of enemy to prioritize |
| `max_duration_secs` | No | Timeout in seconds (default: 300) |
| `output_path` | No | Custom path for match log output |

### Available Classes

- `Warrior` - Melee fighter, high HP, rage resource (Battle Shout, Charge, Mortal Strike, Rend, Pummel)
- `Mage` - Ranged caster, frost spells (Ice Barrier, Frostbolt, Frost Nova, Arcane Intellect)
- `Rogue` - Melee burst, stealth (Ambush, Sinister Strike, Kidney Shot, Kick)
- `Priest` - Healer, holy/shadow (Flash Heal, Mind Blast, PW:Shield, PW:Fortitude)
- `Warlock` - DoT caster, shadow (Corruption, Shadow Bolt, Fear)

## Match Log Format

Logs are saved to `match_logs/match_<timestamp>.txt` and include:

**Header:**
```
================================================================================
ARENA MATCH REPORT
================================================================================

MATCH METADATA
--------------------------------------------------------------------------------
Arena: Basic Arena
Duration: 22.73s
Winner: Team 2
```

**Team Compositions:**
```
TEAM 1 COMPOSITION
--------------------------------------------------------------------------------
  Slot 1: Warrior (HP: 0/200, Mana: 68/100)
    Position: (18.72, 1.00, -3.00)
    Damage Dealt: 100, Damage Taken: 200
```

**Combat Log Events:**
```
[  0.00s] [EVENT] Match started (headless mode)!
[  0.00s] [CAST] Team 1 Warrior uses Battle Shout
[ 15.46s] [DMG] Team 2 Mage's Frostbolt hits Team 1 Warrior for 51 damage
[ 18.67s] [CAST] Team 1 Warrior uses Pummel to interrupt enemy cast
[ 22.70s] [DEATH] Team 1 Warrior has been eliminated
```

## Workflow Examples

### Testing a Balance Change

1. Create a baseline:
   ```bash
   ./scripts/run_combat_tests.sh -o match_logs/regression_baseline
   ```

2. Make your code changes

3. Run tests and compare:
   ```bash
   cargo build --release
   ./scripts/run_combat_tests.sh -b match_logs/regression_baseline
   ```

### Testing a Specific Matchup

```bash
# Test Warrior vs Mage with same seed multiple times
for i in {1..5}; do
  echo "{\"team1\":[\"Warrior\"],\"team2\":[\"Mage\"],\"random_seed\":$i}" > /tmp/test.json
  cargo run --release -- --headless /tmp/test.json
done

# Analyze results
grep "^Winner:" match_logs/match_*.txt | sort | uniq -c
```

### Debugging a Specific Ability

```bash
# Run match with specific seed
echo '{"team1":["Mage"],"team2":["Priest"],"random_seed":42}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json

# Search for specific ability usage
grep "Mind Blast" match_logs/$(ls -t match_logs/match_*.txt | head -1)
```

## Test Suite Structure

The test suite (`tests/combat/test_suite.json`) includes:

**1v1 Matchups (10 tests):**
- All unique class pairings
- Mirror matches (Warrior vs Warrior, Mage vs Mage)

**2v2 Matchups (5 tests):**
- Double DPS vs healer/DPS
- Warrior/Healer mirrors
- Caster cleave vs melee cleave

**3v3 Matchups (3 tests):**
- Triple DPS (no healing)
- Standard teams with healers
- Double healer vs triple DPS

Each test includes expected outcomes like:
- `completes: true` - Match should finish (not timeout)
- `max_duration: 60` - Should complete within 60 seconds
