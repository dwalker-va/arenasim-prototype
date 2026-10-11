# A gameplay loop: from sandbox simulator to game

**Card:** AS-253. **Status:** proposal, for the user to judge. Nothing here is implemented.
**Siblings:** AS-252 (talents, `2026-10-11-talents.md`) and AS-251 (UI design system,
`2026-10-11-ui-design-system.md`). Section 7 says what this loop needs from each.
**Prototype:** [`docs/design/benches/2026-10-11-gauntlet-prototype.html`](../benches/2026-10-11-gauntlet-prototype.html)
is a click-through of the recommended loop's screens. Open it from the checkout so the class
and item icons resolve. Its fight is a scripted stand-in with a weighted coin-flip outcome, not
the sim, and its styling is deliberately neutral. Judge the flow and the decisions.

## Summary

ArenaSim today is *configure, battle, results, repeat*. That is the short-term concept the
game design doc set out, and it is finished. Each match is self-contained: nothing carries
over, nothing is at stake, and no decision made in one match affects the next. That is why it
reads as a sandbox.

This doc proposes four loops and recommends one:

| | Concept | One line |
|---|---|---|
| A | **Arena Season** | A rated ladder with a fixed roster and a gear vendor, like WoW arena |
| B | **The Gauntlet** (recommended) | Draft a team, then win as many matches as possible before your third loss, upgrading between fights |
| C | **The Stable** | The game design doc's long-term vision: manage named combatants across a career of tournaments |
| D | **Counter-Pick Trials** | Authored puzzles: beat a fixed enemy team under constraints |

**The recommendation is B, the Gauntlet.** It is the smallest loop that produces stakes and a
decision between every match. It reuses nearly every system the game has, and its pieces
(team card, opponent intel, rewards, run record) are the parts C needs later. So the phased
plan ends at the Stable rather than replacing it.

**Minimum playable slice:**
- 2v2 only.
- Draft two classes from four offered.
- Fight a visible, escalating opponent until 7 wins or 3 losses.
- After every match, pick one of three gear upgrades.

It needs no talents and no new combat mechanics. Its new parts are a run model, an opponent
and reward table in RON, and four screens.

---

## 1. What the game already has

The aim is to treat each existing subsystem as a building block and find a loop that gives
each one a job. The **Role in a loop** column is what that block can carry.

