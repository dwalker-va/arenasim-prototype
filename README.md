# ArenaSim

A WoW Classic/TBC-inspired arena combat autobattler built with Bevy (Rust). Build teams
of 1–3 combatants, pick their gear and per-class tactics, then watch the AI fight it out
with a full spell/aura/crowd-control combat model — or skip the graphics entirely and run
thousands of matches in parallel to measure balance.

Matches are CPU vs CPU. The player's leverage is everything *before* the gates open:
composition, gear, kill/CC targets, and per-class strategy choices.

## Download and play

Grab a build from the [latest release](https://github.com/dwalker-va/arenasim-prototype/releases/latest)
— `.dmg` for Apple Silicon Macs, `.zip` for 64-bit Windows. Nothing to install
and no toolchain required.

The builds are unsigned, so your OS warns you the first time you open one. The
release notes give the exact click path for each platform; it is a one-time
step.

## Build it yourself

**Prerequisites:** [Rust](https://rustup.rs/) (stable toolchain)

```bash
# Graphical client (first build takes several minutes to compile Bevy)
cargo run --release

# Single headless match
echo '{"team1":["Warrior","Priest"],"team2":["Mage","Paladin"]}' > /tmp/match.json
cargo run --release -- --headless /tmp/match.json
# → match_logs/match_<timestamp>.txt

# Full 8×8 class matchup matrix, 100 runs per cell
cargo run --release -- --matrix 100
# → match_logs/matrix_<timestamp>.{csv,md}

# Tests (unit + integration + audits)
cargo test
```

## Combat Model

- **8 classes** — Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter, Shaman
- **~70 abilities** defined in data (`assets/config/abilities.ron`), no recompile to retune
- **Resources** — Mana, Rage (generated from damage dealt and taken), Energy
- **Crowd control** with **diminishing returns** — per-category DR ladder to immunity,
  with a reset timer, so stun/fear/root chains behave like the real thing
- **Auras** — buffs, debuffs, DoTs, absorb shields, movement impairment, spell lockout,
  break-on-damage thresholds
- **Spell schools & resistances** — Physical, Fire, Frost, Shadow, Arcane, Holy, Nature
- **Cast times, channels, interrupts, dispels, and purges**
- **Line of sight** — obstacle-aware casting, healing, and auto-attacks, including
  verticality (raised platforms and ramps)
- **Arena dampening** — starting 75s after the gates, all healing, absorbs, and lifesteal
  ramp linearly to zero over 120s so attrition endgames resolve instead of drawing
- **Shadow Sight orbs** — spawn 90s in to break stealth stalemates
- **Pets** (Hunter: Spider / Boar / Bird), **totems** (Shaman), and **ground traps**
  (Hunter: Freezing / Frost)
- **Deterministic replay** — seeded RNG reproduces a match byte-for-byte

### Per-class tactics

Each class exposes strategy choices that are configurable per combatant (in the client and
in headless JSON):

| Class | Choice |
|---|---|
| Rogue | Opener (Ambush / Cheap Shot), weapon poison |
| Warlock | Per-target curse (Agony / Weakness / Tongues) |
| Hunter | Pet type (Spider / Boar / Bird) |
| Warrior | Shout (Battle / Demoralizing / Commanding) |
| Mage | Armor (Frost / Mage / Molten) |
| Paladin | Aura (Devotion / Shadow Resistance / Concentration) |

Plus per-team **kill target** and **CC target** priorities.

## AI

Combatants are driven by per-class AI (`src/states/play_match/class_ai/`) with two layers:

- **Ability selection** — a priority rotation per class, with shared guards for
  friendly-CC breaks, line of sight, range, resources, and cooldowns.
- **Movement** — a posture state machine. Healers run FREE / PRESSURED / ESCAPE / DIP
  (formation-keeping, threat repulsion, pillar-based LoS denial, and heal-seeking chases);
  ranged DPS run ENGAGE / KITE (range-band orbiting, LoS seeking, occlusion-chase). Steering
  is context-based (16 compass candidates scored with weighted terms and boolean masks) with
  tangent steering to round obstacles cleanly. All weights and radii live in
  `assets/config/movement.ron` — retuning behavior needs no code change.

### Decision traces

Every AI decision can be dumped as JSONL alongside the match log — each considered ability
with a typed rejection reason, every target switch, every posture transition with its scorer
term breakdown.

```bash
cargo run --release -- --headless /tmp/match.json --trace-mode on
# → match_logs/match_<timestamp>_trace.jsonl

# "Why didn't the Hunter cast Aimed Shot?"
jq -c 'select(.actor.class == "Hunter") | .candidates[]
       | select(.ability == "AimedShot" and .status == "rejected") | .reason' \
   match_logs/match_*_trace.jsonl | sort | uniq -c
```

See [CLAUDE.md](CLAUDE.md) for the full recipe collection and
[docs/solutions/implementation-patterns/ai-decision-trace.md](docs/solutions/implementation-patterns/ai-decision-trace.md)
for the schema.

## Gear

- **~136 items** in `assets/config/items.ron` — plate/mail/leather/cloth armor, weapons,
  and accessories, with item-level stat budgets enforced by `cargo test`
- **Default loadouts** per class in `loadouts.ron`, overridable per combatant
- **Armory screen** to browse every item with filters (slot, armor type, item level, name)
- **Stat scaling** — attack power / spell power coefficients per ability
  (see [design-docs/stat-scaling-system.md](design-docs/stat-scaling-system.md))

## Client

Screens: Main Menu → Configure Match → (View Combatant) → Play Match → Results, plus
Options, Keybindings, and Armory.

- **Match view** — health/resource bars, aura icons, floating combat text, ability
  timeline, live combat log, projectile and spell effects
- **Time controls** — pause plus 0.5× / 1× / 2× / 3× speed
- **Camera** — follow-center, follow-combatant, and manual modes
- **Results** — WoW-Details-style damage/healing breakdown per combatant and ability,
  with pet damage folded into the owner's rows
- **Fully remappable keybindings**, persisted with video settings in `settings.ron`

## Headless & Balance Tooling

| Mode | Flag | Use |
|---|---|---|
| Single match | `--headless <config.json>` | Verify one change end to end |
| Matchup matrix | `--matrix N` | All 8×8 class pairings, N runs each → winrate CSV + Markdown heatmap |
| Parallel batch | `--batch <configs.jsonl> --out <csv>` | The fast path: thousands of arbitrary 2v2/3v3/strategy-variant matches across all cores |

Useful extras: `--seed-base` (reproducible matrices), `--matrix-map PillaredArena` (run every
cell with LoS obstacles), `--save-logs`, `--jobs`, `--max-duration`, `--trace-mode`.

Helper scripts in `scripts/`:

```bash
scripts/hunter_2v2_matrix.sh 100          # 2v2-with-healer sweep (also mage_/shaman_ variants)
scripts/movement_kpis.sh <traces...>      # reduce traces to movement KPIs (path length, proximity)
scripts/gen_sweep.py / agg_sweep.py       # generate + aggregate batch sweeps
scripts/comp_tiers.py                     # comp tier list from sweep output
```

Historical balance baselines are committed under `design-docs/balance/`.

## Testing

`cargo test` runs ~275 integration tests and ~263 unit tests, including several structural
guards worth knowing about:

- **`registration_audit`** — fails if a new Bevy system isn't registered in *both* the
  headless and graphical paths (a historically silent class of bug)
- **`movement_probes`** — dense per-tick behavioral probes over real headless matches, with a
  self-test proving observation doesn't perturb the simulation
- **`decision_trace_audit`** — keeps trace instrumentation in sync with AI predicates
- **Item budget validation** — no item may exceed its item-level stat budget
- **egui snapshot tests** (`--ignored`, needs a GPU adapter) — offscreen renders of the
  Results / Configure Match / Main Menu / team-frame UI for fast visual iteration

## Project Layout

```
src/
  main.rs, cli.rs, settings.rs, keybindings.rs
  combat/log.rs              # combat log + match report
  headless/                  # runner, matrix, parallel batch, JSON config
  states/
    main_menu.rs, configure_match_ui.rs, view_combatant_ui.rs,
    results_ui.rs, armory_ui.rs, match_config.rs
    play_match/
      abilities.rs, ability_config.rs      # data-driven ability defs
      class_ai/                            # per-class AI + posture machines
      combat_core/                         # damage, casting, auto-attack, movement, death
      components/                          # ECS components (combatant, auras, pets, totems…)
      decision_trace/                      # JSONL AI trace
      effects/                             # dispels, holy shock, mana burn, divine shield…
      map_geometry.rs, map_config.rs       # LoS, obstacles, tangent steering
      rendering/                           # graphical-only: HUD, effects, combat log
      systems.rs                           # shared headless+graphical system registration
assets/config/                             # abilities, items, loadouts, characters, maps, movement (RON)
```

## Documentation

- **[CLAUDE.md](CLAUDE.md)** — the working developer guide: how to add an ability, item, or
  system; tuning surfaces; trace recipes; dev loops
- **[design-docs/game-design-doc.md](design-docs/game-design-doc.md)** — long-term game vision
- **[design-docs/wow-mechanics.md](design-docs/wow-mechanics.md)** — implemented WoW mechanics
- **[design-docs/stat-scaling-system.md](design-docs/stat-scaling-system.md)** — damage/healing formulas
- **[design-docs/bevy-patterns.md](design-docs/bevy-patterns.md)** — Bevy/Rust patterns and pitfalls
- **[design-docs/roadmap.md](design-docs/roadmap.md)** — TODOs and milestones
- **[design-docs/session-notes.md](design-docs/session-notes.md)** — development history
- **[docs/solutions/](docs/solutions/)** — documented solutions to past bugs and implementation patterns
- **[docs/known-issues.md](docs/known-issues.md)** — known issues

## Tech Stack

- **[Bevy 0.16](https://bevyengine.org/)** — ECS game engine
- **[bevy_egui 0.34](https://github.com/mvlabat/bevy_egui)** — immediate-mode UI
- **RON** for game data, **serde_json** for headless configs
- **rand 0.9** (pinned — later versions break byte-identical seeded replay)
- **Rust** (stable, edition 2021)

Input today is keyboard/mouse; gamepad support (for the Steam Deck target in the design doc)
is not yet implemented.

## License

The code is MIT-licensed — see [LICENSE](LICENSE).

The ability, class, and item icons under `assets/icons/` are World of Warcraft
artwork sourced from Wowhead. They are **not** covered by that grant and remain
the property of their respective owners; they are here for a non-commercial
hobby project. Fork or redistribute and that artwork is yours to clear or
replace. The Rajdhani typeface is under the SIL Open Font License, and the
application icon is original work covered by the MIT grant.

---

**Built with** [Bevy](https://bevyengine.org/) | **Inspired by** World of Warcraft Arena
