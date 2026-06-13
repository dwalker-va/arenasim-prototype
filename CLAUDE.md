# ArenaSim - Claude Context

This is a WoW Classic-inspired arena combat autobattler built with Rust and Bevy. Teams of 1-3 combatants battle automatically using class-specific abilities, with mechanics inspired by World of Warcraft's PvP system.

## Git Commits

Never include attribution footers in commit messages. No `Co-Authored-By`, no `Generated with [Claude Code]`, no emoji badges. Just the commit subject and body.

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
- `team1`, `team2`: Arrays of class names (Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter)
- `map`: "BasicArena" or "PillaredArena"
- `team1_kill_target`, `team2_kill_target`: Priority target index (0-based)
- `max_duration_secs`: Timeout (default 300)

Use this to verify combat changes without manual testing.

### 2. Wowhead Classic MCP

Look up WoW Classic spell and item data for implementation reference.

**Spell Tools** — use when implementing new abilities:
```
mcp__wowhead-classic__lookup_spell("Frostbolt")
mcp__wowhead-classic__lookup_spell_by_id(116)
mcp__wowhead-classic__get_spell_icon("Mortal Strike")
mcp__wowhead-classic__list_known_spells(classFilter: "Mage")
```
Returns: cast time, mana cost, range, cooldown, damage/healing values, spell school, icon URL.

**Item Tools** — use when adding equipment or verifying item stats:
```
mcp__wowhead-classic__lookup_item("Arcanite Reaper")
mcp__wowhead-classic__lookup_item_by_id(12784)
mcp__wowhead-classic__get_item_icon("Lionheart Helm")
mcp__wowhead-classic__list_known_items(typeFilter: "Plate")
mcp__wowhead-classic__list_known_items(slotFilter: "Head")
```
Returns: item level, slot, armor type, armor value, damage/speed, bonus stats (stamina, intellect, etc.), equip effects, quality, icon URL.

Use spell tools when implementing new abilities to get accurate Classic-era values.
Use item tools when adding items to `items.ron` or downloading equipment icons.

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
        paladin.rs        # Paladin healing and utility
        hunter.rs         # Hunter ranged DPS and pet management
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
    items.ron             # Equipment item definitions (stats, slots, armor)
    loadouts.ron          # Default per-class equipment loadouts
    movement.ron          # Healer posture AI weights, radii, thresholds
