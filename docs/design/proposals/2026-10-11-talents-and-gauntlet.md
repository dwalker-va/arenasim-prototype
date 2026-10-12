# Talents and the Gauntlet: one design

**Card:** AS-255. **Status:** proposal, for the user to judge. Nothing here is implemented.
**Prototype:** [`docs/design/benches/2026-10-11-gauntlet-prototype.html`](../benches/2026-10-11-gauntlet-prototype.html)
clicks through whole runs:
- the difficulty and draft;
- the talent step and the two-tier gear reward after every match;
- Prepare against an opponent briefing that scales with your wins;
- a stand-in fight and the run end;
- the renown abilities a match or a run unlocks, and the encyclopedia's Renown section that
  lists them, locked and unlocked.

Open it from the checkout so the icons resolve. The fight is a scripted coin flip weighted by
budget and answers, not the sim. The Rogue and Paladin trees are not designed here, so the
prototype turns their points into item budget, as §7.3 does for opponents. The styling is
neutral, because the look belongs to the UI design system (AS-251). Judge the flow and the
decisions.

## Summary

A run of the **Gauntlet** is a two-class team fighting a visible ladder of opponents until it
reaches 7 wins or 3 losses. Between fights the player makes three kinds of decision:

| | Talents | Gear | Class choices |
|---|---|---|---|
| Kind | Character progression | Character progression | Per-match strategy |
| What | WoW talents in three small trees per class | One pick of three after each match; a rarer Champion tier after wins | The class's own options: curse, shout, armor, aura, pet, opener, poison (plus kill and CC target) |
| Earned | 2 at the draft, +1 after every match, win or loss, **capped at 8** | 1 per combatant after every match, **capped at 6** | Always available |
| Cost | Locked for the run | Equipped for the run | Free; change them every match |
| Question | *Who is this character becoming?* | *What do I add to it?* | *What beats the team in front of me?* |

- **The line.** A choice is a talent only if WoW put it in a talent tree. Otherwise it is a
  per-match choice if its best value depends on the opponent, and baseline behaviour if not. A
  talent may add an option to a per-match menu; it never picks one (§1).
- **Trees.** Three per class, five to seven nodes each, four tiers. Eight points buy either a
  capstone and a splash or tier 3 in two trees, so a hybrid is open by the end of a run. Every
  legal 8-point build costs within one rank of every other, and `cargo test` checks that by
  enumerating them all (§3, §4). The talent lists in §5 are mocks that fix the shape. Each class's real
  trees are a card of their own.
- **Gear.**
  - Two reward tiers: a Gauntlet tier re-levelled to ilvl 76-80, and a new Champion tier at
    ilvl 90-94 that is offered only after a win.
  - Exciting trinkets: on-use stat bursts and a PvP medallion that breaks crowd control.
  - Six picks per combatant, from the slots where a pick is felt.
  - Gear and talents each add about 90 budget points by the cap, together about +55% over the
    starting kit (§6).
- **Both caps are reached before match 7**, the earliest a perfect run can play its last
  match. Losing more never ends a run stronger.
- **Difficulty (recommended).** Opponents scale with the player's *wins*, matching in budget a
  player who has not lost; a loss puts the player ahead. Out of the run, six difficulty levels
  named for arena titles move opponent gear first, and points only at the top (§8).
- **Meta-progression: renown abilities, awarded when a condition is met.**
  - Nothing is spent and no extra screen is needed.
  - Flagship goals (a clear at each difficulty) grant the key upticks: more draft choices
    (3 up to 6) and more equipment choices (3 up to 5).
  - Specific objectives grant smaller bonuses such as equipment rerolls: clear with a Rogue,
    clear with no healer, a flawless 7-0, a match won in a 1v1. They are chosen to send the
    player somewhere new, and never reward a double-healer team.
  - Renown grants choice, never stats.
  - A data-driven framework evaluates per-match conditions from a digest of the match result
    and combat log, and per-run conditions from the run record. A new ability is one RON entry.
  - The encyclopedia shows each ability's effect, condition and state (§9).
- **Replays** are exact within a run and a game version. Every record is version-stamped. A
  record from another version keeps its result and combat log, and offers a labelled
  re-simulation, never a promised replay (§10).
- **Minimum playable slice:** the Gauntlet with talents and both gear tiers, drafting from the
  four classes that get trees first. Meta-progression and difficulty levels are phase 2 (§12).

---

## 1. Two kinds of choice

### What exists today

Seven typed per-class choices live in `src/states/match_config.rs`, stored as per-slot
vectors on `MatchConfig`, passed to `spawn_combatant` field by field, and read by the class
AI. The headless config takes them as string arrays. View Combatant draws a panel for each.

| Class | Choice | Options | Which side |
|---|---|---|---|
| Warlock | Curse, **per enemy slot** | Agony, Weakness, Tongues | Per-match |
| Hunter | Pet | Spider, Boar, Bird | Per-match |
| Rogue | Opener | Ambush, Cheap Shot | Per-match |
| Rogue | Poison | Crippling (one option) | Per-match |
| Warrior | Shout | Battle, Demoralizing, Commanding | Per-match |
| Mage | Armor | Frost, Mage, Molten | Per-match |
| Paladin | Aura | Devotion, Shadow Resistance, Concentration | Per-match |
| Every team | Kill target, CC target | One enemy each | Per-match (not a class choice) |
| Priest, Shaman, Druid | — | — | No per-match choice today |

Every one of these is a WoW baseline choice: a curse, a demon, a shout, an armor, an aura, a
pet, a poison and an opener were all trained, not talented, and all were swapped freely out of
combat. They stay **pre-match choices**, unchanged in the sim. Their job in the Gauntlet is to
answer the opponent briefing (§7.4).

Several shipped *abilities* were talents in Classic or TBC: Mortal Strike, Ice Barrier, Holy
Shock, Swiftmend, Unstable Affliction (TBC), and Corruption's instant cast (Improved
Corruption). They are baseline today, and the trees leave them baseline so no class loses a
button. Decision 3 asks whether that holds.

### The rule for placing a new choice

Ask three questions, in order.

1. **Did WoW put it in a talent tree?** Yes: it is a **talent**. It can be a node that
   changes the character, or a node that adds an option to a per-match menu. No: go on.
2. **Does its best value depend on the opponent or the map?** Yes: it is a **per-match
   choice**. No (one value is best against everyone): it is not a choice at all. Make it
   baseline, or let the AI decide it.
3. **Check the cost.** A talent is paid for and locked for the run. A per-match choice is
   free every match and must be something the briefing can argue for or against. A candidate
   that fails its side's test (a "talent" a player would want to re-pick every match, or a
   per-match option that is simply better) has been placed wrong.

Two corollaries keep the line from blurring:

- **A talent may widen a per-match menu; it never orders from it.** Curse of Exhaustion
  (Affliction) adds a fourth curse. Demonic Sacrifice (Demonology) adds "sacrifice the
  Felhunter at the gates" to the Warlock's demon choice. Sanctity Aura (Classic Retribution)
  would add a fourth Paladin aura. Which option to take against this opponent stays a
  per-match decision.
- **A per-match option never ranks up over a run.** There is no "rank 2 Bird". Making one
  answer better is a talent's job (Improved Curse of Agony), and that talent pays only in the
  matches where the player chooses that answer. Progression can *bias* the counter-pick, which
  is a real build identity, but it never takes it away.

**Worked placements.**

| Candidate | Q1 | Q2 | Side |
|---|---|---|---|
| Curse of Exhaustion | Affliction talent | — | Talent that widens the curse menu |
| A second Warlock demon (Succubus) | Baseline in WoW | Yes: Seduction vs Spell Lock depends on the enemy | Per-match (the demon choice) |
| Improved Curse of Agony | Affliction talent | — | Talent; pays when Agony is chosen |
| Shaman Tremor vs Grounding totem | Baseline | Yes: fear teams vs caster teams | Per-match: the Shaman's missing choice |
| Priest Shadow Protection | Baseline | Yes: only against Shadow casters | Per-match: a candidate Priest choice |
| Shadowform | Shadow capstone in WoW | — | Talent, but a role change (no healing), so it is held out of the trees (§5.2) |
| Inner Fire | Baseline | No: always worth casting | Neither: baseline AI behaviour |

The Priest, Shaman and Druid having no per-match choice is a gap the rule exposes, not one for
talents to fill.

## 2. Reference games: what to borrow, what to avoid

