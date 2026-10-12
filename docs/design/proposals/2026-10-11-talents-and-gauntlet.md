# Talents and the Gauntlet: one design

**Card:** AS-255. **Status:** proposal, for the user to judge. Nothing here is implemented.
**Prototype:** [`docs/design/benches/2026-10-11-gauntlet-prototype.html`](../benches/2026-10-11-gauntlet-prototype.html)
clicks through one run: the draft, the talent step after every match, the Prepare step against
the opponent briefing, a stand-in fight, the reward pick and the run end. Open it from the
checkout so the icons resolve. The fight is a scripted coin flip weighted by gear, points and
answers, not the sim. The Rogue and Paladin trees are not designed here, so the prototype
turns their points into item budget, as §6.3 does for opponents. The styling is neutral, because the look
belongs to the UI design system (AS-251). Judge the flow and the decisions.

## Summary

A run of the **Gauntlet** is a two-class team fighting a visible, escalating ladder of
opponents until it reaches 7 wins or 3 losses. Between fights the player makes two kinds of
decision. They are kept apart on purpose:

| | Character progression: **talents** | Per-match strategy: **class choices** |
|---|---|---|
| What | WoW talents, in trees: ranks, tier gates, prerequisites, a capstone | The class's own options: curse, shout, armor, aura, pet, opener, poison (plus kill and CC target) |
| When | One point per combatant after every match; spent at the hub | In Prepare, before every match |
| Cost | Spent points stay spent for the run | Free; change them every match |
| Question it answers | *Who is this character becoming?* | *What beats the team in front of me?* |
| Where it shows | A spec label and talent icons on the frames, procs and new buttons in the fight | The opponent briefing's "your answers" lines |

- **The line.** A choice is a talent only if WoW put it in a talent tree. Otherwise it is a
  per-match choice if its best value depends on the opponent, and baseline behaviour if not. A
  talent may *add* an option to a per-match menu (Curse of Exhaustion adds a curse); it never
  *picks* one (§1).
- **The trees.** Three trees per class with WoW's names, five to seven nodes each, four tiers.
  A class has about 18 nodes and 27 ranks. A run pays out at most **9 points**, so one run
  touches about a third of the class: one capstone and a splash, or a spread across trees
  (§3).
- **Tractable by construction.** Every rank is priced on the item budget (11 points; a
  capstone 22), so every legal 9-point build costs within 15% of every other. That is checked by
  enumerating all of them in `cargo test`: 9,721 Warlock builds, 5,747 Priest. The AI needs one
  rule per granted ability and nothing else, about five per class. Sweeps measure **nodes and
  reference builds**, never all builds. Every node must leave a footprint a viewer can see
  (§4).
- **The run.** One point per combatant at the draft and one after every match, win or loss. A
  loss buys a point against the same tier, which is the run's catch-up. The reward pick stays
  gear. Opponents carry exactly the points of their tier. Nothing carries between runs in
  power; records, replays and saved talent plans persist (§6).
- **Minimum playable slice:** the Gauntlet *with* talents, from a four-class pool that has
  trees (Warlock, Priest, Rogue, Paladin). Opponents may be any class: a class without a tree
  carries its points as equal item budget (§8).

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
answer the opponent briefing (§6.4).

Several shipped *abilities* were talents in Classic or TBC: Mortal Strike, Ice Barrier, Holy
Shock, Swiftmend, Unstable Affliction (TBC), and Corruption's instant cast (Improved
Corruption). They are baseline today. The trees here leave them baseline, so no class loses a
button; decision 5 asks whether that holds.

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
  matches where the player chooses that answer. So progression can *bias* the counter-pick,
  which is a real build identity, but it never takes it away.

**Worked placements.**