```

## Documentation Index

For deeper context, see these focused references:

- **[Session Notes](design-docs/session-notes.md)** - Full development history (16 sessions)
- **[WoW Mechanics](design-docs/wow-mechanics.md)** - Implemented game mechanics (CC, resources, combat)
- **[Bevy Patterns](design-docs/bevy-patterns.md)** - Rust/Bevy learnings and common pitfalls
- **[Roadmap](design-docs/roadmap.md)** - Long-term TODOs and milestones
- **[Stat Scaling](design-docs/stat-scaling-system.md)** - Damage/healing formulas and coefficients
- **[Game Design](design-docs/game-design-doc.md)** - High-level game vision
- **[Documented Solutions](docs/solutions/)** - Documented solutions to past problems (bugs, implementation patterns, workflows) organized by category, with YAML frontmatter (`module`, `tags`, `category`). Relevant when implementing or debugging in documented areas.

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

2. **Add to validation list** in `ability_config.rs`:
   - Add `AbilityType::NewAbility` to the `expected_abilities` array in `validate()`

3. **Add definition to `abilities.ron`**:
   ```ron
   NewAbility: (
       name: "New Ability",
       icon: "icons/abilities/<icon_name>.jpg",
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

4. **Add AI logic** in the appropriate `class_ai/<class>.rs` file:
   - Implement when to use the ability in the class's `decide_action()` method
   - Use `CombatContext` helpers like `ctx.target_info()`, `ctx.has_aura()`, etc.
   - **AI decision trace** — at each predicate gate that rejects this ability,
     call `builder.reject(AbilityType::NewAbility, RejectionReason::...)`
     (use `classify_pre_cast_failure` for `pre_cast_ok` failures). On the
     success branch, call `builder.choose(ability, target, was_instant)`.
     This is mechanical instrumentation — no new module-level wiring is
     needed; the builder is already threaded through every class AI
     function. See `class_ai/warrior.rs` for the canonical pattern.

5. **Add spell icon** for the ability timeline UI:
   - Download icon: `mcp__wowhead-classic__get_spell_icon("New Ability")` to get the URL
   - Save to `assets/icons/abilities/<icon_name>.jpg`
   - Add `icon: "icons/abilities/<icon_name>.jpg"` to the ability entry in `abilities.ron`

6. **Add special handling** in `combat_core.rs` if the ability has unique mechanics
   (most abilities work automatically via the config)

7. **Test with headless simulation**:
   ```bash
   cargo run --release -- --headless /tmp/test.json
   ```

**Available aura types**: `Absorb`, `Root`, `Stun`, `Fear`, `MovementSpeedSlow`, `HealingReduction`, `DamageOverTime`, `MaxHealthIncrease`, `MaxManaIncrease`, `SpellLockout`

**Tip**: Use the Wowhead MCP to look up accurate WoW Classic values:
```
mcp__wowhead-classic__lookup_spell("Pyroblast")
```

### Adding a New Item

Items are data-driven via `assets/config/items.ron`. Every item must stay within its **item level budget** — enforced by `cargo test`.

1. **Add entry to `items.ron`**:
   ```ron
   NewItem: (
       name: "New Item",
       item_level: 58,
       slot: Head,
       armor_type: Plate,        // Plate, Mail, Leather, Cloth, or omit for accessories
       armor: 290.0,             // Free stat — does not consume budget
       max_health: 12.0,
       attack_power: 6.0,
       crit_chance: 0.01,
   )
   ```

2. **Check the item level budget** before finalizing stats:
   - Effective budget = `item_level × 0.75 × slot_multiplier`
   - Slot multipliers: Head/Chest = 1.0, Legs = 0.875, Shoulders/Hands/Feet = 0.75, Waist = 0.625, Wrists = 0.5, accessories/weapons = 0.5625
   - Stat costs: max_health/max_mana = 1.0/pt, attack_power/spell_power = 1.5/pt, crit_chance = 300.0/pt (0.01 = 3.0), movement_speed = 30.0/pt (0.1 = 3.0), resistances = 0.4/pt, mana_regen = 5.0/pt
   - **Free stats** (excluded from budget): `armor`, `attack_damage_min`, `attack_damage_max`, `attack_speed`
   - Budget usage = sum of (stat_value × weight) across all non-free stats
   - Items may exceed the budget by up to 5% tolerance

3. **Add to a class loadout** in `loadouts.ron` if it should be default equipment

4. **Add `ItemId` variant** in `equipment.rs` to the `ItemId` enum

5. **Add item icon** (optional, for UI):
   - Download: `mcp__wowhead-classic__get_item_icon("New Item")`
   - Save to `assets/icons/items/<icon_name>.jpg`
   - Add mapping to `ITEM_ICON_PATHS` in `rendering/mod.rs`

6. **Run `cargo test`** to verify the item passes budget validation

**Tip**: Use the Wowhead MCP to look up WoW Classic item stats as a reference:
```
mcp__wowhead-classic__lookup_item("Lionheart Helm")
```

### Tuning healer posture behavior

The Priest and Paladin movement AI (the FREE/PRESSURED/ESCAPE/DIP posture
state machine) is fully data-driven via `assets/config/movement.ron`. Loaded
and validated identically in headless and graphical modes by
`MovementConfigPlugin` (`src/states/play_match/movement_config.rs`), which
panics at startup if the file is missing, malformed, or fails `validate()`.
Every field has a struct default, so a partial file overrides one value
without restating the rest. No code changes are needed to retune behavior —
edit the RON, run `cargo test`, then sweep with the matrix.

**Shared params** (`shared:` block — radii in yards, windows in seconds,
fractions in 0..1; values below are the shipped defaults):
- `danger_radius: 12.0` — a targeting/melee/pet/closing enemy inside this flips PRESSURED
- `threat_intent_radius: 30.0` — intent bound on the melee/pet/closing trigger branch
- `heal_range: 40.0` — PRESSURED constraint: stay within heal range of the anchor ally
- `formation_offset: 8.0` — FREE formation point offset behind the engaged-ally centroid
- `center_bias: 0.3` — FREE formation point bias toward arena center (fraction)
- `commit_window: 0.6` — committed-direction window (anti-zigzag; band 0.4–0.8)
- `pressured_hold: 1.5` — hysteresis floor before PRESSURED may relax to FREE
- `directive_ttl: 1.0` — `MovementDirective` TTL (must cover `commit_window`)
- `escape_min_window: 0.5` — ESCAPE windows shorter than this are ignored
- `urgency_hp_threshold: 0.5` — defer non-critical casts during ESCAPE/DIP unless an ally is below this HP fraction
- `anchor_switch_margin: 0.1` — sticky-anchor switch requires this HP-fraction injury margin
- `wand_range: 30.0` — wand-range pull target distance (Priest)

**Per-class scorer weights** (`priest.weights:` / `paladin.weights:` —
`score_directions` term weights; `0.0` disables a term):
- `threat_repulsion` (3.0/3.0) — pull away per visible threat, weighted by proximity
- `ally_anchor` (1000.0) — HARD constraint: outside heal range of the anchor ally; must dominate all soft terms (enforced by `validate()`)
- `formation_pull` (Priest 2.0 / Paladin 0.0) — pull toward the FREE backline point (Paladin keeps its melee identity, so 0 disables it)
- `boundary_penalty` (1000.0) — HARD constraint: never score an out-of-bounds step
- `corner_penalty` (Priest 6.0 / Paladin 4.0) — graded penalty approaching arena corners
- `wand_pull` (Priest 0.5 / Paladin 0.0) — low-weight pull toward wand range of the kill target (`0.0` disables it for the wandless Paladin)
- `commitment_bonus` (1.5/1.5) — bonus toward the committed direction during the commit window

**Paladin-only block** (`paladin:` — alongside its `weights:`):
- `fallback_range: 15.0` — PRESSURED retreat range (instead of face-tanking at melee)
- `dip_budget: 6.0` — DIP walk-stun-return duration budget in seconds
- `healing_heavy_hp: 0.6` — lowest team HP fraction (self included, pets excluded) below which the Paladin pulls to fallback range even before it is focused

After editing, validate and sweep:
```bash
cargo test                          # validate() + posture probes/unit tests
scripts/hunter_2v2_matrix.sh 100    # 2v2-with-healer balance sweep (adapt teams as needed)
```

### Class Design
- **Warrior**: Rage (generates on damage), melee, Charge/Mortal Strike/Pummel
- **Mage**: Mana, ranged, Frostbolt/Frost Nova/Polymorph
- **Rogue**: Energy, melee, Stealth/Ambush/Kick/Eviscerate
- **Priest**: Mana, healer, Flash Heal/Mind Blast/Power Word: Fortitude
- **Warlock**: Mana, DoT caster, Corruption/Shadow Bolt/Fear
- **Paladin**: Mana, healer/melee, Holy Shock/Flash of Light/Hammer of Justice
- **Hunter**: Mana, ranged physical DPS with pet, Aimed Shot/Arcane Shot/Concussive Shot/Disengage/Freezing Trap/Frost Trap. Pet engagement model: pet inherits Hunter's target, pursues into melee via existing target-pursuit movement, and retreats ("Heel") when pet HP drops below 25%. Per-pet headline abilities (Spider Web, Boar Charge, Master's Call) are dispatched by Hunter AI via the `PetCommand` component (hybrid model — Hunter owns headline calls, pet handles auto-attacks and pursuit). When Hunter is mid-cast (CastingState excludes it from `decide_abilities`), `pet_ai_system` falls back to autonomous dispatch using the same predicate logic; trace events distinguish via `dispatched_by` (set for Hunter dispatch, omitted for autonomous). Iteration 2a shipped pet target ownership + Heel predicate + PetCommand framework; iteration 2b shipped the Hunter `try_dispatch_*` helpers plus the pet-side `pet_command_rejection` authoritative check, with the autonomous fallback kept to cover Hunter's CastingState windows.

