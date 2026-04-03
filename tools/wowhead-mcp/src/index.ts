#!/usr/bin/env node
/**
 * Wowhead Classic MCP Server
 *
 * Provides tools for looking up WoW Classic spell and item data from Wowhead.
 * Returns structured data for spells (matching our AbilityDefinition format)
 * and equipment items (stats, icons, slot info for our equipment system).
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

// Wowhead API endpoints
const SPELL_TOOLTIP_API = "https://nether.wowhead.com/classic/tooltip/spell";
const ITEM_TOOLTIP_API = "https://nether.wowhead.com/classic/tooltip/item";
const ICON_BASE_URL = "https://wow.zamimg.com/images/wow/icons/large";
const WOWHEAD_SPELL_URL = "https://www.wowhead.com/classic/spell";
const WOWHEAD_ITEM_URL = "https://www.wowhead.com/classic/item";

// Common spell ID mappings for quick lookup
const KNOWN_SPELLS: Record<string, number> = {
  // Mage
  "frostbolt": 116,
  "frost nova": 122,
  "fireball": 133,
  "polymorph": 118,
  "blink": 1953,
  "ice block": 11958,
  "counterspell": 2139,
  "arcane intellect": 1459,

  // Priest
  "flash heal": 2061,
  "mind blast": 8092,
  "shadow word: pain": 589,
  "power word: shield": 17,
  "power word: fortitude": 1243,
  "psychic scream": 8122,
  "dispel magic": 527,
  "renew": 139,

  // Warrior
  "charge": 100,
  "heroic strike": 78,
  "mortal strike": 12294,
  "rend": 772,
  "hamstring": 1715,
  "pummel": 6552,
  "execute": 5308,
  "intercept": 20252,
  "intimidating shout": 5246,

  // Rogue
  "ambush": 8676,
  "sinister strike": 1752,
  "kidney shot": 408,
  "kick": 1766,
  "gouge": 1776,
  "backstab": 53,
  "eviscerate": 2098,
  "cheap shot": 1833,
  "vanish": 1856,
  "sprint": 2983,

  // Warlock
  "shadow bolt": 686,
  "corruption": 172,
  "fear": 5782,
  "curse of agony": 980,
  "immolate": 348,
  "drain life": 689,
  "death coil": 6789,
  "howl of terror": 5484,
  "life tap": 1454,

  // Paladin
  "holy light": 635,
  "flash of light": 19750,
  "hammer of justice": 853,
  "blessing of freedom": 1044,
  "divine shield": 642,
  "lay on hands": 633,
  "cleanse": 4987,

  // Hunter
  "aimed shot": 19434,
  "multi-shot": 2643,
  "arcane shot": 3044,
  "serpent sting": 1978,
  "concussive shot": 5116,
  "scatter shot": 19503,
  "freezing trap": 1499,

  // Druid
  "moonfire": 8921,
  "wrath": 5176,
  "rejuvenation": 774,
  "regrowth": 8936,
  "entangling roots": 339,
  "hibernate": 2637,
  "cyclone": 33786,
  "bash": 5211,

  // Shaman
  "lightning bolt": 403,
  "chain lightning": 421,
  "earth shock": 8042,
  "flame shock": 8050,
  "frost shock": 8056,
  "healing wave": 331,
  "lesser healing wave": 8004,
  "purge": 370,
};

// Spell school detection from tooltip HTML
function detectSpellSchool(tooltip: string): string {
  const schoolPatterns = [
    { pattern: /fire damage/i, school: "Fire" },
    { pattern: /frost damage/i, school: "Frost" },
    { pattern: /shadow damage/i, school: "Shadow" },
    { pattern: /holy damage/i, school: "Holy" },
    { pattern: /nature damage/i, school: "Nature" },
    { pattern: /arcane damage/i, school: "Arcane" },
    { pattern: /physical damage/i, school: "Physical" },
  ];

  for (const { pattern, school } of schoolPatterns) {
    if (pattern.test(tooltip)) {
      return school;
    }
  }

  // Check for healing spells
  if (/heal/i.test(tooltip)) {
    return "Holy";
  }

  return "Unknown";
}

// Parse cast time from tooltip
function parseCastTime(tooltip: string): string {
  // Look for cast time patterns
  const castMatch = tooltip.match(/(\d+(?:\.\d+)?)\s*sec(?:ond)?s?\s*cast/i);
  if (castMatch) {
    return `${castMatch[1]}s`;
  }

  if (/instant/i.test(tooltip)) {
    return "Instant";
  }

  // Check for channeled spells
  const channelMatch = tooltip.match(/channeled/i);
  if (channelMatch) {
    return "Channeled";
  }

  return "Unknown";
}

// Parse mana/resource cost from tooltip
function parseResourceCost(tooltip: string): string {
  const manaMatch = tooltip.match(/(\d+)\s*mana/i);
  if (manaMatch) {
    return `${manaMatch[1]} Mana`;
  }

  const rageMatch = tooltip.match(/(\d+)\s*rage/i);
  if (rageMatch) {
    return `${rageMatch[1]} Rage`;
  }

  const energyMatch = tooltip.match(/(\d+)\s*energy/i);
  if (energyMatch) {
    return `${energyMatch[1]} Energy`;
  }

  return "Unknown";
}

// Parse range from tooltip
function parseRange(tooltip: string): string {
  const rangeMatch = tooltip.match(/(\d+)\s*(?:yard|yd)s?\s*range/i);
  if (rangeMatch) {
    return `${rangeMatch[1]} yards`;
  }

  if (/melee range/i.test(tooltip)) {
    return "Melee";
  }

  return "Unknown";
}

// Parse cooldown from tooltip
function parseCooldown(tooltip: string): string {
  const cdMatch = tooltip.match(/(\d+)\s*(?:sec(?:ond)?|min(?:ute)?)\s*cooldown/i);
  if (cdMatch) {
    const value = cdMatch[1];
    const unit = /min/i.test(cdMatch[0]) ? "min" : "sec";
    return `${value} ${unit}`;
  }

  return "None";
}

// Parse damage values from tooltip
function parseDamage(tooltip: string): { min: number; max: number } | null {
  // Pattern for "X to Y damage"
  const rangeMatch = tooltip.match(/(\d+)\s*to\s*(\d+)\s*(?:\w+\s+)?damage/i);
  if (rangeMatch) {
    return { min: parseInt(rangeMatch[1]), max: parseInt(rangeMatch[2]) };
  }

  // Pattern for single damage value
  const singleMatch = tooltip.match(/(\d+)\s*(?:\w+\s+)?damage/i);
  if (singleMatch) {
    const value = parseInt(singleMatch[1]);
    return { min: value, max: value };
  }

  return null;
}

// Parse healing values from tooltip
function parseHealing(tooltip: string): { min: number; max: number } | null {
  // Pattern for "heals for X to Y"
  const rangeMatch = tooltip.match(/heals?\s+(?:for\s+)?(\d+)\s*to\s*(\d+)/i);
  if (rangeMatch) {
    return { min: parseInt(rangeMatch[1]), max: parseInt(rangeMatch[2]) };
  }

  // Pattern for single healing value
  const singleMatch = tooltip.match(/heals?\s+(?:for\s+)?(\d+)/i);
  if (singleMatch) {
    const value = parseInt(singleMatch[1]);
    return { min: value, max: value };
  }

  return null;
}

// Parse duration from tooltip (for auras/buffs)
function parseDuration(tooltip: string): string | null {
  const durationMatch = tooltip.match(/(?:for|lasts?)\s*(\d+)\s*(sec(?:ond)?s?|min(?:ute)?s?)/i);
  if (durationMatch) {
    return `${durationMatch[1]} ${durationMatch[2]}`;
  }
  return null;
}

// Strip HTML tags from tooltip
function stripHtml(html: string): string {
  return html
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/\s+/g, ' ')
    .trim();
}

// Fetch spell data from Wowhead tooltip API
async function fetchSpellById(spellId: number): Promise<{
  name: string;
  icon: string;
  iconUrl: string;
  wowheadUrl: string;
  tooltip: string;
  castTime: string;
  resourceCost: string;
  range: string;
  cooldown: string;
  spellSchool: string;
  damage: { min: number; max: number } | null;
  healing: { min: number; max: number } | null;
  duration: string | null;
  rawTooltip: string;
} | null> {
  try {
    const response = await fetch(`${SPELL_TOOLTIP_API}/${spellId}`);
    if (!response.ok) {
      return null;
    }

    const data = await response.json() as {
      name?: string;
      icon?: string;
      tooltip?: string;
      buff?: string;
    };

    if (!data.name) {
      return null;
    }

    const tooltip = data.tooltip || "";
    const strippedTooltip = stripHtml(tooltip);

    return {
      name: data.name,
      icon: data.icon || "inv_misc_questionmark",
      iconUrl: `${ICON_BASE_URL}/${data.icon || "inv_misc_questionmark"}.jpg`,
      wowheadUrl: `${WOWHEAD_SPELL_URL}=${spellId}`,
      tooltip: strippedTooltip,
      castTime: parseCastTime(strippedTooltip),
      resourceCost: parseResourceCost(strippedTooltip),
      range: parseRange(strippedTooltip),
      cooldown: parseCooldown(strippedTooltip),
      spellSchool: detectSpellSchool(strippedTooltip),
      damage: parseDamage(strippedTooltip),
      healing: parseHealing(strippedTooltip),
      duration: parseDuration(strippedTooltip),
      rawTooltip: strippedTooltip,
    };
  } catch (error) {
    console.error(`Error fetching spell ${spellId}:`, error);
    return null;
  }
}

// Search for spell ID by name
function findSpellId(name: string): number | null {
  const normalized = name.toLowerCase().trim();

  // Direct match
  if (KNOWN_SPELLS[normalized]) {
    return KNOWN_SPELLS[normalized];
  }

  // Partial match
  for (const [spellName, spellId] of Object.entries(KNOWN_SPELLS)) {
    if (spellName.includes(normalized) || normalized.includes(spellName)) {
      return spellId;
    }
  }

  return null;
}

// Known item ID mappings for quick lookup
const KNOWN_ITEMS: Record<string, number> = {
  // Plate Armor
  "lionheart helm": 12640,
  "helm of wrath": 16963,
  "conqueror's breastplate": 21331,
  "breastplate of wrath": 16966,
  "legplates of wrath": 16962,
  "gauntlets of might": 16863,
  "sabatons of might": 16862,
  "belt of might": 16864,
  "bracers of might": 16861,
  "shoulderguards of might": 16868,
  "lawbringer chestguard": 16958,
  "lawbringer helm": 16955,
  "lawbringer legplates": 16959,
  "lawbringer gauntlets": 16956,
  "lawbringer boots": 16954,

  // Mail Armor
  "beaststalker's cap": 16677,
  "beaststalker's tunic": 16674,
  "beaststalker's pants": 16676,
  "beaststalker's gloves": 16675,
  "beaststalker's boots": 16672,
  "beaststalker's belt": 16673,
  "beaststalker's bindings": 16671,
  "beaststalker's mantle": 16678,
  "giantstalker's helmet": 16846,
  "giantstalker's breastplate": 16845,

  // Leather Armor
  "nightslayer cover": 16821,
  "nightslayer chestpiece": 16820,
  "nightslayer pants": 16822,
  "nightslayer gloves": 16826,
  "nightslayer boots": 16824,
  "nightslayer belt": 16827,
  "nightslayer bracelets": 16825,
  "nightslayer shoulder pads": 16823,
  "shadowcraft cap": 16707,
  "shadowcraft tunic": 16721,

  // Cloth Armor
  "magister's crown": 16686,
  "magister's robes": 16688,
  "magister's leggings": 16687,
  "magister's gloves": 16684,
  "magister's boots": 16682,
  "magister's belt": 16683,
  "magister's bindings": 16685,
  "magister's mantle": 16689,
  "devout crown": 16693,
  "devout robe": 16690,
  "dreadmist mask": 16698,
  "dreadmist robe": 16700,

  // Cloaks
  "cloak of the shrouded mists": 17102,
  "cape of the black baron": 13340,
  "cloak of firemaw": 19398,

  // Necklaces
  "onyxia tooth pendant": 18404,
  "mark of fordring": 15411,
  "choker of the fire lord": 18814,

  // Rings
  "band of accuria": 17063,
  "signet ring of the bronze dragonflight": 21205,
  "ring of protection": 11669,
  "don julio's band": 19325,

  // Trinkets
  "mark of the champion": 23206,
  "blackhand's breadth": 13965,
  "briarwood reed": 12930,
  "royal seal of eldre'thalas": 18473,

  // Two-Handed Weapons
  "arcanite reaper": 12784,
  "sulfuras, hand of ragnaros": 17182,
  "ashkandi, greatsword of the brotherhood": 19364,
  "barb of the sand reaver": 21126,
  "the untamed blade": 19334,

  // One-Handed Weapons
  "dal'rend's sacred charge": 12940,
  "dal'rend's tribal guardian": 12939,
  "chromatically tempered sword": 19352,
  "brutality blade": 18832,
  "perdition's blade": 18816,
  "fang of the mystics": 19354,
  "deathbringer": 17068,
  "gutgutter": 17071,

  // Staves
  "staff of dominance": 18842,
  "benediction": 18608,
  "anathema": 18609,
  "staff of the shadow flame": 19356,

  // Wands
  "wand of biting cold": 22820,
  "touch of chaos": 18482,
  "skul's ghastly touch": 13396,

  // Ranged Weapons
  "rhok'delar, longbow of the ancient keepers": 18713,
  "crossbow of imminent doom": 18836,
  "striker's mark": 17069,
  "ancient bone bow": 21459,

  // Off-Hand / Shields
  "tome of the ice lord": 19358,
  "drillborer disk": 19353,
  "elementium reinforced bulwark": 19349,
};

// Item quality names
const QUALITY_NAMES: Record<number, string> = {
  0: "Poor",
  1: "Common",
  2: "Uncommon",
  3: "Rare",
  4: "Epic",
  5: "Legendary",
};

// Parse armor value from item tooltip HTML
function parseArmor(tooltip: string): number | null {
  const match = tooltip.match(/<!--amr-->(\d+) Armor/);
  return match ? parseInt(match[1]) : null;
}

// Parse weapon damage range from item tooltip HTML
function parseItemDamage(tooltip: string): { min: number; max: number } | null {
  const match = tooltip.match(/<!--dmg-->(\d+)\s*-\s*(\d+)\s*Damage/);
  return match ? { min: parseInt(match[1]), max: parseInt(match[2]) } : null;
}

// Parse weapon speed from item tooltip HTML
function parseItemSpeed(tooltip: string): number | null {
  const match = tooltip.match(/Speed\s*<!--spd-->([\d.]+)/);
  return match ? parseFloat(match[1]) : null;
}

// Parse DPS from item tooltip HTML
function parseItemDps(tooltip: string): number | null {
  const match = tooltip.match(/\(([\d.]+)\s*damage per second\)/);
  return match ? parseFloat(match[1]) : null;
}

// Parse bonus stats from item tooltip HTML (Stamina, Intellect, Strength, etc.)
function parseBonusStats(tooltip: string): Record<string, number> {
  const stats: Record<string, number> = {};
  // Pattern: <!--statN-->+X Stat Name (supports multi-word stats like "Fire Resistance")
  const statRegex = /<!--stat\d+-->\+(\d+)\s+([\w\s]+?)(?=<|$)/g;
  let match;
  while ((match = statRegex.exec(tooltip)) !== null) {
    stats[match[2].trim()] = parseInt(match[1]);
  }
  return stats;
}

// Parse equip effects from item tooltip HTML (Attack Power, Spell Power, etc.)
function parseEquipEffects(tooltip: string): string[] {
  const effects: string[] = [];
  const effectRegex = /Equip:\s*<!--useEffect:\d+:\d+--><a[^>]*>(.*?)<\/a>/g;
  let match;
  while ((match = effectRegex.exec(tooltip)) !== null) {
    effects.push(stripHtml(match[1]));
  }
  return effects;
}

// Parse item level from tooltip HTML
function parseItemLevel(tooltip: string): number | null {
  const match = tooltip.match(/Item Level\s*<!--ilvl-->(\d+)/);
  return match ? parseInt(match[1]) : null;
}

// Parse slot type from tooltip HTML
function parseItemSlot(tooltip: string): string | null {
  // Slot appears in a table cell: <td>Two-Hand</td> or <td>Head</td> etc.
  const slotMatch = tooltip.match(/<td>(Head|Chest|Legs|Hands|Feet|Waist|Wrist|Shoulder|Back|Finger|Neck|Trinket|One-Hand|Two-Hand|Main Hand|Off Hand|Ranged|Held In Off-hand|Relic)<\/td>/);
  return slotMatch ? slotMatch[1] : null;
}

// Parse armor type from tooltip HTML
function parseArmorType(tooltip: string): string | null {
  const match = tooltip.match(/<span class="q1">(Plate|Mail|Leather|Cloth|Shield|Axe|Sword|Mace|Dagger|Staff|Polearm|Fist Weapon|Wand|Bow|Crossbow|Gun)<\/span>/);
  return match ? match[1] : null;
}

// Parse required level from tooltip HTML
function parseRequiredLevel(tooltip: string): number | null {
  const match = tooltip.match(/Requires Level\s*<!--rlvl-->(\d+)/);
  return match ? parseInt(match[1]) : null;
}

// Fetch item data from Wowhead tooltip API
async function fetchItemById(itemId: number): Promise<{
  name: string;
  quality: string;
  qualityNum: number;
  icon: string;
  iconUrl: string;
  wowheadUrl: string;
  itemLevel: number | null;
  requiredLevel: number | null;
  slot: string | null;
  armorType: string | null;
  armor: number | null;
  damage: { min: number; max: number } | null;
  speed: number | null;
  dps: number | null;
  bonusStats: Record<string, number>;
  equipEffects: string[];
  tooltip: string;
} | null> {
  try {
    const response = await fetch(`${ITEM_TOOLTIP_API}/${itemId}`);
    if (!response.ok) {
      return null;
    }

    const data = await response.json() as {
      name?: string;
      quality?: number;
      icon?: string;
      tooltip?: string;
    };

    if (!data.name) {
      return null;
    }

    const tooltip = data.tooltip || "";
    const qualityNum = data.quality ?? 1;

    return {
      name: data.name,
      quality: QUALITY_NAMES[qualityNum] || "Unknown",
      qualityNum,
      icon: data.icon || "inv_misc_questionmark",
      iconUrl: `${ICON_BASE_URL}/${data.icon || "inv_misc_questionmark"}.jpg`,
      wowheadUrl: `${WOWHEAD_ITEM_URL}=${itemId}`,
      itemLevel: parseItemLevel(tooltip),
      requiredLevel: parseRequiredLevel(tooltip),
      slot: parseItemSlot(tooltip),
      armorType: parseArmorType(tooltip),
      armor: parseArmor(tooltip),
      damage: parseItemDamage(tooltip),
      speed: parseItemSpeed(tooltip),
      dps: parseItemDps(tooltip),
      bonusStats: parseBonusStats(tooltip),
      equipEffects: parseEquipEffects(tooltip),
      tooltip: stripHtml(tooltip),
    };
  } catch (error) {
    console.error(`Error fetching item ${itemId}:`, error);
    return null;
  }
}

// Search for item ID by name
function findItemId(name: string): number | null {
  const normalized = name.toLowerCase().trim();

  // Direct match
  if (KNOWN_ITEMS[normalized]) {
    return KNOWN_ITEMS[normalized];
  }

  // Partial match
  for (const [itemName, itemId] of Object.entries(KNOWN_ITEMS)) {
    if (itemName.includes(normalized) || normalized.includes(itemName)) {
      return itemId;
    }
  }

  return null;
}

// Format item data into markdown output
function formatItemOutput(itemData: NonNullable<Awaited<ReturnType<typeof fetchItemById>>>, itemId?: number): string {
  const statsLines = Object.entries(itemData.bonusStats)
    .map(([stat, value]) => `  - +${value} ${stat}`)
    .join("\n");

  const heading = itemId
    ? `## ${itemData.name} (ID: ${itemId}, ${itemData.quality})`
    : `## ${itemData.name} (${itemData.quality})`;

  return `${heading}

**Wowhead URL:** ${itemData.wowheadUrl}
**Icon URL:** ${itemData.iconUrl}
**Icon Name:** ${itemData.icon}

### Item Info
- **Item Level:** ${itemData.itemLevel ?? "Unknown"}
- **Required Level:** ${itemData.requiredLevel ?? "Unknown"}
- **Slot:** ${itemData.slot ?? "Unknown"}
- **Type:** ${itemData.armorType ?? "Unknown"}
${itemData.armor ? `- **Armor:** ${itemData.armor}` : ""}
${itemData.damage ? `- **Damage:** ${itemData.damage.min} - ${itemData.damage.max}` : ""}
${itemData.speed ? `- **Speed:** ${itemData.speed}` : ""}
${itemData.dps ? `- **DPS:** ${itemData.dps}` : ""}

${statsLines ? `### Bonus Stats\n${statsLines}` : ""}
${itemData.equipEffects.length > 0 ? `\n### Equip Effects\n${itemData.equipEffects.map(e => `- ${e}`).join("\n")}` : ""}

### Tooltip
${itemData.tooltip}`;
}

// Item groups for list_known_items
const ITEM_GROUPS: Record<string, Record<string, string[]>> = {
  "Plate": {
    "Head": ["lionheart helm", "helm of wrath", "lawbringer helm"],
    "Chest": ["conqueror's breastplate", "breastplate of wrath", "lawbringer chestguard"],
    "Legs": ["legplates of wrath", "lawbringer legplates"],
    "Hands": ["gauntlets of might", "lawbringer gauntlets"],
    "Feet": ["sabatons of might", "lawbringer boots"],
    "Waist": ["belt of might"],
    "Wrists": ["bracers of might"],
    "Shoulders": ["shoulderguards of might"],
  },
  "Mail": {
    "Head": ["beaststalker's cap", "giantstalker's helmet"],
    "Chest": ["beaststalker's tunic", "giantstalker's breastplate"],
    "Legs": ["beaststalker's pants"],
    "Hands": ["beaststalker's gloves"],
    "Feet": ["beaststalker's boots"],
    "Waist": ["beaststalker's belt"],
    "Wrists": ["beaststalker's bindings"],
    "Shoulders": ["beaststalker's mantle"],
  },
  "Leather": {
    "Head": ["nightslayer cover", "shadowcraft cap"],
    "Chest": ["nightslayer chestpiece", "shadowcraft tunic"],
    "Legs": ["nightslayer pants"],
    "Hands": ["nightslayer gloves"],
    "Feet": ["nightslayer boots"],
    "Waist": ["nightslayer belt"],
    "Wrists": ["nightslayer bracelets"],
    "Shoulders": ["nightslayer shoulder pads"],
  },
  "Cloth": {
    "Head": ["magister's crown", "devout crown", "dreadmist mask"],
    "Chest": ["magister's robes", "devout robe", "dreadmist robe"],
    "Legs": ["magister's leggings"],
    "Hands": ["magister's gloves"],
    "Feet": ["magister's boots"],
    "Waist": ["magister's belt"],
    "Wrists": ["magister's bindings"],
    "Shoulders": ["magister's mantle"],
  },
  "Weapons": {
    "Two-Hand": ["arcanite reaper", "sulfuras, hand of ragnaros", "ashkandi, greatsword of the brotherhood", "barb of the sand reaver", "the untamed blade"],
    "One-Hand": ["dal'rend's sacred charge", "dal'rend's tribal guardian", "chromatically tempered sword", "brutality blade", "perdition's blade", "fang of the mystics", "deathbringer", "gutgutter"],
    "Staff": ["staff of dominance", "benediction", "anathema", "staff of the shadow flame"],
    "Wand": ["wand of biting cold", "touch of chaos", "skul's ghastly touch"],
    "Ranged": ["rhok'delar, longbow of the ancient keepers", "crossbow of imminent doom", "striker's mark", "ancient bone bow"],
    "Off-Hand": ["tome of the ice lord", "drillborer disk", "elementium reinforced bulwark"],
  },
  "Accessories": {
    "Cloak": ["cloak of the shrouded mists", "cape of the black baron", "cloak of firemaw"],
    "Neck": ["onyxia tooth pendant", "mark of fordring", "choker of the fire lord"],
    "Ring": ["band of accuria", "signet ring of the bronze dragonflight", "ring of protection", "don julio's band"],
    "Trinket": ["mark of the champion", "blackhand's breadth", "briarwood reed", "royal seal of eldre'thalas"],
  },
};

// Initialize MCP server
const server = new McpServer({
  name: "wowhead-classic",
  version: "1.0.0",
});

// Tool schemas - use raw shape objects for MCP SDK
const LookupByIdSchema = {
  spellId: z.number().describe("The Wowhead spell ID"),
};

const LookupByNameSchema = {
  spellName: z.string().describe("The spell name to search for"),
};

const GetIconSchema = {
  spellIdOrName: z.string().describe("Spell ID (number) or name (string)"),
};

// Register tools
server.tool(
  "lookup_spell_by_id",
  "Look up detailed WoW Classic spell data by Wowhead spell ID. Returns cast time, mana cost, range, cooldown, damage/healing values, spell school, and icon URL.",
  LookupByIdSchema,
  async (args) => {
    const spellData = await fetchSpellById(args.spellId);

    if (!spellData) {
      return {
        content: [{
          type: "text" as const,
          text: `Spell with ID ${args.spellId} not found.`,
        }],
      };
    }

    const output = `## ${spellData.name}

**Wowhead URL:** ${spellData.wowheadUrl}
**Icon URL:** ${spellData.iconUrl}
**Icon Name:** ${spellData.icon}

### Stats
- **Cast Time:** ${spellData.castTime}
- **Resource Cost:** ${spellData.resourceCost}
- **Range:** ${spellData.range}
- **Cooldown:** ${spellData.cooldown}
- **Spell School:** ${spellData.spellSchool}
${spellData.damage ? `- **Damage:** ${spellData.damage.min} - ${spellData.damage.max}` : ""}
${spellData.healing ? `- **Healing:** ${spellData.healing.min} - ${spellData.healing.max}` : ""}
${spellData.duration ? `- **Duration:** ${spellData.duration}` : ""}

### Tooltip
${spellData.tooltip}`;

    return {
      content: [{
        type: "text" as const,
        text: output,
      }],
    };
  }
);

server.tool(
  "lookup_spell",
  "Look up WoW Classic spell data by name. Returns cast time, mana cost, range, cooldown, damage/healing values, spell school, and icon URL.",
  LookupByNameSchema,
  async (args) => {
    const spellId = findSpellId(args.spellName);

    if (!spellId) {
      // Return list of known spells that might match
      const suggestions = Object.keys(KNOWN_SPELLS)
        .filter(name => name.includes(args.spellName.toLowerCase()))
        .slice(0, 5);

      return {
        content: [{
          type: "text" as const,
          text: `Spell "${args.spellName}" not found in known spells database.${
            suggestions.length > 0
              ? `\n\nDid you mean: ${suggestions.join(", ")}?`
              : ""
          }\n\nYou can also use lookup_spell_by_id with a Wowhead spell ID directly.`,
        }],
      };
    }

    const spellData = await fetchSpellById(spellId);

    if (!spellData) {
      return {
        content: [{
          type: "text" as const,
          text: `Found spell ID ${spellId} for "${args.spellName}" but failed to fetch data from Wowhead.`,
        }],
      };
    }

    const output = `## ${spellData.name} (ID: ${spellId})

**Wowhead URL:** ${spellData.wowheadUrl}
**Icon URL:** ${spellData.iconUrl}
**Icon Name:** ${spellData.icon}

### Stats
- **Cast Time:** ${spellData.castTime}
- **Resource Cost:** ${spellData.resourceCost}
- **Range:** ${spellData.range}
- **Cooldown:** ${spellData.cooldown}
- **Spell School:** ${spellData.spellSchool}
${spellData.damage ? `- **Damage:** ${spellData.damage.min} - ${spellData.damage.max}` : ""}
${spellData.healing ? `- **Healing:** ${spellData.healing.min} - ${spellData.healing.max}` : ""}
${spellData.duration ? `- **Duration:** ${spellData.duration}` : ""}

### Tooltip
${spellData.tooltip}`;

    return {
      content: [{
        type: "text" as const,
        text: output,
      }],
    };
  }
);

server.tool(
  "get_spell_icon",
  "Get the icon URL and icon name for a WoW Classic spell. Useful for downloading spell icons. Pass spell name or numeric ID as string.",
  GetIconSchema,
  async (args) => {
    let spellId: number;

    // Check if it's a numeric string (spell ID)
    const numericId = parseInt(args.spellIdOrName, 10);
    if (!isNaN(numericId) && numericId > 0) {
      spellId = numericId;
    } else {
      // It's a spell name
      const foundId = findSpellId(args.spellIdOrName);
      if (!foundId) {
        return {
          content: [{
            type: "text" as const,
            text: `Spell "${args.spellIdOrName}" not found.`,
          }],
        };
      }
      spellId = foundId;
    }

    const spellData = await fetchSpellById(spellId);

    if (!spellData) {
      return {
        content: [{
          type: "text" as const,
          text: `Failed to fetch spell with ID ${spellId}.`,
        }],
      };
    }

    return {
      content: [{
        type: "text" as const,
        text: `**${spellData.name}**
- Icon Name: ${spellData.icon}
- Icon URL: ${spellData.iconUrl}
- Wowhead URL: ${spellData.wowheadUrl}`,
      }],
    };
  }
);

server.tool(
  "list_known_spells",
  "List all spells in the known spells database, optionally filtered by class.",
  {
    classFilter: z.string().optional().describe("Filter by class name (e.g., 'Warrior', 'Mage')"),
  },
  async (args) => {
    // Group spells by rough class based on spell names
    const classGroups: Record<string, string[]> = {
      "Mage": ["frostbolt", "frost nova", "fireball", "polymorph", "blink", "ice block", "counterspell", "arcane intellect"],
      "Priest": ["flash heal", "mind blast", "shadow word: pain", "power word: shield", "power word: fortitude", "psychic scream", "dispel magic", "renew"],
      "Warrior": ["charge", "heroic strike", "mortal strike", "rend", "hamstring", "pummel", "execute", "intercept", "intimidating shout"],
      "Rogue": ["ambush", "sinister strike", "kidney shot", "kick", "gouge", "backstab", "eviscerate", "cheap shot", "vanish", "sprint"],
      "Warlock": ["shadow bolt", "corruption", "fear", "curse of agony", "immolate", "drain life", "death coil", "howl of terror", "life tap"],
      "Paladin": ["holy light", "flash of light", "hammer of justice", "blessing of freedom", "divine shield", "lay on hands", "cleanse"],
      "Hunter": ["aimed shot", "multi-shot", "arcane shot", "serpent sting", "concussive shot", "scatter shot", "freezing trap"],
      "Druid": ["moonfire", "wrath", "rejuvenation", "regrowth", "entangling roots", "hibernate", "cyclone", "bash"],
      "Shaman": ["lightning bolt", "chain lightning", "earth shock", "flame shock", "frost shock", "healing wave", "lesser healing wave", "purge"],
    };

    let output = "# Known Spells Database\n\n";

    for (const [className, spells] of Object.entries(classGroups)) {
      if (args.classFilter && !className.toLowerCase().includes(args.classFilter.toLowerCase())) {
        continue;
      }

      output += `## ${className}\n`;
      for (const spell of spells) {
        const id = KNOWN_SPELLS[spell];
        output += `- ${spell} (ID: ${id})\n`;
      }
      output += "\n";
    }

    return {
      content: [{
        type: "text" as const,
        text: output,
      }],
    };
  }
);

// ============================================================================
// Item Tools
// ============================================================================

server.tool(
  "lookup_item_by_id",
  "Look up detailed WoW Classic item data by Wowhead item ID. Returns stats, slot, armor type, damage, speed, bonus stats, equip effects, icon URL, and quality.",
  {
    itemId: z.number().describe("The Wowhead item ID"),
  },
  async (args) => {
    const itemData = await fetchItemById(args.itemId);

    if (!itemData) {
      return {
        content: [{
          type: "text" as const,
          text: `Item with ID ${args.itemId} not found.`,
        }],
      };
    }

    return {
      content: [{
        type: "text" as const,
        text: formatItemOutput(itemData),
      }],
    };
  }
);

server.tool(
  "lookup_item",
  "Look up WoW Classic item data by name. Returns stats, slot, armor type, damage, speed, bonus stats, equip effects, icon URL, and quality.",
  {
    itemName: z.string().describe("The item name to search for"),
  },
  async (args) => {
    const itemId = findItemId(args.itemName);

    if (!itemId) {
      const suggestions = Object.keys(KNOWN_ITEMS)
        .filter(name => name.includes(args.itemName.toLowerCase()))
        .slice(0, 5);

      return {
        content: [{
          type: "text" as const,
          text: `Item "${args.itemName}" not found in known items database.${
            suggestions.length > 0
              ? `\n\nDid you mean: ${suggestions.join(", ")}?`
              : ""
          }\n\nYou can also use lookup_item_by_id with a Wowhead item ID directly.`,
        }],
      };
    }

    const itemData = await fetchItemById(itemId);

    if (!itemData) {
      return {
        content: [{
          type: "text" as const,
          text: `Found item ID ${itemId} for "${args.itemName}" but failed to fetch data from Wowhead.`,
        }],
      };
    }

    return {
      content: [{
        type: "text" as const,
        text: formatItemOutput(itemData, itemId),
      }],
    };
  }
);

server.tool(
  "get_item_icon",
  "Get the icon URL and icon name for a WoW Classic item. Useful for downloading equipment icons. Pass item name or numeric ID as string.",
  {
    itemIdOrName: z.string().describe("Item ID (number) or name (string)"),
  },
  async (args) => {
    let itemId: number;

    const numericId = parseInt(args.itemIdOrName, 10);
    if (!isNaN(numericId) && numericId > 0) {
      itemId = numericId;
    } else {
      const foundId = findItemId(args.itemIdOrName);
      if (!foundId) {
        return {
          content: [{
            type: "text" as const,
            text: `Item "${args.itemIdOrName}" not found.`,
          }],
        };
      }
      itemId = foundId;
    }

    const itemData = await fetchItemById(itemId);

    if (!itemData) {
      return {
        content: [{
          type: "text" as const,
          text: `Failed to fetch item with ID ${itemId}.`,
        }],
      };
    }

    return {
      content: [{
        type: "text" as const,
        text: `**${itemData.name}** (${itemData.quality})
- Icon Name: ${itemData.icon}
- Icon URL: ${itemData.iconUrl}
- Wowhead URL: ${itemData.wowheadUrl}`,
      }],
    };
  }
);

server.tool(
  "list_known_items",
  "List all items in the known items database, optionally filtered by armor type or slot.",
  {
    typeFilter: z.string().optional().describe("Filter by category (e.g., 'Plate', 'Mail', 'Leather', 'Cloth', 'Weapons', 'Accessories')"),
    slotFilter: z.string().optional().describe("Filter by slot (e.g., 'Head', 'Chest', 'Two-Hand', 'Ring')"),
  },
  async (args) => {
    let output = "# Known Items Database\n\n";

    for (const [category, slots] of Object.entries(ITEM_GROUPS)) {
      if (args.typeFilter && !category.toLowerCase().includes(args.typeFilter.toLowerCase())) {
        continue;
      }

      let categoryOutput = "";
      for (const [slot, items] of Object.entries(slots)) {
        if (args.slotFilter && !slot.toLowerCase().includes(args.slotFilter.toLowerCase())) {
          continue;
        }

        categoryOutput += `### ${slot}\n`;
        for (const item of items) {
          const id = KNOWN_ITEMS[item];
          categoryOutput += `- ${item} (ID: ${id})\n`;
        }
        categoryOutput += "\n";
      }

      if (categoryOutput) {
        output += `## ${category}\n${categoryOutput}`;
      }
    }

    return {
      content: [{
        type: "text" as const,
        text: output,
      }],
    };
  }
);

// Start the server
async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("Wowhead Classic MCP server running");
}

main().catch((error) => {
  console.error("Fatal error:", error);
  process.exit(1);
});