| Candidate | Q1 | Q2 | Side |
|---|---|---|---|
| Curse of Exhaustion | Affliction talent | — | Talent that widens the curse menu |
| A second Warlock demon (Succubus) | Baseline in WoW | Yes: Seduction vs Spell Lock depends on the enemy | Per-match (the demon choice) |
| Improved Curse of Agony | Affliction talent | — | Talent; pays when Agony is chosen |
| Shaman Tremor vs Grounding totem | Baseline | Yes: fear teams vs caster teams | Per-match: the Shaman's missing choice |
| Priest Shadow Protection | Baseline | Yes: only against Shadow casters | Per-match: a candidate Priest choice |
| Shadowform | Shadow capstone in WoW | — | Talent, but it is a role change (no healing), so it is held out of the trees (§5.2) |
| Inner Fire | Baseline | No: always worth casting | Neither: baseline AI behaviour |

The Priest, Shaman and Druid having no per-match choice is a gap the rule exposes. It is not
one for talents to fill. Candidate cards are in FOLLOWUPS.

## 2. Reference games: what to borrow, what to avoid

**Autobattlers.**
- *Teamfight Tactics:* borrow scouting (look at the opponent's board and adjust) and
  round-by-round escalation. Avoid the 8-player lobby and the shop economy.
- *Super Auto Pets:* borrow lives and a win target (you are never out after one bad fight) and
  asynchronous opponents (past teams as snapshots). Avoid opaque single-item rolls; every offer
  here is a choice of three.
- *Hearthstone Arena:* borrow the run shape: draft, play to 12 wins or 3 losses, rewards
  scale with wins. **The closest single template for the run.**
- *Teamfight Manager, Mechabellum:* borrow the draft against a visible opponent and the
  commit-watch-adapt rhythm. Avoid long training grinds between matches.

**Talent progression.**
- *WoW Classic trees:* borrow tiers gated by points spent, arrows (prerequisites), multi-rank
  nodes, a capstone that defines the spec, and the "31/20/0" shorthand a viewer can read.
  Avoid 51 points of +1%. A tree is only worth watching if its nodes act in the fight.
- *Diablo II skill tree:* borrow points arriving on a fixed cadence and planning a path
  toward a payoff skill. Avoid permanent mistakes; here a bad build costs one run.
- *Path of Exile passive tree:* avoid. 1,300 nodes is a game of its own, and no AI or sweep
  can cover it.
- *Slay the Spire, Hades:* borrow a build that lives for one run, and meta-progression that
  adds variety, not power. Avoid Hades' Mirror: permanent stat power makes the first runs weaker
  by design.

**Management and tactics.**
- *Football Manager:* borrow the scouting report ("their left back is weak") and watching the
  match to see whether the tactics worked. Avoid spreadsheet depth.
- *Into the Breach:* borrow **telegraphed intent**. Naming what the enemy will do makes the
  answer the whole skill. Here it is the opponent briefing.
- *WoW arena:* borrow the 2v2/3v3 brackets and the fantasy of a comp learned deeply. Avoid
  gear gaps that decide matches, and rating loss as the only stake.

## 3. The talent system

### Tree shape

Each class has **three trees**, named as in WoW. Each tree has **five to seven nodes in four
tiers**.

| Tier | Opens at (points already in this tree) | Nodes | Ranks per node | Price per rank |
|---|---|---|---|---|
| 1 | 0 | 2 | 2 | 11 |
| 2 | 2 | 1-3 | 1-2 | 11 |
| 3 | 4 | 1-2 | 1 | 11 |
| 4 (capstone) | 6 | 1 | 1 | 22 |

- **Prerequisites** are WoW's arrows. A capstone usually needs one tier-3 node. Classic's own
  arrows are used where they exist (Shadow Mastery needs Siphon Life; Conflagrate needs
  Improved Immolate), and the rest are marked *here*.
- **The capstone is the only node worth two ranks of budget.** It is the spec's identity: a new
  button, a mechanic or the tree's signature number. Reaching it takes 7 points, so in a run it
  arrives around match 7 of about 9: late in the run, against the hardest tiers.
- **A class has about 18 nodes and 27 ranks, and a run pays out 9 points.** So a run spends a
  third of the class. "Deep Demonology with an Affliction splash" and "three trees at three
  points" are both legal, and the class has runs left to discover.

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

**The resolved ability book.** Today about 470 call sites read the global
`AbilityDefinitions`. Talents need *this combatant's* Drain Life. At spawn each combatant
gets an `AbilityBook`: its class's configs, plus granted abilities, with every patch applied.
Damage, healing, aura, cooldown, mana and AI code already take an `&AbilityConfig`, so a site
that fetches it from the book changes behaviour with no other edit. Migration is incremental: a
combatant with no talents gets a book identical to the global table, so unmigrated sites stay
correct, and a `talent_book_audit` test (the shape of `heal_site_audit`) then pins
`AbilityDefinitions::get` out of sim paths that have a combatant to hand. The book is
resolved once at spawn. Anything that changes during a match is an aura.

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
- **Build parity:** the test enumerates **every legal build** at each point count from 1 to 9
  and fails if any is more than 15% from that count's mean. It is cheap: under a second for a
  class. For the two trees in §5 the spread is exactly zero up to 5 points, and −1%/+11% at
  9 (the worst case is +14% at 7, a capstone reached as early as it can be).
- A `Grant` names a `talent: true` ability, and each such ability is granted by exactly one
  node.

### What talents are worth

A rank is 11 budget points; the default Warlock loadout is about 334. A full 9-point build is
about **100**, close to the about **94** a run's gear climb adds (ilvl 57 to about 73 across
the loadout). Talents and gear are two progression axes of about the same size: talents
planned, gear opportunistic. Decision 4 asks whether that share is right.