## Common Tasks

### Test a balance change
```bash
# Make changes, then:
cargo build --release
echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
cat match_logs/$(ls -t match_logs | head -1)
```

### Run a 2v2-with-healer balance sweep

`--matrix N` runs the 7×7 1v1 matrix. For 2v2-with-healer validation
(Hunter+Priest vs each-class+Priest), use the wrapper script:

```bash
# Default N=100, output to match_logs/hunter_2v2_<timestamp>.csv
cargo build --release
scripts/hunter_2v2_matrix.sh 100 --seed-base 0

# Custom output path (e.g., commit to design-docs/balance/)
scripts/hunter_2v2_matrix.sh 100 \
  --seed-base 0 \
  --out design-docs/balance/matrix_baseline_<date>_2v2.csv
```

CSV columns are byte-compatible with the 1v1 matrix output from
`src/headless/matrix.rs:217` (`team1,team2,runs,team1_wins,team2_wins,
draws,team1_winrate,draw_rate,avg_duration_secs`).

### Diagnose AI behaviour with the decision trace

Capture the AI's per-tick reject/choose decisions as JSONL alongside the
match log. The trace shows every ability the AI considered with a typed
rejection reason (out of range, on cooldown, friendly-CC guard, etc.) —
turns "why didn't X cast Y?" from a code-read into a `jq` query.

