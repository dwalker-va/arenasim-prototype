# Long Term Game Concept
Do not implement these, just consider how possible decisions align with the long term conceptual goals and use that as a factor in relevant decisions.

You are the manager of a fledgling stable of combatants. Your goal is to continuously improve your teams and strategies to the point where you can win championships. You start with few combatants, all of poor quality, and must compete in tournaments in order to obtain the resources necessary to improve your team. 

Arena matches are played CPU vs CPU (“autobattler”), so the only thing that matters is how combatants are customized in terms of statistics and abilities, and what team tactics are designed before and employed during the match, all configurable by the player.

There is an emphasis placed on experimentation with little to no long-term penalties for losing games.

Influences:

- World of Warcraft Classic/TBC/Wrath
  - Combat System (Autoattacks, Spells, Buffs, Debuffs, Auras, Effects, Crowd Control, etc)
  - Character Customization (Talents, Gear)
  - Class and Ability Design (This can’t be 1:1., but Rogues have Stealth and wear leather armor, Warriors have stances and rage and wear plate armor, Mages have mana and wear cloth armor, some characters focus on healing and support etc.)
- Super Smash Brothers/Fighting games
  - Configuring matches
# Short Term Game Concept
A prototype implementation of the above scoped down to just the game loop of “configure match->battle->results->repeat”

# Target Platforms
PC
SteamDeck

# Player controls
Keyboard/Mouse
Gamepad (ie for SteamDeck)

# Visual Guidance
Game world: Low-poly, flat-shaded, grid-aligned diorama with a limited palette. Basic primitive meshes, let the geometry and lighting with flat/vertex colors do the work as opposed to textures.

In terms of color palettes, GUI, fonts, etc, we’re going for a high fantasy feel, similar to World of Warcraft, but it can have a slightly lo-fi, budget or retro vibe.

Clarity is the number one factor - players need to be able to see and recognize what is happening, even when multiple things are happening at once, which is basically all the time.

# Scenes
These may not be all of the scenes required for technical reasons and many of them could just be graphical overlays rather than dedicated scenes, but they represent the distinct states the player’s focus and attention experiences.

## Main Menu
Choices: Match (goes to “Configure Match”), Exit, Options

## Options
Change basic video and audio settings like fullscreen vs fullscreen windowed, volume, and whatever else the engine readily supports.

## Configure Match
The first complicated scene. This is where a player configures the parameters of a match before playing it. The match configuration of a game like the latest Super Smash Brothers games are a good example.

The player needs to be able to:
- Configure team sizes 
  - 1v1, 2v2, 3v3, or even imbalanced matchups like 1v2, 3v2, etc.
  - Min team size: 1, max team size: 3
- Configure map choice
  - One test map for now is ok, but in the future we want to offer a variety.
- Configure team membership
  - “Character select”
- Future: Configure characters
  - In the future we will be able to edit the details of characters (to adjust gear, talents, create/edit/delete saved customizations/loadouts)
  - Stub out the workflow here, mark it as coming soon but the link to this workflow is visible from this scene
- Future: Configure strategy
  - This would be a workflow where you can configure the “AI” of each team (likely selecting from saved strategies in the simplest case)
  - I am unsure what this should look like or be similar to, so we can totally skip it for now.
- Start Match
  - Once the prerequisites are met (selected teams are full with assigned characters) the user just has to press this to start the match, in case they are not done configuring their match even if it’s in a valid enough state for us to run the match.
  - This transitions to the Play Match scene

## Play Match
The core scene of the game.This is where the “autobattle” is simulated for the player with appropriate visualization between two teams of combatants in an arena.

A team loses the match when all of their combatants are dead. The team that survives is the winner.

When the match is loaded, a countdown (of 10s) is started for the match to begin, where the teams are placed in "pens" behind gates where they can apply buffs before the match starts. When the countdown ends, the gate opens and the teams run towards each other to battle.

During this countdown, a summary preview of the match is overlaid.

Before the gates open, the mana of all characters is restored to 100% every second, to make sure there is no penalty for pre-match preparation by any team.

### Shadow Sight Orbs (Stealth Stalemate Breaker)
In matches where all combatants can enter stealth (e.g., Rogue vs Rogue), there is potential for indefinite stalemates where no one can find each other. Inspired by WoW arena's Shadow Sight mechanic:

- **Spawn Timer**: Two Shadow Sight orbs spawn 90 seconds after gates open
- **Spawn Locations**: Positioned symmetrically (north and south sides of arena center)
- **Pickup**: Any alive combatant can collect an orb by walking near it (2.5 unit radius)
- **Effect**: Grants 15-second buff allowing the holder to see stealthed enemies (and be seen by enemies)
- **Visual**: Purple glowing orbs with outer aura, bobbing/rotating/pulsing animation
- **Pickup Animation**: Orb shrinks and moves toward the collector before despawning

This ensures stealth matchups will always reach combat within ~90 seconds, as combatants will seek orbs when they have no visible targets.

The player should be able to:

- Control the speed of the simulation with a Pause/0.5x/1x/2x/3x control
- Explore the visualization
  - Camera control
    - Follow midpoint/center
    - Zoom in/out
    - Follow combatant
    - Manual drag/rotate/pitch
  - View Key combatant data
	- Combatant Health/Resource (Mana/Rage/Energy/etc)
	- Auras applied to a combatant and their remaining duration
  - Highlight critical situations
	- Combatant with low HP
	- Combatants in Crowd Control
	  - When we say “Crowd Control”, we mean “hard cc” that causes the complete (or almost complete) loss of control for a combatant. “Soft cc” might refer to debuffs that reduce a character’s effectiveness like a slow or damage dealt reduction, but not prevent them from using most of their abilities.
  - Review a combat log
	- A text stream of what has happened in the battle (events like damage, auras, ability usage)
	- Scroll up/scroll down
	- Filter down to only events that attempt to change Combatant HP.

After the match has been decided, give the winning team a few seconds to show a celebration animation and then transition to the results scene.

## Results
Shows a summary of the results and an explorable visualization of the statistics of the match.

Use the combat log resource generated during the match to parameterize all of the visualizations.

The player should be able to:
  - View a summary of the results
    - Show basic stats per combatant like killing blows, damage done, damage received, healing done, healing received, crowd control done (measured in seconds), crowd control received (measured in seconds).
    - Show team summaries of the above stats
    - View a detailed damage/healing breakdown for a character.
      - Similar to the “Details” WoW addon visualization ("damage meter")
      - Shows how that character’s abilities contributed to their total damage done (40% shadow bolt, 25% corruption, 25% curse of agony, 10% siphon life) and separately, their healing done (50% greater heal, 30% flash heal, 20% renew)
 - Select Done to go back to the Main Menu state.
