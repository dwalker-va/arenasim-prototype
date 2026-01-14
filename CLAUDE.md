# ArenaSim - Claude Context

This is a WoW Classic-inspired arena combat autobattler built with Rust and Bevy. Teams of 1-3 combatants battle automatically using class-specific abilities, with mechanics inspired by World of Warcraft's PvP system.

## Available Tools

### 1. Headless Match Simulation (`/arena-match`)

Run combat simulations without the graphical client to test changes.

```bash
# Create a config file
echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json

# Run the simulation
cargo run --release -- --headless /tmp/test.json

# Results saved to match_logs/match_*.txt
```

**Config options:**
- `team1`, `team2`: Arrays of class names (Warrior, Mage, Rogue, Priest, Warlock)
- `map`: "BasicArena" or "PillaredArena"
- `team1_kill_target`, `team2_kill_target`: Priority target index (0-based)
- `max_duration_secs`: Timeout (default 300)

Use this to verify combat changes without manual testing.

### 2. Wowhead Classic MCP

Look up WoW Classic spell data for ability implementation reference.

```
mcp__wowhead-classic__lookup_spell("Frostbolt")
mcp__wowhead-classic__lookup_spell_by_id(116)
mcp__wowhead-classic__get_spell_icon("Mortal Strike")
mcp__wowhead-classic__list_known_spells(classFilter: "Mage")
```

Returns: cast time, mana cost, range, cooldown, damage/healing values, spell school, icon URL.

Use this when implementing new abilities to get accurate Classic-era values.

## Project Structure

```
src/
  main.rs                 # Entry point, CLI handling
  cli.rs                  # Command-line argument parsing
  headless/               # Headless simulation mode
    config.rs             # JSON config parsing
    runner.rs             # Match execution without graphics
  combat/
    mod.rs                # CombatPlugin
    log.rs                # Combat logging and match reports
  states/
    mod.rs                # Game states and system registration
    match_config.rs       # MatchConfig, CharacterClass, ArenaMap
    play_match/
      mod.rs              # Match setup, constants
      abilities.rs        # Ability definitions (AbilityType, AbilityDefinition)
      components.rs       # Combatant component, stats, markers
      combat_ai.rs        # Target selection, ability decision logic
      combat_core.rs      # Damage/healing application, interrupts
      auras.rs            # Buffs, debuffs, DoTs
      projectiles.rs      # Projectile travel and hit detection
      match_flow.rs       # Countdown, match end, victory
      rendering.rs        # Health bars, combat text (graphical only)
      camera.rs           # Camera controls (graphical only)
```

## Key Concepts

### Combat Flow
1. **Pre-match** (10s countdown): Combatants can buff, mana restored each frame
2. **Gates open**: Combat begins, AI takes over
3. **Combat loop**: Target acquisition → ability decisions → casting → damage/healing
4. **Match end**: When one team is eliminated, logs saved, results displayed

### Adding a New Ability
1. Add variant to `AbilityType` enum in `abilities.rs`
2. Add `AbilityDefinition` in `get_ability_definition()` with stats
3. Add AI logic in `combat_ai.rs` (`decide_abilities` function)
4. Add effect application in `combat_core.rs` if needed
5. Test with headless simulation

### Class Design
- **Warrior**: Rage (generates on damage), melee, Charge/Mortal Strike/Pummel
- **Mage**: Mana, ranged, Frostbolt/Frost Nova/Polymorph
- **Rogue**: Energy, melee, Stealth/Ambush/Kick/Eviscerate
- **Priest**: Mana, healer, Flash Heal/Mind Blast/Power Word: Fortitude
- **Warlock**: Mana, DoT caster, Corruption/Shadow Bolt/Fear

## Common Tasks

### Test a balance change
```bash
# Make changes, then:
cargo build --release
echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
cat match_logs/$(ls -t match_logs | head -1)
```

### Look up spell data for implementation
```
mcp__wowhead-classic__lookup_spell("Pyroblast")
```

### Run the graphical client
```bash
cargo run --release
```