```bash
# Single match — opt in via --trace-mode on
cargo run --release -- --headless /tmp/test.json --trace-mode on
# Trace lands at match_logs/match_<timestamp>_trace.jsonl

# Matrix run — trace is on by default; opt out with --trace-mode off
cargo run --release -- --matrix 100
# 4900 files at match_logs/traces/match_<seed>_<c1>_v_<c2>_trace.jsonl

# Common jq recipes (assumes a trace file):
T=match_logs/match_*_trace.jsonl

# All rejection reasons for Hunter across the whole match
jq -r 'select(.actor.class == "Hunter") | .candidates[] | select(.status == "rejected") | .reason | if type == "object" then keys[0] else . end' $T | sort | uniq -c

# Why didn't Hunter cast Aimed Shot? Show rejections by reason
jq -c 'select(.actor.class == "Hunter") | .candidates[] | select(.ability == "AimedShot" and .status == "rejected") | .reason' $T | sort | uniq -c

# Target switches over the match (when did Rogue switch from Paladin to Mage?)
jq -c 'select(.kind == "target_acquisition" and .changed)' $T

# Pet decisions grouped by owner
jq -c 'select(.kind == "pet_decision") | {owner, pet_type, ability: .outcome.ability}' $T

# Hunter-dispatched pet abilities (hybrid model — `dispatched_by` set when
# the pet's owner AI commanded the ability instead of the pet deciding
# autonomously). Field is `Option<u32>` and omitted from JSON when None;
# this recipe filters to non-null values.
jq -c 'select(.kind == "pet_decision" and .dispatched_by != null) | {owner, pet_type, ability: .outcome.ability, dispatched_by}' $T

# Heel-state retreats (pet HP < 25%, target cleared, returns to owner's
# flank, queued PetCommand despawned without execution)
jq -c 'select(.kind == "pet_decision") | .candidates[]? | select((.reason | if type == "object" then keys[0] else . end) == "LowHealthHeel")' $T | wc -l

# Healer posture transitions over time (movement_decision events fire on
# posture transitions + committed direction changes only — never per-tick).
# `previous_posture` is present only on real transitions; re-commits
# (CommitExpired / FormationShift) omit it.
jq -c 'select(.kind == "movement_decision" and .actor.class == "Priest") | {t: .sim_time, from: .previous_posture, to: .posture, trigger}' $T

# Movement trigger histogram (triggers are unit variants — bare strings,
# no object unwrapping needed)
jq -r 'select(.kind == "movement_decision") | .trigger' $T | sort | uniq -c

# Position track for one entity at its movement decisions (coarse path
# sketch — decision points only, not a per-tick trail; use the probe
# harness for full timelines)
jq -c 'select(.kind == "movement_decision" and .actor.entity_id == 7) | {t: .sim_time, position, posture}' $T

# Scorer term breakdown — which weighted terms drove a Priest's chosen
# direction? (`scorer_terms` is a {name: value} map, present only when the
# decision ran the scorer; re-commits / Point goals omit it.)
jq -c 'select(.kind == "movement_decision" and .actor.class == "Priest" and .scorer_terms != null) | {t: .sim_time, posture, dir: .chosen_direction, terms: .scorer_terms}' $T

# Masked candidates — the `masked` field is a u16 bitmask over the 16 compass
# directions (bit i set when candidate i was eliminated by the boundary or
# ally-anchor mask). Present only when the scorer ran. A value of 65535
# (0xFFFF) is an all-masked frame, where the fallback ladder fired — the ONLY
# legitimate source of Part A behavior divergence from the old penalty scheme,
# so this is the query R6 byte-identity attribution uses on a divergent cell.
jq -c 'select(.kind == "movement_decision" and .masked == 65535) | {t: .sim_time, class: .actor.class, entity: .actor.entity_id, posture}' $T

# Paladin HoJ dips: DipEnter carries the goal entity (the enemy healer) in
# the event's `target` view; DipComplete fires when HoJ lands, DipAbort when
# the dip bails without casting (teammate HP dive / target dead-or-immune /
# budget). Pair with the Paladin's HoJ ability_decision to confirm the stun
# landed on the enemy HEALER.
jq -c 'select(.kind == "movement_decision" and .actor.class == "Paladin" and (.trigger | startswith("Dip"))) | {t: .sim_time, trigger, goal_healer: .target.entity_id}' $T

# Did the HoJ reservation suppress rotation HoJ? (rejection note fires only
# while a living enemy healer exists AND the Paladin is not PRESSURED)
jq -c 'select(.kind == "ability_decision" and .actor.class == "Paladin") | .candidates[]? | select(.ability == "HammerOfJustice" and (.reason.PreconditionUnmet.note // "") == "HoJ reserved for enemy-healer dip")' $T | wc -l

# NOTE: pets are excluded from `acquire_targets` events. Pet target state
# lives in pet_decision actor views and the match log, not in
# target_acquisition events.
```

