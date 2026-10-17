# AS-333: Baseline kit visuals — Client-Data Research

This doc covers Life Tap, the four weapon poisons, Shadow Word: Pain, Shadowform, and the
Dodge/Parry combat text. Findings come from two builds:

- Classic Era **1.15.9.69547**;
- TBC Classic **2.5.6.69795**.

The sources are the wago.tools DB2 CSVs, joined directly; M2 files fetched from CASC and
parsed; and Blizzard's own FrameXML Lua and the `GlobalStrings` DB2 for the combat text.
No wiki or prose source was used as evidence. Every row below comes from a join that was
run. Where a join could not be made, the row says **UNRESOLVED**.

## Method

- **Inventory (skill-line spells).** `scripts/db2_spell_sweep.py --build <b> --skill-line
  40 355 78 --name <n> --events --caster-anims` was run at both builds for each name.
  Skill lines: 40 Poisons, 355 Affliction, 78 Shadow Magic. Every name resolves at
  Probability 1 to one `SpellVisual` (property 4 **HELD**), with these counts:
  - Life Tap: `2 = 1 + 1` (Improved Life Tap is NO-VISUAL).
  - Shadow Word: Pain: `2 = 1 + 1`.
  - Shadowform: `1 = 0 + 1`.
  - Deadly Poison: `5 = 0 + 5` in Era, `7 = 0 + 7` in TBC.
  - Wound Poison: `4 = 0 + 4` in Era, `5 = 0 + 5` in TBC.
  - Mind-numbing Poison: `3 = 0 + 3`.
  - Crippling Poison: `2 = 0 + 2`.

  Each run is narrowed by `--name`, so these are claims about the named spells only, not
  about the whole skill lines.
- **Helper spells (the poison procs).** These sit on no skill line, so the chain was
  joined from the application spell instead:
  - the application spell carries `SpellEffect.Effect 54` (enchant item temporary);
  - its `EffectMiscValue_0` is a `SpellItemEnchantment` row;
  - that row's `Effect_n = 1` (combat spell) gives an `EffectArg_n` proc SpellID;
  - the proc SpellID then goes `SpellXSpellVisual → …` as usual.

  This is how the proc IDs below were found. None of them was taken on trust.
- **Joins.** The chain is `SpellXSpellVisual → SpellVisual → SpellVisualEvent →
  SpellVisualKitEffect`, joined on column `Effect`. The effect types are:
  - type 2 is a model: `SpellVisualKitModelAttach → SpellVisualEffectName.ModelFileDataID`;
  - type 6 is a body animation (`SpellVisualAnim`);
  - type 1 is a procedural effect (`SpellProceduralEffect`);
  - type 5 is a sound and is ignored.

  Event pairs follow the established vocabulary:
  - `(1,2)` precast loop;
  - `(3,13)` cast;
  - `(6,13)` one-shot impact;
  - `(7,8)` sustained aura state.

  As `2026-09-06-warlock-dot-client-data.md` established, no event pair fires per
  periodic tick.
- **M2 parser** (v274, MD21-chunked). It was validated by re-deriving
  `slow_impact_base.m2` exactly as `2026-09-26-slow-state-client-data.md` publishes it:
  rates 65.1 and 14, lives 0.7 and 0.395, and the yellow → violet colour track.
  - The Era copies of most spell models carry an empty name string.
  - The TBC copies carry names, and they parse: `Poison_Impact_Chest`,
    `Shadow_Precast_Low_Hand`, `ShadowWordPain_Chest` (both builds), `PoisonDrip`.
  - **None of the models in this doc has a `GPID` chunk.** Every emitter is a sprite;
    there are no particle meshes.
- **Units and conventions.**
  - Model units ≈ yards.
  - Particle colours are RGB 0–255 over normalised particle life, and alpha is 0–1.
  - Emitter blend values: 2 = alpha blend, 4 = additive.
  - Attachments: 19 = Base (feet), 20 = Head, 21/22 = spell hands, 34 = Chest (community
    enum).
- **Scripts** are under the AS-333 scratchpad (`as333/`):
  - `db.py` loads the CSVs and fetches anything missing into `.db2-cache` with curl;
  - `chain.py` is the full spell → kit → model chain;
  - `poisons.py` is the enchant → proc join;
  - `reverse.py` is the model → kit → visual → spell reverse join;
  - `proctypes.py` surveys `SpellProceduralEffect` by type;
  - `m2.py` is the M2 and skin dumper.

## Summary table