## 4. Why trees stay tractable

Trees produce many builds: 9,721 legal 9-point Warlock builds against the 27 a set of three
pick-one rows would allow. The three things that made rows attractive (the AI, the sweeps and
legibility) each have an answer.

### The AI copes per ability, never per build

- **Stats and patches need nothing.** The AI already reads cost, cooldown and magnitude from
  the config it is handed; through the book it reads the talented numbers. Improved Psychic
  Scream's shorter cooldown is used without a line of AI code.
- **A granted ability needs one decision rule**, instrumented with `builder.reject` /
  `builder.choose` like any other. A `talent_ai_audit` test fails until every `talent: true`
  ability is reachable from its class's `decide_action`. The Warlock trees grant four
  buttons (Amplify Curse, Siphon Life, Shadowburn, Conflagrate); the Priest's grant five.
- **A proc that changes the next cast needs one priority line.** Shadow Trance up means Shadow
  Bolt next.
- **A widened menu needs nothing new in the AI.** The player picks Curse of Exhaustion in
  Prepare; the AI casts the curse it was given, as it does today.
- **Nodes do not talk to each other through the AI.** They interact only through auras the
  AI already reads, so the AI cost of a class's tree is the sum of its nodes, not a product.
  Build-level strategy (keeping the Felhunter alive under Soul Link) is later work, gated
  behind an `AiProfile` like every new behaviour.

The trace header records the build, so a trace explains itself.

### Sweeps measure nodes and reference builds

No sweep tries to cover 9,721 builds. Four instruments do the work, and each is sized by
the tiers in `sweep-tiers.md`.

1. **The price model** (`cargo test`): build parity above. It bounds what any build can
   *cost*. The sweeps then test whether price predicts power.
2. **Reference builds.** Three per class, one deep in each tree at 9 points, written in
   `talents.ron`. They are what authored opponents carry, what the bot's default policy plays,
   and what the milestone sweep reports a class's standing *with*: at 0 points (today's
   baseline, byte-identical) and at each reference build. That is three arms per class, not
   thousands.
3. **Node arms**, DIRECTIONAL, for the nodes that carry risk: every grant, proc and mechanic.
   Each arm is a reference build with its last point moved onto that node, paired at identical
   seeds against the reference. The success condition is that no node is dominant: none moves
   its class by more than the tier's resolution floor beyond a stat rank of the same price.
   `Stats` and plain `Patch` nodes are priced by formula and covered by the reference arms.