**Autobattlers.**
- *Teamfight Tactics:* borrow scouting (look at the opponent's board and adjust) and
  round-by-round escalation. Avoid the 8-player lobby and the shop economy.
- *Super Auto Pets:* borrow lives and a win target (you are never out after one bad fight) and
  asynchronous opponents (past teams as snapshots). Avoid opaque single-item rolls; every offer
  here is a choice of three.
- *Hearthstone Arena:* borrow the run shape: draft, play to a win target or 3 losses, better
  rewards for more wins. **The closest single template for the run.**
- *Teamfight Manager, Mechabellum:* borrow the draft against a visible opponent and the
  commit-watch-adapt rhythm. Avoid long training grinds between matches.

**Progression and difficulty.**
- *WoW Classic trees:* borrow tiers gated by points spent, arrows (prerequisites), multi-rank
  nodes, a capstone that defines the spec, and the "31/20/0" shorthand a viewer can read.
  Avoid 51 points of +1%. A tree is only worth watching if its nodes act in the fight.
- *Diablo II skill tree:* borrow points arriving on a fixed cadence and planning a path toward
  a payoff skill. Avoid permanent mistakes; here a bad build costs one run.
- *Path of Exile passive tree:* avoid. 1,300 nodes is a game of its own, and no AI or sweep
  can cover it.
- *Slay the Spire:* borrow a build that lives for one run, and **Ascension**: difficulty
  levels unlocked one at a time by clearing the one below. That is the model for §8.
- *Hades:* borrow the **Fated List of Minor Prophecies**: objectives that pay out on their own
  when met, which is what renown is (§9). Borrow **Heat** as opt-in difficulty that pays more.
  Avoid the **Mirror**'s permanent stat power, which makes the first runs weak by design.
- *WoW arena:* borrow the arena titles (Combatant, Challenger, Rival, Duelist, Gladiator), the
  2v2/3v3 brackets and the fantasy of a comp learned deeply. Avoid gear gaps that decide
  matches, and rating loss as the only stake.

**Management and tactics.**
- *Football Manager:* borrow the scouting report ("their left back is weak") and watching the
  match to see whether the tactics worked. Avoid spreadsheet depth.
- *Into the Breach:* borrow **telegraphed intent**. Naming what the enemy will do makes the
  answer the whole skill. Here it is the opponent briefing.

## 3. The talent system

### Tree shape

Each class has **three trees**, named as in WoW, each with **five to seven nodes in four
tiers**.

| Tier | Opens at (points already in this tree) | Nodes | Ranks per node | Price per rank |
|---|---|---|---|---|
| 1 | 0 | 2 | 2 | 11 |
| 2 | 2 | 1-3 | 1-2 | 11 |
| 3 | 3 | 1-2 | 1 | 11 |
| 4 (capstone) | 5 | 1 | 1 | 22 |

- **Prerequisites** are WoW's arrows. A capstone usually needs one tier-3 node. Classic's own
  arrows are used where they exist (Shadow Mastery needs Siphon Life; Conflagrate needs
  Improved Immolate), and the rest are marked *here*.
- **The capstone is the only node worth two ranks of budget.** It is the spec's identity: a new
  button, a mechanic or the tree's signature number. It is the sixth point in a tree.
- **A run gives 2 points at the draft and 8 at most.** The two starting points buy a coherent
  start: two ranks of one tier-1 node, or one rank each of a tree's tier-1 pair. At the cap,
  8 points buy:
  - **a deep spec:** a capstone and two points elsewhere ("6/2/0");
  - **a hybrid:** tier 3 in two trees ("4/4/0"), the third tree's tier 1, or any mix
    between.

  A class has about 18 nodes and 27 ranks, so a run spends under a third of it. The class
  has runs left to discover.

### Data model

One RON file, five effect kinds priced on the item budget, and a per-combatant resolved
ability book.

```ron
// assets/config/talents.ron
(
    trees: {
        Warlock: [
            (id: Affliction, name: "Affliction", icon: "icons/talents/spell_shadow_deathcoil.jpg"),
            (id: Demonology, name: "Demonology", icon: "icons/talents/spell_shadow_metamorphosis.jpg"),
            (id: Destruction, name: "Destruction", icon: "icons/talents/spell_shadow_rainoffire.jpg"),
        ],
    },
    talents: {
        DemonicEmbrace: (
            name: "Demonic Embrace", class: Warlock, tree: Demonology, tier: 1, column: 0,
            icon: "icons/talents/spell_shadow_metamorphosis.jpg",
            max_rank: 2,
            per_rank: [ Stats(( max_health: 11.0 )) ],          // computed price: 11 per rank
        ),
        SiphonLife: (
            name: "Siphon Life", class: Warlock, tree: Affliction, tier: 3, column: 0,
            icon: "...", max_rank: 1, budget: Some(11.0),           // declared: a Grant
            per_rank: [ Grant(SiphonLife) ],                       // abilities.ron entry, talent: true
        ),
        ShadowMastery: (
            name: "Shadow Mastery", class: Warlock, tree: Affliction, tier: 4, column: 1,
            icon: "...", max_rank: 1, budget: Some(22.0),
            requires: Some((SiphonLife, 1)),
            per_rank: [ Patch(on: School(Shadow), field: DamageAndDrain, op: Mul(1.10)) ],
        ),
        CurseOfExhaustion: (
            name: "Curse of Exhaustion", class: Warlock, tree: Affliction, tier: 3, column: 1,
            icon: "...", max_rank: 1, budget: Some(11.0),
            per_rank: [ WidenChoice(WarlockCurse, CurseOfExhaustion) ],  // adds a menu option
        ),
    },
)
```

**The five effect kinds, cheapest first.** Each is applied once per rank.

1. **`Stats(...)`**: the `ItemConfig` stat fields, folded in the `apply_equipment` pass and
   **priced by the item budget's own weights**. Flat values, never percentages, so the price
   is exact and does not depend on the loadout. `PetStats(...)` is the same for a pet.
2. **`Patch(on, field, op)`**: rewrites `AbilityConfig` fields of one ability or one school.
   `field` is an exhaustive `AbilityField` enum; there are no string field names. `op` is
   `Add` or `Mul`, adds before multiplies.
3. **`Grant(AbilityType)`**: adds an ordinary `abilities.ron` entry marked `talent: true`.
   The encyclopedia, the icon, `ability_text` and `lands_silently_audit` reach it as they reach
   any ability. **`WidenChoice(choice, option)`** is the same idea for a per-match menu: it
   makes an otherwise hidden option selectable in Prepare.
4. **`Proc(...)`**: the proc-trinket `ProcConfig`, widened with the triggers `DotTick`,
   `SpellHit`, `HealCrit` and `CritTaken`, and the effects `NextCastInstant` and
   `NextCastFree`. The proc's buff carries `Aura::source_talent`, as a trinket proc carries
   `source_item`, so the frames and the encyclopedia show the talent's icon.
5. **`Mechanic(TalentMechanic)`**: an exhaustive enum of bespoke rules (Soul Link, Spirit of
   Redemption, a resist roll), each with one code site. A last resort: **at most one per tree**.

**The resolved ability book.** About 470 call sites read the global `AbilityDefinitions`.
Talents need *this combatant's* Drain Life. At spawn each combatant gets an `AbilityBook`: its
class's configs, plus granted abilities, with every patch applied.
- Damage, healing, aura, cooldown, mana and AI code already take an `&AbilityConfig`, so a
  site that fetches it from the book changes behaviour with no other edit.
- Migration is incremental. A combatant with no talents gets a book identical to the global
  table, so unmigrated sites stay correct.
- A `talent_book_audit` test (the shape of `heal_site_audit`) then pins
  `AbilityDefinitions::get` out of sim paths that have a combatant to hand.
- The book is resolved once at spawn. Anything that changes during a match is an aura.

**Storage.**

```rust
pub struct Build { pub ranks: BTreeMap<TalentId, u8> }   // BTreeMap: AS-58's float-fold rule
```

`MatchConfig` gains `team1_builds` / `team2_builds`, and the headless JSON gains
`"team1_builds": [{"DemonicEmbrace": 2, "FelStamina": 2, "SoulLink": 1}]`. The per-match
choices keep their own fields: they are not part of a build. An empty build is the default, so
every existing config, baseline and replay is unchanged.

**Validation, in `cargo test`.**

- Every node is in its own class's tree; tiers, columns and prerequisites are consistent; a
  prerequisite is in the same tree and a lower tier.
- `Stats`-only nodes have no declared budget (it is computed); every other node declares one.
- **Rank parity:** every rank prices at 11 ±15%, and every capstone at 22 ±15%.
- **Build parity:** the test enumerates **every legal build** at each point count from 2 to
  8. It fails if the dearest build at a count costs more than one rank (11) above the cheapest.
  The run takes well under a second a class. For the two mock classes:
  - the spread is zero up to 5 points;
  - from 6 points it is exactly one rank, the capstone's premium;
  - at the cap of 8, the 8,119 Warlock builds and 4,973 Priest builds cost 88-99.
- A `Grant` names a `talent: true` ability, and each such ability is granted by exactly one
  node.

**What talents are worth.** A capped build is 88-99 budget points. A combatant's starting kit
is about 334 (the default Warlock), and the run's gear adds about as much again as its talents
(§6.2).

## 4. Why trees stay tractable

Trees produce many builds: 8,119 legal 8-point Warlock builds against the 27 that three
pick-one rows would allow. The three things that made rows attractive (the AI, the sweeps and
legibility) each have an answer.

### The AI copes per ability, never per build

- **Stats and patches need nothing.** The AI already reads cost, cooldown and magnitude from
  the config it is handed; through the book it reads the talented numbers. Improved Psychic
  Scream's shorter cooldown is used without a line of AI code.
- **A granted ability needs one decision rule**, instrumented with `builder.reject` /
  `builder.choose` like any other. A `talent_ai_audit` test fails until every `talent: true`
  ability is reachable from its class's `decide_action`. The mock Warlock trees grant four
  buttons (Amplify Curse, Siphon Life, Shadowburn, Conflagrate); the Priest's grant five.
- **A proc that changes the next cast needs one priority line.** Shadow Trance up means Shadow
  Bolt next.
- **A widened menu needs nothing new in the AI.** The player picks Curse of Exhaustion in
  Prepare; the AI casts the curse it was given, as it does today.
- **Nodes do not talk to each other through the AI.** They interact only through auras the AI
  already reads, so the AI cost of a class's tree is the sum of its nodes, not a product.
  Build-level strategy (keeping the Felhunter alive under Soul Link) is later work, gated
  behind an `AiProfile` like every new behaviour.

The trace header records the build, so a trace explains itself.

### Sweeps measure nodes and reference builds

No sweep tries to cover 8,119 builds. Four instruments do the work, each sized by the tiers in
`sweep-tiers.md`.

1. **The price model** (`cargo test`): build parity above. It bounds what any build can
   *cost*. The sweeps then test whether price predicts power.
2. **Reference builds.** Three per class, one deep in each tree at 8 points, written in
   `talents.ron`. They are:
   - what authored opponents carry;
   - what the bot's default policy plays;
   - what the milestone sweep reports a class's standing *with*: at 0 points (today's
     baseline, byte-identical) and at each reference build. That is three arms per class, not
     thousands.