| Effect | Client spell → visual | Cast side | Landing (one-shot) | Sustained `(7,8)` state | TBC |
|---|---|---|---|---|---|
| Life Tap | 1454…11689 (+27222 TBC) → **1225** | kits 217/218: `shadow_precast_low_hand.m2` on both hands; anims 52/54 (Omni) | kit 1167: `lifetap.m2` @Chest, 3334 ms | **none** | identical (same model bytes) |
| Deadly Poison (proc) | 2818, 2819, 11353, 11354, 25349 (+26968, 27187 TBC) → **19** (Era) / **5100** (TBC) | none (a proc has no cast) | kit 3031: `poison_impact_chest.m2` @Chest | **none** | same kit and model |
| Wound Poison (proc) | 13218, 13222, 13223, 13224 (+27189 TBC) → 19 / 5100 | none | kit 3031 (same) | **none** | same |
| Mind-numbing Poison (proc) | 5760, 8692, 11398 → 19 / 5100 | none | kit 3031 (same) | **none** | same |
| Crippling Poison (proc, shipped) | 3409, 11201 → 19 / 5100 | none | kit 3031 (same) | **none** | same |
| Poison application (skill line) | 2835… / 13220… / 5763… / 3420… → **92** | kit 163: `potionb_spellobject.m2` @21 and `potiona_spellobject.m2` @22, anim 123 | — | — | same |
| Weapon coating (enchant) | enchants 7/8/22/35/603/626/627/643/703–706/2630 (+2642–2644 TBC) → ItemVisual **26** | `spells/enchantments/poisondrip.m2` on the weapon | — | while enchanted | same |
| Shadow Word: Pain | 589…10894 (+2 TBC ranks) → **71** | kits 217/218 (the same shadow hands as Life Tap) | kit 304: `shadowwordpain_chest.m2` @Chest, 900 ms | **none** | identical apart from model flags |
| Shadowform | 15473 → **3619** | none | kit 6436 is **sound only** | kit 3129: `shadow_precast_med_base.m2` @Base, **body tint `#270042`**, **transparency 0.65** | TBC carries only the TargetType-2 `(7,8)` row |

### Four findings that shape the work

1. **All four weapon poisons land the same visual.** Every proc spell (Deadly, Wound,
   Mind-numbing, Crippling, every rank, both builds) resolves to the one impact kit 3031,
   `poison_impact_chest.m2`. The client gives each poison **no visual identity of its
   own** on the victim. Era visual 19 is shared by 108 spells (NPC poisons, Slow Poison,
   Aura of Rot …). TBC moved the player procs to visual 5100 (36 spells in TBC), which
   carries the same kit and nothing else.
2. **No effect here has a persistent victim state.** The poison procs, Shadow Word: Pain
   and Life Tap all have no `(7,8)` row. Shadowform's state sits on the Priest itself,
   not on a victim.
3. **Shadowform is a tint, a transparency AND an attachment**, all inside one `(7,8)` kit:
   - a procedural type 1 colour, `#270042`;
   - a procedural type 14 alpha of 0.65;
   - a feet-anchored flame and smoke particle model.
4. **Life Tap and Shadow Word: Pain cast with the hands the Warlock already uses.** Kits
   217/218 attach `shadow_precast_low_hand.m2`, the same model as kits 114/118
   (Corruption, Shadow Bolt). Only the body anim differs: Omni 52/54 instead of Directed
   51/53, as recorded in the warlock DoT doc §5.

---

## 1. Life Tap

**Chain:** Life Tap ranks 1454, 1455, 1456, 11687, 11688, 11689 (TBC adds 27222) →
`SpellXSpellVisual` → visual **1225**. It has no missile (`HasMissile 0`). Events:

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 217 | anim loop **52** (ReadySpellOmni); `shadow_precast_low_hand.m2` (fdid 166807) @21 and @22 |
| (3,13) cast | 1 | 218 | anim loop **54** (SpellCastOmni); the same hand model @21/@22 |
| (6,13) impact | 1 and 2 | 1167 | `spells/lifetap.m2` (fdid 166470) @**34 Chest**, scale 1; sound |

Life Tap is a self-cast, so the impact plays on the Warlock's own chest.

### `lifetap.m2` (impact, @Chest)

- **Sequence.** One sequence of **3334 ms**, with a global loop of 833 ms.
- **Mesh.** 8 vertices: two **camera-facing quads** (root bone flag `0x8`, spherical
  billboard). One is 1.84 × 2.38 yd and the other 1.66 × 2.14 yd, centred on the anchor.
  - Texture `spells/genericglow64.blp`, material blend 4 (additive).
  - Colour track: constant blue `(81,101,255)` = `#5165FF`.
  - Alpha: 0 → **1.0 at 1334 ms** → 0 at 3334 ms.
  - The two bones scale-pulse 0.67 → 0.99 → 0.72 → 0.84 → 0.73 on the 833 ms loop.
  - **Identity: a soft blue glow, about a body tall, that swells over 1.3 s, throbs, and
    fades by 3.3 s.**