**Tolerating truncated traces.** A match that exits via SIGKILL / abort / OOM
skips the BufWriter flush and leaves a partial last line. Read defensively:

```bash
# Skip the partial line on the way in
head -n -1 $T | jq ...

# Or let jq skip parse errors (jq 1.6+)
jq -c '. // empty' $T 2>/dev/null
```

See `docs/solutions/implementation-patterns/ai-decision-trace.md` for the
full schema and the variant-to-predicate map.

### Extract movement KPIs from traces

`scripts/movement_kpis.sh` reduces one or more decision-trace JSONL files to a
per-(match, entity) CSV of position-derivable KPIs — distance traveled and
proximity-to-enemy stats — computed from the positions carried on trace
events. It needs no extra instrumentation: it reads the same traces the
diagnosis recipes above use.

```bash
# Single trace
scripts/movement_kpis.sh match_logs/match_*_trace.jsonl

# Many traces (e.g. a whole matrix run) — one CSV with all matches
scripts/movement_kpis.sh match_logs/traces/*.jsonl > /tmp/kpis.csv

# Override the gates-open time (default 10.0 — the fixed 10s countdown);
# pre-gate samples are excluded from path length, included in distance KPIs
scripts/movement_kpis.sh --gate-time 10.0 match_logs/match_*_trace.jsonl
```