4. **The Gauntlet bot** (§6.7) plays runs with random legal builds and with greedy builds.
   That measures the **spread of the build space** directly: if some build wins runs far more
   often than the reference builds, the bot finds it, and its nodes become node arms.

### Legibility: every node leaves a footprint

The rule: **a node must show in the match** as an ability cast (which
reaches the timeline), an aura with its icon in the buff bar (procs carry `source_talent`), or
a logged event. A `Stats` or `Patch` node shows before the match instead, in a tooltip that
reads the book's patched numbers. Nodes that leave no trace in this sim are cut: Classic's
Suppression (no spell hit roll), Healing Focus and Fel Concentration (no pushback), and every
+1% that a viewer cannot see.

On top of that, for a viewer:

- **A spec label.** The tree with the most points names the spec ("Demonology Warlock"),
  with WoW's shorthand ("1/8/0") under it, on the team frames and the Results screen.
- **Talent icons on the frame:** only the nodes that act in the fight (grants, procs and
  mechanics), at most about four a combatant.
- **The briefing names the enemy's talents as threats** (§6.4).
- **A `[TALENTS]` line at spawn**, beside `[EQUIPMENT]`, and the build in the replay config.

## 5. Two worked classes

**Scale.** Classic rank-max Corruption is 822 damage; ours is 60. So a Classic damage number
scales by about **0.073**. Cooldowns longer than a match are cut by the Death Coil precedent:
2 min becomes 30s, 3 min becomes 45s. Where Classic's per-rank value fits our budget, it is
used unchanged.

**Sources.** Every Classic tooltip below was looked up on the Wowhead Classic MCP this
session; the link is the spell ID it returned, and the value quoted is that rank's.
**Tier placements and Classic's arrows are from the 1.12 tree layout as remembered**: the
MCP returns spell tooltips, not tree positions. **TBC** marks anything taken from TBC.

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

Shadow Mastery is the one capstone in these trees that is a number rather than a button.
It is what the user named and what Classic put there. If every capstone must act visibly
in play, Dark Pact is Classic's alternative 31-point node, but it needs Felhunter mana the
sim does not model.

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

**Three Warlocks that read differently.**

- **Demonology (1/8/0):** sturdier, with a linked Felhunter, or a sacrificed one when the
  match calls for it. Answers the "dies first" failure.
- **Affliction (8/0/1):** Siphon Life and drains keep it alive by attrition; Shadow Trance
  turns a DoT spread into instant Shadow Bolts.
- **Destruction (0/2/7):** Shadow Vulnerability windows, Conflagrate and Shadowburn as burst.

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

TBC's Pain Suppression (−40% damage taken for 8s on an ally) is the alternative capstone.
It is the stronger arena button, and decision 6 covers TBC sources.

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
spells, so it turns a healer into a damage dealer. That is a role change, not progression, and
the AI has no Shadow-damage role to switch into. Its place is a later card if the user wants a
damage Priest.

**Two Priests that read differently.** *Discipline (8/1/0)* shields, resists the fear and
hands its partner Power Infusion at the kill window. *Holy (1/8/0)* heals bigger, protects
with Inspiration and keeps healing for 8s after it dies.

## 6. The Gauntlet, reinterpreted around talents

### 6.1 The run

1. **Draft.** Classes are offered, at least one healer among them. Pick two. Each starts in its
   default loadout (ilvl 54-60) with **0 points spent and 1 point to spend**.
2. **Hub.** The record as pips (wins out of 7, losses out of 3), your team with spec labels and
   gear, and the next opponent's **briefing**.
3. **Talents.** If a combatant has an unspent point, the hub shows it. Spend it, or bank it.
4. **Prepare.** The per-match choices, pre-filled with last match's, against the briefing.
5. **Fight.** PlayMatch exactly as today, kill calls included. Results as today, plus
   **Continue run**.