- **Four emitters.** All are sprites and none has a GPID.

| # | Texture | Blend | Window (ms) | Rate /s | Life s | Speed yd/s | Size (yd, birth → mid → death) | Colour over life | Reading |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `clouds8x8fade` (8×8 atlas) | 2 | 0–334 | 100 | 1.0 | 1.94 (drag 8) | 0.11 → 0.44 → 1.11 | green `(67,177,0)` → `(67,177,0)` → `(30,0,0)` | a green puff that bursts out and stalls |
| P1 | `flare` | 2 | 34–334 | 100 | 1.0 | 1.39 | 0.08 → 0.06 → 0.03 | green `(39,165,0)` → dark red | green sparks |
| P2 | `star4` | 4 | gated on and off: 1000–1167, 1334–1500, 1634–1834, 2067… | 50 | 0.2 | 0.28 | 0.014 → 0.11 → 0.014, spin −4 | cyan `(0,199,255)` → pale `(155,216,255)` | blinking cyan twinkles in a 0.56 × 1.11 yd area |
| P3 | `blue_glow2` | 4 | 834–2167 | 50 | 1.0 | **−2.22 (inward)** | 0.03 → 0.14 → 0.28 | **red `(255,0,0)` → blue `(63,179,255)`** | motes that converge from a 1.39 yd area, turning red to blue |

- **Read as a sequence:** a green burst at the chest (0–0.33 s). Then red motes are drawn
  inward and turn blue (0.83–2.17 s), while cyan twinkles blink and the blue glow swells
  and fades. The red → blue pull is the health-into-mana identity.
- **Era vs TBC:** the model is byte-identical (same md5) and the kits are identical.

### `shadow_precast_low_hand.m2` (cast hands — already documented)

The model is described in `2026-09-06-warlock-dot-client-data.md` §5, and this parse
re-derives it:

- **P0:** `fire1a2` green `(32,255,2)` → black licks, 10.2/s, life 1.2 s.
- **Two `shockwave10` rings** at 5/s, life 0.5 s: one green `(18,255,0)`, one violet
  `(114,0,255)`.

The TBC copy differs only in its bounding box.

### What this means for design (Life Tap)

- The cast side is the shipped Warlock hand. Only the body anim pair (Omni) is specific.
- The landing is a self-only, one-shot chest event of about 3.3 s. It claims **no
  sustained channel**.
- Its colour story (green burst, red → blue converging motes, blue glow) is distinct from
  every heal landing documented so far.

---

## 2. Weapon poisons: on hit, and the victim state

### The proc join (both builds)

| Application spell (skill line 40) | Enchant | Proc spell (lands on victim) | Proc effect | Stacks |
|---|---|---|---|---|
| Deadly Poison 2823 / II 2824 / III 11355 / IV 11356 / V 25351 | 7 / 8 / 626 / 627 / 2630 | **2818 / 2819 / 11353 / 11354 / 25349** | `Effect 6`, aura 3 (periodic damage), 3000 ms period | 5 |
| (TBC) Deadly VI 26967 / VII 27186 | 2642 / 2643 | **26968 / 27187** | the same | 5 |
| Wound Poison 13219 / 13225 / 13226 / 13227 | 703 / 704 / 705 / 706 | **13218 / 13222 / 13223 / 13224** | Era: aura **115** (flat healing taken), −56 … −136 | 5 |
| | | | TBC: aura **118** (healing taken %), −11, plus a direct damage effect (16 … 52) | 5 |
| (TBC) Wound Poison V 27188 | 2644 | **27189** | aura 118, −11, plus 64 damage | 5 |
| Mind-numbing 5761 / II 8693 / III 11399 | 35 / 23 / 643 | **5760 / 8692 / 11398** | Era aura 65; TBC aura 216 (cast-speed slow) | — |
| Crippling 3408 / II 11202 | 22 / 603 | **3409 / 11201** | aura 33 (move-speed slow), −51 / −71 | — |

The IDs in the brief (2818, 13218, 5760, 3409) are confirmed by this join.

### Victim visual: one kit for all of them

Every proc spell above has exactly one `SpellXSpellVisual` row at Probability 1:

| Build | Visual | Events | Kit | Model |
|---|---|---|---|---|
| Era | **19** | (1,2) kit 36 (nature precast hands, anim 51); (3,13) kit 37 (nature cast hands, anim 53); **(6,13) TargetType 1+2 kit 3031** | 3031 | `spells/poison_impact_chest.m2` (fdid 166643) @**34 Chest** |
| TBC | **5100** | **(6,13) TargetType 1+2 kit 3031** only | 3031 | same fdid 166643 |

