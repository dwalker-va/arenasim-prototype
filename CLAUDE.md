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
- `ai_profile`: "Legacy" (the headless default) or "TeamPlan" — which AI
  implementation to run. `Legacy` is the reactive per-unit AI every balance
  baseline and movement probe is calibrated against; `TeamPlan` is the team AI
  (positioning, plays, sight and focus — see
  `docs/design/team-level-positioning-ai.md`). **Players get `TeamPlan`; headless
  defaults to `Legacy`.** A match set up in the graphical client plays `TeamPlan`
  on both sides (`AiProfiles::client_default`), while `--headless`, `--batch` and
  `--matrix` play `Legacy` unless the config names a profile, so every baseline,
  probe and sweep keeps its meaning. `--replay` follows the headless rule, so a
  replay config with no `ai_profile` plays `Legacy` and reproduces its headless
  run. The match report header (`AI profile:`) and the Results screen name what
  played. Because matches are deterministic,
  running the same seed under both profiles is a PAIRED comparison in which the AI
  is the only variable. Also available as `--ai-profile` for `--matrix`, which
  records it in the CSV header and the output filename. `team1_ai_profile` /
  `team2_ai_profile` override it PER TEAM — required for head-to-head
  measurement (a uniform A/B compares two internally consistent worlds and
  cannot answer "is the new AI better"; see `scripts/headtohead_sweep.py`).
- `team1_pillar_modes`, `team2_pillar_modes`: `false` turns that team's
  healer pillar modes off (AS-373, *Healer pillar modes* below) — a
  measurement control like `team{1,2}_withheld_plays`. A `TeamPlan` team
  without them plays exactly as `TeamPlan` did before them (byte-identical),
  so one side on and one off is the modes' head-to-head.
- `map`: "BasicArena", "TwinPillars", "PillaredArena", or "TestVerticality"
  ("PillaredArena" is the Nagrand Arena replica — a ~120yd circular bowl with four
  octagonal pillars and 10yd starting rooms; "TwinPillars" is the original
  two-cylinder cover map, which the line-of-sight probe suite is calibrated
  against; "TestVerticality" is a headless-only LoS test asset with a raised
  platform and ramp — `--matrix` rejects it and it never appears in the graphical
  map-select list)
- `team1_kill_target`, `team2_kill_target`: Priority target index (0-based)
- `max_duration_secs`: Timeout (default 300)

Use this to verify combat changes without manual testing.

### 2. Wowhead Classic MCP

Look up WoW Classic spell and item data for implementation reference.

**Setup (required on a fresh checkout):** the server is a local Node project at
`tools/wowhead-mcp/` whose `dist/` and `node_modules/` are gitignored, so it
must be built before use:
```bash
cd tools/wowhead-mcp && npm install && npm run build
```
Without this, the MCP fails to connect with `-32000` (Node can't find
`dist/index.js`) and `mcp__wowhead-classic__*` tools are unavailable. After
building, reconnect via `/mcp` (or restart). The server fetches live from
Wowhead's Classic tooltip API; icon URLs are `https://wow.zamimg.com/images/wow/icons/large/<icon>.jpg`.

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

### 3. Dispatch board MCP (`tools/dispatch-board/`)

The agent pipeline's kanban board (`docs/design/agent-pipeline.md`): ONE local
daemon owns a SQLite board and serves MCP (registered in `.mcp.json` as the
`dispatch-board` `http` server at `http://127.0.0.1:7453/mcp`), the web UI at
`http://127.0.0.1:7453/`, and the event feed the orchestrator waits on. Same
build shape as the Wowhead server — `dist/` and `node_modules/` are gitignored:

```bash
# from the repo root
npm --prefix tools/dispatch-board ci && npm --prefix tools/dispatch-board run build
node tools/dispatch-board/dist/cli.js serve   # leave running; DB: <main checkout>/.dispatch/board.db
```