3. **Node arms**, DIRECTIONAL, for the nodes that carry risk: every grant, proc and mechanic.
   - Each arm is a reference build with its last point moved onto that node, paired at
     identical seeds against the reference.
   - The success condition is that no node dominates: none moves its class by more than the
     tier's resolution floor beyond a stat rank of the same price.
   - `Stats` and plain `Patch` nodes are priced by formula and covered by the reference arms.
4. **The Gauntlet bot** (§7.6) plays runs with random legal builds and with greedy builds. That
   measures the **spread of the build space** directly. If some build wins runs far more often
   than the reference builds, the bot finds it, and its nodes become node arms.

### Legibility: every node leaves a footprint

**A node must show in the match.** It can be an ability cast (which reaches the timeline), an
aura with its icon in the buff bar (procs carry `source_talent`), or a logged event. A
`Stats` or `Patch` node shows before the match instead, in a tooltip that reads the book's
patched numbers. Nodes that leave no trace in this sim are cut: Classic's Suppression (no
spell hit roll), Healing Focus and Fel Concentration (no pushback), and every +1% a viewer
cannot see.

On top of that, for a viewer:

- **A spec label.** The tree with the most points names the spec ("Demonology Warlock"),
  with WoW's shorthand ("2/6/0") under it, on the team frames and the Results screen.
- **Talent icons on the frame:** only the nodes that act in the fight (grants, procs and
  mechanics), at most about four a combatant.
- **The briefing names the enemy's talents as threats** (§7.4).
- **A `[TALENTS]` line at spawn**, beside `[EQUIPMENT]`, and the build in the match record.

## 5. Two worked classes (mocks)

The trees below fix the **shape**: tier sizes, ranks, prices, the five kinds, the footprint
rule, and how WoW's talents translate. They are not final content. **Each class's real trees
are defined in a card of their own** (§13), researched against the client's own talent tables
and held to §1, §3 and §4.

**Scale.** Classic rank-max Corruption is 822 damage; ours is 60. So a Classic damage number
scales by about **0.073**. Cooldowns longer than a match are cut by the Death Coil precedent:
2 min becomes 30s, 3 min becomes 45s. Where Classic's per-rank value fits our budget, it is
used unchanged.

**Sources.** Every Classic tooltip below was looked up on the Wowhead Classic MCP; the link is
the spell ID it returned, and the value quoted is that rank's. **Tier placements and Classic's
arrows are from the 1.12 tree layout as remembered**: the MCP returns spell tooltips, not tree
positions. Each class card confirms them from the client's `TalentTab` and `Talent` tables.
**TBC** marks anything taken from TBC.

### 5.1 Warlock

Default loadout: **372 HP, 302 mana, 120 SP**; the Felhunter spawns at 0.45 × owner HP, so
**167 HP**. Shadow Bolt averages 15 + 0.85 × 120 = **117**. Drain Life deals and heals 10 a
tick for 5 ticks. The Warlock is the 2v2 competitive floor at 40.0%, and its failure is
"dies first" with no defensive cooldown. Demonology is the tree that answers that.

