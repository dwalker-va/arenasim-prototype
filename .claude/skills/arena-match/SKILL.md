---
name: arena-match
description: Run a headless arena match simulation to test combat system changes. Use when you need to verify balance changes, test new abilities, compare team compositions, or validate bug fixes without launching the graphical client.
---

# Arena Match Simulation

Run headless arena matches to test combat mechanics without the graphical client.

## Quick Start

1. Create a JSON config file with team compositions
2. Run the simulation with `cargo run --release -- --headless <config.json>`
3. Read the match log from `match_logs/match_*.txt`

## JSON Config Format

```json
{
  "team1": ["Warrior", "Priest"],
  "team2": ["Mage", "Rogue"],
  "map": "BasicArena",
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
| `team1_kill_target` | No | Index (0-based) of enemy to prioritize |
| `team2_kill_target` | No | Index (0-based) of enemy to prioritize |
| `max_duration_secs` | No | Timeout in seconds (default: 300) |

### Available Classes

- `Warrior` - Melee fighter, high HP, rage resource
- `Mage` - Ranged caster, frost spells, crowd control
- `Rogue` - Melee burst, stealth, interrupts
- `Priest` - Healer, holy/shadow spells
- `Warlock` - DoT caster, shadow damage, fear

## Running a Match

```bash
# Create config
echo '{"team1":["Warrior","Priest"],"team2":["Mage","Rogue"]}' > /tmp/match.json

# Run simulation
cargo run --release -- --headless /tmp/match.json --max-duration 60

# Find the latest log
ls -t match_logs/ | head -1
```

## Match Log Output

The log includes:
- **Match metadata**: Arena, duration, winner
- **Team compositions**: Final HP/mana, damage dealt/taken, positions
- **Combat log**: Timestamped events (damage, healing, abilities, deaths)

### Example Log Entries

```
[  7.32s] [DMG] Team 1 Warrior's Mortal Strike hits Team 2 Mage for 67 damage
[ 11.36s] [CC] Team 2 Mage casts Frost Nova
[ 16.87s] [DEATH] Team 2 Mage has been eliminated
```

## Workflow Example

When testing a change to an ability:

1. Make the code change
2. Build: `cargo build --release`
3. Run multiple test matches with different compositions
4. Analyze the logs to verify the change works as expected
5. Compare results to baseline if needed

## CLI Options

```
--headless <CONFIG>     Path to JSON config file
--output <PATH>         Custom output path for log (optional)
--max-duration <SECS>   Timeout in seconds (default: 300)
```