Start the daemon **before** the sessions that use it, or reconnect via `/mcp`
afterwards: a down `http` server just shows as failed and its
`mcp__dispatch-board__*` tools are absent. Every worktree resolves the same
DB (the main checkout's), so there is one board. `npm test` there runs its own
suite (not part of `cargo test` — see its README for why); `dist/cli.js export`
is the backup.

### 4. Client-data DB2 sweep (`scripts/db2_spell_sweep.py`)

**The canonical join for client-data research.** Wowhead gives you gameplay
numbers; the *visual* research the animation cards run (AS-9, AS-15, AS-19,
AS-37) comes from the Classic client's own DB2 tables via wago.tools. Use this
script rather than writing another scratchpad join — every previous session
wrote its own, and twice the result was a published table containing a row that
had never actually been joined.

```bash
# The Warlock book: skill lines 354 Demonology / 355 Affliction / 593 Destruction
scripts/db2_spell_sweep.py --skill-line 354 355 593

# ...plus the caster-side body-animation chain, grouped by anim signature
scripts/db2_spell_sweep.py --skill-line 354 355 593 --caster-anims --group-by-anims

# Find skill-line ids (so those aren't hardcoded either)
scripts/db2_spell_sweep.py --list-skill-lines | grep -i affliction

# Spot-check one spell — still inventory-scoped, so it cannot pick up a
# same-named spell from another class
scripts/db2_spell_sweep.py --skill-line 354 355 593 --name Immolate --events
```

**Why it takes skill lines and not spell IDs.** A curated spell-ID list cannot
be audited for what is *missing* from it, and that is the exact defect this
script exists to make impossible:

- AS-15's `as15_chain.py` hardcoded three spells and the rest of its kit table
  was filled in from an assumed school default — Curse of Weakness was wrong
  for months, because **the row was never joined at all**.
- AS-37 round 1 fixed that row, then asserted a completeness claim over a
  hand-curated 41-ID list that provably omitted era spells (Curse of
  Exhaustion, Soul Link, the whole stone-creation family).

So the spell set is an **inventory** derived from `SkillLineAbility`. Four
properties make an omission or a broken assumption visible in the output:

1. **Inventory, not a list** — nothing to forget to add.
2. **Three counts as a checkable sum** — inventory names, names with no
   `SpellXSpellVisual` row, names with a visual. The Warlock book prints
   `114 = 42 + 72`, matching the sweep recorded in §5 of
   `docs/design/2026-09-06-warlock-dot-client-data.md` (the worked example of
   what this script produces).
3. **An explicit `--era-cut`** (default `SpellID >= 400000`) — this build
   carries Season of Discovery runes on the same skill lines. The script
   *proves* the cut partitions cleanly (era max 28610 | cut min 403501) and
   names any spell straddling it, instead of asserting the threshold is safe.
4. **Asserts one `SpellVisual` per name at Probability 1**, and exits non-zero
   on a split. Rank collapse is the assumption every client-data doc rests on;
   a name that splits means it has broken for that spell. (Real example: the
   Priest lines split on Holy Nova and Touch of Weakness.) Every knob that
   narrows the inventory — `--name`, and an `--era-cut` that excluded anything
   — adds a `SCOPE:` clause under the verdict, so a `HELD` from a narrowed run
   is never readable as a claim about the book. Splits and off-`Probability`
   rows are selected by SHAPE, not by the provenance marker, so a row that
   splits stays fatal even when a higher-priority marker (an unnamed `SpellID`)
   is what prints on it. `--allow-split` downgrades those two verdicts only: a
   dangling `SpellVisualID` has no rows to read, so it stays fatal.

The reporting blocks have their own offline fixture suite,
`scripts/tests/test_db2_spell_sweep.py`, driving `main(argv)` over hand-built
CSVs with no network round-trip. `cargo test` runs it, with the five balance
sweep suites and the harness's own, via `tests/script_fixture_suites.rs`; run
it directly while editing it. Add a case there for any new claim the script
prints — a reporting regression is otherwise invisible until a human reads a
sweep. The scaffolding all six share — the no-subprocess guard that keeps
them offline, the `main(argv)` driver, the scratch directory, the output
assertions and the Python 3.9 interpreter floor — lives in
`scripts/tests/_harness.py` and is itself pinned by
`scripts/tests/test_harness.py`, which also holds the repo-wide check that
every tracked `.py` still imports on the stock system interpreter.

Every output row carries a provenance marker — `RESOLVED`, `NO-VISUAL`
(passive talents; no joinable row, whether absent or pointing at visual 0),
`SPLIT`, `LOW-PROB`, `UNRESOLVED` (a missing `SpellName` row is reported; a
dangling `SpellVisualID` is fatal) — so an unjoinable row reads as a gap in
the output instead of being quietly absent.
Resolution is **inventory-scoped, never name-scoped globally**: "Immolate" has
five visuals across all of `SpellName`, but all eight Warlock-line ranks
resolve to 46.

CSVs are fetched with `curl` and a browser User-Agent (Python's `urllib` gets
a 403 from wago.tools) and cached under `.db2-cache/<build>/`. Deeper chains
the script does not cover — M2/BLP model parsing via CASC, particle tracks —
are in the `wow-client-data-recipe` project memory and the per-card docs.

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

- **[Session Notes](docs/design/session-notes.md)** - Full development history (17 sessions)
- **[WoW Mechanics](docs/design/wow-mechanics.md)** - Implemented game mechanics (CC, resources, combat)
- **[Bevy Patterns](docs/design/bevy-patterns.md)** - Rust/Bevy learnings and common pitfalls
- **[Roadmap](docs/design/roadmap.md)** - Long-term TODOs and milestones
- **[Team-Level Positioning AI](docs/design/team-level-positioning-ai.md)** -
  PARTLY IMPLEMENTED design for the `TeamPlan` layer (team stance, obligations,
  positioning solve). Read before touching pillar/cover behaviour, and read the
  2026-08-04 amendments before building any of it — the design's shape held up
  but several specifics were wrong in ways only measurement exposed, and step 4's
  retirement goal is not achievable as written. Steps 2, 3 and the healer half of
  4 have shipped: Nagrand occlusion `Legacy` 0.0s -> `TeamPlan` 27.4s per match
  (15.1s before contact, 12.3s after; `tests/camp_sweep.rs`, 2026-10-19, with
  AS-362's crowd-control avoidance and AS-292's closing rule — the robust
  result, per-frame, thousands of samples). Its Priest heals its Warrior 307
  per match (241 without AS-292), but its heal line is blocked on 33% of frames
  (23%): `docs/design/balance/2026-10-17-as292-closing-clause.md`.
  The DPS half was reverted:
  constraint satisfaction cannot express a kiter's distance-maximisation.
  `Legacy` is byte-identical throughout.
  **Head-to-head** (2026-10-25 at `40c75048`, AS-395;
  `docs/design/balance/2026-10-25-0.8-milestone-sweep.md` §2): the TeamPlan
  side against the same side in the all-Legacy twin, both seats, every 2v2
  cell on all three maps (93,312 sides) and a 600-cell 3v3 sample (10,800).
  2v2 competitive **+5.5pt** (z=27; BasicArena +1.2, Nagrand +4.8,
  TwinPillars +10.5); 3v3 competitive **+4.8pt** (z=8.3; BasicArena −0.2,
  Nagrand +2.9, TwinPillars +11.5). Every class gains in 2v2 competitive play
  on pillar maps, from the Druid (+2.1) to the Warlock (+12.0); a double-healer
  TeamPlan team loses (−4.7pt on Nagrand). Each effect replicates over three
  seed blocks and is measured against the `Legacy` baseline of the same comps,
  so a change to `Legacy` moves it too.
  **Sample-size warning stands:** an n=12 win-rate figure is noise around
  these values. Prefer per-frame mechanism metrics
  (`tests/camp_sweep.rs`); for win rate use `scripts/headtohead_sweep.py`
  (~100 matches/cell via the parallel `--batch` runner; a paired effect with a
  seed-clustered bootstrap interval and McNemar, single-arm Wilson CIs beside it).
  **Measurement rule:** `AiProfiles` is PER-TEAM; a uniform-profile A/B cannot
  answer "is the new AI better", so set the sides differently and run both
  assignments — see *How to measure a step* in that doc, and `tests/camp_sweep.rs`.
  Background on the camp: **[the camp handoff](docs/design/2026-08-01-nagrand-camp-handoff.md)**.
- **[Sweep Tiers](docs/design/balance/sweep-tiers.md)** - what DIRECTIONAL and
  AUTHORITY mean, how to size and report each, and what a sweep costs on this
  box. A card states its tier and its success condition when it is written.
- **[Stat Scaling](docs/design/stat-scaling-system.md)** - Damage/healing formulas and coefficients
- **[Game Design](docs/design/game-design-doc.md)** - High-level game vision
- **[Concepts](CONCEPTS.md)** - Vocabulary substrate: the words that mean something
  specific in this codebase, defined once so other docs can cite rather than
  redefine them. Currently seeded for the animation/visual-effects area.
- **[Documented Solutions](docs/solutions/)** - Documented solutions to past problems (bugs, implementation patterns, workflows) organized by category, with YAML frontmatter (`module`, `tags`, `category`). Relevant when implementing or debugging in documented areas.

## Key Concepts

### Combat Flow
1. **Pre-match** (10s countdown): Combatants can buff; mana and energy held full
   each frame, rage held at 0 (Warriors enter the gates with an empty bar)
2. **Gates open**: Combat begins, AI takes over
3. **Combat loop**: Target acquisition → ability decisions → casting → damage/healing.
   **Mana is charged only when a completed cast actually LANDS** (WoW-faithful):
   `process_casting` (`combat_core/casting.rs`) deducts `mana_cost` at the cast's
   resolution point in pass 2 — the projectile-spawn site, or after an
   instant-effect passes the alive + line-of-sight completion gates. A cast that
   fizzles at completion (target juked out of LoS, or target dead) costs NO mana,
   and an interrupted cast never reaches completion so it costs nothing either.
   Self-cast buffs always land (self is alive and in LoS of itself), so they still
   cost mana. This means juking a caster into fizzles CANNOT drain its mana. The
   damage/crit RNG pre-calc stays in pass 1 and runs for every completed cast
   (fizzle or land), so the `game_rng` draw order is independent of the mana
   charge site. NOTE: instant abilities (0 cast time, applied in class AI without
   a `CastingState`) charge mana atomically at their application site and are
   unaffected. Channels (Drain Life) pay the full cost up-front at channel start
   (WoW-faithful — an interrupted channel is not refunded).
4. **Arena dampening**: starting `DAMPENING_START_SECS` (75s) after gates, ALL healing,
   absorb shields, and lifesteal ramp linearly to zero over `DAMPENING_RAMP_SECS` (120s;
   both in `constants.rs`). Ticked by `match_flow::update_dampening` into the
   `ArenaDampening` resource. Guarantees attrition endgames (healer-vs-healer especially)
   resolve instead of drawing at the cap — expect `[EVENT] Arena dampening reaches N%`
   milestones in logs of matches longer than ~85s. **Every heal lands through
   `combat_core::apply_healing`** (`combat_core/healing.rs`) — direct heals, heal over
   time ticks, the bloom, drains and lifesteal — which applies `HealingReduction`
   (Mortal Strike cuts ALL healing received, HoT ticks included, as in Classic), the
   Mortal Wounds tell and dampening in one place; a rule for all healing (an immunity)
   goes at its top. `tests/heal_site_audit.rs` fails on a health raise anywhere else and
   on a change to the set of systems that call it. Absorbs are not healing: a NEW absorb
   mechanic applies `Res<ArenaDampening>` at its own application site.
5. **Match end**: When one team is eliminated, logs saved, results displayed. Attacks
   queued in a frame all land even if the attacker died earlier that same frame
   (dying-blow semantics) — simultaneous mutual lethal is a DRAW, not an
   iteration-order win.

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
   - Add its row to `expected` in `tests/aura_dispel_type_audit.rs` — the
     removal class its aura resolves to and the dispel type TBC gives it
     (`SpellCategories.DispelType` at wago.tools build 2.5.6.69795 — TBC rules
     are the guide; the Classic Era client is 1.12 and differs). The match has
     no wildcard, so the test suite does not compile until the row exists. A
     poison or curse on a magic-school ability needs `dispel_type: Poison` /
     `Curse` in step 3, or a Priest can dispel it; a spell TBC gives no dispel
     type at all (a Paladin aura, a shapeshift) needs `dispel_type: Untyped`,
     or a Shaman can purge it

3. **Add definition to `abilities.ron`**:
   ```ron
   NewAbility: (
       name: "New Ability",
       class: Mage,           // REQUIRED - the owning class. No serde default:
                              // omit it and the config fails to parse at startup.
                              // For a PET ability this is the pet's OWNER, and
                              // `pet: Some(Spider)` names the pet.
       prevention: Silence,   // REQUIRED - TBC's PreventionType: Silence, Pacify
                              // or None. Run the as287 TBC check script (below).
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
   - **Read a situation; never re-derive one.** A fact more than one decision
     cares about — the match phase, the team pressing or trailing, the enemy
     healer locked (and until when), a teammate dying, pressured, or both
     (`ally_in_danger`) — is classified by `situation::update_situations`
     (`src/states/play_match/situation.rs`) inside `decide_abilities`, from the
     snapshot every unit decides from: once as the dispatch starts, and again
     after each instant CC an earlier unit lands that frame. Read it through
     `ctx.situation()`: `.phase` / `.attrition_remaining()`, `.tempo`,
     `.enemy_healer_locked`, `.ally_in_danger`, `.dying_allies()`,
     `.is_pressured(unit)`, or pick an order with `ctx.play(role)`
     (the play pipeline, *Plays* below; an exhaustive match). A situation
     a non-decision system acts on is read from the `Situations` resource
     instead, one frame on: target acquisition reads the team's
     `target_focus` that way (*Team target focus* below). Every
     `ability_decision` in the trace carries the `situation` it saw. Its
     thresholds are `movement.ron`'s `shared:` block, and the table in the
     module docs names which neighbouring constants were deliberately NOT
     unified. A context built outside `decide_abilities` (`CombatContext::new`
     alone — a pet's, a test's) has no situation and `situation()` panics on
     it; a test classifies one with `.classified(&cfg, MatchClock::...)`. A
     NEW situation goes into `classify` with a `situation_change` transition
     and a scene test, not into a local predicate at its first reader. Design
     and roadmap: `docs/design/proposals/2026-10-11-ai-architecture.md`.
   - **Stealth needs no check.** `ctx.combatants` is what the deciding unit can
     PERCEIVE — a stealthed enemy it cannot see is not in it (`CombatContext::new`
     builds the view from `stealth_visible`, the one visibility rule), so decide
     from it freely. The one exception is RESOLVING an area effect: take its
     victims from `ctx.area_victims(...)`, which includes unseen enemies, because
     an area does not aim. Two funnels reveal, both through
     `combat_core::reveal_stealthed` (which logs `[STEALTH] X is revealed by
     Y`): damage that reaches health, in `apply_damage_with_absorb`, and a
     hostile aura (`is_hostile_effect`) that lands, in `apply_pending_auras`. A
     fully absorbed hit or a blocked aura reveals nothing.
     `tests/stealth_enforcement.rs` drives every class AI against a stealthed
     Rogue, and classifies every ability as an enemy area effect or not (an
     exhaustive match — a new ability must answer it); each area effect must
     be shown catching a stealthed Rogue standing in it.
   - **An ability's config comes from its caster's book.** Every combatant
     carries an `AbilityBook` (`ability_book.rs`), resolved once at spawn:
     its class's abilities, or its pet type's. The class AI's `abilities` is
     the deciding unit's book; a question about ANOTHER unit's ability (an
     enemy's cast, its interrupt reach or crowd control, a teammate's dispel,
     an aura's source) reads that unit's `CombatantInfo::book`. Never the
     global `AbilityDefinitions`: talents (AS-297) resolve into the book, and
     `tests/talent_book_audit.rs` fails on a new sim reader of the table.
   - **The cast gate, and the AI decision trace** — ask
     `cast_guard::cast_gate` (the caster's own `Combatant` and its own
     `ActiveAuras`, never `None` for a caster that has them): `Err` is the
     typed reason to `builder.reject(AbilityType::NewAbility, reason)`, `Ok`
     is a `Cleared` to spend on `builder.choose(cleared, target,
     was_instant)`. `choose` takes nothing else, and only the gate makes a
     `Cleared`, so a cast cannot be chosen past Travel Form, a school lockout,
     a silence, its cooldown or its resource — a `try_*` that skips the gate
     does not compile. A ground-placed or self cast passes `target: None` and
     checks its own range. Reject at every other predicate gate too. A
     pet-command DISPATCH (an order the pet gates when it executes it) is
     recorded through `start_pet_command_dispatch`. See `class_ai/warrior.rs`
     for the canonical pattern.

5. **Add spell icon** for the ability timeline UI:
   - Download icon: `mcp__wowhead-classic__get_spell_icon("New Ability")` to get the URL
   - Save to `assets/icons/abilities/<icon_name>.jpg`
   - Add `icon: "icons/abilities/<icon_name>.jpg"` to the ability entry in `abilities.ron`

6. **Add special handling** in `combat_core.rs` if the ability has unique mechanics
   (most abilities work automatically via the config)

7. **Nothing to do for the per-class UI lists.** Every surface that shows a class kit
   (View Combatant, the Animation Sandbox, and the encyclopedia) derives it from the
   `class` field you set in step 3, via `AbilityDefinitions::abilities_for_class` /
   `own_abilities_for_class` / `pet_abilities_for_class`. The display name comes from
   the RON `name` field the same way. There is no hand-maintained list to update, and
   no way to silently drop the ability from a screen — that trap (a `get_class_abilities()`
   `Vec` that was not exhaustiveness-checked) is retired. The encyclopedia also gives it
   an index entry, a search hit and a full detail page for free; its prose comes from
   `src/states/ability_text.rs`, generated from the fields you set in step 3, so only an
   effect the numeric fields cannot express needs a `description` in the RON.

   **The per-ability VISUAL is not free, and `cargo test` will say so.**
   `tests/lands_silently_audit.rs` scans the config into families (heal, direct
   damage, DoT, crowd control, buff/debuff, dispel, interrupt) and fails until the
   new ability is named as a member of each family it joins AND its landing reaches
   a visual in that family — or it is named on that family's known-silent list with
   the card that will draw it. A DoT's state is routed by its RON `name:` through
   `DotStateVisual::for_dot` (`rendering/effects/dot_state.rs`), the one router every
   DoT renderer asks.

8. **Test with headless simulation**:
   ```bash
   cargo run --release -- --headless /tmp/test.json
   ```

**Available aura types**: `Absorb`, `Root`, `Stun`, `Fear`, `MovementSpeedSlow`, `HealingReduction`, `DamageOverTime`, `HealingOverTime`, `MaxHealthIncrease`, `MaxManaIncrease`, `ArmorIncrease`, `SpellLockout` (the full list is `AuraType::ALL`)

**Stacking, refreshing and blooming auras** — three opt-in fields on an
`applies_aura` block, all general (the Druid's kit is their first user):
- `stacking: Some((max_stacks: N))` makes the aura STACKING (`AuraStacks` in
  `components/auras.rs`). It is keyed by its SOURCE ability rather than its type,
  so it coexists with a same-type aura from anywhere else (Rejuvenation beside a
  Healing Stream pulse); and when that source applies it again,
  `apply_pending_auras` REFRESHES it — duration back to full, per-stack numbers
  re-snapshot, tick cadence kept — and adds a stack up to `N`, instead of refusing
  the recast the way a one-per-type buff is refused. `max_stacks: 1` is
  refresh-only. A periodic tick is `magnitude` PER STACK (`Aura::tick_amount`,
  exact for every non-stacking aura). Logged as `<target>'s <aura> refreshed (N stacks)`.
  **Whose stack it is follows the aura's KIND** (`StackScope::for_aura`, derived
  from `aura_type` — no RON knob): a periodic effect (`HealingOverTime`,
  `DamageOverTime`) is **per caster**, so two Druids each keep, refresh and bloom
  their own Lifebloom on one ally; anything else is **per target** (Classic's
  same-buff rule), so a second Druid's Mark of the Wild refreshes the first.
  `Aura::stack_owner` / `Aura::same_source_as` carry the key, and every lookup
  of a source-keyed aura goes through them — the refresh and the already-applied
  gate.
  Swiftmend eats its caster's own Rejuvenation first, and the Druid AI reads
  only its own heals over time. Two Druids marking in one frame never mark the
  same ally (`marked_this_frame`, the Paladin's `paladin_aura_this_frame` guard).
- `bloom: Some((heal_base: B, heal_coefficient: C))` makes the aura BLOOM: when it
  ENDS — runs out, or is dispelled or purged — it lands a direct heal of
  `(B + spell power × C) × stacks` on its bearer (TBC Lifebloom). A dispel or
  purge takes ONE stack of a stacked aura (`ActiveAuras::take_one_at`), so it
  blooms for that one stack and leaves the rest. A refresh does
  not bloom it, and neither does its bearer dying. Both endings spawn a
  `BloomPending`; `effects/bloom.rs` `process_blooms` is the one heal site, and
  applies healing reduction and `ArenaDampening`. Logged as `<caster>'s <aura>
  blooms on <target> for N`.
- `compound: Some(<CompoundDebuff>)` makes the aura the FACE of a compound (one
  aura to the player, several effects, removed as one — the Frost Armor chill's
  mechanism). Its riders come from `combat_core::compound_riders`. Mark of the Wild
  is the first compound BUFF: a purge that takes it takes all of it, and a purged
  or expired `MaxHealthIncrease` gives its health back either way
  (`Combatant::reverse_stat_mutation`).

`process_casting` applies every aura it lands through
`AuraPending::from_ability_scaled`, so an instant routed through a zero-length
`CastingState` (the Druid's heals, Frost Shock) may set `magnitude_coefficient`;
add it to `SP_SCALED_AURA_WIRED`. Such a cast is still an instant: no interrupt
takes it (`AbilityConfig::has_cast_bar`, read from `cast_time`; see Interrupts in
`docs/design/wow-mechanics.md`). A heal may also `consumes_aura: Some(<Ability>)`
— Swiftmend eats the target's Rejuvenation as it lands.

**Every ability declares `prevention`** — TBC's `SpellCategories.PreventionType`:
`Silence` (a spell), `Pacify` (a physical attack or shot) or `None` (Travel
Form). It is required, with no default, and
`docs/design/balance/sweeps/2026-10-12-as287-tbc-interruptibility.py` checks
every ability's value against the TBC client (an ability whose name the client
lacks is listed there with the spells it is read from). It decides two things.
A silence stops only a `Silence` ability that costs mana, begun or already
being cast (`AbilityConfig::silence_stops`): a silenced Hunter still shoots, and
a silenced Druid still shifts. An interrupt takes only a `Silence` cast bar
(`AbilityConfig::can_be_interrupted`): a `Pacify` one (Aimed Shot) is passed by
unspent, its cast bar is grey and its text says "Cannot be interrupted." Crowd
control still stops any cast, with no lockout. Nothing in the sim pacifies.

**Shared cooldowns** — `cooldown_category: Some(<CooldownCategory>)` puts the
ability in a cooldown category, after the Classic client's `SpellCategory`:
using any member puts EVERY member on cooldown (Freezing Trap and Frost Trap are
both `Trap`, Classic category 411, 15s). Members must declare the same
`cooldown` and be cast by one unit, so they sit in one ability book
(`validate()` panics otherwise), and a member already waiting longer
keeps its time. Start a cooldown with `combatant.start_cooldown(ability)` — never by writing `ability_cooldowns` directly, which would skip
the category; `tests/cooldown_site_audit.rs` fails on such a write. Readiness
checks need nothing: a locked member is simply on cooldown, and is traced
`OnCooldown`.

**Tip**: Use the Wowhead MCP to look up accurate WoW Classic values:
```
mcp__wowhead-classic__lookup_spell("Pyroblast")
```

### Talents

Each class's trees are data in `assets/config/talents.ron`
(`src/states/play_match/talents.rs`; design:
`docs/design/proposals/2026-10-11-talents-and-gauntlet.md` §3-4). A match
carries one `Build` (talent → rank) per combatant, `MatchConfig::team{1,2}_builds`
and the headless `"team1_builds": [{"DemonicEmbrace": 2, "FelStamina": 2}]`;
an empty build is the default and changes nothing, so `Legacy` and BasicArena
stay byte-identical. Both spawns resolve the build ONCE
(`TalentDefinitions::resolve`, through `resolve_for_spawn`) into the
combatant's `AbilityBook` — `Grant`s added, then `Patch`es applied, adds
before multiplies — and its stats: `StatPercent` scales the totals after
`apply_equipment` and before the pet spawns, so the pet's share grows with
it; `PetStatPercent` then scales the pet. A talented match logs a `[TALENTS]`
line beside `[EQUIPMENT]`, and its trace opens with a `match_header` line
carrying every slot's build.

- **Adding a node:** a `TalentId` variant (`talent_ids!`) and its entry. The
  loader rejects a node outside its class's tree, a bad tier, column, rank or
  prerequisite, and a price rule broken: a node of `StatPercent` effects on
  stats the item budget weighs has its price COMPUTED at the class's
  `reference` combatant (starting kit plus three Gauntlet picks) and declares
  no `budget`; every other node declares one. `tests/talents.rs` holds every
  rank to 11 ±15% (a capstone to 22) and every legal build at 2-8 points to
  within one rank of the cheapest. Never give armor or another free stat an
  item weight to price a talent: that re-prices every item.
- **Adding a talent ability:** mark it `talent: true` in `abilities.ron` (it
  leaves its class's base book) and `Grant` it from exactly one node. Its
  class AI decides it with `abilities.get(..)`, never `get_unchecked`: an
  untalented unit's book lacks it. `tests/talent_ai_audit.rs` fails until the
  ability has a row there whose scene shows the AI deciding it when granted
  and leaving it alone when not.
- **Adding a patchable field:** an `AbilityField` variant;
  `every_numeric_ability_field_is_decided` destructures `AbilityConfig`
  without a rest pattern, so a new numeric field fails to compile until it is
  placed or declared not patchable.

### Adding a New Item

Items are data-driven via `assets/config/items.ron`. Every item must stay within its **item level budget** — enforced by `cargo test`.

An item's `slot:` is an **`ItemSlotType`** — what KIND of slot it fills (`Ring`,
`Trinket`, `Head`, …), never a socket number. The character's sockets are the
separate `ItemSlot` enum and are what `loadouts.ron` keys (`Ring1`, `Ring2`, …);
any ring fits either ring socket, and the same item may not occupy both
(unique-equipped, enforced at loadout load and in the equipment picker).

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
   - Slot multipliers (TBC's slot groups; `slot_budget_multiplier`): Head/Chest/Legs = 1.0, Shoulders/Hands/Waist/Feet = 0.75, Trinket = 0.70, Wrists/Neck/Back/Ring/Ranged = 0.5625. The values are the TBC client's `RandPropPoints` columns; trinket 0.70 and ranged 0.5625 are rulings (TBC's ranged is 0.32)
   - Hand items are priced by how they are HELD (`held_budget_multiplier`): two-hander = 1.0, one-hander = 0.42, off-hand (shield/frill) = 0.5625. However the hands are filled they draw about one full slot (0.42 + 0.5625 = 0.9825). A shipped two-hander is priced at exactly what the one-hander + off-hand pair it displaces spends, not its cap — `two_handers_are_priced_at_the_pair_they_displace` pins each one to its pair, and a new two-hander must be added there
   - Stat costs: max_health/max_mana = 1.0/pt, attack_power/spell_power = 1.5/pt, crit_chance = 300.0/pt (0.01 = 3.0), movement_speed = 30.0/pt (0.1 = 3.0), resistances = 0.4/pt, mana_regen = 5.0/pt
   - **Free stats** (excluded from budget): `armor`, `attack_damage_min`, `attack_damage_max`, `weapon_speed`
   - **A weapon's `weapon_speed` is seconds per swing — the Classic tooltip's "Speed"**, taken from a
     named real Classic item (same weapon type, hand and role, nearest item level). The item is the only
     source of a hand's swing speed; `every_weapon_declares_its_classic_speed` names every weapon, so a new
     one fails `cargo test` until it has a speed and a row there. A two-hander's weapon DPS carries Classic's
     premium over the one-hander it displaces (`two_handers_carry_the_classic_premium_over_the_one_hander_they_displace`)
   - Budget usage = sum of (stat_value × weight) across all non-free stats
   - Items may exceed the budget by up to 5% tolerance

   **A trinket may carry a `proc:` block** — a trigger and an effect on that
   trinket's OWN internal cooldown (`src/states/play_match/proc_trinkets.rs`):
   ```ron
   proc: Some((
       trigger: MeleeHit,         // MeleeHit | SpellCast | Heal
       chance: 0.15,              // per qualifying event, in (0.0, 1.0]
       effect: AttackPowerIncrease,
       magnitude: 55.0,           // in the effect aura's own units
       duration: 10.0,
       internal_cooldown: 45.0,   // must be >= duration
   )),
   ```
   - **Triggers:** `MeleeHit` is a landed MELEE swing (never a ranged Auto Shot
     or a wand); `SpellCast` is a completed cast with a cast time that LANDED —
     the point mana is charged, so a fizzled or interrupted cast procs nothing,
     and neither does an instant or a channel; `Heal` is the healing subset of
     `SpellCast`. A `MeleeHit` chance is rolled per landed hit, whatever the
     weapon's speed, so a slow weapon's fewer swings proc less often per
     second. That is Classic's shape for Windfury and poisons; the sim's proc
     trinkets have no Classic counterpart, so none is rated per minute.
   - **Per-trinket ICD, no global lock.** A proc cannot fire again while its own
     buff is up (`internal_cooldown >= duration` is validated), and two
     DIFFERENT trinkets can be live at once. A proc buff coexists with a
     same-stat buff from an ability (Battle Shout and a Dragonspine proc stack).
   - **Pricing.** The proc is charged to the budget at
     `stat_weight × magnitude × duration / (duration + internal_cooldown)` — the
     ICD's LONG-RUN uptime bound. `chance` does not enter the price. It is not a
     ceiling over a short life: a trinket starts ready, so a life shorter than
     one cycle runs above it (Dragonspine realises 23% against its 18.2% bound in
     a 43s arena life — `realised_uptime_against_the_long_run_bound` in
     `tests/proc_trinket_probes.rs`). Only the six stat auras
     `proc_effect_budget_weight` prices may be an `effect`.
   - **Procs are not purgeable.** A proc buff is `DispelType::Physical` — an
     item's effect is not a spell, however magical it looks — so no purge,
     dispel or cleanse removes it.
   - Every item with a `proc:` gets, with no code change: its own Buffs &
     Debuffs entry in the encyclopedia; its proc sentence (`proc_description`,
     the one generator) on the item page, the item tooltip and a second line
     under its row in View Combatant's equipment picker; and a proc buff that
     wears the TRINKET's icon (`ItemConfig.icon`) in the buff bar, the team
     frames and the encyclopedia — the buff carries `Aura::source_item`, and
     `item_aura_icons` registers the art in both aura icon loaders.

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
- `press_advantage_margin: 0.2` — tempo ONSET: team-HP-fraction lead at/above which a team "presses" (the same deficit makes it trail): its healers' `cover_pull` zeroes and the melee tempo reset stays disarmed (denial is reserved for the trailing side)
- `press_release_margin: 0.1` — tempo RELEASE: a pressing (trailing) team keeps that tempo until its lead (deficit) falls below this, so a lead wobbling around the onset does not flip it with every hit and heal. `TeamPlan` only (`SituationConfig::under`): a `Legacy` team releases at the onset, unlatched as before, and stays byte-identical. At most `press_advantage_margin` (`validate()`); equal disables the band
- `cc_ready_horizon: 1.5` / `cc_reach_margin: 3.0` — `TeamPlan` crowd-control reach (*Out of the enemy's crowd-control reach* below): an enemy hard CC is READY when its owner could start it within the horizon (seconds), and a healer keeps out of its reach plus the margin (yards)

**Per-class scorer weights** (`priest.weights:` / `paladin.weights:` —
`score_directions` term weights; `0.0` disables a term). All terms here are
additive *interest* terms; the hard constraints (boundary, ally-anchor) are
boolean masks in the scorer, not weights — the old `ally_anchor` /
`boundary_penalty` penalty knobs and their dominance invariant were retired
with the context-steering mask refactor.
- `threat_repulsion` (3.0/3.0) — pull away per visible threat, weighted by proximity
- `formation_pull` (Priest 2.0 / Paladin 0.0) — pull toward the FREE backline point (Paladin keeps its melee identity, so 0 disables it)
- `corner_penalty` (Priest 6.0 / Paladin 4.0) — graded penalty approaching arena corners
- `wand_pull` (Priest 0.5 / Paladin 0.0 / Mage 0.0) — low-weight pull toward wand range of the kill target (`0.0` disables it for the wandless Paladin). The Mage keeps this at `0.0`: it HAS a wand and DOES fall back to it when out of mana, but via the pursuit stop distance (see the DPS kiter OOM wand fallback below), not this orbit-scorer term — the term's LoS-preserving lateral meander is slow to corner a juking target
- `range_band` (0.0 for healers; Mage/Hunter 2.0 / 0.5) — ring-attraction toward the kill target's `[min, max]` band; disabled for healers
- `flee` (0.0 for healers + Mage; Hunter 6.0; Druid 6.0, read only in Travel Form) — constant pull away from the nearest threat, NOT proximity-weighted (distance-maximization), so a chased ranged DPS outruns an un-impaired chaser at all ranges. Wherever `flee` is on, `corner_penalty` must EXCEED it or the kiter flees into corners — Hunter and Druid both 8.0 over 6.0, and `validate()` refuses a block that breaks it.
- `commitment_bonus` (1.5/1.5) — bonus toward the committed direction during the commit window
- `los_seek` (0.0 for healers; Mage 2.0 / Hunter 1.0) — reward for candidate steps that have/restore line of sight to the kill target; drives occluded-in-range casters to orbit to a sighted angle instead of idling
- `cover_pull` (Priest/Shaman 1.5, Paladin 1.0, 0.0 for DPS) — reward for candidate steps occluded from threats; drives pressured-healer pillar denial. Kept below `threat_repulsion` so denial shapes retreat direction without overriding escape; zeroed when a healable teammate is below `urgency_hp_threshold` or the team is pressing (tempo, `press_advantage_margin` / `press_release_margin`)
- `trap_setup` (Hunter 3.0, 0.0 for everyone else) — while Freezing Trap is ready and an enemy healer would be caught by it (`hunter_dip::trap_setup_healer`), pull toward that healer once a KITE step would leave it beyond throw range (the trap's `range` less its trigger radius); zero inside that range, absent while the trap is on cooldown. Kept below `flee` so the kite still escapes the melee, bent toward the healer. Disengage bends its leap by the same two weights (`hunter_dip::trap_setup_disengage`) and names the healer as the Disengage's `target_id` in the trace when it did; KITE decisions carry the term as a `trap_setup` scorer term
- `stance_pull` (Mage/Hunter 1.0, 0.0 for everyone else; `TeamPlan`) — while the kiter runs the Pillar play, a `formation_pull`-shaped pull toward the ring point at its team's pillar stance side (`pillar_stance::side_point`); absent otherwise. Below `range_band`/`los_seek` and far below `flee`, so it bends a kite, never overrides it; traced as a `stance_pull` scorer term

**PRESSURED onset (`Legacy`)** — no RON knob; reuses `danger_edge_margin`. A
caster targeting a healer pressures it only while CLOSING: between its own
preferred range and `threat_intent_radius`, a 2yd band for a 28yd caster. Under
`Legacy` a FREE healer ENTERS PRESSURED only when that branch holds
`danger_edge_margin` inside both its edges (`situation::AllyStanding::pressure_onset`,
`healer_postures::compound_pressure_onset`); it stays PRESSURED on the plain
test. Proximity, melee and pets onset exactly as before. Without the gap a
caster pursuing the healer flipped PRESSURED on for a frame, `pressured_hold`
backed the healer off, and its formation point walked it back in: a ~3s
in-and-out bob (AS-249; `tests/healer_positioning.rs` pins the reversal rate).
`TeamPlan` enters on the plain trigger: there PRESSURED is the team solve's,
which holds cover in reach of the ally, and the gap measurably cost its heal line.

**Out of reach of a focused teammate (`Legacy` PRESSURED)** — the scorer's
anchor mask keeps a PRESSURED healer in heal range and sight of its anchor ally.
When the healer is already out of reach, so every step is masked, the fallback
ladder used to drop the anchor and let the healer flee freely. For an anchor
the enemy is targeting it now first keeps the steps that end NEARER it
(`AnchorConstraint::close_when_out`, rung 0 in `score_directions`), so a melee
between the healer and its focused teammate is walked round, not fled from.
ESCAPE windows keep the plain ladder.

**Healer pillar modes: hug or open (`TeamPlan`, AS-373)** —
`class_ai::pillar_mode`; design and measurements in
`docs/design/2026-10-19-healer-pillar-modes.md`. Constants in the module, no
RON knob. The hug is the healer's half of its team's pillar stance (AS-387,
`Pillar` under *Plays* below): a healer hugs ONLY while its team holds a
`pillar_stance` and it runs the Pillar play, on the stance's pillar, and only
while that pillar's ring has ground that sees every living teammate in heal
range and that no crowd control a pillar defends against covers (`team_arc`
over `team_threats`: one that needs sight, reaches past `MELEE_CC_REACH`, 8yd,
and is not a pet's — AS-362's `CcField`), in a run wide enough to stand the
sight margin inside both edges (`holds_margin`). It never hugs alone (design
D3). Otherwise it plays the OPEN. Anti-strobe: the stance owns the Schmitt
pair (`HUG_ENTER` 12yd in, `HUG_LEAVE` 20yd out), the crowd-control release
(`HUG_RELEASE`, 3s), the dwell (`MODE_DWELL`, 2s) and the pillar switch
(`PILLAR_SWITCH`, 8yd); the healer's mode is held `MODE_DWELL` and its hugged
pillar changes only when the stance's does; a team arc too thin to hold the
margin ends a hug at once. Every change is a `PillarModeEnter` /
`PillarModeLeave` movement decision whose scorer terms name the `pillar` and
the two keys (`key_stance`, `key_arc`).
- **Hug** is a ring ANGLE round the pillar at `ρ = disc + MOVER_RADIUS +
  COVER_STANDOFF` (8yd on Nagrand). The target angle is chosen in the team
  arc's core — the arc less `HUG_SIGHT_MARGIN` (3yd) plus the arc the anchor's
  own motion round the pillar sweeps in `HUG_LEAD_SECS` (1.5s), off the edge
  that motion sweeps toward the healer — then out of the positioning
  `CcField`, then fewest casters in sight, then nearest. It turns at most `v/ρ`
  and only through the arc, so a change of side is a walk round the near side.
  In FREE the ring point replaces the formation point; in PRESSURED the
  `OccupyCover` solve gains the arc's ring points, a soft leash beyond
  `HUG_SLACK` (3yd, `team_solve::C_RING`) and the margin (`C_MARGIN`, above
  crowd control and cover, below sight); its deep-shadow samples stay.
- **Open** is the FREE formation point with the sight constraint on the
  HEADING (`formation::open_heading`): a heading whose point stands in the
  ANCHOR's widened shadow (AS-206's wedge, near-obstacle limit and
  `REPAIR_LIMIT`) is forbidden; the turn holds its side and crosses only the
  other way round the centroid, through the open. Another teammate's shadow
  does not bind the heading; AS-206's point repair, which still runs after
  it, keeps the point in that teammate's sight where it can. With nothing
  forbidden the heading is `turn_limited` bit for bit.
- The Paladin (`HUG_EXCEPTIONS`, design D5) is untouched by both modes: its
  FREE is melee pursuit, with no formation point. Medic chase, dispel walk,
  a hunt station, ESCAPE and DIP outrank it and leave the mode as it was.
  Obstacle-free maps are untouched; `Legacy` is byte-identical.

**Medic chase (heal-seeking movement)** — shared across Priest/Paladin/Shaman
(`healer_postures::medic_chase_override` / `medic_chase_tick`; no RON knob,
reuses `urgency_hp_threshold`). When a living non-pet teammate is below
`urgency_hp_threshold` AND occluded from the healer, a direct
`MovementGoal::Point` walk toward that ally (most-injured occluded one)
OVERRIDES FREE formation and PRESSURED cover-denial so the healer walks around
cover to regain sight and heal — never during DIP (its teammate-HP abort
composes) or the committed ESCAPE window, and never while the healer is
hard-CC'd (`is_ccd`, Root included). Keyed on OCCLUSION, not range, so it is a
provable no-op on obstacle-free maps (BasicArena stays byte-identical). Traced
via the existing `SeekLos` trigger with a `point` goal and the ally in the
target view. Once live, the chase HOLDS after sight returns until its
directive's `commit_window` has run (while its teammate is still alive and
below the threshold): letting go on the frame sight returned let a formation
point on the occluded side of a pillar edge step the healer straight back, and
the two traded it every frame (`healer_postures::held_medic_chase`,
`tests/medic_chase_hold.rs`). Only the release waits — the onset, and a switch
to a more-injured teammate out of sight, are never held. **The walk wins over a heal that is not critical:** while it is
live the healer's plan defers movement-locking casts exactly as a dip or ESCAPE
window does (`escape_defer` / `cast_defer`), so the heal fallback above cannot
root it short of the dying ally to heal a less-hurt one it can see. An ally in
sight who is ALSO at or below `urgency_hp_threshold` is still healed, and
instants (Holy Shock, Power Word: Shield, the Druid's kit) still fire
(`tests/medic_chase_heal_rule.rs`). The Druid holds its own damage and
utility for the walk instead — see *Dying teammate first* below.

**The FREE formation point keeps sight (`TeamPlan`)** — `class_ai::formation`
(AS-206; no RON knob). The medic chase walks only for a teammate below
`urgency_hp_threshold`, so a formation point in a pillar's shadow from a
HEALTHY teammate used to park the Priest, Shaman or Druid there blind. Under
`TeamPlan` the point is moved out of any teammate's shadow it stands in, cast by
an obstacle near it, widened by `SIGHT_MARGIN` (2.5yd, past the 1.5yd formation
dead zone): toward the exit — the point moved out until it stands that margin
beyond the shadow's edge ray — that sees the teammates best (the anchor, the
most injured, first). It is a constraint with a nearest-feasible repair, not a
scored term. A point in no widened shadow comes back exactly as the
unconstrained machine returns it. Distances are measured from the raw point,
not its standable projection (which jumps between a pillar's faces). An exit up
to `REPAIR_FULL` (10yd) from the raw point is taken in full, one up to
`REPAIR_LIMIT` (16yd) part of the way, a farther one not at all. The exit nearest
the one last chosen (`HealerPosture::formation`, forgotten on leaving FREE)
counts `KEEP_MARGIN` (6yd) nearer, so the point keeps to its exit until another
is better by that band or its own loses the anchor. Every exit is continuous in
the raw point across every edge of its region (`EDGE_FADE` fades the move out at
the shadow's near end and the near-obstacle limit), so the repair starts and
lets go where the point stands; a change of exit still moves the point the whole
gap at once, and is left a step: the exits stand either side of a shadow, so any
path between them crosses blind ground or detours round the pillar, and easing
the goal across blind ground was ruled out (about six such steps over 5yd per
100 Warrior+Mage+Priest v Rogue+Warlock+Druid matches).
The Paladin has no formation point. `Legacy` is untouched (byte-identical), and
BasicArena with it under both profiles. Pinned by `tests/formation_sight.rs` (a
staged pillar edge where the `TeamPlan` Druid and Priest each regain sight of
their Mage within 1s while the `Legacy` pair stays blind; a seed bank over three
comps bounding the healers' formation goals; leaving FREE drops the kept exit)
and the tests in `formation.rs`. A new FREE goal (AS-328's standoff) belongs
behind the same constraint.

**The FREE formation point turns instead of flipping (`TeamPlan`)** —
`formation::turn_limited` (AS-370; no RON knob). "Behind the line" flips
whenever the enemy nearest the engaged-ally centroid crosses it, and the point
used to flip with it, 5-16yd in a frame. The heading now turns from the last
one at no more than the healer's walking speed over `formation_offset`, so the
point swings round the centroid instead; and the free tick re-issues a walk
that aims more than `formation_shift_threshold` off the point even while the
healer stands in its dead zone (`formation::walk_is_stale`). Both are asked
through `formation::continuous`; the heading is remembered on
`HealerPosture::formation` with the kept exit. Over the five comps of `measure_formation_goal_jumps` (100
matches each) the goal's jumps over 5yd fall from 193 to 25 in all; the
measure attributes each jump to its cause. BasicArena moves under `TeamPlan`
(the heading needs no obstacle); `Legacy` keeps the flip and stays
byte-identical. Pinned by `tests/formation_continuity.rs` (a staged crossing
on BasicArena paired with `Legacy`, and one-tick scenes of each formation
machine) and the tests in `formation.rs`.

**Dispel walk (Paladin, Priest)** — `healer_postures::dispel_chase_override`,
choosing its ally with `class_ai::dispel_chase_target` (no RON knob). Cleanse and
Dispel Magic only reach allies within their 30yd range AND in sight, so a
teammate held in urgent crowd control the dispel removes
(`URGENT_DISPEL_PRIORITY`: Freezing Trap, Polymorph, the UA silence, Fear)
beyond that range or behind cover would otherwise sit out the whole CC. The
healer walks straight at the nearest such teammate (a `MovementGoal::Point`,
tangent-steered round a pillar, same walk as the medic chase, which outranks it)
until the rotation's urgent dispel reaches it, deferring non-critical casts as
it does during a dip. It shares the medic chase's gate (FREE/PRESSURED only,
never while hard-CC'd, retired under `TeamPlan` PRESSURED), and does not walk
while a qualifying teammate is already reached, the healer cannot afford the
dispel, it cannot cast it (silenced or locked out of its school —
`class_ai::can_cast_dispel`), or ANOTHER living non-pet teammate is below
`urgency_hp_threshold` (dying first, as under `TeamPlan`; the CC'd teammate's
own health does not count). Once a walk reaches, the healer HOLDS that point
until the dispel lands or stops being owed (traced `DispelHold`), so the posture
cannot step it back out of range while the GCD runs. Traced as
`DispelChase`; a dispel that declines because its only
candidates are unreachable is traced `LosBlocked` (one in range behind cover) or
`OutOfRange` with the distance, never `NoValidTarget`.

**Roots (Paladin Cleanse)** — a magic root (Frost Nova, Entangling Roots) that
is HOLDING A TEAMMATE BACK (`class_ai::root_holds_back`: a melee teammate whose
target is out of `MELEE_RANGE`, or a ranged one with an enemy melee or pet
targeting it inside `SAFE_KITING_DISTANCE`; never the Paladin itself, whose
own root stays a routine dispel) rises to `ROOT_DISPEL_PRIORITY` (85): below
the urgent bar, above the routine dispels. The root is why the Cleanse is
spent; what it removes is still the random pick among everything Cleanse takes
on that teammate. The Paladin's rotation
spends this Cleanse right after its emergency Holy Shock, ahead of Hammer of
Justice and its heals, unless a living non-pet teammate (itself and the rooted one
included) is below `LOW_HP_THRESHOLD` (`class_ai::root_dispel_allowed`). A root
that is not holding anyone back stays a routine dispel (80). The walk owes such
a root only when nothing urgent is owed, under the same gate, and only when the
Paladin reaches Cleanse range within `ROOT_DISPEL_WALK_MAX_SECS` (2s) at its
base speed with a GCD of the root still left — a root lasts 4-8s and breaks at
80 damage, so a longer walk arrives as it ends. The Priest's rotation has no
root tier, so its walk never owes one (`class_ai::clears_held_roots`). The tier
names no spell: it takes whatever root the Cleanse's scope takes, so a root's
removal class (`dispel_type`) is what keeps it in or out.

Under `TeamPlan` PRESSURED the team solve walks there instead:
`team_solve::dispel_goal` hands the same teammate to the healer's `OccupyCover`
solve as a `DispelGoal`, and while one is owed REACHING it (`C_DISPEL`: in range
less `DISPEL_REACH_MARGIN`, in sight) is the only thing the solve scores — the
nearest reaching spot wins, cover and ally sight wait one GCD, and with no
reaching candidate the solve walks straight at the teammate. Once the walk
reaches, the goal HOLDS until the dispel lands or stops being owed (the
`Legacy` hold's rule), so the solve cannot step the healer back to cover out
of range while the GCD runs. Not owed by a
healer that is hard-CC'd, silenced or locked out of the dispel's school, nor
while any OTHER living teammate is below `urgency_hp_threshold` — a dying
teammate comes first, as the medic chase outranks the `Legacy` walk. Traced
as a PRESSURED `DispelChase` once per walk; the Paladin's band-hold steps aside
for it. A no-op whenever no dispel is owed, so a `TeamPlan` match without one
is byte-identical.

**A dying teammate anchors the `TeamPlan` healer.** While the healer can heal
and a living non-pet teammate is below `urgency_hp_threshold`, `OccupyCover`
anchors on the most hurt such teammate (not the nearest) and REACHING it
(`team_solve::C_DYING`: within `heal_range` less `HEAL_REACH_MARGIN`, in sight)
ranks above every other constraint — cover is chosen only among the spots the
heal still lands from. When no candidate spot can see the healer's anchor
ally at all (a pillar between them), the solve walks straight at the ally,
tangent-steered, until one can — the dispel goal's fallback — instead of
parking blind. A silenced or school-locked healer owes no reach and keeps to
cover and distance. The Paladin's band-hold parks it only where it reaches
such a teammate (`team_solve::dying_unreached`); out of reach, it steps aside
for the solve, as it does for an owed dispel. The Priest has no offensive
Psychic Scream dip under `TeamPlan` to need a reach rule.

**Out of the enemy's crowd-control reach (`TeamPlan`).** `class_ai::cc_reach`
is the one definition of which enemy crowd control can land on a healer, and
where: every (enemy, ability) pair whose ability HOLDS (`holds`: stun, fear,
polymorph, incapacitate, cyclone — never a root) and that its owner could start
within `cc_ready_horizon` — the cast gate's own-state checks asked from the
enemy's side (shapeshift, held, school lockout, silence, cooldown with its
category, resource, stealth for an opener). Reach is the ability's `range`,
plus `TRAP_TRIGGER_RADIUS` for one thrown at the ground; a spell aimed at a
unit covers only what its caster can SEE, an area round the caster or a trap
its whole disc (`cc_reach::aim`, an exhaustive match). A healer immune to the
landing (DR-immune, damage-immune, fear-immune to a fear) is not threatened. A
spot is covered inside `reach + cc_reach_margin`; the spot a healer stands on
is judged by `reach + margin / 2`, so a healer on the rim is not moved by the
caster's every step. Two consumers read it. The team solve's
`team_solve::C_CC_REACH` on `OccupyCover` (PRESSURED Priest, Paladin, Shaman,
Druid), ranked above occlusion from casters at any caster count (`W_CC` plus
`W_OCCLUDED` per enemy caster) and below sight of the anchor at any caster
count, below a dying teammate's reach and an owed dispel. A blind spot always
pays `W_SIGHT` plus `W_OCCLUDED` per enemy caster in the world, with or without
a CC field; a field adds only the CC ceiling. A spot beyond heal range earns
no credit for being out of reach. And the FREE formation point's reach
repair (`formation::reach_repaired_point`, Priest, Shaman, Druid — never the
Paladin, whose FREE is melee pursuit): the nearest spot out of every disc that
still reaches every teammate the point reached and sees them no worse,
composed after the sight repair on AS-370's turned heading; with no such spot,
heal first and the point stands. The Paladin keeps out only of discs it can
leave without leaving PRESSURED — none reaching past `threat_intent_radius`
(a Warlock's Fear) and none of its own target's — and its band-hold parks at
the nearest spot out of them (`CcField::nearest_exit`); otherwise PRESSURED
walked it out and its FREE pursuit walked it back. Each walk out of reach is
traced once as it begins, trigger `OutOfCcReach`. The solve's flat-field anti-statue key
is judged without the crowd-control term, so a melee's small stun disc does
not disarm it. Under `TeamPlan` a healer parked at its FREE formation point
also keeps its walk alive (`formation::continuous`): a lapsed one handed the
legs to target pursuit for a step and back every directive TTL, and healers
kept FREE longer by the reach rule reversed heading about once a second.
`Legacy` builds no field and is byte-identical. Pinned by
`tests/healer_cc_avoidance.rs` (staged pairs, each against its CC on
cooldown). Design: `docs/design/2026-10-18-healer-cc-avoidance.md`.

**Target choice needs sight.** Every cast that CHOOSES its target from
several candidates filters by reach before it ranks: `class_ai::cast_reach`,
range then line of sight, the same two gates and order `pre_cast_ok` applies to
every targeted cast. So an occluded best pick yields to the best one in sight
instead of being picked and then refused; with none in reach the cast is
rejected `LosBlocked` — unless the caster could not have cast anyway, which
is reported first, in `pre_cast_ok`'s order (`cast_guard::unreached_reason`:
Travel Form, lockout, silence, cooldown, mana). The choosers: the ally dispels (Dispel Magic, Cleanse,
Devour Magic, Master's Call), Purge (`select_purge` — the best odds it can
see; the buff that comes off is a random pick, never the AI's), every heal that ranks allies by health (`CombatContext::lowest_health_ally_in_reach`:
Flash Heal, Flash of Light, Holy Light, the Holy Shock heal, Lesser Healing
Wave, the Druid's emergency heal and top-up Rejuvenation), Holy Shock damage
(the first enemy within 20yd in sight), the rotation Hammer of Justice (its
healer-preferring pick among the enemies it reaches; the dip's arrival asks the
same, so a dip in range behind a pillar walks on round it), the Shaman's Frost
Shock peel (the nearest attacker in sight) and every interrupt
(`class_ai::interrupts`: Kick, Pummel and Wind Shear, decided in
`check_interrupts`, and the Felhunter's Spell Lock), whose caster scans rank
only the casters they reach — the Shaman's healer-first pick, every
interrupter's Cyclone-first pick, Spell Lock's heal-first pick — and whose
refusal for sight is traced `LosBlocked`. `process_interrupts` asks sight again
at resolution: an interrupt that reaches a caster its interrupter cannot see is
spent and does nothing. The choosers that rank without a range
filter — Power Word: Shield, Power Word: Fortitude, Mark of the Wild, the
Druid's heal focus, its Cyclone peel and its step-8 Entangling Roots — drop a
candidate only SIGHT keeps them off (`class_ai::sight_blocks`) before ranking,
so a best pick beyond range is still picked and refused as before. The Druid's
step-8 Roots also passes over an attacker immune to damage. Its Roots PEEL for a
dying teammate is reach-first (range, then sight, then immunity): an attacker it
cannot land on yields to the next, then to the next dying teammate's. The
Druid's peels take an attacker in sight even when the ally they peel for is
not: the control focus is ranked over every ally. Both of the Priest's Psychic
Screams — the defensive one and the offensive dip's — hold for a critical heal
(the Priest's own included) only when the dying ally is in reach — one behind
cover is the medic chase's to reach, and no heal can land on it yet. (A dying
teammate also aborts the dip outright, in reach or not.)
**A cast aimed at one fixed target** falls back only to a target that serves
the same purpose, and otherwise waits for sight. The Druid's step-8 Cyclone
passes an occluded enemy healer over for another enemy healer in sight, never
for anyone else: the kill needs a healer gone. Moonfire and Lightning Bolt wait
for sight of the kill target. Moonfire's DoT on any other enemy would stand the
team's Polymorph, traps and Chain hand-offs down on it
(`has_friendly_dots_on_target`). Lightning Bolt sits above the Shaman's
sustain heal, so a bolt at whoever is in sight costs the heal its time.
Measured on a fallback: across the TwinPillars juke bank the Shaman's Warrior
died sooner, and on PillaredArena four of its six flips were losses turned into
300-second stalemates. A refusal
for sight reports the caster's own refusal first.
The dispel walk asks `cast_reach`, so it ends exactly where the cast becomes
possible. The pet strikes, Spider Web and Boar Charge, ask
`pet_ai::pet_strike_reach` (range, Boar Charge's dead zone, sight) on every
path: the Hunter's dispatch, the pet's own while its Hunter casts, and the
pet's re-check when it executes a dispatched command
(`pet_ai::pet_command_rejection`), which also re-checks a Master's Call's
sight. A Hunter trap is thrown at a POINT, so every throw asks sight of
its landing instead (`hunter::try_place_trap_at`, after range): one a pillar
hides is held, traced `LosBlocked`, never moved. The lane Freezing Trap picks
its landing with sight in mind: when a pillar hides the lane, it lands on an
enemy's route round that pillar, at the first point of it the Hunter sees once
the trap would arm (`hunter_lane::route_lane`). With no such route it is held
in `fallback_trap` itself (`LosBlocked`), so the Serpent Sting never stays
reserved for that throw. A no-op on obstacle-free maps
(BasicArena stays byte-identical).

**Dying teammate first (Druid)** — no RON knob; reuses `urgency_hp_threshold`.
A teammate below the threshold that the Druid can reach (range, then sight) gets
what its kit can still add before any damage or utility (Mark of the Wild, a
Cyclone or Entangling Roots that is not a peel, Moonfire): below
`DRUID_EMERGENCY_HP` (0.45) Swiftmend or the Rejuvenation that arms it;
otherwise its own Rejuvenation, then a Lifebloom stack while it is under attack
(a threat only closing on it gets the Rejuvenation, not the stack). Entangling
Roots on a melee enemy or pet attacking or closing on a dying teammate is a PEEL
and comes before that stack. With nothing left to add, the damage goes out. While the
medic walk is live and its teammate is NOT yet in reach, damage and utility are
held outright (traced `PreconditionUnmet`, "holding the global cooldown ..."):
every Druid spell is an instant on the global cooldown, so a Moonfire cast as
the walk rounds a pillar edge would spend the 1.5s in which sight comes back.
The peels (Cyclone for a focus below `DRUID_URGENT_HP`, Roots for a dying
teammate), Travel Form, Innervate and the heals are never held. The rule lives
in the Druid's rotation order (`class_ai/druid.rs`, `DruidTurn::take` /
`try_dying_heal` / `try_roots_peel` / `held_for_medic_walk`; the walk's
teammate reaches it as `CasterHealerPlan::medic_walk`), so no other healer's
deferral changes (`tests/druid_dying_heal.rs`). Under `TeamPlan` PRESSURED,
where the medic chase is retired, the walk is the team solve's: the dying
teammate its last re-solve owed reach to (`C_DYING`, and the sight fallback
that walks straight at it — `team_solve::dying_anchor`), kept on
`HealerPosture::solve_dying` and read through
`healer_postures::solve_dying_walk`, which drops it outside PRESSURED, while
the healer is hard-CC'd, and once the teammate is dead or no longer dying. Unlike
the `Legacy` walk it covers a dying teammate in sight but beyond heal range,
since the solve walks into range too. It feeds only the Druid's hold; any other
healer may read it (`tests/druid_teamplan_dying_walk.rs`).

**Travel Form escape (Druid)** — `druid.weights.flee`. A shifted Druid is in
ESCAPE for as long as it stays shifted (`evaluate_caster_healer_posture`'s
`shifted` argument; the Shaman passes `false`): it runs from every visible
CHASER (melee or pet — the form cannot outrun a spell) inside
`threat_intent_radius` and re-scores each `commit_window`, traced as
`ShiftEscape` on entry and `CommitExpired` after. It is a kiter's run, not a
healer's retreat: `flee` maximises distance from the nearest chaser at every
range, and `commitment_bonus` pulls each re-score toward the last heading.
Proximity-faded `threat_repulsion` alone was worth a fraction of a point
against a chaser 10yd off, so the edge and cover terms flipped the heading back
toward the chaser every window (AS-200). `escape_tick` reads both only for a
chaser escape, so the Priest's, Paladin's and Shaman's impairment windows are
unchanged. The run keeps room to run (AS-241): its `flee` scores a heading by
the distance from the chaser it gains over the open ground it has, up to
`SHIFT_FLEE_REACH` (8yd, `ScorerInputs::flee_reach`), so a Druid with a wall
or a pillar at its back runs along it rather than into it; and it never steps
deeper into a dead end (`MASK_DEAD_END`, `ArenaBounds::dead_end_depth` —
Nagrand's starting rooms), a mask kept past the anchor's on the fallback
ladder. On the AS-164 Nagrand Druid panel a shifted Druid entered a starting
room in 2.1% of `Legacy` and 2.0% of `TeamPlan` matches, from 23.1% and 21.6%
(`docs/design/balance/2026-10-31-as241-travel-form-escape.md`); staged in
`tests/druid_escape_room_to_run.rs`. The shift and the shift out
are the Druid AI's (`class_ai/druid.rs` — `shift_trigger`, `should_leave_form`);
rules in `docs/design/wow-mechanics.md` (Travel Form). A shift that breaks a
root or snare takes the Druid's next global cooldown: an emergency heal may
take ONE first (`Combatant::druid_emergency_heal_spent` remembers it until the
Druid is free), pinned by `a_root_is_broken_on_the_next_global_cooldown`.
Mark of the Wild waits behind a shift owed to break a root or snare, as it
waits behind a dying teammate's heals: a Mark purged off the Druid is not
recast while it stands rooted under a chaser (`DruidTurn::take`, pinned by
`a_root_is_shifted_out_of_before_a_lost_mark_is_recast`).

**Tangent steering (goal-directed pillar rounding)** — `map_geometry::steer_toward_goal`
(pure, unit-tested; no RON knob). When a mover with a DESTINATION has the
straight line to it blocked by an obstacle, it aims at the obstacle's TANGENT
POINT on the better-progress side (for a cylinder: the external tangent to the
`radius + MOVER_RADIUS` circle; for a prism: the tangent vertex of the inflated
polygon; for a box: the nearer visible silhouette corner)
instead of pointing at the goal through the obstacle — so it rounds the pillar in
a clean full-speed arc rather than oozing along the surface. Without it,
`slide_against` removed only the inward step component, leaving a near-zero
tangential sliver whenever the goal sat directly behind a pillar (the "stuck to
the pillar" ooze). `resolve_movement` stays the final no-clip backstop; steering
just keeps the mover off the surface so the resolver rarely bites (`slide_against`
was left unchanged — the tangent aim already preserves full speed, and touching it
risked the collision probes / byte-identity). Wired into the FOUR goal-directed
branches of `move_to_target`: `MovementGoal::Point` (chase/medic/formation walks,
incl. the `seek_chase_timeout` direct chase), `MovementGoal::Entity` (DIP chases),
normal pursuit-to-target, and pet-follow-to-owner. NOT applied to
`MovementGoal::Direction` (scorer output — the context-steering mask already
avoids obstacles), fear/polymorph wander, or Charge/Disengage (scripted dashes).
Side commitment is emergent, not stored — no per-frame committed-side state — but
only because each shape's side SCORE is chosen to be self-reinforcing (once off
the center line, that side keeps winning), with a `STEER_TIE_EPS` fixed default
for the exact-tie instant. A tie-break alone does not buy that: the cylinder
scores heading alignment, which holds on a curve, but on a prism's flat face
alignment is anti-reinforcing (a step toward one end tilts the goal toward the
other), so a mover flush against a Nagrand face with its goal straight behind
flipped sides every tick and jittered in place (AS-181). The prism therefore
scores the taut-path length round each side, which shortens by the full step on
the side being walked. A new obstacle shape needs a score with that property,
and a flush-against-the-face step-loop test (`steer_flush_against_a_prism_face_commits_to_one_side`)
to prove it. A goal INSIDE a prism's footprint (a spot no one can stand on) is
steered to its nearest shell point and held a body radius short of it — the
helper returns `Some(Vec2::ZERO)`, which every caller treats as "don't move";
rounding toward the unreachable point itself parked the mover on the nearest
vertex and flipped it across that vertex every tick. That hold is a backstop,
not a licence to issue such goals: the healers' FREE formation point (under both
AI profiles) and the `TeamPlan` team solve both move an in-footprint point onto
the footprint's skin (`map_geometry::nearest_standable`, AS-190 and AS-203), and
`tests/standable_point_goals.rs` fails on any `Point` goal it observes inside a
footprint, in its `TeamPlan` cases on Nagrand and its `Legacy` cases on Nagrand
and TwinPillars. A new producer of `Point` goals owes the same projection. The
helper's first line is
`if obstacles.is_empty() { return None }` and each caller falls back to its exact
legacy direct-normalize on `None`, so **BasicArena stays byte-identical**. This
makes competent pursuers (melee and Mage) round pillars cleanly; a documented
consequence is that a pressured healer's `cover_pull` LoS-denial buys far less
sustained occlusion against a steered melee trainer (a competent melee now
re-acquires around the pillar quickly — see the `u8_healer_cover` probe note).

**Pursuit walks for sight** — `combat_core::movement::pursuit_closes` (no RON
knob). Normal pursuit stops at a unit's preferred range only once it SEES its
target: a non-healer in range and out of sight keeps walking (the same
tangent-steered pursuit step), and stops the frame sight returns — sight by
`has_line_of_sight` between the two positions, the casts' own test. Stopping
in range behind a pillar parked a Warlock with every cast refused `LosBlocked`
beside a lone, out-of-mana healer that was parked too, and the match drew at
the cap (AS-360). Healers do not walk for sight: their posture machine owns
where they stand, and one out of sight of its enemy is usually in cover on
purpose. Both AI profiles; a no-op on obstacle-free maps (BasicArena stays
byte-identical). The Warlock is the class it moves — the Mage and Hunter
answer occlusion with their ENGAGE machine first. Staged guard:
`tests/pursuit_sight_walk.rs`.

**Team sight yield (`TeamPlan`)** — no RON knob; AS-386, design
`docs/design/2026-10-24-team-sight-cohesion.md` §3.1/§3.4. The `sight_yield`
situation names, per DPS–healer pair, whether the DPS must not break its
healer's sight (`situation::SightPair`, `YieldRule`). First rule that holds: a
unit that cannot move leads (a healer hard-CC'd, rooted or casting: its DPS
yields; a DPS hard-CC'd or rooted: its healer follows); a pressured healer
leads; the team's pillar stance leads (`TeamSituation::pillar_stance`, read in
the same classification: `update_situations` classifies the stance, then the
yield); a hurt DPS (below `SIGHT_YIELD_HP` 80%, held until 90%) yields;
otherwise the DPS leads. A yield holds `SIGHT_YIELD_DWELL` (2s) unless rule 1
ends it, and every onset / end / rule shift is a `situation_change` named
`sight_yield` (`subject` the DPS, `healer`, `rule`). A yielding DPS: the
kiter scorer's ally-anchor mask points at its healer for SIGHT only (a MASK on
the kite objective, never a replacement — `flee`, `range_band`, hysteresis and
the wand fallback are untouched; a kiter already blind gets `close_when_out`);
its pursuit takes the tangent side round a pillar whose waypoint its healer
sees, and HOLDS it (`PursuitSide`, `combat_core::movement::sight_kept_heading`
— a sight preference is not self-reinforcing). `team{1,2}_sight_yield: false`
is the measurement switch, and `team{1,2}_yield_healer_cast: false` leaves the
healer's cast out of rule 1 (measured to change almost nothing); `Legacy`
classifies no pair and is byte-identical. `validate()` keeps
`urgency_hp_threshold` at or below `SIGHT_YIELD_HP`, so a dying DPS is a hurt
one. Scenes `tests/sight_yield_scenes.rs`, staged `tests/sight_yield_staged.rs`;
measured with the stance as one change in
`docs/design/balance/2026-10-24-as385-package-measurement.md`.

**Team target focus (`TeamPlan`)** — no RON knob; AS-389, design
`docs/design/2026-10-25-ranged-dps-vs-hidden-targets.md`. Target acquisition
never counted sight as part of a target's validity, so a ranged DPS tunnelled
an enemy healer hiding behind a pillar while the DPS it had left exposed stood
in reach. The `target_focus` situation (`class_ai::target_focus`) keeps, per
ranged DPS (Mage, Hunter, Warlock), how long its target has been out of its
sight (`TargetSight`, the cast's own sight test). Once one has been blind for
`FOCUS_HIDDEN_SECS` (1.5s) and another living, visible, non-immune, non-pet
enemy stands within `FOCUS_REACH` (35yd, Auto Shot) and in its sight, the
team's focus moves to the nearest such enemy (`Focus { subject, from, since }`).
A focus holds `FOCUS_DWELL` (2s); it returns to its old target only by the same
rule (the focus itself hidden 1.5s, the old target in sight), and ends when its
subject can no longer be taken (dead, stealthed, immune). `combat_ai::acquire_targets`
is its only reader: it reads the focus from the `Situations` resource (last
frame's classification) through `target_focus::retarget`. A ranged DPS takes it
when it stands in its reach and sight, or when its own target is out of its
sight too. A DPS on the team's kill window subject never leaves it: its blindness
moves no focus (`classify`), and it takes none another DPS moved (`retarget`);
the player's kill call (`team{1,2}_kill_target`) wins over it, and the Hunter's
trap retarget runs after it. Melee DPS do not follow it (their target-blind time
is under 1% of their life, and they have their own swap). Traced as a
`situation_change` named `target_focus` (`subject` the focus, `from` the target
it left) and on every `ability_decision` as `situation.target_focus`.
`team{1,2}_target_focus: false` is the measurement switch; `Legacy` classifies
no focus and is byte-identical, as is a map with no obstacles. Scenes
`tests/target_focus_scenes.rs`, staged `tests/target_focus_staged.rs` (`Scene::acquire`
reads the retarget). Measured head-to-head over three seed blocks: +4.4pt (z +25.5) for the side
that runs it; in a world where both do, the ranged DPS gain 4-8pt, the Rogue
loses 10.3pt and the healers lose too (Druid -2.0, Priest -3.5, Shaman -2.9;
Paladin -0.8, not an effect), pooled over three blocks: the ruled balance shift
(§8 of the design).

**DPS kiter blocks** (`mage:` / `hunter:` — the shared ENGAGE/KITE machine, `DpsMovementConfig`):
- `weights:` (above) plus `range_band_min`/`max` (orbit ring; min = SAFE_KITING_DISTANCE / HUNTER_DEAD_ZONE 8), `kite_hold` (anti-strobe hysteresis), `directive_ttl` (must cover the longest cast), `commit_window`.
- `kite_entry_radius`/`kite_sustain_radius` — proximity-gated kiters only (Hunter: KITE when a melee is within entry, exit when kited past sustain). The Mage is aura-gated (KITE keys off its own root/slow), so it ignores these.
- **Mage OOM wand fallback** (no config knob — behavior, like the melee tempo reset). A Mage out of mana for a Frostbolt parks at its Frostbolt-safe preferred range (38yd), which is OUTSIDE its equipped wand's range (30yd), so it idles the mana refractory dealing ~1 damage event per 20s (the lone-Shaman 2v1 drag). Fix: while out of mana, the Mage's ENGAGE pursuit stop distance drops to wand range so `move_to_target` closes it to 30yd and the wand auto-attack chips through the refractory. Gated by a hysteresis latch on `KitePosture::wand_oom`: `update_oom_wand_latch` engages it when a Frostbolt is unaffordable (`mana < cost`, cost read from `AbilityDefinitions`, never hardcoded) and releases it only once mana recovers to a two-cast buffer (`>= 2*cost`), so the standoff doesn't strobe 38↔30 as mana sawtooths across one cast's worth. `evaluate_dps_posture` folds the latch each evaluation (via `WandPullGate`; the Hunter passes `None`); `move_to_target` reads it. A DIRECT radial pursuit is used, not the KITE orbit scorer, because the orbit's LoS-preserving lateral meander is slow to corner a juking target — and because it only touches the SIGHTED-parking case, the occluded juke-chase seeds are byte-identical. Inert at healthy mana (latch false → preferred range 38, unchanged).
- `seek_chase_timeout` (mage/hunter 3.5) / `seek_chase_decay` (mage/hunter 0.5) — the leaky-bucket occlusion-chase arm. An ENGAGE kiter accumulates "occlusion units" while occluded from its kill target in shot range (the `los_seek` orbit-seek stall): the bucket **fills** at a fixed 1.0/sec and **drains** at `seek_chase_decay`/sec while it has sight, clamped at 0. Once the bucket reaches `seek_chase_timeout` the chase arms — the kiter abandons orbit-seeking and walks straight at the target's live position (a `Point` directive, TTL = `directive_ttl`) while it remains occluded, until sight returns. This counters a target hugging a thin pillar, where `los_seek` gives no gradient because every candidate step is occluded. The bucket is the fix for a JUKING target (occlude mid-cast, flash back between casts) that the old continuous clock never armed against — because `seek_chase_decay` (0.5) is below the 1.0 fill, intermittent occlusion still ratchets to the threshold instead of resetting on each sight flicker. A target under CONTINUOUS occlusion fills at 1.0/sec, so it still arms at exactly `seek_chase_timeout` seconds — the static pillar-hug case is unchanged. `seek_chase_timeout` `0.0` disables the chase; `seek_chase_decay` `0.0` never drains (permanent arm once crossed) and `>= 1.0` restores continuous-only arming. The bucket (`KitePosture::occlusion_accum`) is owned by the per-frame `tick_kite_occlusion` system, which ticks even CASTING kiters (excluded from the ability-decision query) so the mid-cast juke is observed; `evaluate_dps_posture` only reads it. No-op on obstacle-free maps (never occluded → bucket stays 0). Traced via the existing `SeekLos` trigger; a `goal_kind` of `point` distinguishes chase from the `direction` orbit-seek.

**Paladin-only block** (`paladin:` — alongside its `weights:`):
- `fallback_range: 15.0` — PRESSURED retreat range (instead of face-tanking at melee)
- `dip_budget: 6.0` — DIP walk-stun-return duration budget in seconds
- `healing_heavy_hp: 0.6` — lowest team HP fraction (self included, pets excluded) below which the Paladin pulls to fallback range even before it is focused

The Paladin's **while-CC Divine Shield** (`try_divine_shield_while_cc`, the CC-break path) fires when self HP is below `DIVINE_SHIELD_HP_THRESHOLD` (0.3, unchanged) OR — the widened teammate trigger — a non-self ally is below `LOW_HP_THRESHOLD` (0.5) AND the max remaining incapacitation on the Paladin is `>= DIVINE_SHIELD_MIN_CC_REMAINING` (2.0s). The bubble purges the Paladin's own CC, so it is the fear-break tool; the CC-remaining floor keeps the 5-minute cooldown from burning when the break would buy no real acting time. Decision seam: pure `divine_shield_while_cc_should_fire`. This changes behavior on ALL maps where the trigger actually fires (fear + low ally, BasicArena included) — intended, like the melee tempo reset.

After editing, validate and sweep:
```bash
cargo test                          # validate() + posture probes/unit tests
```

Then sweep at the tier the change calls for — see *Run a balance sweep* below
and `docs/design/balance/sweep-tiers.md`. A retune is DIRECTIONAL unless its
whole point is the magnitude:

```bash
scripts/gen_sweep.py --full 2 --exclude-double-healer \
  --affects Priest --control-cells 8 --n 10 > /tmp/sweep.jsonl
# one arm per movement.ron, then:
scripts/paired_sweep.py before.csv after.csv \
  --affects Priest --tier directional --expect nothing
```

### Class Design
- **Warrior**: Rage (generates on damage), melee, Charge/Mortal Strike/Pummel
- **Mage**: Mana, ranged, Frostbolt/Frost Nova/Polymorph
- **Rogue**: Energy, melee, Stealth/Ambush/Kick/Eviscerate
- **Priest**: Mana, healer, Flash Heal/Mind Blast/Power Word: Fortitude
- **Warlock**: Mana, DoT caster, Corruption/Shadow Bolt/Fear; Life Tap turns health into mana when it is short of mana and healthy (`docs/design/wow-mechanics.md`, Life Tap)
- **Paladin**: Mana, healer/melee, Holy Shock/Flash of Light/Hammer of Justice
- **Shaman**: Mana, offensive healer, Lightning Bolt/Lesser Healing Wave/Purge/Wind Shear/Frost Shock and totems
- **Druid**: Mana, restoration healer (instant heals over time) and control, Rejuvenation/Lifebloom/Swiftmend/Entangling Roots/Cyclone/Moonfire/Innervate, a Travel Form escape shift; no wand
- **Hunter**: Mana, ranged physical DPS with pet, Aimed Shot/Arcane Shot/Concussive Shot/Disengage/Freezing Trap/Frost Trap/Flare. Two auto-attacks on separate timers: Auto Shot at 8-35yd, and a melee swing from its main-hand (and off-hand) weapon inside `MELEE_RANGE` when it holds one (the default loadout does not); nothing in the dead zone between (see `docs/design/wow-mechanics.md`, Auto-Attacks). Pet engagement model: pet inherits Hunter's target, pursues into melee via existing target-pursuit movement, and retreats ("Heel") when pet HP drops below 25%. Per-pet headline abilities (Spider Web, Boar Charge, Master's Call) are dispatched by Hunter AI via the `PetCommand` component (hybrid model — Hunter owns headline calls, pet handles auto-attacks and pursuit). When Hunter is mid-cast (CastingState excludes it from `decide_abilities`), `pet_ai_system` falls back to autonomous dispatch using the same predicate logic; trace events distinguish via `dispatched_by` (set for Hunter dispatch, omitted for autonomous). Iteration 2a shipped pet target ownership + Heel predicate + PetCommand framework; iteration 2b shipped the Hunter `try_dispatch_*` helpers plus the pet-side `pet_command_rejection` authoritative check, with the autonomous fallback kept to cover Hunter's CastingState windows.

## Common Tasks

### Test a balance change
```bash
# Make changes, then:
cargo build --release
echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json
cargo run --release -- --headless /tmp/test.json
cat match_logs/$(ls -t match_logs | head -1)
```

### What a byte-identity result proves (read before citing one)

Nearly every sim-adjacent change here is gated on headless byte-identity: run
the seeded matches before and after, diff the logs or the CSV, and a clean diff
is read as "the change did nothing". That reading is sound, but it is one step
weaker than it looks, and it is worth knowing exactly where the step is.

**A clean diff means "the change did nothing observable AND no float wobble
flipped a roll" — not, strictly, "nothing changed".** The sim is a chain of
comparisons against RNG draws, so a difference too small to print can still
flip a roll and cascade through the whole log; equally, a real difference can
hide inside a comparison's margin and print nothing. Byte-identity is evidence
about OUTPUT, never a proof about state.

**The only known source of run-to-run wobble that reaches the match log is
fixed (AS-58).** Loadouts were `HashMap`s, and applying one sums float stats in
iteration order — which the default `RandomState` reseeds every process.
Measured on one unmodified release binary over 40 runs: Rogue `crit_chance`
took three distinct bit patterns (`0x3e2e147a` / `0x3e2e147b` / `0x3e2e147c`, a
2-ULP spread, ~3e-8). Loadouts are now `Loadout = BTreeMap<ItemSlot, ItemId>`,
so the order is a property of the type; `equipment::tests::loadout_is_ordered`
and `tests/loadout_order_audit.rs` keep it that way.

**Read that narrowly — other hash-ordered float reductions still exist.** Card
AS-75 carries four more. They are latent rather than fixed: their results
(`CombatLog::total_damage_dealt` / `total_healing_done` among them) have
test-only callers today, so nothing they sum reaches a match report. Give one a
production caller and the wobble is back. At any NEW site that folds floats out
of a `HashMap`, use an ordered map — or sort before the fold.

**What follows for how you read a diff:**

- **A clean result was never at risk.** This bug could only ever manufacture a
  FALSE DIFFERENCE — a diff with no code change behind it. It could not
  manufacture a clean one. Every "byte-identical" result in the project's
  history therefore still stands.
- **Attribute a difference positively, never by elimination.** "The only thing
  I changed was X, so the diff is X" is the reasoning this bug broke. Say what
  in the diff the change predicts, and check the trace for it — the way
  `docs/plans/2026-06-12-001-refactor-context-steering-masks-plan.md` R6 does,
  requiring an all-masked frame in the trace of the entity that actually moved.
- **When you cannot attribute a difference positively, run a same-binary
  control** — the identical binary twice — before you spend time on it. That
  control costs one sweep and is now the first thing to try on any surprising
  one-row difference, whatever its cause.
- **Report non-vacuity.** A byte-identity claim over a batch that drew no
  crits, or ended every match by timeout, proves nothing. Count the decisive
  events (kills, crits, distinct durations), not just the rows.

**A last-ULP stat change is far below the resolution of an outcome — but it is
the arithmetic that shows it, not the bracket.** Measured directly: a build
shifting every combatant's `crit_chance` down by exactly the 2 ULP the wobble
spanned produced logs byte-identical to canonical across 48 Rogue matches —
4295 damage events, 462 of them crits. (The ROLLS carry the non-vacuity weight,
not the crits: every non-DoT damage event rolls once against `crit_chance`, so
of order 4000 rolls, ~90 a match.)

**That bracket had to come out clean, which is why it is not general evidence.**
`random_f32()` lands on multiples of 2^-24 (~6e-8) and `roll_crit` is a strict
`roll < crit_chance`, so a shift flips a roll only if a multiple of 2^-24 lies
inside the window it scans. This one straddles none: `0x3e2e147c` is itself a
grid point (2852127 x 2^-24) and the smallest one at or above `0x3e2e147a`, so
`[0x3e2e147a, 0x3e2e147c)` contains no drawable value at all. The general claim
is the RATE: a 3e-8 window holds at most one drawable value, so a roll flips
with probability ~3e-8 — of order one flipped match per few hundred thousand at
~90 rolls each. The protocol has not been losing signal. But that is the rate
talking, and a window that DID straddle a grid point would flip at it, so a
clean 2-ULP bracket over one window is no licence for last-ULP changes at large.

### Run a balance sweep — and pick its TIER first

**[Sweep tiers](docs/design/balance/sweep-tiers.md) is the authority. Read it
before sizing a sweep or citing one.** Two named instruments, and a card
states which it is using and why when the card is written:

- **DIRECTIONAL** — the cells the change can reach, sized in minutes. Answers
  *which way, roughly how much*. **May not be cited as a class's standing.**
- **AUTHORITY** — the full treatment, for a number that will be cited later (a
  canonical baseline, or a change whose whole point is the magnitude).

**The cheap tier is SMALLER, not looser.** Both keep every rule about what a
number means: paired at identical seeds, McNemar on the flips, non-vacuity
reported, no per-cell figures at small n. What the directional tier cuts is
CELLS, never seeds — in a paired design the significance comes from the flip
count across the whole run, not from per-cell precision.

The pipeline is three commands. Both arms run the SAME JSONL, so every match
has a twin differing only by the change:

```bash
cargo build --release
# 1. cells: everything a Shaman change reaches, plus 8 control cells it cannot
#    (233 cells x 10 seeds = 2,330 matches, the default directional size)
scripts/gen_sweep.py --full 2 --exclude-double-healer \
  --affects Shaman --control-cells 8 --n 10 > /tmp/sweep.jsonl
# 2. one arm per binary (or per config), same file both times
target/release/arenasim --batch /tmp/sweep.jsonl --out before.csv \
  --jobs 6 --trace-mode off   # key --jobs off who else is sweeping, see below
# 3. the analysis: control, four slices, McNemar, resolution floor, verdict
scripts/paired_sweep.py before.csv after.csv \
  --affects Shaman --tier directional --expect nothing
```

`--expect` is the card's success condition; `paired_sweep.py` answers it in
its own terms and prints the resolution the run actually bought, because a
null result means nothing without it.

**The control fields every class the change cannot reach, on both sides.**
`gen_sweep.py` builds it that way and refuses a `--control-cells` below the
fewest cells that can (4 for a one-class 2v2 change, 8 for 1v1; the default 8
clears both; `0` drops the control with a warning). `paired_sweep.py` prints
which classes the control fields per side, and a class the sweep fields in an
unaffected team but the control never does is a `CONTROL BLIND SPOT` — the
control fails and the tool exits 1, because a leak into that class would pass
it clean. `--control-elsewhere` excuses it only by a declaration printed into
the report. Detail in the sweep-tiers doc. `scripts/agg_sweep.py` does single-arm
win-rate tables; `scripts/{hunter,mage,shaman}_2v2_matrix.sh` and `--matrix N`
are the older per-cell-CSV wrappers (columns per `src/headless/matrix.rs:217`).

**Size a sweep at about 20 matches/sec per `--jobs` worker.** Measured on a
2,700-match 2v2 BasicArena sweep at load ~26: 45/sec at `--jobs 2`, 87/sec at
`--jobs 4` and 118/sec at `--jobs 6`. A match costs ~0.04 CPU-seconds, about
2% of it kernel time. Halve that for a mix heavy in 3v3, PillaredArena or
`TeamPlan` (~0.065 CPU-seconds a match), or on a box far past its core count
(a 5,442-match mixed batch ran at 23/sec at `--jobs 4` at load 46). A
directional sweep is about a minute, and an authority run is minutes, not
hours. That rests on the headless runner putting EVERY schedule on the
single-threaded executor (`force_single_threaded_executors`, guarded by
`tests/headless_executor.rs`). A schedule left multi-threaded parks each worker
on a condvar every tick and costs 30x or more with byte-identical output, so
only the throughput shows it. See "What a sweep actually costs" in the
sweep-tiers doc. Size
`--jobs` off how many agents are sweeping RIGHT NOW rather than a number
picked at launch; a conservative 6 chosen off `uptime` still lost once a
third agent started. Contention costs wall clock
and nothing else — outcomes do not depend on how the box was scheduled, so a
busy box returns what a quiet one would, later.

### Diagnose AI behaviour with the decision trace

Capture the AI's per-tick reject/choose decisions as JSONL alongside the
match log. The trace shows every ability the AI considered with a typed
rejection reason (out of range, on cooldown, friendly-CC guard, etc.) —
turns "why didn't X cast Y?" from a code-read into a `jq` query.

```bash
# Single match — opt in via --trace-mode on
cargo run --release -- --headless /tmp/test.json --trace-mode on
# Trace lands beside the report, with its stem: match_logs/match_<secs>_<pid>.txt
# and match_logs/match_<secs>_<pid>_trace.jsonl, or <out>_trace.jsonl beside an
# --output <out>.txt — so concurrent runs never overwrite each other's

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

# Situations (`situation.rs`). Every ability_decision carries the `situation`
# its team was in: `phase` ("prematch" / "engaged" / "attrition") and `tempo`
# ("even" / "pressing" / "trailing") always; `enemy_healer_locked`
# ({subject, since, until} on the sim_time clock) while the enemy healer is in
# cast-preventing CC; `ally_in_danger` ({subject, since}) while a teammate is
# both dying and pressured. `situation_change` events mark the transitions only
# (onset / end / extended / shift), carry `team` instead of an `actor`, and
# sort ahead of the decisions in their frame — so a change classified MID-frame
# (after an earlier unit's instant CC) prints ahead of that frame's decisions
# that never saw it. Read a frame with a change in it with care;
# `decision_trace_audit` skips those frames for that reason.
# Per class, the ability chosen while the enemy healer was locked:
jq -r 'select(.kind == "ability_decision" and .situation.enemy_healer_locked != null and .outcome.type == "action_taken") | "\(.actor.class) \(.outcome.ability)"' $T | sort | uniq -c
# Every lock window, per team (a burst window the team could use):
jq -c 'select(.kind == "situation_change" and .name == "enemy_healer_locked") | {t: .sim_time, team, change, healer: .subject, since, until}' $T
# Every teammate-in-danger window, and what each class chose inside one:
jq -c 'select(.kind == "situation_change" and .name == "ally_in_danger") | {t: .sim_time, team, change, ally: .subject, since}' $T
jq -r 'select(.kind == "ability_decision" and .situation.ally_in_danger != null and .outcome.type == "action_taken") | "\(.actor.class) \(.outcome.ability)"' $T | sort | uniq -c

# Plays. `situation.play` is the deciding unit's play when it is not the
# default: {"name": "burst"}, or {"name": "peel", "ally": <id>} (TeamPlan
# only). `situation.play_step == "peel"` marks a choice the Peel play's own
# step made — crowd control on an attacker of the teammate in danger — rather
# than the chain below it. Every peel, and what each class peeled with:
jq -c 'select(.situation.play_step == "peel") | {t: .sim_time, class: .actor.class, ability: .outcome.ability, on: .outcome.target_id, for: .situation.play.ally}' $T
# Why a unit under the Peel play did NOT peel (its peel tools' rejections):
jq -r 'select(.situation.play.name == "peel" and .situation.play_step == null) | .actor.class as $c | .candidates[] | select(.status == "rejected") | "\($c) \(.ability) \(.reason | if type == "object" then keys[0] else . end)"' $T | sort | uniq -c | sort -rn

# Who was a trap AIMED at? A trap is placed at a POSITION and springs on the
# first enemy to reach it, so its victim is not its target and the outcome
# alone cannot say who the Hunter meant to catch. The chosen event carries the
# intended victim in `target_id`; pair it with the `[TRAP] ... triggers on ...`
# line in the .txt log to see whether the trap caught who it was aimed at.
# (The Frost Trap dropped at the Hunter's own feet aims at nobody, and a lane
# Freezing Trap thrown for a stealthed enemy cannot name it: target_id is
# absent there.) Measured answer: `docs/design/balance/
# 2026-09-18-as68-freezing-trap-diagnosis.md`.
jq -c 'select(.outcome.ability == "FreezingTrap") | {t: .sim_time, aimed_at: .outcome.target_id}' $T

# Why a Freezing Trap on the enemy healer was held while a melee was on the
# Hunter (`hunter::try_pressure_trap`) — out of throw range, would not spring
# it cleanly, a teammate killing it, a teammate of its would free it. (A throw
# moves the Hunter off that healer onto the melee, kill target or not; the
# target-switch recipe above shows it.)
jq -r 'select(.actor.class == "Hunter") | .candidates[]? | select(.ability == "FreezingTrap") | .reason.PreconditionUnmet.note // empty' $T | grep '^healer trap' | sort | uniq -c

# Melee avoidance (`combat_core/avoidance.rs`): every dodged or parried melee
# attack, the attacker as `actor` and the defender as `target`. Matches no
# event in a match where nobody has a dodge or parry chance.
jq -c 'select(.kind == "melee_avoided") | {t: .sim_time, by: .actor.class, on: .target.entity_id, attack, avoided}' $T

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
# directions (bit i set when candidate i was eliminated by the boundary,
# ally-anchor, or obstacle (MASK_LOS) mask). Present only when the scorer ran.
# A value of 65535 (0xFFFF) is an all-masked frame, where the fallback ladder
# fired (lift order: anchor -> LoS -> boundary) — the ONLY legitimate source
# of Part A behavior divergence from the old penalty scheme, so this is the
# query R6 byte-identity attribution uses on a divergent cell.
jq -c 'select(.kind == "movement_decision" and .masked == 65535) | {t: .sim_time, class: .actor.class, entity: .actor.entity_id, posture}' $T

# LoS-only eliminations — `los_masked` is a strict subset of `masked` carrying
# just the obstacle-blocked candidates; emitted only when nonzero, so it never
# appears on obstacle-free maps (BasicArena traces are unchanged).
jq -c 'select(.kind == "movement_decision" and .los_masked != null) | {t: .sim_time, class: .actor.class, masked, los_masked}' $T

# Why didn't the Mage cast? LoS rejections by ability (fires on PillaredArena /
# TestVerticality when a pillar blocks the segment at cast start)
jq -c 'select(.actor.class == "Mage") | .candidates[]? | select(.status == "rejected" and .reason == "LosBlocked") | .ability' $T | sort | uniq -c

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

### Test an AI decision: scenes first, and never re-pin a seed

**A seed is a proxy for a situation, and proxies drift.** Every
balance-relevant change moves the RNG stream, which moves which seeds contain
the situation a seed-pinned probe was written for — and a situational rule that
merely reorders a priority chain often leaves every match byte-identical, so no
seed probe or sweep can see it at all. Test the decision where the situation is
constructed:

- **Scene tests (tier 1, milliseconds)** —
  `arenasim::states::play_match::class_ai::scene::Scene` builds the units
  (class, team, position, health, resource, auras, cooldowns, obstacles,
  profile) and `scene.decide(who)` runs **`combat_ai::dispatch_class_decision`,
  the same entry point `decide_abilities` runs**, for one tick, returning a typed
  `DecisionView` (chosen ability and target, every candidate with its
  `RejectionReason`, the movement decisions, the `situation`). No seed: no
  class decision draws from `GameRng`. Write each as a **pair** — the situation
  and its control with its one condition removed — so the test cannot pass
  vacuously. `tests/situation_scenes.rs` is the worked example.
- **Staged matches (tier 3, under a second each)** — for a behaviour that
  needs time: a walk, a chase, an escape, a posture cycle.
  `arenasim::headless::run_headless_match_staged(config, …, stage, observer)`
  plays a real match whose state the `Stage` builder (`headless::stage`) sets
  on the frame the gates open: positions, health, resource, targets, auras
  (`aura` straight in, or `land` through `apply_pending_auras`), cooldowns,
  posture states, the match clock, or anything else via `world`. Units are
  named by team and slot (`Unit::new(2, 1)`). The countdown still plays, so
  the pre-match buffs land; from the gates on, the production schedule plays
  the staged world under the trace and frame observer. The seed governs only
  the rolls — the situation holds by construction. `tests/common/staged.rs`
  plays one and reads it back; `tests/druid_dying_heal.rs`,
  `tests/medic_chase_hold.rs` and `tests/dispel_walk_dying_first.rs` are worked
  examples. `tests/staged_match.rs` holds the harness to two claims: an empty
  stage plays its seed byte for byte, and a stage that writes a seeded match's
  whole state at a quiet frame T onto a match on ANOTHER seed plays on from T
  byte-identically to the seeded match. A stage describes state, not things in
  flight: a projectile, a pending effect or a cast in progress is not in its
  vocabulary.
- **The tiers beyond** (decision properties over a scene grid,
  situation-conditional probes over a seed bank) are planned in
  `docs/design/proposals/2026-10-11-ai-architecture.md` §3.2.
  `tests/peel_play_probe.rs` is the first situation-conditional probe: it
  reads its windows off the trace's `situation_change` events, plays the same
  bank with the play excepted in every context (`team{1,2}_exceptions`
  naming `plays::every_exception_entry`, the control of a graduated play),
  and asserts in units of the bank's own noise (a z), never against a fitted
  number.

**Plays.** A play is a named reordering of a class's own chain
(`situation::Play`). Every play runs only under `TeamPlan`. A new play is
GATED until a milestone sweep graduates or deletes it: a gated play can be
withheld from one team (`team{1,2}_withheld_plays`), the paired control it is
measured against. A GRADUATED play is permanent under `TeamPlan` and cannot be
withheld (the config refuses it, `PlayId::is_gated`); `Legacy` stays frozen
as the reference, so graduating deletes no `Legacy` rule. Its paired control
is its exceptions table naming every context (`team{1,2}_exceptions` set to
`plays::every_exception_entry`, `Scene::except_everywhere`). At the 0.8 sweep
(`docs/design/balance/2026-10-25-0.8-milestone-sweep.md` §3; user rulings
2026-10-25, applied by AS-398) Peel, Hunt, Chain and Burst graduated,
TakeCover was deleted (−0.2pt in both formats, −1.7pt to Paladin teams), and
Pillar stays gated. Each play is a STAGE (`class_ai::plays::PlayStage`):
its id, `calls` (the team situation asks for it, for this unit and role),
`admits` (this unit has a tool for it), its own named exceptions table with
the measurement behind each entry, and its config key. ONE table orders them,
`plays::PLAY_ORDER`: **Peel, Hunt, Pillar, Chain, Burst**, then the class's
default chain. `plays::walk` is the only composition rule, and both
`TeamSituation::play_for` and `CombatContext::play` take it: a unit runs the
first stage its team runs, the situation calls and the unit admits (its
exceptions included), and a stage that does not apply passes it to the NEXT
stage — a gated peeler still chains, and a unit with nothing to hand off
still bursts. `plays::tests` pins the order and the walk.

**Adding a play.** A new play is: a new module holding its stage and its
exceptions table; its step in each class's chain that runs it; its enum arms
(`Play`, `PlayId`, `PlayId::ALL`, `Play::id`, `PlayId::is_gated` — a new play
starts gated — and the trace's `PlayTag`, which the compiler asks for);
usually a window accessor on `CombatContext` for its step to read (as
`burst_window` and `chain_window` are); the pinned order tests
(`plays::tests`); and ONE line in `PLAY_ORDER`. What needs no change:
withholding (`team{1,2}_withheld_plays`), exceptions overrides
(`team{1,2}_exceptions`), `situation::WithheldPlays`, the scene builder
(`Scene::withhold` / `except_everywhere` / `lift` / `exceptions`) and the
walk, all keyed by the play's id. Graduating a play is one arm of
`PlayId::is_gated`, and its probes' controls moving from the withhold to
`every_exception_entry`. A play card that edits another play's stage, or the walk, is a
composition change, not an addition.

**Exceptions (user ruling, 2026-10-12).** A play that measures negative in a
context is not used there: not every behaviour suits every composition. An
exception is a NAMED entry in the play's own table — never a scattered `if` —
with a one-line justification citing its measurement: for Peel,
`peel::PEEL_GATES`, a (peeler class, class of the teammate in danger) pair;
for Hunt, Pillar, Chain and Burst, `hunt::HUNT_EXCEPTIONS`,
`pillar_stance::PILLAR_EXCEPTIONS`, `chain::CHAIN_EXCEPTIONS` and
`burst::BURST_EXCEPTIONS`, by class. At the 0.8
sweep Burst and Chain keep empty tables, Hunt keeps the Paladin, Druid and
Shaman, and Peel keeps AS-290's nine gates and adds Paladin@Hunter and
Hunter@Priest. Peel also has FORMAT gates (`peel::PEEL_FORMAT_GATES`, keyed
`ExceptionKey::in_format`, spelled `"Mage@3v3"` in a config): the Mage and
the Druid do not peel at all on a team of three
(`docs/design/balance/2026-10-25-as398-play-graduation.md` §1); the rest of
the 3v3 context selection is AS-327's. `tests/peel_gates.rs` lists the Peel gates by set equality and
plays every peeler for every teammate class, so adding or removing one fails
a default test until the list is edited too. Every exception is re-checked at
each milestone sweep with it lifted for the side under test
(`"team1_exceptions": {"Peel": []}` — an empty table lifts the play's
exceptions; a named one, `{"Burst": ["Hunter"]}` or `{"Peel": ["Mage/Hunter"]}`,
replaces them): it stays only while it still measures negative, and a context
that turns negative joins. The bar and the method are in
`docs/design/balance/2026-10-12-as290-peel-context-gates.md`. The older keys
`team{1,2}_lifted_gates`, `burst_exceptions` and `chain_exceptions` are a
deprecation shim onto the same overrides, kept so the committed sweep
generators still run.

`Peel`: while `ally_in_danger` names a teammate, every OTHER unit on the team
puts its peel step at the head of its chain — crowd control on one of the
teammate's `danger_attackers` (`class_ai::peel`; the tools per class are
`peel::peel_tools`, and the Warrior has none). Every peel goes through the
cast gate, skips an attacker the team already holds, and a movement peel
(root, snare) answers only a melee or pet. The Hunter's Spider and Boar peel
too, through its pet dispatch (`peel::pet_peel_tool`). The Priest's peel is
Psychic Scream on an attacker already inside it; it never walks to bring one
into range (*Psychic Scream under `TeamPlan`* below). Graduated at the 0.8
milestone sweep (+0.3pt in 2v2; `docs/design/balance/2026-10-12-as274-peel-play.md`
is its design) with eleven context gates and two 3v3 format gates.

`Hunt` (AS-394): the enemy is down to a lone healer
at a pillar (`hunt`, `class_ai::hunt`; within `HUNT_ENTER` of its skin, held
to `HUNT_LEAVE`, hidden from one of the team at onset) and the team has two or
more units, so two of them take STATIONS half a turn apart round that pillar,
`HUNT_STANDOFF` off its skin, about the healer's bearing at onset — from there
a hugger is in one station's sight at every bearing (the unit test
`two_stations_leave_a_hugger_nowhere_to_hide`; 160° leaves a sliver). Ranged
DPS take stations first; the healer takes the second when only one ranged DPS
does, and only while no teammate is dying (D2: a dying teammate is owed its
sight; once owed, not again until all are back to `SIGHT_YIELD_HP`); melee
take none; one viewer is no hunt. `HUNT_EXCEPTIONS` names the Paladin, Druid
and Shaman — healers with no attack that costs no mana, whose stations
lengthened endgames and drew matches — so only a Priest stations. The two
stationed units are the first that qualify in Entity order, not the nearest
(a 3v3 with two ranged DPS stations the lower-numbered pair). A unit holds its
station until it first reaches it; after that only while one of the two sees
the healer (`hold = !arrived || covered`), so an arrived unit gives its station
up for its ordinary walk for sight whenever nobody sees the healer, even while
its partner is still walking, and takes it back once one of them does. A MOVEMENT play, walked by `hunt::walk_to_station` (a `Point`
directive) from the kiter's ENGAGE, the Warlock's dispatch and the healers'
`hunt_station_tick`; Burst still leads the chain, and the play stands down while
the healer is locked so Chain extends the lock. It sits before Pillar: a team
hunting the last enemy does not hide from its crowd control. Traced as
`situation_change` `hunt` (subject: the healer; `pillar`) and the
`MovementTrigger::Hunt` walk. Measured (`docs/design/2026-10-25-ranged-dps-vs-hidden-targets.md`
§9): endgames about 19% shorter where a team can staff both stations (about
21% of endgames), about 2.5s over all endgames; wins and draws unchanged. Endgame lengths per match:
`tests/hunt_endgame_sweep.rs` (ignored, a measurement).

**Psychic Scream under `TeamPlan` (AS-388).** The Priest has no offensive
scream dip: `evaluate_priest_posture` offers it only under `Legacy`, which keeps
the dip, its budget and its dying-teammate abort (AS-371) unchanged. Its
offensive Scream is the Peel step alone, on an attacker of the teammate in
danger already inside the Scream; the defensive Scream is unchanged. Measured
against `Legacy` over seeds 501-512, Priest cells on Nagrand and TwinPillars:
a walk to Scream an attacker beyond range cost the Priest against no walk in
every form tried, so there is none
(`docs/design/balance/2026-10-24-as388-scream-dip-retired.md`). Scenes:
`tests/priest_dip_retired.rs`.

`Pillar` (AS-387; still gated): the team takes a pillar TOGETHER (`pillar_stance`,
`class_ai::pillar_stance`; design
`docs/design/2026-10-24-team-sight-cohesion.md` §3.3). The stance enters for
`AvoidCc` when a crowd control a pillar defends against (`pillar_mode::team_threats`:
sight-needing, from beyond 8yd, not a pet's) is ready for a teammate, the
fight (the healer's teammates' centroid or its anchor) stands within
`HUG_ENTER` of a pillar's skin, the ring has a TEAM arc (`pillar_mode::team_arc`:
ground seeing every teammate in heal range, out of those discs, holding the
margin) and no kill is on; Schmitt pair, release and dwell as AS-373's, the arc
and the kill end it at once. Under it the healer runs AS-373's hug on the team
arc — and ONLY under it (design D3: no healer pillars alone; the AS-373 mode
key's preference keys are the stance) — and a kiter (Mage, Hunter) gains the
`stance_pull` scorer term toward the stance's side. A melee is not called (its
half is AS-386's sight yield, rule 3, which reads the situation), so it keeps
Chain and Burst; a unit that runs Pillar does not run Chain while the stance
holds. `Stall` (D4) is stubbed: `STALL_COMPS` ships empty for AS-390. Traced
as `situation_change` `pillar_stance` (subject: the healer; `pillar`,
`reason`) and in every decision's `situation.pillar_stance`.

`Chain`: while the team's hold on an enemy (`lock_ending`: the soonest-ending
stun, fear, polymorph, incapacitate or cyclone on a visible enemy) ends within
`CHAIN_HORIZON_SECS`, every unit with a hand-off (`chain::chain_tools` —
Hammer of Justice, Kidney Shot, Fear, Death Coil, Polymorph, Cyclone) puts
its chain step at the head of its chain: the hand-off goes when it would
LAND in the lock's last `HANDOFF_LEAD` (cast time and flight, as the Burst
fit counts them), and the unit holds its global cooldown when that moment is
within one. Under the play the hold rule judges a hand-off where it lands
(`cast_guard::hold_lands_after`). A hand-off never deals damage onto a lock
that breaks on damage, never puts a breakable or shielding CC on an enemy
the team is hitting, and a healer does not chain while a teammate is dying.
It is scored on lockout, not win rate alone: `tests/cc_lifecycle_probe.rs`
measures denied, healer-locked and counterplay-free seconds and the longest
lockout per arm. Findings and the graduate criterion:
`docs/design/balance/2026-10-12-as276-chain-play.md`.

`Burst`: while `kill_window` names an enemy (low, below `kill_hp_threshold`,
and beyond every enemy healer's help — locked, out of mana, or out of reach),
every DPS puts its burst step at the head of its chain — its
`burst::burst_tools` on that enemy — and the cast gate refuses any cast on it
that would land after the window's `until` (`cast_guard::fits_window`, traced
`OutlastsWindow`). Under `Legacy` (or for a class the play's exceptions name)
the old lock-window reorder runs instead (`CombatContext::lock_window`):
graduating Burst kept it, because deleting it moves `Legacy`. Findings and the graduate criterion:
`docs/design/balance/2026-10-12-as275-burst-play.md`.

**The migrate-not-re-pin rule (user decision, 2026-10-12).** When a
seed-pinned probe goes stale — a change moved the RNG stream and the seed no
longer contains its situation — it is **migrated**, never re-pinned to a fresh
seed: to a scene test when one tick decides it, to a staged match when it
needs time (a walk, a chase, an escape), or to a situation-conditional probe
that finds its windows over a seed bank and asserts a response rate. A fix
that re-pins a stale probe's seed is not accepted, whoever writes it.

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

### Tune a map's dimensions (annotated top-down snapshot)

Map geometry is RON data (`assets/config/maps.ron`), so tuning it needs no
rebuild of game logic — only a re-render. `tests/arena_layout_snapshot.rs` draws
the **loaded** geometry from above with every dimension **measured off it** (pillar
spacings from actual volume centres, wall clearance by marching outward until the
point leaves the bounds), so a RON edit that misses its intent shows up in the
labels rather than being restated back at you.

```bash
# Render every map; writes tests/snapshots/arena_layout_<map>.new.png on any diff
cargo test --release --test arena_layout_snapshot -- --ignored
# ...read that PNG, edit assets/config/maps.ron, repeat (sub-second loop)

# Bless the baselines once the layout is right:
UPDATE_SNAPSHOTS=1 cargo test --release --test arena_layout_snapshot -- --ignored
```

`nagrand_dimensions_are_as_specified` in the same file pins the intended numbers
as plain assertions (no GPU), so a geometry regression fails the default
`cargo test` even without rendering.

Same offscreen-egui pattern as the Results/Configure screens — see the egui
snapshot loop above.

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

### Iterate on an egui screen fast (offscreen snapshot loop)

Tuning an egui screen by launching the client and driving it to the right
state is a ~90s human-in-the-loop cycle. For the **Results screen** there is a
fast loop instead: `draw_results_screen()` in `src/states/results_ui.rs` is a
pure egui function (no Bevy ECS — takes plain `&MatchResults` / `&CombatLog` /
`&ClassIcons`), so `tests/results_screen_snapshot.rs` renders it offscreen via
`egui_kittest` (wgpu, no window, mock 2v2 data) in a fraction of a second.

```bash
# Render the screen → writes tests/snapshots/results_screen.new.png
# (the test "fails" on any pixel diff; that's how it hands you the new image)
cargo test --release --test results_screen_snapshot -- --ignored

# ...open/Read that PNG, edit results_ui.rs, repeat. Once it looks right,
# bless the baseline so the test goes green and guards against regressions:
UPDATE_SNAPSHOTS=1 cargo test --release --test results_screen_snapshot -- --ignored
```

**Blessing is part of the change, not a follow-up.** Every one of these
harnesses is `#[ignore]`d, and CI runs only the default `cargo test` (see the CI
section at the end of this file — the snapshot suites are deliberately excluded,
because a runner's GPU adapter is a different rasterizer), so nothing anywhere
catches a stale baseline. A commit that touches a harnessed
`draw_*` function — `draw_results_screen`, `draw_sandbox_ui`, any other — **or a
harness's own MOCK DATA** must re-render and bless in the SAME commit. Read each
`.new.png` before blessing and confirm every visible difference is one you
intended: a rubber-stamped bless converts a loud failure into a silent wrong
baseline. The animation sandbox's mock drifted that way across four commits and
about a month, ending up pinning a dummy-off state the shipped panel cannot
produce (it showed offensive casts enabled where the real panel greys them).

The test is `#[ignore]`d so the default `cargo test` skips it (it needs a GPU
adapter; CI runners may lack one). `egui_kittest` is a dev-dependency pinned to
the same egui version as `bevy_egui` (0.31). Fidelity caveat: kittest has no
Bevy textures, so class icons render as class-color fallback squares —
layout/spacing/color iterate faithfully; pixel-exact icon fidelity still needs
the real client. Fonts are NOT a gap: every harness installs the client's own
look by calling `arenasim::ui::install(ctx)` at the top of its app closure (the
one call the client's Startup system also makes), so glyph coverage and text
metrics match the game. A new harness must call it too, or its baselines
picture a screen nobody plays — `tests/snapshot_font_audit.rs` fails the
default `cargo test` if one forgets.
**To extend this pattern to another screen**, refactor its UI system the same
way: split the Bevy wrapper (grabs `EguiContexts` + resources, applies
actions) from a pure `draw_*(ctx, &data...) -> Action` function, then drive
that function from a kittest harness with mock data.
`tests/main_menu_snapshot.rs` and `tests/encyclopedia_snapshot.rs` follow it;
the encyclopedia's harness loads the real `items.ron` and `abilities.ron`
rather than mock data, so a content change shows up in the snapshot.

**UI values come from `ui::theme`.** A colour, text size, gap, radius or icon
size is chosen once, in `src/ui/theme.rs`, and named by its role (`TEXT_MUTED`,
never `GREY_142`); a literal written anywhere else in `src/` fails
`tests/ui_token_audit.rs`, which allows none outside the theme. Use the token
for that role, and if the theme lacks one, add it to `theme.rs` by role in the
same commit. **UI pieces come from `ui::widgets`** (button, header and
`Nav`, panel, list row, tabs, chip, tooltip, modal, …); a component the kit
lacks is added there, with its row in `tests/ui_gallery_snapshot.rs`, in the
same commit. **The style is installed once:** `ui::install` sets the whole egui
`Style` (`theme::style()`) for the client and every harness; a screen never
calls `set_style`/`style_mut` on the context (egui's one sticky global leaked
each screen's patch into the next), only `ui.scope(|ui| ui.style_mut()…)`. **A
new screen does not compile until `GameState::chrome()` and `harness()`
answer for it** (`src/states/chrome.rs`, exhaustive): its template, where its
exit goes, and the snapshot harness that pins it or why it has none. Exits are
contextual — a screen returns to whatever opened it, and its exit label names
that screen. A wrapper moves forward only with `ScreenNav::open` (which records
the opener) and out with `ScreenNav::leave`, and takes Esc from
`ScreenNav::escape`, so the key and the visible exit cannot disagree. The design
is `docs/design/proposals/2026-10-11-ui-design-system.md`.

### Verify the RUNNING client with a UI script (input injection)

The egui snapshot loop above iterates a screen's LOOK offscreen. It cannot tell
you whether the real client responds to a real click — and `egui_kittest`
proving a widget's contract is not the same claim (AS-65 round 2 approved a
`secondary_clicked()` assertion that the running client did not honour).
`screencapture` / `osascript` are permission-blocked here, so the answer is to
drive the client itself:

```bash
cargo run --release -- --ui-script tests/ui-scripts/view-combatant-tooltips.script
# one line per step on stdout and in match_logs/ui_script_<ts>.log;
# exit 1 on the first failed assertion, so a script doubles as a smoke test
```

Input is injected at BEVY's input events (`CursorMoved` / `MouseButtonInput` /
`KeyboardInput` / `MouseWheel`) in `PreUpdate` before `InputSystem`, so
`ButtonInput<KeyCode>` and bevy_egui's own conversion both run exactly as they
do for a physical device. Steps: `hover`, `click <id> [left|right]`, `key`,
`wait`, `assert-state`, `assert-view`, `assert-note`, `assert-no-note`,
`await-note <secs> <substring>` (polls on the wall clock for something the
match produces on its own, e.g. a banter line), `assert-visible` (drawn AND on screen), `assert-absent` (not drawn at all —
a widget that is merely scrolled out of view FAILS both, distinctly, so a
negative can never pass for the wrong reason), `assert-enabled`, `dump`.

A screen opts a widget in with ONE call —
`ui_driver::mark(ui, rect, enabled, format_args!("kit:{ability:?}"))` — plus
`ui_driver::note(...)` where only the draw can see what happened (which tooltip
body ran, what colour a row rendered). Keep the opt-in explicit; do not
instrument every widget. **Inert without the flag**: `UiDriverPlugin::build`
registers nothing, so `mark`/`note` are one failed hash lookup each, proved by
mutation in `tests/ui_driver.rs`.

**The limit to state out loud: this proves a HANDLER runs, never that the
user's input reaches it.** macOS Ctrl+click arrives at winit as Left+ctrl, so a
right-click affordance can pass here and still look broken to someone using
it. Ask what device a reporter used.

**Never write a driver wait as a frame count unless the thing you are waiting
for is measured in frames.** egui gates tooltips on wall-clock — chiefly
`is_still()`, whose position history has a 0.1-SECOND window — so a 30-frame
hover settle was correct only on a slow enough machine: it passed for two
agents under load and failed for the user on an idle M5 Max. Worse, the
required frame count GROWS with the frame rate, because a faster machine fits
more samples into that window. `hover` waits on `tooltip_gate`, which asks
egui directly. A frame count and a seconds constant are the same bug in
different units.

Full loop, both timing traps, and the shipped scripts:
`docs/solutions/workflows/client-input-injection-driver.md`.

### Browsing game content in-game (the Encyclopedia)

`GameState::Encyclopedia` (`src/states/encyclopedia/`) is the player-facing
reference screen — reached from the main menu, and the screen that subsumed the
old standalone Armory. It is a navigation FRAMEWORK plus per-section content:

- **`Topic`** (`topic.rs`) addresses every entity — `Class | Ability | Aura |
  Item`. Navigation state is a stack of `View`s (section tab + optional topic).
- **The linked-icon widget** (`widget.rs`) is the invariant to preserve: every
  icon the screen draws goes through it, so hovering always shows that entity's
  tooltip and clicking always navigates to its page. Tooltip text must delegate
  to the entity's existing builder — never write a second copy here.
- **Search** (`search.rs`) is a registry each section populates from its own
  data source. Adding a section means one call in `build_registry`.
- **Zero marginal cost:** every list, page and search hit derives from the RON
  configs / Rust registries. A new item in `items.ron`, or a new ability in
  `abilities.ron`, appears in its index, the filters, search, its owning class's
  kit and its own detail page with no code change.
- **Sections:** `classes.rs` (base stats from `class_base_stats`, kit from
  `AbilityDefinitions::abilities_for_class`, a subsection per pet), `abilities.rs`
  (all 70, filterable by class and school, grouped by owning class in the derived
  kit order — do NOT add a second sort), `items.rs`, and `auras.rs` (Buffs &
  Debuffs: every aura an ability applies, plus the engine-originated ones —
  totem buffs, interrupt lockouts, one entry per proc trinket, and the rest —
  each with a removal badge answered by the engine's own dispel predicates).
- **Ability and aura PROSE is `src/states/ability_text.rs`** — one generator, shared
  with View Combatant, so an ability's description reads the same wherever the
  player meets it. Totem text wins over a hand-written `abilities.ron`
  `description`, which wins over the generated text.
- **`tests/encyclopedia_boot.rs`** enters the state in a real (headless) Bevy app.
  The snapshot tests drive the PURE draw and so cannot catch a `Res<T>` nothing
  inserts; this does. Adding a field to `EncyclopediaData` means a new resource in
  the Bevy wrapper — run this.
- **Back and Exit are separate, always-visible affordances.** Back pops one
  stack level (disabled at the root); Exit leaves the screen outright, from any
  depth, in one click. `Esc` walks the Back ladder — clear an active search,
  then pop, and only exit from the root — so no depth traps the reader.
- **The exit returns to the CALLING CONTEXT.** The encyclopedia is an
  informational context you enter *from* somewhere, so leaving returns you
  there — the rule every screen follows (`ScreenContext`, `src/states/chrome.rs`).
  A new entry point opens it with `ScreenNav::open(GameState::Encyclopedia)`,
  which records where it came from (a deep link calls `state.open_at(topic)`
  first), and needs to set nothing else. The exit button names the destination, and every
  exit path funnels through `leave()`, which clears the search and resets the
  stack so a re-entry is always fresh.
- `draw_encyclopedia` is pure egui; `encyclopedia_ui` is the Bevy wrapper that
  applies the returned `EncyclopediaAction`. Snapshot loop as above.

### Adding a New Combat System

`tests/registration_audit.rs` enforces that every Bevy system function (`pub fn` taking SystemParam types) under `src/states/play_match/` is registered in one of three places. When adding a new system, pick the correct registration path:

- **`add_core_combat_systems` in `src/states/play_match/systems.rs`** — for systems that must run in BOTH headless and graphical modes (combat logic, auras, AI, projectiles, damage application). Add the system to the appropriate phase tuple (Phase 1 `ResourcesAndAuras`, Phase 2 `CombatAndMovement`, or Phase 3 `CombatResolution`) and add the matching `pub use` re-export at the top of `systems.rs`. This path is the home for ~30 systems today and is the answer for almost every gameplay-affecting system.

- **`StatesPlugin::build()` in `src/states/mod.rs`** — for systems that run in graphical mode only (visual effects, HUD rendering, camera, animations, UI for non-PlayMatch states). Add to one of the existing `.add_systems()` blocks or create a new one with the appropriate `.run_if(in_state(...))` gate. Visual-effect systems traditionally use `.after(CombatSystemPhase::CombatResolution)`.

- **`ALLOWLIST` in `tests/registration_audit.rs`** — only for `pub fn` items that take a SystemParam type by value (e.g. `Commands` directly, not `&mut Commands`) but are called manually from a system body rather than registered as a system. Each entry must include a one-line justification. Most helpers in this codebase take references and don't need allowlist entries.

If you forget to register a new system, `cargo test` fails with the file path, line number, and the three registration paths to choose from. The audit is name-agnostic — it detects systems by signature, so renaming a registered function without updating its registration is also caught.

The historical bugs this prevents: `process_dispels`, `process_holy_shock_heals`, `process_holy_shock_damage`, and `process_divine_shield` were each registered in only one of the two paths and silently failed in the other mode. See `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md` for context.

### Reading a lazily-loaded resource from a new screen

The icon resources (`ClassIcons`, `SpellIcons`, `EmojiIcons`, `AbilityIcons`,
`ItemIcons`, `HunterPetIcons`) are filled LAZILY by a loader system that
self-guards on an internal `loaded` flag. The rule, enforced by
`tests/icon_loader_registration_audit.rs`:

> **A state that reads a lazily-loaded resource must register that resource's
> loader in its own chain.** Never rely on an earlier state having filled it.

`--replay` boots straight into `PlayMatch` (`build_graphical_app(Some(cfg))`
ends `.insert_state(GameState::PlayMatch)`), so it never passes through
ConfigureMatch and never runs `load_class_icons` there. Anything that assumed
"ConfigureMatch ran first" drew blank icons for the whole replay. The loaders
are idempotent, so the fix is always one line in that state's chain in
`src/states/mod.rs`. A consumer that provably never paints with the resource
goes in the audit's `ALLOWLIST` with a justification.

**A registered loader still paints nothing if Bevy looks in the wrong
directory.** Every asset read — the RON configs and Bevy's `AssetServer` —
resolves one root, `arenasim::paths::assets_dir()`, the first `assets/` holding
`config/abilities.ron` in this order: beside the executable (a packaged
build); in the checkout the binary was COMPILED in (compile-time
`CARGO_MANIFEST_DIR`, so a dev binary reads its own build's assets wherever it
runs from — an external `CARGO_TARGET_DIR`, a sweep arm copied out of
`target/`); the nearest ancestor of the executable that has one. None found is
fatal, listing every path tried. Nothing about the run steers it: not the
working directory, and never the RUNTIME `CARGO_MANIFEST_DIR`, which an
inherited shell variable can point at another checkout. The client builds
`DefaultPlugins` with `paths::asset_plugin()`; Bevy's default resolves beside
the EXECUTABLE whenever cargo is not the launcher, so `cargo run` cannot
reproduce a blank-icon report and `target/release/arenasim` (what the review
page's replay button runs) can. `tests/asset_root.rs` holds both halves.

### Continuous integration

**The Rust version is pinned in `rust-toolchain.toml`, and that file is the only
place it is chosen.** rustup reads it for every cargo invocation — yours and
CI's — and installs the toolchain on demand, so a fresh checkout needs no
`rustup update` to agree with the workflows, and all three of them
(`ci.yaml`, `lint.yaml`, `release.yaml`) provision from it with a bare
`rustup toolchain install`. Do not name a version in a workflow as well; two
copies can drift, and drift is exactly what the pin exists to stop. Before this
existed CI floated `stable` to 1.98.1 while everyone ran 1.96.0, and the Lint
job failed on all 90 of the sites those two releases disagreed about — on every
run from the day it was added. **When you raise the pin, expect newly-widened
lints, and read the first red Lint run as a question about the toolchain before
reading it as a question about the diff.**

`.github/workflows/ci.yaml` runs `cargo build --release --locked` and
`cargo test --locked` on `macos-latest`, on every push to `main` except
docs-only pushes (`**.md`, `docs/**`, `.claude/**`, `LICENSE` are
`paths-ignore`d), and on demand on any branch via `workflow_dispatch`
(`gh workflow run ci.yaml --ref <branch>`). It does **not** run on pull
requests: a run is 16-18 minutes cached and 39 cold, and many PRs are
docs-only. Nothing blocks a merge — a red run on `main` is a signal to go and
look, not a gate.

**Covers:** the whole default `cargo test` set — every test not `#[ignore]`d,
in every target — plus a release build of the shipping binary. (No count is
quoted here: one goes stale with every suite added, and a stale count reads
the same as a suite that stopped running.) So `registration_audit`, `aura_catalog_audit`,
`loadout_order_audit`, the exhaustiveness tests, the movement probes,
`determinism_pin` and the db2 fixture suite hold on every code push to `main`
instead of only when someone remembers to run them.

**Deliberately not covered** (each reasoned out in the workflow's own comments):

- the `#[ignore]`d egui/wgpu **snapshot suites** — a runner's GPU is a different
  rasterizer; blessing stays part of the commit that changes a `draw_*` function
  or its mock data (see the egui snapshot loop above);
- the other `#[ignore]`d tests — the 98-match determinism sweeps, the
  obstacle-active companion, the exploratory `scan_*` seed scanners in
  `movement_probes.rs`, `camp_sweep` — minutes of simulation apiece;
  `determinism_pin` is the always-on sentinel that stands in for them;
- the **balance sweeps** — measurements a human reads, not assertions.

Three choices in the file are load-bearing and explained in place; do not
"simplify" them away: `macos-latest` (the machine class every baseline and the
`f32::to_bits` pin were recorded on), tests in the DEV profile (the only profile
in which `src/`'s 16 `debug_assert!` invariants exist), and `python3` installed
explicitly (the db2 fixture suite panics without it, on purpose).