**Affliction** (7 nodes, 10 ranks): the attrition tree.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Improved Drain Life | 2 | +2% health drained ([17804](https://www.wowhead.com/classic/spell=17804)) | +15% drain: 10 → 11.5 → 13 a tick | Patch | Bigger drain ticks |
| 1 | Improved Curse of Agony | 2 | +4% Curse of Agony damage ([18829](https://www.wowhead.com/classic/spell=18829)) | +10%: 84 → 92 → 101 over 24s. Pays only in matches where Agony is chosen | Patch | Tooltip; curse ticks |
| 2 | Amplify Curse | 1 | Next CoW or CoA +50%, 3 min ([18288](https://www.wowhead.com/classic/spell=18288)) | Same; **45s** cooldown | Grant | Cast, buff icon |
| 2 | Nightfall | 2 | 4% per Corruption or Drain Life tick: next Shadow Bolt instant ([18095](https://www.wowhead.com/classic/spell=18095)) | **5% per rank** per tick → Shadow Trance, 10s | Proc | Shadow Trance buff; instant Shadow Bolt |
| 3 | Siphon Life | 1 | 45 health every 3s for 30s, 365 mana ([18881](https://www.wowhead.com/classic/spell=18881)) | **4 every 3s for 30s**: 40 Shadow, healed to the Warlock; 25 mana; Magic. Needs a general `leech` aura field (Death Coil's lifesteal becomes its second user) | Grant | Debuff icon; heal ticks |
| 3 | Curse of Exhaustion | 1 | −10% movement for 12s ([18223](https://www.wowhead.com/classic/spell=18223)) | **Adds a fourth curse to the curse menu: 30% slow for 12s (TBC magnitude)** | WidenChoice | A new option in Prepare |
| 4 | **Shadow Mastery** | 1 | +10% Shadow damage and life drained ([18275](https://www.wowhead.com/classic/spell=18275)) | Same, +10%. **Requires Siphon Life** (Classic's arrow) | Patch | Tooltip; every Shadow number |

Shadow Mastery is the one capstone in these trees that is a number rather than a button;
Classic put it there. Dark Pact is Classic's other deep Affliction node, but it needs Felhunter
mana the sim does not model.

**Demonology** (6 nodes, 9 ranks): the survival tree.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Demonic Embrace | 2 | +15% Stamina, −5% Spirit ([18701](https://www.wowhead.com/classic/spell=18701)) | **+11 max HP** (372 → 383 → 394). Applied before the pet spawns, so the Felhunter gains 5 a rank | Stats (price computed) | Health bar; tooltip |
| 1 | Fel Stamina | 2 | Demon max health +15% ([18752](https://www.wowhead.com/classic/spell=18752)) | Felhunter **+10% max HP** (167 → 184 → 200) | PetStats | Felhunter's bar |
| 2 | Unholy Power | 2 | Felhunter melee +20% ([18773](https://www.wowhead.com/classic/spell=18773)) | Felhunter melee **+10%** | PetStats | Felhunter's hits |
| 2 | Master Demonologist | 1 | Felhunter: +0.2 resistances per level, Warlock and demon ([23785](https://www.wowhead.com/classic/spell=23785)) | **+5 all resistances** on both while the Felhunter lives (6 × 5 × 0.4 = 12) | Stats, conditional aura | Aura icon on both |
| 3 | Demonic Sacrifice | 1 | Sacrifice the demon for a buff; the Felhunter's is 2% mana every 4s ([18788](https://www.wowhead.com/classic/spell=18788)) | **Adds "Sacrifice" to the Warlock's demon choice.** Sacrificed at the gates: **+15% Shadow damage** for the match. The Felhunter's own Classic effect is too weak a trade for losing Spell Lock, so this borrows the Succubus's | WidenChoice + aura | A new option in Prepare; no Felhunter; buff icon |
| 4 | **Soul Link** | 1 | 30% of damage taken goes to the demon; both deal +3% ([19028](https://www.wowhead.com/classic/spell=19028)) | **12% of the Warlock's damage taken goes to the Felhunter** while it lives. No damage rider: +3% leaves no footprint. **Requires Fel Stamina 2/2** (*here*) | Mechanic | Link aura on both; the Felhunter's bar drops when the Warlock is hit; logged |

At 12%, Soul Link gives the Warlock about +14% effective health until the Felhunter has
soaked about 50, the price of two ranks. Classic's 30% would be +43%: several capstones' worth.
**Soul Link and Demonic Sacrifice exclude each other per match**: sacrificing the Felhunter
turns the link off. A player with both has a real Prepare decision against a melee team that
will kill the Felhunter anyway.

**Destruction** (6 nodes, 9 ranks): the burst tree.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Improved Shadow Bolt | 2 | Shadow Bolt crits: +20% Shadow damage taken, 4 hits or 12s ([17803](https://www.wowhead.com/classic/spell=17803)) | **10% per rank** on a Shadow Bolt hit (not a crit: crits are rare at this gear) → Shadow Vulnerability +10%, 4 hits or 12s | Proc | Debuff icon on the target |
| 1 | Bane | 2 | −0.1s Shadow Bolt and Immolate cast ([17788](https://www.wowhead.com/classic/spell=17788)) | Same, −0.1s (2.0 → 1.9 → 1.8) | Patch | Shorter cast bars |
| 2 | Aftermath | 2 | 2% chance to daze for 5s ([18119](https://www.wowhead.com/classic/spell=18119)) | **5% per rank** on a Destruction hit → 30% slow for 5s | Proc | Slow debuff |
| 2 | Shadowburn | 1 | Instant 91-104 Shadow, 15s, a shard ([17877](https://www.wowhead.com/classic/spell=17877)) | Instant **10 + 0.35 × SP ≈ 52**, 20yd, 15s, 40 mana, no shard | Grant | Cast |
| 3 | Improved Immolate | 1 | +5% Immolate initial damage ([17815](https://www.wowhead.com/classic/spell=17815)) | **+25%** (5 ranks folded into one): 35 → 44 | Patch | Tooltip |
| 4 | **Conflagrate** | 1 | Consumes Immolate: 249-316 Fire, 10s ([17962](https://www.wowhead.com/classic/spell=17962)) | **20 + 0.4 × SP ≈ 68 Fire**, consumes the Warlock's Immolate, 15s, 45 mana. **Requires Improved Immolate** (Classic's arrow) | Grant | Cast; Immolate ends |

**Warlocks that read differently at 8 points.**

- **Demonology (2/6/0):** sturdier, with a linked Felhunter, or a sacrificed one when the
  match calls for it; an Improved Drain Life splash.
- **Affliction (6/0/2):** Siphon Life and drains keep it alive by attrition; Shadow Trance
  turns a DoT spread into instant Shadow Bolts.
- **Destruction (0/2/6):** Shadow Vulnerability windows, Conflagrate and Shadowburn as burst.
- **A hybrid (4/4/0):** Siphon Life *and* Demonic Sacrifice, with no capstone.

### 5.2 Priest

Default loadout: **342 HP, 280 mana, 117 SP**. Flash Heal averages 17.5 + 0.75 × 117 =
**105**. Power Word: Shield absorbs 25 + 0.4 × 117 = **72**. The Priest has no per-match
choice today. Its Shadow tree is written for a healer: the control half of Classic's Shadow,
without Shadowform.

**Discipline** (6 nodes, 9 ranks): shields and cooldowns.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Unbreakable Will | 2 | +15% to resist stun, fear and silence ([14791](https://www.wowhead.com/classic/spell=14791)) | **10% per rank** chance to resist a stun, fear or silence | Mechanic (one roll in `apply_pending_auras`) | "Resisted" text; logged |
| 1 | Improved Power Word: Shield | 2 | +15% absorb ([14769](https://www.wowhead.com/classic/spell=14769)) | +5% a rank (Classic's per-rank value): 72 → 76 → 79 | Patch | Tooltip; absorb numbers |
| 2 | Mental Agility | 2 | −10% mana cost of instants ([14783](https://www.wowhead.com/classic/spell=14783)) | −5% a rank on Shield, Scream, Dispel and Fortitude | Patch | Tooltip |
| 2 | Martyrdom | 1 | Focused Casting for 6s after a melee or ranged crit taken ([14774](https://www.wowhead.com/classic/spell=14774)) | 6s of `LockoutDurationReduction` 0.5 (the Concentration Aura effect). The sim has no pushback, so that half is dropped | Proc (`CritTaken`) | Buff icon |
| 3 | Inner Focus | 1 | Next spell free, +25% crit, 3 min ([14751](https://www.wowhead.com/classic/spell=14751)) | Same; **45s** | Grant + `NextCastFree` | Cast, buff icon |
| 4 | **Power Infusion** | 1 | +20% spell damage and healing for 15s, 3 min ([10060](https://www.wowhead.com/classic/spell=10060)) | **+24 SP** (20% of a caster's ~120) on an ally for 15s through `SpellPowerIncrease`; 45s. **Requires Inner Focus** (*here*) | Grant | Cast; buff on the ally |

TBC's Pain Suppression (−40% damage taken for 8s on an ally) is the alternative capstone, and
the stronger arena button. Decision 4 covers TBC sources.

**Holy** (6 nodes, 9 ranks): bigger heals and a last stand.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Holy Specialization | 2 | +5% crit on Holy spells ([15011](https://www.wowhead.com/classic/spell=15011)) | **+2.5% crit** on Holy spells | Patch | Crit heals |
| 1 | Spiritual Healing | 2 | +10% healing ([15356](https://www.wowhead.com/classic/spell=15356)) | **+5%**: Flash Heal 105 → 110 → 116 | Patch | Heal numbers |
| 2 | Inspiration | 2 | Target armor +25% for 15s after a heal crit ([15363](https://www.wowhead.com/classic/spell=15363)) | **+12% armor a rank**, 15s, after a heal crit | Proc (`HealCrit`) | Buff on the ally |
| 2 | Blessed Recovery | 1 | Heal 25% of a melee or ranged crit taken over 6s ([27816](https://www.wowhead.com/classic/spell=27816)) | Same | Proc (`CritTaken`) | HoT icon |
| 3 | Holy Nova | 1 | 29-34 Holy to enemies, 54-63 heal to party within 10yd, 185 mana ([15237](https://www.wowhead.com/classic/spell=15237), rank 1) | **13-15 damage, 25-29 healing** within 10yd (rank 1's ratio, sized to the budget); 50 mana | Grant | Cast; nova on the field |
| 4 | **Spirit of Redemption** | 1 | On death, 10s spirit: free heals, untargetable ([20711](https://www.wowhead.com/classic/spell=20711)) | **8s** spirit. **Requires Spiritual Healing 2/2** (*here*) | Mechanic | Spirit form; logged |

**Shadow** (5 nodes, 8 ranks): control for a healer.

| Tier | Node | Ranks | Classic (verified tooltip) | Here, per rank | Kind | Footprint |
|---|---|---|---|---|---|---|
| 1 | Improved Psychic Scream | 2 | −4s cooldown ([15448](https://www.wowhead.com/classic/spell=15448)) | −2s a rank (Classic's per-rank value): 30 → 28 → 26 | Patch | Tooltip; more Screams |
| 1 | Improved Mind Blast | 2 | −1s cooldown ([15312](https://www.wowhead.com/classic/spell=15312)) | Same, −1s a rank: 8 → 7 → 6 | Patch | Tooltip |
| 2 | Blackout | 2 | 10% chance to stun for 3s on Shadow damage ([15326](https://www.wowhead.com/classic/spell=15326)) | **4% a rank** on Mind Blast, 3s stun | Proc (`SpellHit`) | Stun |
| 3 | Vampiric Embrace | 1 | Party healed for 20% of the Priest's Shadow damage to the target, 1 min ([15286](https://www.wowhead.com/classic/spell=15286)) | Same, 60s debuff; heals go through `apply_healing` | Grant | Debuff icon; heals |
| 4 | **Silence** | 1 | Silence 5s, 45s, 20yd ([15487](https://www.wowhead.com/classic/spell=15487)) | Same. **Requires Blackout** (*here*) | Grant | Cast; silence on the target |

Shadowform ([15473](https://www.wowhead.com/classic/spell=15473)) is held out. It forbids Holy
spells, so it turns a healer into a damage dealer: a role change, not progression, and the AI
has no Shadow-damage role to switch into.

**Priests that read differently at 8 points.** *Discipline (6/2/0)* shields, resists the fear
and hands its partner Power Infusion at the kill window. *Holy (2/6/0)* heals bigger, protects
with Inspiration and keeps healing for 8s after it dies. *A Discipline-Holy hybrid (4/4/0)*
carries Inner Focus and Holy Nova, with no capstone.

## 6. Gear

### 6.1 Tiers

| Tier (`item_tier`) | Item level | Where it comes from | Gain on the slot it replaces |
|---|---|---|---|
| Starting kit (0) | 54-60 | The default loadouts, unchanged | — |
| **Gauntlet** (1) | **76-80** | The common reward | about +34% |
| **Champion** (2) | **90-94** | Offered only after a win | about +59%, plus an effect on most trinkets and weapons |

**Why the numbers move.** The budget is linear in item level. Under it, today's high tier at
ilvl 69-75 adds about +26% to the one slot it replaces, and the average slot is a sixteenth of
a combatant. So a pick from it adds about 7 budget points: +7 health, or +5 spell power. A
reward that small is invisible.

The fix keeps the budget formula and the test, and moves the tiers.
- **The existing 69-75 items are re-levelled to 76-80**, with their stats re-spent to the new
  budget. No default loadout uses them, so no baseline moves; the card checks that no test or
  probe pins their stats either.
- **The Champion tier is new content at 90-94.** Its anchor is Kiss of the Spider, a Classic
  ilvl 85 trinket.
- **Offers come from the slots where a pick is felt:** the six armor slots with multipliers of
  0.75-1.0 (head, chest, legs, shoulders, hands, feet), weapons, and trinkets, whose value is
  their effect. Neck, back, rings, wrists and waist are not offered.

| Pick | Typical slot | Starting budget | After the pick | Gain |
|---|---|---|---|---|
| Gauntlet head or chest | 1.0 | 43.5 | 58.5 | **+15** |
| Gauntlet shoulders, hands or feet | 0.75 | 32.6 | 43.9 | **+11** |
| Champion head or chest | 1.0 | 43.5 | 69.0 | **+26** |
| Champion trinket | 0.5625 | 24.5 | 38.8 | **+14 and an effect** |

### 6.2 How much gear a run hands out

- **One pick per combatant after every match**, from three offers. That is two picks per match
  for the team.
- **At most 6 picks per combatant.** A perfect run takes its sixth after match 6, so the
  last pick lands before the last match, as the last talent point does.
- **Champion offers come only after a win.** On a win, each combatant's offer of three
  includes one Champion item with probability 1/3, from the run's own seed. A perfect run
  expects two Champion picks a combatant. A loss offers Gauntlet gear only. That is the in-run
  reward for winning.
- **What a capped combatant gains:** about 4 Gauntlet picks (+12 each) and 2 Champion picks
  (+22 each, plus effects), about **+90**. That is about the same as its talents (88-99).
  Together they add about +180 to a starting kit of about 334, **roughly +55% by the final
  match.** Six picks cover six of the ten or so slots offers are drawn from.

### 6.3 Trinkets worth winning

Trinkets carry the effects, so the Champion tier is mostly trinkets and weapons. Each new
effect kind needs one AI rule, not one per item.

| Champion trinket (Classic source) | Classic tooltip | Here | Effect kind |
|---|---|---|---|
| **Medallion of the Alliance / Horde** (TBC; Classic's Insignia, [18854](https://www.wowhead.com/classic/item=18854)) | Classic Insignia (Warrior): dispels immobilizing, slowing and stun effects, 5 min | **Breaks every stun, fear, polymorph, root and slow on the wearer; once a match** (TBC's medallion scope) | **On-use: CC break** (new) |
| Talisman of Ephemeral Power ([18820](https://www.wowhead.com/classic/item=18820)) | +175 spell damage and healing for 15s, 90s | **+40 SP for 15s, 45s** | On-use: stat burst (new) |
| Earthstrike ([21180](https://www.wowhead.com/classic/item=21180)) | +280 AP for 20s, 2 min | **+70 AP for 20s, 60s** | On-use: stat burst |
| Diamond Flask ([20130](https://www.wowhead.com/classic/item=20130)) | 9 health every 5s and +75 Strength for 60s, 6 min | **A heal over time and +AP for 30s; once a match** | On-use: stat burst plus a heal over time |
| Hand of Justice ([11815](https://www.wowhead.com/classic/item=11815)) | +20 AP; 2% chance on a melee hit of an extra attack | **+AP; 5% chance of an extra swing** | Proc: extra attack (new effect) |
| Darkmoon Card: Blue Dragon ([19288](https://www.wowhead.com/classic/item=19288)) | 2% chance on a cast: full mana regen while casting for 15s | **5% on a cast: +mana regen for 15s** | Proc (existing machinery) |
| Zandalarian Hero Charm ([19950](https://www.wowhead.com/classic/item=19950)) | +204 spell damage / +408 healing for 20s, each cast takes 1/12 off, 2 min | **Later:** needs a decaying buff | — |
| Kiss of the Spider ([22954](https://www.wowhead.com/classic/item=22954), ilvl 85) | +1% crit and hit; on use +20% attack speed for 15s, 2 min | **Later:** needs a haste aura | — |

- **The CC-break AI rule** reuses the shape of the Paladin's while-CC Divine Shield
  (`divine_shield_while_cc_should_fire`): fire when hard-CC'd with 2s or more left and a
  teammate low, or when the wearer is the kill target under burst. The trinket answers a
  briefing threat by name ("their Rogue chains Kidney Shot"), so it is the gear that makes
  Prepare matter most.
- **A stat on-use** fires in the kill window (a damage dealer) or when an ally drops low (a
  healer).

### 6.4 How the item budget test fits

The test still prices every item at `item_level × 0.75 × slot multiplier` with the same stat
weights. Four changes come with the tiers:

1. **Tier bands.** `item_tier` (already in `ItemConfig`) is validated against its item level
   band: 0 at 54-60, 1 at 76-80, 2 at 90-94.
2. **On-use pricing.** An on-use stat effect is priced by its **uses in a median arena life**
   (43s, measured in `tests/proc_trinket_probes.rs`), not by the long-run uptime bound procs
   use. An on-use starts ready, so in a 43s life a 45s cooldown is one use: 15s of +40 SP is
   `1.5 × 40 × 15/43 ≈ 21`. The long-run bound (`15/60`) would charge 15 and underprice it.
3. **Declared prices** for effects with no stat weight: the CC break and the extra attack. A
   declared price is sweep-checked, as a talent mechanic is.
4. **Offer floors** in `gauntlet.ron` validation, not in the item test: an offered item must
   add at least 30% (Gauntlet) or 55% (Champion) over the starting item in its slot. That
   keeps a reward from ever being a sidegrade.

## 7. The Gauntlet run

### 7.1 The run

1. **Difficulty and draft.** Choose a difficulty from those unlocked (§8). Classes are offered,
   **3 at first and up to 6 with meta-progression** (§9), at least one a healer. Pick two. Each
   starts in its default loadout with **2 talent points to spend**.
2. **Hub.** The record as pips (wins out of 7, losses out of 3), your team with spec labels and
   gear, and the next opponent's **briefing**.
3. **Talents.** Spend unspent points, or bank them.
4. **Prepare.** The per-match choices, pre-filled with last match's, against the briefing.
5. **Fight.** PlayMatch exactly as today, kill calls included. Results as today, plus
   **Continue run**.
6. **Point and rewards.** Each combatant gains **1 talent point** (up to 8) and **picks one
   reward of three** (up to 6), win or loss. Champion offers appear only after a win.
7. **Run end** at 7 wins or 3 losses: the record, the team with its builds and gear, each
   match with its record (§10), and any renown ability the run unlocked (§9).

### 7.2 The caps

| Before match | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|
| Talent points | 2 | 3 | 4 | 5 | 6 | 7 | **8** | 8 | 8 |
| Reward picks | 0 | 1 | 2 | 3 | 4 | 5 | **6** | 6 | 6 |

- **Points and picks follow matches; the opponent's tier follows wins.** So a loss buys a
  point and a pick against the *same* tier. That is the run's catch-up.
- **Both caps land before match 7**, the earliest a perfect run can play its last match. By
  then every live run has the same points and picks, so a run with losses never ends stronger
  than a perfect one, and there is no reason to lose.
- **Spent points stay spent for the run.** That makes the line between a commitment and an
  answer mechanical: the per-match choices are where the player adapts. Banking a point for a
  node a few matches away is allowed.

### 7.3 Opponents scale with wins (recommended)

**Opponents scale with the player's wins, to budget parity with a player who has not lost.**
An opponent at tier *t* (the player's wins plus one) carries:
- **min(t + 1, 8) talent points** a combatant;
- **t − 1 reward picks** a combatant, at the default rate of Champion items (one from tier 4,
  two from tier 7).

So at the default difficulty, a player who has won every match meets an equal budget, and a
player with a loss is a point and a pick ahead. The last fight is a fair fight, decided by the
draft, the builds and Prepare.

Why scale rather than stay static:
- **A static ladder gets easier as the player grows.** The last fights would be the softest,
  the opposite of a climb.
- **Scaling with wins rather than with matches** is what lets a loss help.
- **Parity at the default** makes difficulty an explicit setting (§8), not an accident of how
  the ladder happened to be tuned.

How opponents are authored and graded:
- Authored opponents carry a class's reference builds (or named variations) cut to their point
  count, and gear drawn from their tier's reward tables.
- **A class without a tree yet carries its points as item budget**, 11 per point, so opponents
  can be any of the nine classes from the first slice.
- Opponents are graded by sweep against reference teams.
- The AI profile is an authored property of each opponent. It is set where that comp is
  stronger with it, and it is never a difficulty lever. `TeamPlan`'s effect depends on the
  comp: +25pt for Warlock+Priest, −45pt for Rogue+Priest (n=300, 2026-10-10).

### 7.4 The per-match choices keep their role: the briefing

The briefing shows:

- the opponent's classes, map, AI profile, **talent points and reward picks**;
- their **spec labels** ("Affliction Warlock 4/0/0"), the talent icons that act in a fight,
  and their notable gear (a Medallion, a Talisman);
- **threat lines** generated from their kit, their per-match choices, their talents and their
  trinkets: "Siphon Life heals their Warlock: dispel it", "their Rogue's Medallion breaks your
  first stun";
- **your answers** with the current Prepare settings, recomputed as you change them: "Curse of
  Tongues slows their healer's casts", "the Felhunter can devour Polymorph off your healer",
  and **what is missing**: "no slow for their kiter: Curse of Exhaustion is an Affliction
  talent".

Talents and gear can make an answer possible (Curse of Exhaustion against a kiting Hunter, a
Medallion against a stun chain) or better (Improved Curse of Agony). Choosing that answer
happens in Prepare, every match, for free.

### 7.5 What a run remembers

The run save holds the **resolved state**: roster, builds, gear, record, the current offers and
the next opponent. It also holds the run seed, which generates only what is still to come. A
game update in the middle of a run therefore changes nothing already decided (§10).

### 7.6 The bot and calibration

- **A bot plays the Gauntlet headlessly** (`--gauntlet <seed> --difficulty <level> --policy
  <name>` under `--batch`). Its policies pick talents (follow a reference build, random legal,
  greedy by price) and rewards (highest gain, effects first).
- Hundreds of runs per difficulty give:
  - the clear rate;
  - the wins-per-run distribution;
  - the walls in the ladder;
  - the dominant pairs;
  - the spread of builds.
- Each difficulty level is calibrated to a **target clear rate** (§8), measured for a fresh
  profile and for a fully unlocked one.
- The bot evaluates every renown condition on every run, which gives each ability's **earn
  rate** before any player sees it (§9.4).
- **Post-match odds (later):** after a loss, twenty background seeds of the same matchup say
  whether the loss was the dice or the setup.

## 8. Difficulty (recommended)

**A ladder of six levels, named for WoW's arena titles, each unlocked by clearing the one
below.** The levels move **opponent gear first**, because gear is the lever with the finest
steps and the one a player can see in the briefing. Points move only at the top two levels,
and the AI profile never moves.

| Level | Opponent gear, against parity | Opponent points | Unlocked by | Target clear rate (fully unlocked bot) |
|---|---|---|---|---|
| Novice | 2 picks fewer, no Champion items | Parity | Always | ~70% |
| **Combatant** (default) | Parity | Parity | Always | ~40% |
| Challenger | 1 more pick, 1 more Champion item | Parity | A Combatant clear | ~25% |
| Rival | 2 more picks, 2 more Champion items | Parity | A Challenger clear | ~15% |
| Duelist | 3 more picks, all Champion | +1 a tier (still at most 8) | A Rival clear | ~8% |
| Gladiator | 4 more picks, all Champion | 8 from tier 1 | A Duelist clear | ~3% |

- **The targets are starting values**, set by the calibration card (G-calibration) from bot
  runs and then held by it. A level whose measured clear rate leaves its band is re-tuned, the
  way an opponent tier is re-graded.
- **Each step is about one pick** (+12 to +25 budget, about 3-5% of a capped combatant). That
  is fine enough that a level is a real step and coarse enough that the briefing shows it.
- **A clear at a level earns its title**, kept in the profile and shown in the encyclopedia.
  The highest title is the headline record.
- **The flagship renown abilities are clears at each level** (§9), so a profile grows by
  climbing, and farming Novice earns nothing.

## 9. Meta-progression: renown

The goal is replay value beyond the draw. A player should have reasons to clear a run, to come
back, and to try something different next time, and should feel stronger for it, without a
newcomer being weak by design.

**Renown abilities are awarded, not bought.** Each renown ability carries a **condition**, and
the ability unlocks the moment its condition is met. Nothing is spent and there is no screen to
manage it: the encyclopedia shows every ability, what it does, what unlocks it, and whether the
profile has it (§9.5). A spend-it-yourself screen could come later (§9.6).

### 9.1 What renown grants: choice, not stats

Renown widens the decisions a run offers. It never raises a cap or adds budget, so a veteran's
edge is better options, and the difficulty ladder (§8) carries the climb.

| Effect | Base | Ceiling | Earned from |
|---|---|---|---|
| **Draft choices** | 3 classes offered | 6 (and never more than the pool) | Flagship goals |
| **Equipment choices** | 3 reward offers | 5 | Flagship goals |
| **Equipment rerolls** | 0 a run | 5 a run | Specific objectives |
| **Titles** | — | — | Clears at each level; some objectives |

### 9.2 What earns it: conditions that reward variety

- **Flagship goals grant the key upticks.** A clear at each difficulty level grants one draft
  choice or one equipment choice, alternating up the ladder: Combatant, a draft choice;
  Challenger, an equipment choice; Rival, a draft choice; Duelist, an equipment choice;
  Gladiator, a draft choice.
- **Specific objectives grant smaller bonuses**, mostly an equipment reroll or a title. They
  are written to send the player somewhere new: a class, a comp, a style of play. Examples:
  - **per run:** clear with a Rogue; clear with a team that has no healer; a flawless 7-0 run;
    clear without ever equipping a Champion item;
  - **per match:** win a match that comes down to a 1v1; land a killing blow while the
    off-target is in hard crowd control;
  - **across runs:** clear with four different classes.
- **No condition rewards a double-healer team.** The validator rejects any condition that
  requires two healers, or that a double-healer team meets more easily than any other comp
  ("clear without taking damage on a healer" would be one).
- **The specific list is designed later**, in its own card, measured against the bot (§9.4).
  This section fixes the hooks.

### 9.3 The framework

**Data.** `assets/config/renown.ron`, loaded and validated like `movement.ron`:

```ron
(
    abilities: {
        ClearCombatant: (
            name: "Arena Combatant", icon: "icons/renown/combatant.jpg",
            effect: DraftChoices(1),
            condition: Run(All([ Cleared(min_level: Combatant) ])),
        ),
        OpenHanded: (
            name: "Open-Handed", icon: "...",
            effect: EquipmentRerolls(1),
            condition: Run(All([ Cleared(min_level: Combatant), NoItemTierEquipped(2) ])),
        ),
        CloakAndDagger: (
            name: "Cloak and Dagger", icon: "...",
            effect: EquipmentRerolls(1),
            condition: Run(All([ Cleared(min_level: Combatant), TeamIncludes(Rogue) ])),
        ),
        LastOneStanding: (
            name: "Last One Standing", icon: "...",
            effect: EquipmentRerolls(1),
            condition: Match(All([ Won, CameDownTo(1, 1) ])),
        ),
        CleanKill: (
            name: "Clean Kill", icon: "...",
            effect: Title("the Precise"),
            condition: Match(All([ Won, KillingBlowWhileOffTargetIn(HardCc) ])),
        ),
    },
)
```

**Three scopes, three fact sources.** A condition is a small tree of **leaves** joined by
`All` / `Any` / `Not`. Its scope says what the leaves read. **No leaf reads raw log text.**

| Scope | Evaluated | Reads | Example leaves |
|---|---|---|---|
| `Match` | When a Gauntlet match ends | **`MatchFacts`**, a digest built once from `MatchResult` and the combat log's structured events | `Won`, `CameDownTo(ours, theirs)`, `KillingBlowWhileOffTargetIn(cc)`, `DurationUnder(s)`, `NoDeathsOnTeam` |
| `Run` | When a Gauntlet run ends | **`RunRecord`**: difficulty, team, record, every match's `MatchFacts`, every reward taken | `Cleared(min_level)`, `LossesAtMost(n)`, `TeamIncludes(class)`, `HealerCount(n)` (n ≤ 1), `NoItemTierEquipped(tier)`, `MatchesWhere(match condition, at_least: n)` |
| `Profile` | When a run ends | **The profile's history**: past clears with their teams | `DistinctClassesCleared(n)`, `ClearedWithEveryHealer` |

- **`MatchFacts`** holds what conditions need and nothing else. The match and the log already
  carry most of it: the winner and end reason, each combatant's class and role, its death
  time and killer (`StructuredEventData::Death`), killing blows (`Damage { is_killing_blow }`)
  and crowd control applied (`CrowdControl`). From those it derives:
  - **the living count on each side over time**, from the deaths ("came down to a 1v1": both
    sides had one living non-pet combatant before the end);
  - **at each killing blow, the crowd control on the victim's living teammates.**
  
  The one missing input is **when crowd control ends early**: a fear broken by damage, or a
  dispel. Today that is only an `AuraRemoved` message. The framework's first card adds a
  structured end event, so CC intervals are exact. "Hard CC" uses the AI's own
  loss-of-control classification (stun, fear, horror, polymorph, incapacitate, cyclone, trap;
  roots excluded). Pets never count as a side's last combatant.
- **The `RunRecord`** is the run save (§7.5) plus each match's `MatchFacts`, a few hundred
  bytes a match. Run conditions can therefore count matches (`MatchesWhere`) without
  re-reading a log.
- **Only Gauntlet runs award renown.** Custom Match and replays never do, so nothing is farmed
  in a sandbox.

**Awarding.**
- One function, `evaluate(scope, facts, profile) -> Vec<RenownId>`, runs at the two points
  above. It returns the abilities whose condition holds and that the profile does not have.
- Awards are idempotent and permanent.
- **An award takes effect from the next run.** A run's options never change halfway through,
  which keeps a run's save and its records consistent.
- The Results or Run-end screen names each new award in one line, linked to its encyclopedia
  entry.

**Persistence.** The profile save (§9.6) keeps the unlocked set as a `BTreeSet<RenownId>`,
each with the run and date that earned it, plus the history that `Profile` conditions need.
- On load, an id that no longer exists is dropped, and a new ability starts locked.
- A condition that was already met in past history is re-checked against the stored history
  on load, so adding a `Profile` condition can award it retroactively. `Run` and `Match`
  conditions are judged only from the day they ship.

**Adding a renown ability costs one RON entry** when its condition is built from leaves that
already exist. That will be the common case, and the first list is meant to be mostly that.
- **A new leaf** is one enum variant, one predicate over `MatchFacts`, `RunRecord` or the
  profile, and one fixture test. The leaf enum is exhaustive, so the condition prose generator
  and the validator must handle a new leaf before anything compiles.
- **A new effect** is rarer: an enum variant and the code that applies it at run start.
- **Validation, in `cargo test`:**
  - every ability's condition parses and stays in its scope (a `Match` leaf cannot appear in
    a `Run` condition except inside `MatchesWhere`);
  - no condition requires two healers;
  - every effect stays under its ceiling when every ability is unlocked;
  - every leaf has a fixture that fires on a hand-built `MatchFacts` or `RunRecord` and one
    that does not.

### 9.4 Measured before it ships

The Gauntlet bot (§7.6) evaluates every condition on every run it plays, so each ability's
**earn rate** is a measurement before any player sees it. An objective nobody can reach, or one
everybody reaches by accident, shows up in the calibration report.

### 9.5 The encyclopedia

**A Renown section** (`Topic::Renown`), generated from `renown.ron`. Each entry shows:
- **its effect**, in words ("+1 equipment reroll per run");
- **its unlock condition**, as prose generated from the condition tree by one generator, the
  way `ability_text.rs` writes ability prose ("Clear a Gauntlet run at Combatant or above with a
  Rogue on your team");
- **its state:** locked or unlocked, with the run that unlocked it;
- for `Profile` and counting conditions, **progress** ("3 of 4 classes").

The section opens with the profile's totals: draft choices, equipment choices and rerolls.
Difficulty levels and titles have pages in the same section. Champion items appear in Items
and Buffs & Debuffs like any other item, with their tier shown.

### 9.6 What persists, and what could come later

A **profile save** under the settings directory holds:
- clears and titles per level;
- unlocked renown abilities;
- the clear history that `Profile` conditions read;
- records (best per class, pair and spec);
- saved talent plans (a named path through a class's trees that the talent step proposes one
  click at a time);
- run history (§10).

**Later, if wanted:** a spend screen. Renown would become a currency and the same abilities
would be bought instead of awarded. The conditions would still decide what can be bought, so
the framework above carries over unchanged.

## 10. Replays and saves across versions

**A seed reproduces a match only on the build that played it.** Any change to the sim or the
RON configs can change the outcome. So:

- **Within a run, records are exact.** A match record is its config (classes, builds, gear,
  per-match choices, map, seed) plus its kill calls with sim timestamps. That reproduces the
  match on the same build, which is all an in-run replay needs.
- **Every record is version-stamped**: the game version and a hash of the RON configs.
- **A record from another version is never replayed as if it were the same match.** History
  shows its stored result, damage and healing summary, and **combat log text** (a few KB,
  kept with the record), so nothing is lost. It offers **Re-simulate in this version**,
  labelled "the outcome may differ".
- **The run save stores resolved state, not only a seed** (§7.5), so an update mid-run keeps
  everything already decided.
- **Ghost opponents** (a finished run's team becoming an opponent) are team configs, not
  replays. They re-fight in the current sim, so they survive every version by design.
- **Exact playback across versions** would need a state tape: per-tick positions, casts and
  auras, about 100-200 KB a match, played back by the renderer with no sim. Nothing needs it
  yet. It becomes worth building if shared replays matter (decision 5).

## 11. A worked run: Warlock and Priest

The player drafts the Warlock and the Priest at **Combatant**. The Warlock heads into
**Demonology** (the survival tree, against its "dies first" failure) with an Affliction
splash. The Priest builds a **Discipline-Holy hybrid**. "Them" is the opponent's points and
picks per combatant.

| # | Opponent (tier, map) · them | Points (Wl/Pr) | Talents taken | Per-match choices | Result | Rewards (Wl / Pr) |
|---|---|---|---|---|---|---|
| — | Draft | 2 / 2 | Wl: Demonic Embrace, Fel Stamina. Pr: Imp. PW:Shield, Unbreakable Will | — | — | — |
| 1 | Gate Rats: Warrior + Priest (1, BasicArena) · 2 pts, 0 picks | 2 / 2 | — | Kill the Priest. Weakness on the Warrior, Tongues on the Priest. Felhunter | Win | Gauntlet gloves / Gauntlet chest |
| 2 | Dustcloaks: Rogue + Druid (2, Nagrand) · 3, 1 | 3 / 3 | Wl: Fel Stamina 2. Pr: Unbreakable Will 2 | Kill the Druid. Agony on the Druid, Weakness on the Rogue | Win | **Medallion** (Champion) / Gauntlet legs |
| 3 | Ashen Pair: Mage + Shaman (3, TwinPillars) · 4, 2 | 4 / 4 | Wl: Demonic Sacrifice. Pr: Inner Focus | Felhunter to devour Polymorph. Tongues on the Shaman | **Loss** | Gauntlet head / Gauntlet head |
| 4 | Cinder Pact: Mage + Priest (3, BasicArena) · 4, 2 | 5 / 5 | Wl: Unholy Power. Pr: Spiritual Healing | Kill the Priest; Tongues on it | Win, the Warlock alone against their Mage at the end: **Last One Standing** unlocks | Gauntlet shoulders / Gauntlet shoulders |
| 5 | Thornwatch: Warlock + Priest (4, Nagrand) · 5, 3 | 6 / 6 | Wl: **Soul Link** (capstone). Pr: Spiritual Healing 2 | Spell Lock their Priest; Tongues on it, Weakness on their Warlock | Win | Gauntlet legs / **Talisman of Ephemeral Power** (Champion) |
| 6 | Stormhold: Hunter + Paladin (5, Nagrand) · 6, 4 | 7 / 7 | Wl: Improved Drain Life. Pr: Holy Specialization | Felhunter kept: Spell Lock answers the Paladin. Tongues on the Paladin. The briefing notes no slow for the Hunter | Win | **Champion robe** / Gauntlet feet (both at 6: capped) |
| 7 | Gilded Fangs: Rogue + Priest (6, BasicArena) · 7, 5 | 8 / 8 | Wl: Improved Drain Life 2. Pr: **Holy Nova** | Felhunter kept for Soul Link. Weakness on the Rogue. The Medallion breaks Kidney Shot | Win | — (capped) |
| 8 | Champions: Warrior + Paladin (7, BasicArena) · 8, 6 | 8 / 8 | — (capped) | **Sacrifice the Felhunter** (+15% Shadow; Soul Link off): two melee will kill it anyway. Weakness on the Warrior | Win | — |

**7-1, cleared at Combatant.** Two renown abilities unlock, and both take effect from the next
run:
- **Last One Standing**, after match 4 (+1 equipment reroll a run);
- **Arena Combatant**, the flagship for a Combatant clear (a fourth draft choice).

The run also earns the Combatant title and access to Challenger. Three objectives stay locked,
and the encyclopedia now points at them:
- **Flawless**: one loss;
- **Open-Handed**: Champion gear was equipped;
- **Cloak and Dagger**: no Rogue.

So the next run has a reason to try something different.

- **Warlock 2/6/0, Demonology:** Demonic Embrace 1, Fel Stamina 2, Demonic Sacrifice, Unholy
  Power 1, Soul Link; Improved Drain Life 2. Price 99. Gear: 4 Gauntlet picks and 2 Champion
  picks, the Medallion and the robe.
- **Priest 4/4/0, a hybrid:** Improved PW:Shield 1, Unbreakable Will 2, Inner Focus; Spiritual
  Healing 2, Holy Specialization 1, Holy Nova. Price 88. Gear: 5 Gauntlet picks and the
  Talisman.

What the run shows:

- **Two points at the draft** gave each combatant a coherent start, and the **hybrid** came
  together at match 7.
- **The loss bought a point and a pick** against the same tier, and the player stayed one
  ahead until the caps closed the gap at the final match.
- **Both caps were reached before the last two fights.** The final match was budget parity:
  8 points and 6 picks on each side.
- **Gear and talents widened per-match decisions without making them.** The Medallion answered
  the Rogue in match 7, and Sacrifice was kept back until match 8.

## 12. Minimum playable slice

**The first slice includes talents and both gear tiers.** Without them the Gauntlet's only
progression is a gear draw, and the tree is the loop's spine. To stay small, the slice narrows
the *class pool*, not the system.

1. **Main menu → Gauntlet.** One active run, resumable. Custom Match stays the sandbox, and
   gains the talent panel.
2. **Draft three of four:** Warlock, Priest, Rogue and Paladin, the four classes with trees
   first. Two healers in a pool of four means every offer of three has a healer. Six pairs, five
   with a healer.
3. **Hub** with the briefing, **Talents**, **Prepare**, **Fight**, **Rewards** (Gauntlet and
   Champion tiers, the Medallion and two stat on-use trinkets), **Run end**, as in §7.1.
4. **Opponents** from all nine classes, about 20 in 7 tiers, scaling with wins (§7.3). The five
   classes without trees carry their points as item budget.
5. **One difficulty, Combatant.** Records are exact within the version, with the version stamp
   and stored combat log from the start (§10).

**Out of the slice:** the other difficulty levels, renown, the other five classes' trees, 3v3,
ghosts, daily seeds and post-match odds. The slice does keep a `MatchFacts` digest on every
match record, so renown can later be judged from runs played since then.

**What the slice needs from AS-251 (the UI design system):**
- a choice card (draft offers and rewards) with a **tier frame** for Gauntlet and Champion;
- a team card with spec label;
- a pip track;
- an intel panel with linked icons;
- an item comparison tooltip showing the gain;
- a hub template;
- **a talent tree component:** three columns of tier-gated nodes with rank pips, arrows, a
  locked state and a "spend" affordance.

Exit from a run means save and leave; abandoning is its own confirmed action.

## 13. Phased plan

Cards in dependency order. Each is shippable and keeps `main` green. **Every card before the
slice leaves the default build empty and the default loadouts unchanged, so `Legacy` and
BasicArena stay byte-identical and `determinism_pin` holds.**

**Phase 0: engines and foundations (no new screens).**

| Card | Delivers |
|---|---|
| T1 Talent engine | `talents.ron` (trees, tiers, ranks, prerequisites) and a validating loader; `Build` per slot in `MatchConfig` and headless; the `AbilityBook` and the migration of the cast, channel, aura-tick and AI paths; rank and build parity tests; `talent_ai_audit`; the `[TALENTS]` line and the build in the trace header. The general mechanisms the trees need: the `leech` aura field, the proc triggers (`DotTick`, `SpellHit`, `HealCrit`, `CritTaken`) and effects (`NextCastInstant`, `NextCastFree`), `WidenChoice`, and the `Mechanic` hook |
| T2 Talent panel and encyclopedia | The tree in View Combatant and Custom Match (8 points); `Topic::Talent` and a Talents section; spec labels on frames and Results |
| **Per class: "define and build class X's trees"** | One card per class, in this order: **Warlock, Priest, Rogue, Paladin** (the slice's four), then Warrior, Mage, Hunter, Shaman, Druid. Each card does five things: research the class's Classic and TBC trees from the client's `TalentTab` / `Talent` tables; write its three trees under §1, §3 and §4 (the mocks in §5 are its starting point for the first two); implement them; write its three reference builds; run a DIRECTIONAL node-arm sweep for its grants, procs and mechanics |
| E1 Gear tiers | Re-level the 69-75 tier to 76-80 and re-spend its stats; tier bands validated on `item_tier`; Champion tier content (armor and weapons) |
| E2 Trinkets | `on_use` on items, priced by uses in a median life; the CC-break and extra-attack effects with declared prices; their AI rules; the Medallion, Talisman, Earthstrike, Diamond Flask, Hand of Justice and Blue Dragon |
| G1 Run model | `RunState` (resolved state plus seed); offers on their own RNG stream; version stamp and stored combat log on every match record |
| G2 Gauntlet data | `gauntlet.ron`: run rules and caps, opponents by tier with builds and gear (or budget-equivalent gear), reward tables with offer floors; validated at load |
| G3 Recorded kill calls | Kill calls with sim timestamps in the match record; a replay reproduces the match on the same build |
| G4 Headless driver | `--gauntlet <seed> --difficulty <level> --policy <name>`, with talent and reward policies |
| G5 Calibration | DIRECTIONAL: reference-build arms per class, bot runs for the clear rate, wins-per-run, walls, pair dominance and build spread. Output: opponent tiers, reward tables and the Combatant target, in a findings doc |

**Phase 1: the minimum playable slice.**

| Card | Delivers |
|---|---|
| G6 Entry and draft | Main-menu entry, draft screen, starting a run |
| G7 Hub, briefing and Prepare | The hub; the briefing with points, picks, talent and trinket threats, and answers and gaps; Prepare reusing the pre-match widgets |
| G8 Talent step | The tree screen in a run: spend or bank, locked once spent |
| G9 Rewards and run end | Per-combatant reward picks with tier frames and the gain shown; run end with builds, gear and records |
| G10 Save, resume, abandon | Resume after a quit; a confirmed abandon; a UI script drives a short run as a smoke test |

Milestone review: **play three runs.**

**Phase 2: meta-progression and difficulty.**
- M1 Profile save, clears and titles.
- M2 Difficulty levels and their calibration (the G5 bot per level, fresh and fully unlocked).
- M3 The renown framework:
  - `renown.ron` and its validator;
  - the condition tree with its `Match`, `Run` and `Profile` leaves, each with fixtures;
  - `MatchFacts`, plus the structured crowd-control-end log event it needs;
  - `evaluate` at match and run end;
  - awards persisted in the profile;
  - the bot's earn-rate report.
- M4 Renown effects: draft choices, equipment choices and equipment rerolls, applied at run
  start, and the flagship clear abilities.
- M5 The encyclopedia's Renown section, with prose generated from each condition, the
  locked/unlocked state and progress.
- M6 The first list of specific objectives, designed and then checked against bot earn rates.
- Alongside: the other five classes' trees, 3v3 runs, map draws in the briefing, saved talent
  plans, daily seed, run history with re-simulation, post-match odds, and per-match choices for
  the Priest, Shaman and Druid.

**Phase 3: depth.**
- Build-aware AI behind an `AiProfile` (the Felhunter as a Soul Link sponge).
- The later trinkets (Zandalarian Hero Charm, Kiss of the Spider).
- Ghost opponents.
- A state tape, if decision 5 calls for one.
- The Stable (named combatants carried between runs) and Counter-Pick Trials (authored puzzles
  against a fixed enemy team), which reuse the opponent format.

**The first card:**

> **Talents 1: the engine.** Add `assets/config/talents.ron` with a validating loader, `Build`
> per slot in `MatchConfig` and the headless config, and a per-combatant `AbilityBook` built
> at spawn, migrating the cast, channel, aura-tick and AI reads. Add the general mechanisms the
> trees need, the rank and build parity tests, `talent_ai_audit` and the `[TALENTS]` log line.
> Prove it on the Warlock's Demonology tier 1-2 nodes (Stats and PetStats only).
>
> **Success condition:** empty build by default, so `Legacy` and BasicArena are
> byte-identical. A DIRECTIONAL paired sweep of a 4-point Demonology build against none on the
> Warlock's 2v2 cells says which way and roughly how much it moves the Warlock.

## 14. Decisions still open

1. **Respec.** Spent points locked for the run, as recommended? Or a free respec between
   matches, which blurs talents into per-match choices?
2. **The first slice.** Talents and both gear tiers in the slice with a four-class pool
   (Warlock, Priest, Rogue, Paladin), as recommended? Or all nine classes with gear only, and
   talents as phase 2?
3. **Baked-in talents.** Mortal Strike, Ice Barrier, Holy Shock, Swiftmend, Unstable
   Affliction and instant Corruption stay baseline, as recommended, so no class loses a button?
   Or move them into the trees as capstones, which is more faithful and re-baselines every class?
4. **TBC sources.** Talents and trinkets may draw on TBC where Classic does not fit (Curse of
   Exhaustion's 30%, Pain Suppression, the Medallion's scope)?
5. **Cross-version replays.** Version-stamped records with a labelled re-simulation, as
   recommended, and a state tape only if shared replays become a goal? Or build the state tape
   so any past match can be watched exactly?