CSV columns:
`match,team,slot,class,samples,post_gate_path_len,avg_nearest_enemy,min_nearest_enemy,pct_within_4yd,pct_within_10yd`
(distances on the x/z plane — y is height, and pets float at a different y
than their owners).

**Sparse-sample caveat.** These KPIs are derived from trace *events*, which
are emitted only at decisions (ability casts, target acquisitions, posture
transitions) — NOT every tick. `post_gate_path_len` is therefore an
UNDERESTIMATE (straight lines between sparse samples cut corners), and the
proximity percentages are over paired samples, not wall-clock time. For dense,
per-tick position timelines use the probe harness (below) instead. Truncated
traces (SIGKILL / OOM) are tolerated — the script's `fromjson?` skips the
partial last line.

### Write a movement behavior probe

For dense, per-tick assertions about healer movement (path length, time spent
near an enemy, separation gained during a window), write a probe in
`tests/movement_probes.rs` rather than reducing a sparse trace. The harness
runs an observed headless match via `run_headless_match_observed`, collecting
a full per-frame, alive-only position timeline, then asserts on it with the
KPI helpers:

- `path_length(samples)` — total distance traveled along a sample slice
- `time_within_range_of(a, b, range)` — sim-seconds two entities were within `range`
- `separation_gained_during(a, b, window)` — distance gained over a `[start, end]` window (`None` if vacuous)
- `assert_min_occurrences(label, actual, min)` — fail loudly if a window-conditional probe went vacuous (e.g., a seed shift emptied the window set) instead of passing trivially

The observer is read-only by construction. The harness's load-bearing
self-test (`observed_run_does_not_perturb_outcomes`) proves an observed run
returns a `MatchResult` bit-identical to an unobserved run at the same seed —
so probing never perturbs the sim. Probes pin behavior at fixed seeds; see the
`priest_postures` / `escape_windows` / `paladin_postures` modules for the
established idiom.

### Look up spell data for implementation
```
mcp__wowhead-classic__lookup_spell("Pyroblast")
```

### Look up item data for equipment
```
mcp__wowhead-classic__lookup_item("Arcanite Reaper")
```

### Run the graphical client
```bash
cargo run --release
```

### Adding a New Combat System

`tests/registration_audit.rs` enforces that every Bevy system function (`pub fn` taking SystemParam types) under `src/states/play_match/` is registered in one of three places. When adding a new system, pick the correct registration path:

- **`add_core_combat_systems` in `src/states/play_match/systems.rs`** — for systems that must run in BOTH headless and graphical modes (combat logic, auras, AI, projectiles, damage application). Add the system to the appropriate phase tuple (Phase 1 `ResourcesAndAuras`, Phase 2 `CombatAndMovement`, or Phase 3 `CombatResolution`) and add the matching `pub use` re-export at the top of `systems.rs`. This path is the home for ~30 systems today and is the answer for almost every gameplay-affecting system.

- **`StatesPlugin::build()` in `src/states/mod.rs`** — for systems that run in graphical mode only (visual effects, HUD rendering, camera, animations, UI for non-PlayMatch states). Add to one of the existing `.add_systems()` blocks or create a new one with the appropriate `.run_if(in_state(...))` gate. Visual-effect systems traditionally use `.after(CombatSystemPhase::CombatResolution)`.

- **`ALLOWLIST` in `tests/registration_audit.rs`** — only for `pub fn` items that take a SystemParam type by value (e.g. `Commands` directly, not `&mut Commands`) but are called manually from a system body rather than registered as a system. Each entry must include a one-line justification. Most helpers in this codebase take references and don't need allowlist entries.

If you forget to register a new system, `cargo test` fails with the file path, line number, and the three registration paths to choose from. The audit is name-agnostic — it detects systems by signature, so renaming a registered function without updating its registration is also caught.

The historical bugs this prevents: `process_dispels`, `process_holy_shock_heals`, `process_holy_shock_damage`, and `process_divine_shield` were each registered in only one of the two paths and silently failed in the other mode. See `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` for context.