| Subsystem | What exists | Role in a loop |
|---|---|---|
| **Nine classes** | Warrior, Mage, Rogue, Priest, Warlock, Paladin, Hunter, Shaman, Druid. Four healers. Distinct kits, posture-driven movement AI and class-specific threats (Polymorph, Fear, Cyclone, stealth openers, traps, totems). | The draft pool. Nine classes give 36 2v2 pairs and 84 3v3 trios before any other choice. |
| **Pets** | Hunter picks Spider, Boar or Bird. Warlock has the Felhunter (Spell Lock, devours magic). | A counter-pick lever: Bird against roots, Spider against kiters. |
| **Pre-match choices** | Rogue opener (Ambush / Cheap Shot) and poison, Warlock curse per enemy slot, Warrior shout, Mage armor, Paladin aura, kill target and CC target. All live in `MatchConfig` and in the headless config. | The per-match decision. Today they are set once and forgotten; a loop that shows you the next opponent turns them into answers. |
| **Items** | 159 items in `items.ron` across 15 slot kinds, priced by an item-level budget enforced by `cargo test`. Two tiers: ilvl 54-60 (the default loadouts) and ilvl 69-75 (Warlord's, Judgement, Giantstalker, Arcanist...). Eight trinkets, four with procs. Loadouts in `loadouts.ron`; View Combatant has an equipment picker. | Progression. The two-tier split is already a natural upgrade path from the starting kit to the high tier. The budget keeps every reward honest. |
| **Maps** | BasicArena (open), TwinPillars (two cylinders), PillaredArena (the Nagrand replica). TestVerticality is a test asset. | Variety per round, and a scouting fact ("the next fight is on Nagrand: pillars favour the healer"). |
| **Autobattler matches** | Deterministic seeded combat, 10s gate countdown, dampening from 75s, dying-blow draws. A 2v2 runs roughly 30-95s of combat (healer-mirror medians, AS-164). Speed control 0.5x-3x. | The minute-to-minute content. Short enough that a run of several matches fits one sitting. |
| **AI profiles** | `Legacy` (default) and `TeamPlan` (team positioning, a measured edge on Nagrand). Per-team. | A difficulty knob for opponents, measured by the head-to-head sweep, not guessed. |
| **Live kill call** | Click an enemy team frame mid-match to set your team's kill target. Graphical only. | The one in-match input. It gives the player something to do at the moment that matters. |
| **Banter** | Pictographic team chat on kill-call changes and openings, with a per-class personality matrix. | Identity. In a loop with named combatants, banter is how they come across as characters. |
| **Encyclopedia** | Every class, ability, aura and item, generated from the RON, with search and linked icons. | The reference that makes a draft offer readable. Any offer can link to its page. |
| **Results and combat log** | Per-combatant damage, healing and CC breakdown (Details-style), the combat log, the match log file. | The post-match "why" — did the counter-pick work. |
| **Replay** | `--replay <config.json>` boots the client straight into a seeded match. A whole match is reproducible from a small JSON. | Run history and shareable runs, at the cost of a few hundred bytes per match. |
| **Headless sim and sweeps** | `--headless`, `--batch` (1-2.5 matches/sec), `gen_sweep.py`, `paired_sweep.py`, `headtohead_sweep.py`, decision traces. | Content QA: grading opponents, measuring the difficulty curve, and possibly showing the player the odds after a match. No other game in the reference list has this. |
| **Talents** (AS-252, in ideation) | Proposal under way. It folds the existing pre-match choices into a RON talent system. | A second progression axis beside gear, and a build identity. |

Two properties of the codebase shape every concept below.

- **A team is already data.** `HeadlessMatchConfig` carries a side's classes, pre-match choices
  and full equipment. So an opponent, a saved run team, a puzzle and a replay are the same
  JSON. Adding the 50th opponent costs one entry, not code (see *prefer zero-marginal-cost
  content* in project memory).
- **A match is a pure function of its config and seed.** That makes runs, daily seeds and
  replays cheap. The live kill call is the one exception, because it is an input arriving
  mid-match. Any loop that records matches must record calls with their sim timestamps.
  Phase 0 has a card for this.

## 2. Reference games: what to borrow, what to avoid

### Autobattlers

**Teamfight Tactics.**
- *Borrow:* scouting. Before a round you can look at opponents' boards and adjust, which
  makes the automatic fight a test of a decision made in advance. Also: round-by-round
  escalation, and losses that cost health rather than ending the game outright.
- *Avoid:* the 8-player lobby, the interest economy and the shop-reroll economy. They are
  a game of their own layered over the fight, and here the fight is the point.

**Super Auto Pets.**
- *Borrow:*
  - *Hearts and wins.* The run ends at 10 wins or when the lives run out. You are never
    eliminated after one bad fight, and every fight still matters.
  - *Asynchronous opponents.* Your opponent is a snapshot of another player's team at the
    same round. With a deterministic sim this works locally first: your own past run teams
    can become future opponents, with no server needed.
- *Avoid:* opaque shop randomness. SAP's worst moments are rolls with no answer. Every
  offer here should come as a choice of three, never a single forced item.

**The Bazaar.**
- *Borrow:*
  - *Builds that come from items.* A run's identity comes from what you collected.
  - *Visible encounters.* You see the opponent's board before committing. Our two gear
    tiers and four proc trinkets can carry a light version of this.
- *Avoid:* item-timing synergy as the main skill. The sim already resolves combat in fine
  detail. A second timing system on top would be noise.

**Hearthstone Battlegrounds and Hearthstone Arena.**
- *Battlegrounds — borrow:* hero powers as run-defining starts. A Phase 4 idea: a "captain"
  perk chosen at run start.
- *Arena — borrow:* draft from three offers, play until 12 wins or 3 losses, reward scales
  with wins. **This is the closest single template for the recommendation.**
- *Avoid:* Battlegrounds' 8-player tavern economy, for the same reason as TFT.

**Teamfight Manager and Mechabellum.**
- *Teamfight Manager — borrow:* an esports manager built on auto-resolved fights. A pick-and-ban
  draft against a visible opponent turns team composition into the game.
- *Mechabellum — borrow:* the rhythm of committing, watching, then adapting the next round to
  what you just saw.
- *Avoid:* Teamfight Manager's long between-match training grind.

### Management games

**Football Manager.**
- *Borrow:*
  - *The scouting report:* "they press high; their left back is weak".
  - *The match engine viewed as highlights.* The player watches to see whether the
    tactics worked.
  - *Named people the player grows attached to.*
- *Avoid:* spreadsheet depth, and seasons that take weeks. ArenaSim's appeal is a short
  match you can read.

**Darkest Dungeon.**
- *Borrow:*
  - *The roster as a stable.* Named heroes with quirks: our banter personalities are
    already quirks.
  - *The hamlet as meta-progression.*
  - *Pre-mission party composition against a known dungeon.*
- *Avoid:* permadeath and stress spirals. The game design doc asks for "little to no
  long-term penalties for losing games", and losing a hero you invested in is the opposite.

### WoW arena

**WoW arena itself.**
- *Borrow:*
  - The 2v2/3v3 brackets.
  - *Gear bought with a currency earned by playing.* Honor and arena points, with the top
    pieces gated behind rating.
  - Seasons with titles (Gladiator).
  - The fantasy of a team that learns one comp deeply.
- *Avoid:* **gear gaps that decide matches.** Fresh players facing full-geared ones was the
  classic complaint. Also the pressure of losing rating, which works against the
  design doc's "experimentation without penalty".

### Roguelites

**Slay the Spire, Hades, Into the Breach.**
- *Slay the Spire — borrow:* pick one of three rewards after each fight, and a run that is
  a sequence of small, legible decisions.
- *Hades — borrow:* meta-progression that unlocks *variety*, not raw power.
- *Into the Breach — borrow:* **telegraphed enemy intent.** Showing exactly what the enemy
  will do makes the player's answer the whole skill. Here it becomes opponent intel that
  names the threat: "their Rogue opens Cheap Shot into Kidney Shot on your healer".
- *Avoid:* unlocks that are mandatory power. If the first ten runs are weaker by design,
  experimentation is punished.

## 3. Concept A: Arena Season

*A rated ladder with a fixed roster and a gear vendor. The WoW arena fantasy, played offline.*

**Core loop.**
- *Minute to minute:* set up the team against the shown opponent, watch, make kill calls.
- *Session:* play five to ten ladder matches. Rating moves; arena points are earned and spent
  at the vendor on high-tier pieces.
- *Meta:* a season of fixed length ends; rating sets a title and a cosmetic reward. The next
  season resets rating but keeps gear.

**Player decisions.** Which comp to commit to for the season, and when to change it. Which
piece to buy next (gear is the progression). Pre-match choices against each opponent.

**What the player watches for.** Whether their comp holds up against the comp the ladder
put in front of them, and whether the new piece helped.

**Progression and stakes.** Rating is the stake: a loss costs points. Gear is permanent within
a season. Ladder opponents are drawn near the player's rating from an authored pool of AI
teams, graded by sweep.

**Reuse.** Items and loadouts, View Combatant, every pre-match choice, the results screen.
**New:** a rating model, a vendor screen, a currency, a season calendar, a persistent save,
and an opponent pool with ratings.

**Determinism and tooling.** Opponent ratings can be *measured* rather than authored. A
headless round robin of the pool gives each team an Elo before a player ever sees it.

**Verdict.** It is a faithful fantasy and the gear vendor uses the item tiers well, but its
stakes come from losing rating, and its progression is mostly gear power. Both work against
the design doc. A player who loses ten in a row has no new decision to make; they wait for
points. Best reused later as a *mode* inside the Stable.

## 4. Concept B: The Gauntlet (recommended)

*Draft a team, then win as many matches as you can before your third loss. Upgrade between
fights. Hearthstone Arena's structure, Into the Breach's intel, Slay the Spire's rewards.*

**Core loop.**
- *Minute to minute:* read the next opponent's intel card. Set pre-match choices against it
  (kill target, CC target, Hunter pet, Warlock curses, Rogue opener). Watch the fight and
  make kill calls when the moment comes.
- *Session (one run, 15-25 minutes):* draft two classes from four offered. Play until 7 wins
  or 3 losses. After every match pick one reward from three. The run ends on a summary
  (record, team, key moments) with replays of each fight.
- *Meta:* best record per class and per comp, a run history, and later (Phase 2) a daily
  seed and unlocks that add variety, not power.

**Player decisions.**
1. *The draft:* a pair that covers threats, against the uncertainty of what is coming.
2. *Per-match setup:* answering the opponent shown. This is where the existing pre-match
   choices finally matter.
3. *The reward:* which combatant to upgrade and what to give up. A high-tier chest or a proc
   trinket? Spend on the healer's mana or the DPS's damage?
4. *Kill calls:* switching targets live when the enemy healer is crowd-controlled.

**What the player watches for.** Whether the *answer they chose* lands. Did the Felhunter eat
the Polymorph the intel warned about? Did the Bird's Master's Call free the Warrior from the
roots? Did the new trinket proc in the burst window? The intel names the threat before the
match, so the match becomes a test of a prediction rather than a light show. The kill call
gives the player a moment of agency inside the fight.

**Progression and stakes.**
- *Within a run:* gear climbs from the ilvl 54-60 starting kit toward the 69-75 tier.
  Opponents climb in tier, AI profile and gear in step, so the run gets harder as it gets
  stronger.
- *Stakes:* the three losses. Every match matters; no single match ends the run.
- *Across runs:* nothing carries in power. Losing a run costs only the run, which matches
  "little to no long-term penalties" exactly. Experimenting with a new pair is the default
  behaviour, not a risk.

**Reuse.**
- PlayMatch, Results, the combat log and the kill call, unchanged.
- The pre-match option widgets from Configure Match.
- The equipment picker and item tooltips from View Combatant.
- Encyclopedia links from every offer.
- `HeadlessMatchConfig` as the opponent format.
- The settings directory for the save file.
- `--replay` configs for run history.

**New:**
- A `RunState` model: seed, roster, gear, record and the choice history.
- `assets/config/gauntlet.ron`: an opponent pool by tier, reward tables, and run rules.
- Four screens: draft, run hub with opponent intel, reward pick, and run end.
- A main-menu entry.
- Recorded kill calls.

**Determinism and tooling.** This is where the concept is strongest.
- *A run is event-sourced.* The run seed plus the list of choices reproduces every offer,
  every opponent and every match. The save file is small, a crash loses nothing, and a
  *daily seed* is free: everyone gets the same offers and opponents.
- *A bot can play the Gauntlet headlessly.* Give it a simple policy (always take the highest
  ilvl, kill target the healer) and run a few hundred runs through the batch runner. That
  gives the **difficulty curve as a measurement**: the distribution of wins per run, which
  tier opponents are walls, and which draft pairs over-perform. Balance work stops being
  only class standings and becomes "is this mode fair and fun to climb".
- *Opponents are graded, not guessed.* Each opponent is swept against reference teams. Its
  tier comes from its measured win rate.
- *Post-match odds* (Phase 2). After a loss, a background batch of the same matchup (twenty
  seeds, a few seconds on spare cores) can say "your team wins this 64% of the time: that
  one was the dice", or "18%: the setup was the problem". No other autobattler can tell the
  player honestly whether they were unlucky.

**Risks.**
- *Dominant pairs.* Warrior+Paladin is 77.5% in the AS-164 baseline. Drafting from four
  random offers means it is not always available, and a roguelite tolerates strong picks.
  The bot measures whether any pair trivialises runs.
- *Healer draws.* 1v1 is excluded because healer mirrors draw at the cap. 2v2 opponents can
  be authored to avoid the Druid-mirror endgame.
- *The gear gap is unmeasured.* The power difference between the two item tiers has never
  been swept. Phase 0 measures a fully upgraded team against the default loadout before the
  reward tables are tuned.

## 5. Concept C: The Stable

*The game design doc's long-term vision: "You are the manager of a fledgling stable of
combatants... compete in tournaments in order to obtain the resources necessary to improve
your team."*

**Core loop.**
- *Minute to minute:* as in B. Set up, watch, call.
- *Session:* one tournament on the calendar. Enter teams from your stable, play a bracket,
  earn gold and reputation. Between tournaments, recruit, train (talents), buy gear and
  scout rival stables.
- *Meta:* a career. The stable grows from a few poor combatants to championship teams.
  Rival stables have named teams and histories. Seasons end in a championship.

**Player decisions.**
- Who to recruit, and with what traits.
- Which combatants to pair, since synergies and personalities differ.
- Where to spend gold: gear, training or recruiting.
- Which tournaments to enter: the rules, bracket size and map pool differ.
- How to counter a known rival: scouting reports from their past matches.

**What the player watches for.** As in B, plus attachment. These are *your* people, with names,
banter voices and records. A rival you have lost to twice is a story.

**Progression and stakes.** Persistent and deep: combatants gain talents, gear and reputation.
Stakes are economic (prize money) and narrative (rivalries, championships). Losses cost
opportunity, not combatants (no permadeath).

**Reuse.** Everything B uses, plus banter personalities as character identity, talents as
training, and the AI profiles as rival skill levels. **New:** a persistent career save,
a calendar, an economy, recruitment, rival stables and their simulation (matches the player
does not watch run headlessly), and a talent progression model.

**Determinism and tooling.** Unwatched matches between rival stables run headlessly at batch
speed, so the world advances honestly. Scouting reports can be generated from real sim runs
of the rival's team. The economy needs sweep-backed tuning, which is a new kind of balance
work.

**Verdict.** This is the destination, and it is the right one: it is what the game design doc
asked for. But its minimum fun slice is large. A manager game is only fun once the economy,
recruitment and rivals exist together, and the economy has no known shape until real players
show which decisions they enjoy. **Build B first and treat it as the tournament unit of C.**
A Stable tournament is a Gauntlet run with a persistent roster.

## 6. Concept D: Counter-Pick Trials

*Authored puzzles. "This Warlock+Priest has beaten every team sent against it. Here are three
classes and 40 gear points. Beat it in a best-of-five."*

**Core loop.**
- *Minute to minute:* study the enemy, build an answer, watch a best-of-five on fixed seeds.
- *Session:* clear a handful of trials and earn one to three stars each (win the series,
  win it 5-0, win it under a budget).
- *Meta:* trial chapters themed per class or mechanic. "The stealth chapter", "Nagrand's
  pillars".

**Player decisions.** Composition, gear allocation under a budget, pre-match choices. The whole
build is the answer.

**What the player watches for.** Whether the mechanism they built around fires: the Felhunter
devouring the Fear, the Bird freeing the root.

**Progression and stakes.** Unlocking chapters; stars. Low stakes, high learning value.

**Reuse.** Configure Match almost as is, the item budget (already a pricing system), fixed seeds.
**New:** a trial format in RON, a chapter screen and a star record.

**Determinism and tooling.** Fixed seeds make each trial exactly reproducible. The sweep tools can
*prove* a trial is solvable and not trivial before it ships: search the answer space headlessly
and require at least one 5-0 setup and fewer than some share of naive setups winning.

**Verdict.** Excellent as a tutorial and a teaching layer for the class kits. It is weak as the
core loop: authored content runs out, and a solved trial has no replay value. A good Phase 4
companion mode, and its trial format could be reused for a Gauntlet "boss" opponent.

## 7. The recommendation: the Gauntlet

### Why this one

- **It creates a decision between every match.** That is the precise gap between a sandbox and
  a game. Draft, setup and reward each happen every few minutes, and the intel card makes
  setup a prediction the match then tests.
- **Its stakes fit the design doc's philosophy.** A run is the only thing at risk. Experimenting
  costs nothing beyond the run.
- **It gives every existing system a job,** and needs no new combat mechanics. Pre-match
  choices become counters, items become rewards, maps become scouting facts, the kill call
  becomes in-match agency, replays become run history, and the sweep tools become content QA.
- **Its smallest fun version is small.** Concepts A and C need an economy before they are fun;
  B needs a table of opponents and a pick-one-of-three screen.
- **It leads to the Stable.** The team card, opponent intel, rewards and run record are C's
  parts. The phased plan ends at C.

### Minimum playable slice

The smallest version that is fun, and nothing more:

1. **Main menu → Gauntlet.** One active run at a time, resumable. The existing "Match"
   entry stays as a custom-match sandbox.
2. **Draft.** Four classes offered, at least one of them a healer. Pick two. They start in
   their default loadouts (ilvl 54-60).
3. **Run hub.** Shows:
   - your two combatants with their gear;
   - the record as pips (wins out of 7, losses out of 3);
   - the next opponent's **intel card**: classes, map, AI profile, and generated threat
     lines from their kit and pre-match choices.
4. **Prepare.** The existing pre-match choices for your two classes (kill target, CC target,
   pet, curses, opener, shout, armor, aura), pre-filled with last match's choices.
5. **Fight.** PlayMatch exactly as today, kill calls included. Results exactly as today, plus
   a **Continue run** button.
6. **Reward.** Three offers drawn from the reward table for this round (gear in the run's
   current ilvl band, rising with wins). Each shows the stat change against what it replaces.
   Pick one; it is equipped at once.
7. **Run end.** At 7 wins or 3 losses: the record, the final team, and each match with a
   **Watch replay** link (the existing `--replay` config, opened in-client).

**Out of the slice:** 3v3, talents, daily seeds, post-match odds, meta unlocks, named
combatants, and any currency. Each is a later phase.

**The opponent ladder for the slice.** About 20 authored opponents in 7 tiers. Tier rises with
your win count, so the seventh win is against the hardest tier. Each opponent is a
`HeadlessMatchConfig` side with a name, a tier, an optional AI profile, and gear from the
matching ilvl band. Map is part of the opponent entry, so Nagrand opponents can be authored as
pillar teams.

### What the slice needs from AS-251 (the UI design system)

The Gauntlet adds four screens and changes two. It needs the design system to provide these
as shared components rather than letting the Gauntlet invent them:

- **A choice card:** a selectable card with icon, title, a few stat lines, a rarity or tier
  frame, and selected and disabled states. Used by draft offers and rewards.
- **A team card / roster strip:** class portraits with gear summary (average ilvl, notable
  pieces). Used by the hub, intel and run end.
- **A pip track** for wins and losses.
- **An intel panel:** a header plus short labelled lines, with linked ability icons (the
  existing linked-icon widget).
- **An item comparison tooltip:** the offered item against the equipped one, with deltas.
  Today's item tooltip shows one item.
- **A hub screen template:** a persistent header (run record), a main area and one primary
  action.
- **Back and Exit semantics for a resumable flow.** Exit from a run means "save and leave",
  never "abandon". Abandoning is its own confirmed action.

The Gauntlet should not start screen work until the design system's first components exist, so
it is built from them rather than being one more screen to migrate.

### What the Gauntlet needs from AS-252 (talents)

**Nothing for the minimum slice.** Talents join in Phase 3 as a second kind of reward. For that
to work, the talent system needs four properties:

1. **A unit of progress small enough to be one reward.** "One talent point" if the shape is
   trees, or "one pick from a tier" if it is pick-N. Either works, as long as a single step can
   be granted and shown on a choice card.
2. **Talents are part of the team description.** `HeadlessMatchConfig` (or its successor)
   carries a combatant's talents, so opponents, replays, sweeps and the bot all see them.
3. **Opponents can be authored with builds,** in the same RON as the opponent pool.
4. **A one-line description per talent,** generated like `ability_text.rs`, so an offer card
   and the intel card can show it without hand-written prose.

If AS-252 folds the existing pre-match choices into talents, the Gauntlet's "prepare" step
reads those choices from the talent system instead of `MatchConfig`. Nothing else changes.

### Phased plan

Cards are listed in dependency order. Each is independently shippable and keeps `main` green.
Sweeps use the tiers in `sweep-tiers.md`; the calibration card states DIRECTIONAL.

**Phase 0: foundations (no screens).**

| Card | Delivers |
|---|---|
| G1 Run model | `RunState` (seed, roster, gear, record, choice history). Offers and opponents are generated from the run seed on their own RNG stream (never `GameRng`). The save is the seed plus the choices, written under the settings directory, so a run replays from its save. Unit tests. |
| G2 Gauntlet data | `assets/config/gauntlet.ron`: run rules (wins, losses, draft size), the opponent pool by tier, reward tables by round. `validate()` at load, like `movement.ron`. Each opponent's gear passes the existing budget check. |
| G3 Recorded kill calls | A match carries the kill calls made in it with sim timestamps, and a replay of that record reproduces the match. A test pins it. Needed for honest replays and run history. |
| G4 Headless Gauntlet driver | `--gauntlet <seed> --policy <name>` plays a whole run headlessly with a scripted policy, and `--batch` runs many. |
| G5 Calibration | Sweeps to grade each opponent against reference teams, measure the item-tier gap, and run a few hundred bot runs for the wins-per-run distribution and draft-pair dominance. DIRECTIONAL. Output: tiers and reward tables tuned to a target curve, in a findings doc. |

**Phase 1: the minimum playable slice.**

| Card | Delivers |
|---|---|
| G6 Entry and draft | Main-menu entry, the draft screen, starting a run. |
| G7 Run hub and intel | The hub, the opponent intel card with generated threat lines, and Prepare reusing the pre-match widgets. Fight launches PlayMatch with the locked roster. |
| G8 Rewards and run end | The reward screen with item comparison, run end with replay links, and Continue run on Results. |
| G9 Save, resume, abandon | Resume from the main menu after a quit, and a confirmed abandon. A UI script drives a full short run as a smoke test. |

At the end of Phase 1, the milestone review is: **play three runs.**

**Phase 2: depth.**

- **G10 3v3 runs:** draft three from five; 3v3 opponent pool.
- **G11 Map draws** per round, shown in intel.
- **G12 Post-match odds:** a background batch of the same matchup after a loss, shown on Results.
- **G13 Daily seed:** the same offers and opponents for a day, with a local best-record board.
- **G14 Run history:** past runs, each match replayable.

**Phase 3: talents (after AS-252's first card).**

- **G15** Talent rewards join the reward pool.
- **G16** Opponents carry builds.
- **G17** The bot's policies learn to pick talents, so calibration covers them.

**Phase 4: toward the Stable (concept C).**

- **Ghost opponents:** your own past run teams become opponents. This is Super Auto Pets'
  asynchronous model with no server.
- **Named combatants:** a persistent roster with banter personalities as identity, carried
  between runs as a stable.
- **Tournaments:** Gauntlet-shaped events on a calendar, with an economy for recruiting and
  training.
- **Counter-Pick Trials** (concept D) as a tutorial mode, reusing the opponent format.

## 8. Decisions for the user

1. **Which loop.** The Gauntlet as the first loop, with the Stable as the destination? Or
   another concept?
2. **Run length.** 7 wins or 3 losses (about 15-25 minutes) as proposed, or Hearthstone's
   12/3 (about 30-40 minutes)?
3. **The kill call.** Free and unlimited, as today? Or a limited budget per match (for
   example three calls) so that each call is a decision?
4. **Pre-match choices.** Free every match, as proposed, so they serve as the counter-pick
   answer? Or earned as rewards (unlock the Boar, unlock Cheap Shot)?
5. **The draft.** Single classes from four offers, as proposed? Or whole pre-built pairs?
6. **Meta progression.** None until Phase 4, as proposed? Or Hades-style unlocks earlier,
   adding variety but never power?