- **The precast and cast rows of visual 19 do not draw for a proc.** A proc has no cast,
  so they never play. Visual 19 is a generic visual shared by 108 Era spells. TBC's 5100
  holds the impact row alone, so the victim sees the same thing in both builds.
- **No proc visual has a `(7,8)` row. The client draws no persistent "poisoned" state**
  for Deadly, Wound, Mind-numbing or Crippling Poison. The DoT ticks of Deadly Poison draw
  nothing, since no event fires per tick.

### `poison_impact_chest.m2` (kit 3031, @Chest)

- **Sequences.** Seq 0 (Stand) is **3000 ms**; seq 1 (id 158) is 4000 ms. All emitters
  are sprites and there are no meshes.

| # | Texture | Blend | Window (seq 0) | Rate /s | Life s | Speed yd/s | Spawn | Size (yd) | Colour over life | Alpha |
|---|---|---|---|---|---|---|---|---|---|---|
| P0 | `clouds8x8fade` (8×8) | 2 | 0–3000 ms | 20 | **2.5** | 0.28 (±0.5), full sphere | radius 0.28 | 0.22 → 0.31 → 0.69 | `(18,55,0)` → `(106,146,0)` → `(163,196,64)` | 0.39 → 1 → 0 |
| P1 | `flare` | 2 | **0–167 ms** | **150** | 0.6 | **3.33**, sphere | 0.28 × 0.28 | 0.11 → 0.06 → 0.03 | `(49,102,1)` → `(99,136,20)` → `(115,152,1)` | 1 → 1 → 0 |
| P2 | `flare` | **4** | 0–3000 ms | 20 | 2.5 | 0.28 (±0.5), sphere | radius 0.28 | 0.056 constant | as P0 | 0.39 → 1 → 0 |

- **The burst:** about 25 green flares (150/s × 0.167 s) spray out at 3.3 yd/s. They reach
  a radius of about 2 yd and die by 0.6 s.
- **The cloud:** two slow emitters of green cloud puffs plus pin-point green motes, 20/s
  each. They drift at 0.28 yd/s and swell from 0.22 to 0.69 yd as they go
  olive → yellow-green.
- **Lifetime.** The puffs live 2.5 s, so one impact keeps a wisping cloud at the chest
  for **up to about 5.5 s** (3 s of emission plus 2.5 s of particle life). Whether the
  kit stops emission at the end of seq 0 or the client cuts it earlier is not in the
  tables (UNRESOLVED).
- **Identity: a green spray, then a lingering olive-green chest cloud.**
- **Era vs TBC:**
  - The TBC copy swaps P1's texture to `item/objectcomponents/weapon/flare.blp`;
    Era uses `spells/flare.blp`.
  - Emitter flag bits differ (P0 `0x82000a` vs `0x20009`).
  - The bounding box differs.
  - Rates, lives, sizes and colours are identical.

### Is there a generic "poisoned" state anywhere?

The reverse join from model to kit to visual to spell (`reverse.py`) was run over every
poison-named model under `spells/`:

- **`poison_impact_chest.m2` is in a `(7,8)` state in exactly one place:** the Season of
  Discovery spell Rapid Decomposition (1222430). Everywhere else it is a one-shot `(6,13)`
  impact. Among era IDs, those impacts include visual 19 (108 spells), Paralyzing Poison,
  Tranquilizing Poison, Wyvern Sting and Intoxicating Venom.
- **`poison_impactdot_med_chest.m2` (SVEN 236, @Chest) is the client's sustained poison
  state model.** In the era range it is used only by Itch (25185) in kit 6501. Its other
  `(7,8)` users are SoD spells: Corrosive Breath, Lethargic Poison and Subjugated Spinners.
  No player poison uses it.
- **`poison_impactdot_med_base.m2` (SVEN 620)** is a one-shot `(6,13)` impact (kit 805).
  - It covers NPC diseases and poisons: Rabies, Slowing Poison, Poisoned Shot, Plague
    Cloud, and the NPC copies of Mind-numbing (25810) and Crippling (25809).
  - Its only era `(7,8)` user is Paralyze (25725).
- **Green body tints exist for NPC poisons, never for player poisons.**
  `SpellProceduralEffect` type 1 colours in `(7,8)` kits include:
  - `#00FF00` (kit 221: Abomination Spit, Acid Splash, Bottle of Poison …);
  - `#049931` (kits 564 and 784: Acid Spit, Black Rot, Corrosive Acid …).

  None of these kits is reachable from a player poison proc.

**Conclusion:** the client has sustained poison-state assets (a chest model and green
body tints), but **the four Rogue poisons use none of them**. A Rogue's poison is visible
only as the impact spray-and-cloud each time it lands.

### Wound Poison against Mortal Strike