6. **Point and reward.** Each combatant gains **1 talent point**, win or loss. Then pick
   **one of three gear rewards**, equipped at once.
7. **Run end** at 7 wins or 3 losses: the record, the final team with its builds, and a
   **Watch replay** link for each match.

### 6.2 Earning and spending points

**A point after every match, for each combatant.** Not at the reward pick, and not only at
win milestones.

- **Talents are the planned axis, gear the opportunistic one.** A point every match makes
  the capstone a goal a player can plan toward and watch approach. A gear offer is a draw of
  three. If points were a reward option, every pick would collapse into "one point or one
  item", a single currency comparison that would kill both kinds of excitement.
- **Win or loss.** A loss buys a point against the *same* tier, because opponent tier follows
  wins and points follow matches. That is the run's catch-up, and it means a run never feels
  stalled after a defeat.
- **At most 9 points.** A cleared 7-2 run plays 9 matches and spends 9 points (1 at the draft,
  8 after the first eight matches; the point from the last match is not spendable). A 7-0 run
  spends 7. The capstone (7 points in one tree) arrives around match 7.
- **Spent points stay spent for the run.** That is the line between a commitment and an
  answer, made mechanical: the per-match choices are where the player adapts. Banking a point
  is allowed, to save it for a node a few matches away. Decision 2 asks about respec.
- **The run starts from zero.** No meta-progressed base: the first match of every run is
  played at 1 point, so experimenting with a new class costs nothing.

### 6.3 Opponents carry builds

An opponent at tier *t* carries exactly **t points per combatant**: the fewest a player can
have at that tier. So a player is never behind in points, and is ahead after a loss.

- Authored opponents use a class's reference builds, or a named variation of one, cut to their
  point count by spending in the reference order.
- **A class without a tree yet carries its points as item budget.** Talents are priced on the
  item budget, so *t* points are worth *t* × 11 budget in gear. Opponents can therefore be any
  of the nine classes from the first slice, before five of them have trees.
- Opponent tiers are graded by sweep against reference teams; points are one more input.

### 6.4 The per-match choices keep their role: the briefing

The briefing is where per-match choices earn their place. It shows:

- the opponent's classes, map, AI profile and average item level;
- their **spec labels** ("Affliction Warlock 4/0/0") and the talent icons that act in a fight;
- **threat lines** generated from their kit, their per-match choices *and their talents*:
  "Siphon Life heals their Warlock: dispel it", "their Priest resists fear (Unbreakable
  Will)";
- **your answers** with the current Prepare settings, recomputed as you change them: "Curse of
  Tongues slows their healer's casts", "the Felhunter can devour Polymorph off your healer".

Talents feed the briefing from both sides. A talent can make an answer possible (Curse of
Exhaustion against a kiting Hunter, a Sacrifice against a melee cleave) or better (Improved
Curse of Agony), but choosing that answer happens in Prepare, every match, for free.

### 6.5 The reward pick

Three gear offers from the run's current ilvl band, rising with wins, each
shown against what it replaces. Proc trinkets are in the pool. Gear goes to whichever combatant
the item fits.

### 6.6 What persists between runs

**Nothing that adds power.** A losing run costs only the run; that is the design doc's
"little to no long-term penalties" and what keeps experimenting the default.

What persists:

- **Records:** best result per class, per pair and per spec.
- **Run history:** each match as a replay config, a few hundred bytes each.
- **Saved talent plans:** a named path through a class's trees. In a run, the talent step
  proposes the plan's next node, and one click takes it. That is convenience, not power.
- **Ghosts (later):** a finished run's team, build and gear become an opponent at the tier it
  reached, which is Super Auto Pets' asynchronous model with no server.

### 6.7 Determinism and tooling

- **A run is event-sourced:** the run seed plus the list of choices (draft, points, Prepare
  settings, rewards, kill calls) reproduces every offer, opponent and match. Offers come from
  their own RNG stream, never `GameRng`.
