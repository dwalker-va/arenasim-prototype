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
      mod.rs              # Match setup, plugin registration
      abilities.rs        # AbilityType enum, spell schools, range checking
      ability_config.rs   # Data-driven ability loading from RON
      components/         # ECS components (split by concern)
        mod.rs            # Combatant, casting, resource systems
        auras.rs          # Aura/buff/debuff types
        visual.rs         # Floating combat text, visual effects
      class_ai/           # Class-specific AI decision logic
        mod.rs            # ClassAI trait, CombatContext
        warrior.rs        # Warrior ability priorities
        mage.rs           # Mage kiting, control logic
        rogue.rs          # Rogue stealth, burst logic
        priest.rs         # Priest healing priorities
        warlock.rs        # Warlock DoT management
      combat_ai.rs        # Target selection, interrupt timing
      combat_core.rs      # Damage/healing application, casting
      constants.rs        # Centralized magic numbers (GCD, ranges, etc.)
      systems.rs          # Systems API layer for headless mode
      utils.rs            # Shared helper functions
      auras.rs            # Aura tick/expiration systems
      projectiles.rs      # Projectile travel and hit detection
      match_flow.rs       # Countdown, match end, victory
      rendering.rs        # Health bars, combat text (graphical only)
      camera.rs           # Camera controls (graphical only)

assets/
  config/
    abilities.ron         # Data-driven ability definitions
```

## Documentation Index

For deeper context, see these focused references:

- **[Session Notes](design-docs/session-notes.md)** - Full development history (16 sessions)
- **[WoW Mechanics](design-docs/wow-mechanics.md)** - Implemented game mechanics (CC, resources, combat)
- **[Bevy Patterns](design-docs/bevy-patterns.md)** - Rust/Bevy learnings and common pitfalls
- **[Roadmap](design-docs/roadmap.md)** - Long-term TODOs and milestones
- **[Stat Scaling](design-docs/stat-scaling-system.md)** - Damage/healing formulas and coefficients
- **[Game Design](design-docs/game-design-doc.md)** - High-level game vision

## Key Concepts

### Combat Flow
1. **Pre-match** (10s countdown): Combatants can buff, mana restored each frame
2. **Gates open**: Combat begins, AI takes over
3. **Combat loop**: Target acquisition → ability decisions → casting → damage/healing
4. **Match end**: When one team is eliminated, logs saved, results displayed

### Adding a New Ability

Abilities are data-driven via `assets/config/abilities.ron`. To add a new ability:

1. **Add variant to `AbilityType` enum** in `abilities.rs`:
   ```rust
   pub enum AbilityType {
       // ... existing abilities
       NewAbility,
   }
   ```

2. **Add definition to `abilities.ron`**:
   ```ron
   NewAbility: (
       name: "New Ability",
       cast_time: 1.5,        // 0.0 for instant
       range: 40.0,           // Use MELEE_RANGE (2.5) for melee
       mana_cost: 25.0,
       cooldown: 10.0,
       damage_base_min: 15.0,
       damage_base_max: 25.0,
       damage_coefficient: 0.5,
       damage_scales_with: SpellPower,  // or AttackPower
       spell_school: Fire,    // Physical, Fire, Frost, Shadow, Arcane, Holy, Nature
       // Optional fields:
       applies_aura: Some((
           aura_type: MovementSpeedSlow,
           duration: 5.0,
           magnitude: 0.5,
           break_on_damage: 0.0,  // 0 = doesn't break
       )),
       projectile_speed: Some(35.0),
       projectile_visuals: Some((color: (1.0, 0.5, 0.0), emissive: (1.5, 0.8, 0.0))),
   )
   ```

3. **Add AI logic** in the appropriate `class_ai/<class>.rs` file:
   - Implement when to use the ability in the class's `decide_action()` method
   - Use `CombatContext` helpers like `ctx.target_info()`, `ctx.has_aura()`, etc.

4. **Add special handling** in `combat_core.rs` if the ability has unique mechanics
   (most abilities work automatically via the config)

5. **Test with headless simulation**:
   ```bash
   cargo run --release -- --headless /tmp/test.json
   ```

**Available aura types**: `Absorb`, `Root`, `Stun`, `Fear`, `MovementSpeedSlow`, `HealingReduction`, `DamageOverTime`, `MaxHealthIncrease`, `MaxManaIncrease`, `SpellLockout`

**Tip**: Use the Wowhead MCP to look up accurate WoW Classic values:
```
mcp__wowhead-classic__lookup_spell("Pyroblast")
```

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