| | Wound Poison proc | Mortal Strike (12294 → visual 39) |
|---|---|---|
| Landing kit | 3031 `poison_impact_chest.m2` @Chest | 437 `decisivestrike_impact_chest.m2` (fdid 165913) @Chest, plus victim anim 9 |
| Duration | 3000 ms emission, puffs live 2.5 s | **667 ms** |
| Palette | olive / yellow-green | **violet `(114,35,159)` → red `(190,0,9)` → violet** flare sparks (50/s for 67 ms, tail 10); violet-blue `fireplume64` streaks at 5.56 yd/s; a red → violet `whiteringthin128` ring growing to 1.67 |
| Healing-reduction state | none | none (MS has no `(7,8)` row either) |
| Cast side | none | kit 324: anim 57 plus procedural type 8 (`#F82829`, values 20/600/100; meaning UNRESOLVED) |

**They do not resemble each other.** The client draws Wound Poison as just another
poison, with nothing that marks its healing reduction. Mortal Strike's identity is a
short red and violet strike burst. Neither has a persistent "wounded" state in the client.

### Visual 92: the weapon coating (one line)

- **The apply cast** (kit 163, precast only) holds `potionb_spellobject.m2` in the right
  hand and `potiona_spellobject.m2` in the left, with anim loop 123 (name UNRESOLVED).
- **The coated weapon** carries `SpellItemEnchantment.ItemVisual` **26** for every poison
  enchant at both builds. Its `ItemVisuals.Slot_4` is fdid 166007,
  `spells/enchantments/poisondrip.m2`:
  - green drips `(27,141,0)` → `(161,221,0)`, additive;
  - 0.56 → 0.03 yd, falling under gravity 5.56 for 0.35 s;
  - bursts at 10/s during 0–167 ms and 700–1300 ms, and 5/s during 3400–3834 ms, on a
    5167 ms loop.

### Our current state, for reference

- Crippling Poison's proc already gets a one-shot landing. It uses the **shared Nature
  landing at the chest** (`rendering/effects/slow_ring.rs`, header comment), standing in
  for the client's `poison_impact_chest.m2`.
- The slow itself is the movement-layer ring that `2026-09-26-slow-state-client-data.md`
  decided on.
- The DoT drip indicators in `rendering/effects/affliction.rs` (green poison drops) are
  our own invention. The client has no counterpart for a player poison.

### What this means for design (poisons)

- **Persistent victim state:** none in the client, for any poison. A poison landing costs
  a one-shot apply flash, which is off the sustained-channel budget.
- **There is no client source for telling poisons apart.** All four share one green impact.
  Any per-poison identity would be authored, not transcribed.
- **Frequency:** Deadly and Wound stack to 5 and re-proc on every qualifying hit. A Rogue
  in melee may replay the impact often enough that the 2.5 s puffs overlap. That would
  make a one-shot read close to continuous. This is an inference from the numbers and has
  not been measured.
- Wound Poison has no client visual tied to healing reduction, and none that resembles
  Mortal Strike's.

---

## 3. Shadow Word: Pain

**Chain:** 589, 594, 970, 992, 2767, 10892, 10893, 10894 (TBC adds 2 ranks) → visual
**71**, with no missile. Visual 71 is shared by 52 spells in Era and 71 in TBC.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 217 | anim 52 (ReadySpellOmni); `shadow_precast_low_hand.m2` @21/@22 |
| (3,13) cast | 1 | 218 | anim 54 (SpellCastOmni); the same hands |
| (6,13) impact | 1 and 2 | 304 | `spells/shadowwordpain_chest.m2` (fdid 166835) @**34 Chest**; sound |
| (7,8) | — | — | **none** |

**No sustained state, and no tick visual.** This matches the warlock DoT doc's
observation that SW:Pain is invisible while applied.

### `shadowwordpain_chest.m2` (impact, @Chest)

- One sequence of **900 ms**, with ten emitters. All are sprites.
- Almost everything fires in a burst at **233 ms**.