- **A bot plays the Gauntlet headlessly** (`--gauntlet <seed> --policy <name>` under
  `--batch`). Its policies now pick talents too: follow a reference build, random legal, or
  greedy by node price. Hundreds of runs give the wins-per-run distribution, the walls in the
  ladder, the dominant pairs and the spread of builds.
- **Post-match odds (later):** after a loss, twenty background seeds of the same matchup say
  whether the loss was the dice or the setup.

## 7. A worked run: Warlock and Priest

The player drafts the Warlock and the Priest and aims the Warlock at **Demonology** (the
survival tree, against its "dies first" failure) and the Priest at **Discipline**. The ladder
is the authored one. "Pts" is points available before the match.

| # | Opponent (tier, map) | Pts | Points spent | Per-match choices | Result | Reward |
|---|---|---|---|---|---|---|
| 1 | Gate Rats: Warrior + Priest (1, BasicArena) | 1 / 1 | Wl: Demonic Embrace 1. Pr: Imp. PW:Shield 1 | Kill the Priest. Curses: Weakness on the Warrior, Tongues on the Priest. Felhunter | Win | Arcanist Robes (Wl) |
| 2 | Dustcloaks: Rogue + Druid (2, Nagrand) | 2 / 2 | Wl: Fel Stamina 1. Pr: Unbreakable Will 1 | Kill the Druid. Agony on the Druid, Weakness on the Rogue | Win | Pendant of Clarity (Pr) |
| 3 | Ashen Pair: Mage + Shaman (3, TwinPillars) | 3 / 3 | Wl: Fel Stamina 2. Pr: Unbreakable Will 2 | Felhunter to devour Polymorph. Tongues on the Shaman | **Loss** | Arcanist Crown (Wl) |
| 4 | Cinder Pact: Mage + Priest (3, BasicArena) | 4 / 4 | Wl: Unholy Power 1. Pr: Mental Agility 1 | Kill the Priest; Tongues on it | Win | Ring of Arcane Insight (Pr) |
| 5 | Thornwatch: Warlock + Priest (4, Nagrand) | 5 / 5 | Wl: Master Demonologist. Pr: Mental Agility 2 | Spell Lock their Priest; Tongues on it, Weakness on their Warlock | Win | Arcanist Leggings (Wl) |
| 6 | Stormhold: Hunter + Paladin (5, Nagrand) | 6 / 6 | Wl: **Demonic Sacrifice** (opens Sacrifice). Pr: **Inner Focus** | Keep the Felhunter: Spell Lock answers the Paladin's heals. Tongues on the Paladin | **Loss** | Runestaff of Elements (Pr) |
| 7 | Greycloaks: Hunter + Priest (5, BasicArena) | 7 / 7 | Wl: **Soul Link** (capstone). Pr: **Power Infusion** (capstone) | Felhunter (Soul Link needs it). Weakness on the Hunter: this Warlock has no Curse of Exhaustion, and the briefing says the kiting answer is missing | Win | Wand of the Invoker (Wl) |
| 8 | Gilded Fangs: Rogue + Priest (6, Nagrand) | 8 / 8 | Wl: Demonic Embrace 2. Pr: Imp. PW:Shield 2 | Felhunter kept for Soul Link against Rogue burst. Weakness on the Rogue | Win | Cloak of Wisdom (Pr) |
| 9 | Champions: Warrior + Paladin (7, BasicArena) | 9 / 9 | Wl: Improved Drain Life 1 (Affliction). Pr: Spiritual Healing 1 (Holy) | **Sacrifice the Felhunter** (+15% Shadow; Soul Link off): two melee will kill it anyway, and Spell Lock matters less against a Paladin who stands in melee. Weakness on the Warrior | Win | — |

**Final record 7-2, Gauntlet cleared.** Final builds:

- **Warlock 1/8/0, Demonology:** Demonic Embrace 2, Fel Stamina 2, Unholy Power 1, Master
  Demonologist 1, Demonic Sacrifice 1, Soul Link 1; Improved Drain Life 1. Price 110.
- **Priest 8/1/0, Discipline:** Unbreakable Will 2, Improved PW:Shield 2, Mental Agility 2,
  Inner Focus 1, Power Infusion 1; Spiritual Healing 1. Price 110.

What the run shows:

- **The two losses gave points against the same tier.** Match 4 is a second tier-3 opponent
  faced with 4 points instead of 3.
- **The capstones arrived at match 7**, against the tier-5 opponents.
- **Talents widened a per-match decision without making it.** From match 6 the Warlock could
  sacrifice the Felhunter, and the player kept it twice and gave it up once.
- **Opponent points:** tier 1 had 1 point a combatant; the tier-7 Champions had 7, against
  the player's 9.

## 8. Minimum playable slice

**The first slice includes talents.** Without them the Gauntlet's only progression is a gear
draw, and the reason for reinterpreting the loop is that the tree is its spine. To keep the
slice small, it narrows the *class pool*, not the system.

1. **Main menu → Gauntlet.** One active run, resumable. Custom Match stays the sandbox, and
   gains the talent panel.
2. **Draft three of four:** Warlock, Priest, Rogue and Paladin, the four classes with trees.
   Two healers in a pool of four means every offer of three has a healer. Six possible pairs,
   five of them with a healer.
3. **Hub** with the briefing, **Talents** (the tree screen, one point after every match),
   **Prepare**, **Fight**, **Reward**, **Run end**, as in §6.1.
4. **Opponents** from all nine classes, about 20 in 7 tiers. The five classes without trees
   carry their points as item budget (§6.3).

**Out of the slice:** 3v3, the other five trees, saved plans, ghosts, daily seeds, post-match
odds, and meta unlocks.

The alternative (decision 3) is a slice with all nine classes, gear only, and talents as
phase 2.

**What the slice needs from AS-251 (the UI design system):** a choice card (draft offers and
rewards), a team card with spec label, a pip track, an intel panel with linked icons, an item
comparison tooltip, a hub template, and **a talent tree component**: three columns of
tier-gated nodes with rank pips, arrows, a locked state and a "spend" affordance. Exit from a
run means save and leave; abandoning is its own confirmed action.

## 9. Phased plan

Cards in dependency order. Each is shippable and keeps `main` green. **Every card before the
slice leaves the default build empty, so `Legacy` and BasicArena stay byte-identical and
`determinism_pin` holds.**

**Phase 0: talent engine and run foundations (no new screens).**

| Card | Delivers |
|---|---|
| T1 Talent engine | `talents.ron` (trees, tiers, ranks, prerequisites) with a validating loader; `Build` per slot in `MatchConfig` and headless; the `AbilityBook` and the migration of the cast, channel, aura-tick and Warlock AI paths; rank and build parity tests; the `[TALENTS]` log line and the build in the trace header. Content: Warlock Demonology tiers 1-2 (Stats and PetStats only) |
| T2 Warlock trees | The rest of §5.1: the grants and their AI rules, the `leech` aura field, the procs (`DotTick`, `SpellHit`, `NextCastInstant`), `WidenChoice` for Curse of Exhaustion and Sacrifice, Soul Link. `talent_ai_audit`. May split by tree |
| T3 Priest trees | §5.2: Inner Focus (`NextCastFree`), Power Infusion, Holy Nova, Vampiric Embrace, Silence; `CritTaken` / `HealCrit` procs; Unbreakable Will's resist roll; Spirit of Redemption |
| T4 Rogue trees, T5 Paladin trees | Designed in their own cards under this doc's rules (the §1 rule, the tier shape, prices, the footprint rule) |
| T6 Talent panel and encyclopedia | The tree in View Combatant and Custom Match (9 points); `Topic::Talent` and a Talents section; spec labels on frames and Results; build in the replay config |
| G1 Run model | `RunState`: seed, roster, gear, builds, record, choice history, on its own RNG stream; save is seed plus choices |
| G2 Gauntlet data | `gauntlet.ron`: run rules, opponents by tier with builds or budget-equivalent gear, reward tables; validated at load |
| G3 Recorded kill calls | Kill calls with sim timestamps in the match record; a replay reproduces the match |
| G4 Headless driver | `--gauntlet <seed> --policy <name>`, with gear and talent policies |
| G5 Calibration | DIRECTIONAL: reference-build arms per class, node arms for every grant, proc and mechanic, bot runs for the wins-per-run curve and build spread. Output: tiers and reward tables in a findings doc |

