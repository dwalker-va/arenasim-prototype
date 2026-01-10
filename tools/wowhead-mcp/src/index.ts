#!/usr/bin/env node
/**
 * Wowhead Classic MCP Server
 *
 * Provides tools for looking up WoW Classic spell data from Wowhead.
 * Returns structured data matching our AbilityDefinition format.
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

// Wowhead API endpoints
const TOOLTIP_API = "https://nether.wowhead.com/classic/tooltip/spell";
const ICON_BASE_URL = "https://wow.zamimg.com/images/wow/icons/large";
const WOWHEAD_SPELL_URL = "https://www.wowhead.com/classic/spell";

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
    const response = await fetch(`${TOOLTIP_API}/${spellId}`);
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