| # | Texture | Blend | Window (ms) | Rate /s | Life s | Speed | Size (yd) | Colour | Reading |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `toonsmoke16` (8×8) | 2 | 233–733 | 40 | 0.66 | 0.64, drag 2, spin 5 | 0.17 → 0.27 → 0.43 | black → grey `(84,84,83)` → `(192,192,190)` | dark smoke |
| P9 | `toonsmoke16` | 2 | 233–900 | 60 | **1.44** | 1.10, drag 2, spin −5 | 0.17 → 0.24 → 0.31 | black → grey | dark smoke (the longest tail) |
| P1, P2 | `blobs_black`, `blobs_black2` | 2 | 233–433 / 233–400 | 40 | 1.0 | 1.38 / 1.24, **tail 10** | 0.02 → 0.16 → 0.17 | deep indigo `(4,0,84)` → black → indigo | streaking black blobs |
| P3 | `flare` | 2 | 700–900 | 50 | 1.0 | 1.11, tail 10 | 0.04 → 0.09 → 0.04 | indigo → black | late dark sparks |
| P4 | `gradient64flipb` | 4 | 0–167 | 103.8 | 0.375 | **−4.58 (inward)**, tail 3 | 0 → 0.05 → 0 | violet `(177,42,240)` → magenta `(255,0,216)` | violet streaks sucked in from a 1.11 × 1.56 area |
| P5 | `star5a` | 4 | 233–500 | 48.2 | 1.0 | 1.88, tail 10, spin −7 | 0.05 → 0.11 → 0.04 | blue `(44,136,240)` → violet `(102,35,225)` → slate | stars |
| P6 | `gradient64flipb` | 2 | 233–400 | 39 | 1.03 | 2.09 | 0 → 0.14 → 0.03 | **black** (alpha ≤ 0.6) | dark streaks |
| P7, P8 | `gradient64flipb` | 4 | 233–400 / 233–467 | 40 / 55 | 0.68 / 0.57 | 2.27 | up to 0.17 / 0.04 | lilac → magenta `(255,35,252)` → indigo | violet streak burst |

- **Identity:** violet streaks rush inward for 0.17 s. Then, at 0.23 s, comes a burst of
  black smoke, black streaking blobs and violet/magenta streaks with blue → violet stars.
  The smoke trails off by about **2.3 s** (P9: emission until 0.9 s plus 1.44 s of life).
  It is a dark, violet-edged hit, not a glow.
- **Era vs TBC:** the same model. TBC drops P4's re-enable at 900 ms, sets the sequence's
  loop flag bit (`0x8a0` → `0x8a1`), and zeroes the bounding box.

### What this means for design (SW:Pain)

- The cast is the shipped shadow hand with the Omni anims.
- The landing is a 0.9 s burst whose smoke lingers for about 2.3 s.
- **No sustained channel.** The client gives the DoT nothing while it runs, the same
  finding as Moonfire and Curse of Agony.

---

## 4. Shadowform

**Chain:** 15473 → visual **3619**, with no precast and no cast kit. Visual 3619 is shared
with NPC spells, and the kit is shared with Nefarian's Barrier, Failsafe Phylactery and
Shahram.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 6436 | **sound only** (type 5); there is no impact model |
| (7,8) state | Era: 1 and 2. **TBC: 2 only** | **3129** | see below |

Kit 3129 effects (identical at both builds):

| Type | Effect | Data |
|---|---|---|
| 2 (model) | 368152 | `spells/shadow_precast_med_base.m2` (fdid 166808) @**19 Base**, offset 0, scale 1 |
| 1 (procedural) | 356913 | **Type 1**, `Value_0` = 2555970 = **`#270042`** (RGB 39, 0, 66: near-black violet); `Value_1` 0.2; `Value_2` 2 |
| 1 (procedural) | 356914 | **Type 14**, `Value_0` = **0.65**; `Value_2` 2 |

### What the procedural types are (from table-wide usage, not from documentation)

- **Type 1 is a body colour tint.** Its `Value_0` packs RGB, and 211 rows use it. Every
  one checked tints a state that the game shows as tinted:
  - Frost Nova and Freeze, `#6666FF`;
  - Chilled, `#3C3CFF` (matching the chill colour in the slow-state doc);
  - Stoneform, `#787878` / `#44465E`;
  - Ghost, `#8CB9FD`;
  - Banish, `#350035`;
  - Berserk and Enrage, `#FF1117`.
- **Type 14 is body transparency**, with `Value_0` the alpha. Its 65 users include:
  - stealth-style kit 312 (Vanish copy, Disguise, Burrow) at 0.3;
  - Ghost (kit 989) at 0.5;
  - Banish (kit 1286) at 0.3;
  - Evasion and Cloaking (kit 3450) at 0.5;
  - Possess at 1.0.
- `Value_1` to `Value_3` on both types (Shadowform: 0.2 / 2 and 0 / 2) have no settled
  meaning. They might be fade times or blend modes. **UNRESOLVED.**

### `shadow_precast_med_base.m2` (the state model, @feet)

- A **1000 ms** loop. There is no mesh, only four sprite emitters.
- The bounding box runs up to **4.75 yd** high and ±2.2 yd wide.