**Phase 1: the minimum playable slice.**

| Card | Delivers |
|---|---|
| G6 Entry and draft | Main-menu entry, draft screen, starting a run |
| G7 Hub, briefing and Prepare | The hub, the briefing with talent-aware threat and answer lines, Prepare reusing the pre-match widgets |
| G8 Talent step | The tree screen in a run: spend or bank, locked once spent |
| G9 Rewards and run end | Gear reward with comparison; run end with builds and replay links; Continue run on Results |
| G10 Save, resume, abandon | Resume after a quit; a confirmed abandon; a UI script drives a short run as a smoke test |

Milestone review: **play three runs.**

**Phase 2: breadth.** The other five classes' trees, a card each, in the order the slice's
players ask for them. 3v3 runs. Map draws shown in the briefing. Saved talent plans. Daily seed.
Run history. Post-match odds. Per-match choices for the Priest, Shaman and Druid, if the user
wants them.

**Phase 3: depth.** Build-aware AI behind an `AiProfile` (the Felhunter as a Soul Link
sponge). Ghost opponents. The Stable (named combatants carried between runs) and
Counter-Pick Trials (authored puzzles against a fixed enemy team), which reuse the opponent
format.

**The first card:**

> **Talents 1: the engine and Demonology's first tiers.** Add `assets/config/talents.ron`
> with Warlock trees and a validating loader, `Build` per slot in `MatchConfig` and the
> headless config, and a per-combatant `AbilityBook` built at spawn, migrating the cast,
> channel, aura-tick and Warlock AI reads. Ship Demonic Embrace, Fel Stamina, Unholy Power and
> Master Demonologist (Stats and PetStats only). Add the rank and build parity tests and the
> `[TALENTS]` log line.
>
> **Success condition:** empty build by default, so `Legacy` and BasicArena are
> byte-identical. A DIRECTIONAL paired sweep of the 4-point Demonology build against none on
> the Warlock's 2v2 cells says which way and roughly how much it moves the Warlock.

## 10. Decisions for the user

1. **Point cadence.** One point per combatant after every match, win or loss, as proposed? Or
   only after wins (a snowball, no catch-up), or as one option at the reward pick?
2. **Respec.** Spent points locked for the run, as proposed? Or a free respec between matches,
   which blurs talents into per-match choices?
3. **The first slice.** Talents in the slice with a four-class pool (Warlock, Priest, Rogue,
   Paladin), as proposed? Or all nine classes with gear only, and talents as phase 2?
4. **Run size and power share.** 9 points a run, a capstone at 7, talents worth about as much
   as the run's gear climb (about 100 budget points each)? Or fewer, smaller points?
5. **Baked-in talents.** Mortal Strike, Ice Barrier, Holy Shock, Swiftmend, Unstable
   Affliction and instant Corruption stay baseline, as proposed, so no class loses a button?
   Or move them into the trees as capstones, which is more faithful and re-baselines every
   class at once?
6. **TBC sources.** Talents may draw on TBC values where Classic's do not fit (Curse of
   Exhaustion's 30%, Pain Suppression)? The project's rules intuition is TBC.
7. **Meta-progression.** None in power, as proposed: records, replays and saved plans only?
   Or Hades-style unlocks that add variety (a new reference build, a new opponent tier)?