| # | Texture | Blend | Shape | Rate /s | Life s | Speed / gravity | Size (yd) | Colour over life | Alpha |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `flamelicksmallblue` (4×4 atlas) | 4 | sphere, area width 0.69 | 50 | 1.0 | 0.56 up, **gravity −1.39 (rising)**, tail 0.5 | **1.39** → 0.56 → 0.28 | green `(0,104,7)` → violet `(24,0,86)` → `(34,0,119)` | 0 → **0.39** → 0 |
| P1 | `flare` | 4 | sphere, area width 0.42 | 50 | 1.0 | 1.11 up, gravity −1.39 | 0.28 → 0.28 → 0.14 | the same green → violet | 0 → 1 → 0 |
| P2 | `smoke02a` | 4 | plane 0.08 × 0.03 | **0.8** | 3.0 | 1.67 straight up, **tail 6** | 0.42 → 0.56 → 0.83 | sky `(47,139,210)` → indigo `(45,31,179)` → violet `(64,6,150)` | 0 → 1 → 0 |
| P3 | `smoke02_flip` | 4 | plane 0.08 × 0.03 | **0.3** | 3.0 | 1.39 straight up, tail 7 | 0.42 → 0.61 → 0.97 | pale blue → indigo → violet | 0 → 1 → 0 |

- **Reading:** faint, additive flames lick up from the feet. They are born large (1.4 yd)
  and shrink as they rise, going green → dark violet at low alpha. Small flares rise with
  them. About once a second a long smoke streak climbs to roughly 4–5 yd, blue → violet.
- **Era vs TBC:** the model is byte-identical.

### What the client does to the character

- The body is **tinted near-black violet `#270042`**.
- The body is drawn at **65 % opacity**.
- A **violet flame and smoke shroud** sits at the feet.
- All three last for as long as the form is up (`(7,8)` = apply → remove). The impact
  plays a sound and nothing else.

### What this means for design (Shadowform)

- **It is a tint plus transparency plus an attachment, not one of them.**
- The state is on the caster (a self-buff), so it spends a sustained channel on the
  **Priest's own body**, not on a victim.
- **The tint and transparency are body-material changes.** That puts them in contention
  for `OriginalBodyMaterial` with Fear and Polymorph. This is the same constraint that
  ruled out the chill tint in `2026-09-26-slow-state-client-data.md` and that Travel Form
  avoided with its own restore components. A Shadowformed Priest can be feared or
  polymorphed.
- **TBC drops the TargetType-1 copy of the state row.** Era carries both 1 and 2. Which
  viewer each TargetType denotes is not in the tables (UNRESOLVED).

---

## 5. Dodge, Parry, Miss, Block, Immune (combat text)

These are not spells, and **no DB2 table holds combat-text colours.**

- **The colours** come from Blizzard's FrameXML. The source is `Gethe/wow-ui-source`,
  branch `classic_era` at commit `8165d4cd` ("1.15.9 (70003)", 2026-09-29), and tag
  `2.5.6` for TBC.
- **The TBC files are line-for-line identical** to Era's, ignoring CRLF:
  `CombatText.lua`, `CombatTextConstants.lua`, `CombatTextConstantsOverrides.lua` and
  `CombatFeedback.lua`.
- **The wording** is the `GlobalStrings` DB2, which is identical at both builds.

### Wording (GlobalStrings, both builds)

| Key | Text |
|---|---|
| `DODGE` / `COMBAT_TEXT_DODGE` | **Dodge** |
| `PARRY` / `COMBAT_TEXT_PARRY` | **Parry** |
| `MISS` / `COMBAT_TEXT_MISS` | **Miss** |
| `BLOCK` / `COMBAT_TEXT_BLOCK` | **Block**. A partial block is `-<dmg> (%d blocked)` (`BLOCK_TRAILER`) |
| `IMMUNE` / `COMBAT_TEXT_IMMUNE` | **Immune** |
| `EVADE`, `DEFLECT`, `RESIST`, `ABSORB`, `REFLECT` | Evade, Deflect, Resist, Absorb, Reflect. Partials are `(%d absorbed)` / `(%d resisted)` |

The text is title case. The Lua passes it through unchanged, with no upper-casing.

### Victim side (1): scrolling combat text, `Blizzard_CombatText`

- This draws events **on the player's own unit** (or vehicle) only:
  `C_CombatText.SetActiveUnit("player")`.
- It scrolls for `MessageScrollSpeed` **1.9 s** and fades from 1.3 s. Line height is 25;
  a crit pops 30 → 60 → 30 within 0.2 s.
- A message type marked `isStaggered` is offset randomly left and right within
  `StaggerRange` 20.

| Message type | Colour (r,g,b) | Hex | Staggered | Shown when |
|---|---|---|---|---|
| `DODGE`, `PARRY`, `MISS`, `EVADE` (melee) | 1, 0.1, 0.1 | `#FF1A1A` | yes | CVar `floatingCombatTextDodgeParryMiss_v2` |
| `IMMUNE`, `DEFLECT` (melee) | 1, 0.1, 0.1 | `#FF1A1A` | no | the same CVar |
| `BLOCK`, `RESIST`, `ABSORB` (melee) | 1, 0.1, 0.1 | `#FF1A1A` | no | `floatingCombatTextDamageReduction_v2` |
| `SPELL_DODGE`, `SPELL_PARRY`, `SPELL_MISS`, `SPELL_IMMUNE`, `SPELL_EVADE`, `SPELL_DEFLECT`, `SPELL_REFLECT`, `SPELL_BLOCK` | 1, 1, 1 | `#FFFFFF` | no | the matching CVar |
| `SPELL_RESIST`, `SPELL_ABSORB` | 0.79, 0.3, 0.85 | `#C94CD9` | no | `…DamageReduction_v2` |
| for scale: `DAMAGE` (melee taken) | 1, 0.1, 0.1 | `#FF1A1A` | yes | always |
| for scale: `SPELL_DAMAGE` (taken) | 0.79, 0.3, 0.85 | `#C94CD9` | no | always |

When these CVars are off and the event has a partial amount, the event is shown as plain
damage instead.

### Victim side (2): portrait hit indicator, `CombatFeedback.lua`

- It is wired only to `PlayerFrame` (`PlayerHitIndicator`) and `PetFrame`
  (`PetHitIndicator`), with base font height 30. `TargetFrame` and `PartyMemberFrame`
  do not call it.
- It fades in over 0.2 s, holds 0.7 s, and fades out over 0.3 s, so each message lasts
  **1.2 s**.

| Event | Colour | Size |
|---|---|---|
| Dodge, Parry, Miss, Evade, Deflect, Reflect, Interrupt | white `#FFFFFF` | ×1.0 |
| Block, Absorb, Resist (as wound flags or events) | white | ×0.75 |
| Immune | white | **×0.5** |
| for scale: physical damage taken | white | ×1.0; crit and crushing ×1.5; glancing ×0.75 |
| for scale: non-physical damage taken | yellow `#FFFF00` | the same |
| for scale: heal / energize | green `#00FF00` / `(0.41,0.8,0.94)` `#69CCF0` | crit ×1.5 |

### Attacker side (the text over the enemy you swing at)

**UNRESOLVED.** The in-world floating numbers and words over a target are drawn by the
client engine, not by FrameXML. Classic Era has no Lua that sets their colour or wording,
and no DB2 table that holds them was found. The one Lua-side fact: in both victim-side
displays, Dodge, Parry and Miss are the same `GlobalStrings` words.

### What this means for design (combat text)

- The client words are **Dodge, Parry, Miss, Block, Immune**, in title case.
- Blizzard's own victim-side colours are:
  - **red `#FF1A1A`** for melee avoidance in the scrolling text;
  - **white** for spell avoidance and for the portrait indicator.
- Immune is drawn at half size on the portrait.
- Note: the project's FCT colour budget reserves yellow, green and light blue
  (`color-budget-principle`). Red and white are outside that reserve.

---

## Sustained channels per body (budget count)

- **On a victim:** none of the five effects adds one. The poisons, SW:Pain and Life Tap
  are one-shot in the client.
- **On the caster:** Shadowform adds one self-state (model, tint and transparency) for as
  long as the form is up. Its tint and transparency are body-material writes.
- **The client's own sustained poison assets go unused by player poisons.** These are
  `poison_impactdot_med_chest.m2` and the green tints `#00FF00` / `#049931`. They exist
  only on NPC spells.

## Unresolved

1. **Who TargetType 1 and 2 denote** on a `(7,8)` row. This matters because TBC
   Shadowform carries only TargetType 2.
2. **`SpellProceduralEffect` `Value_1` to `Value_3`** for type 1 (tint) and type 14
   (transparency). For Shadowform these are 0.2 / 2 / 0 and 0 / 2 / 0.
3. **The name of anim 123**, which the poison-application kit 163 loops. It is not in the
   sweep's anim map. Also the meaning of **procedural type 8** on Mortal Strike's cast kit
   (`#F82829`, values 20 / 600 / 100), which is plausibly a weapon glow or trail but
   unconfirmed.
4. **How long the client keeps a `(6,13)` impact kit alive** against
   `poison_impact_chest.m2`'s 3000 ms sequence and 2.5 s puff life. Does a re-proc
   restart the model or stack a second one?
5. **Attacker-side combat text** (the engine-drawn in-world numbers): colour, wording and
   motion.
6. **Which weapon attachment `ItemVisuals.Slot_4` denotes** (poison drip placement on the
   blade).
7. Texture BLPs (`genericglow64`, `clouds8x8fade`, `flamelicksmallblue`, `smoke02a`,
   `blobs_black`) were not decoded. The colours above are the particle and colour tracks,
   not texture pixels.
