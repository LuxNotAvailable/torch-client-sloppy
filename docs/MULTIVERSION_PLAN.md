# Multi-version plan: 26.3 + 1.21.1, vanilla and NeoForge

What has to change to join four kinds of server, and how much of it can be
generated instead of written by hand.

| Target | Protocol | Status today |
|---|---|---|
| 26.3 vanilla | 777 | native (azalea speaks it) |
| 26.3 NeoForge (26.3.x) | 777 | handshake landed (checklist 15d, 15e), waiting on a live run |
| 1.21.1 vanilla | 767 | not joinable |
| 1.21.1 NeoForge (21.1.x) | 767 | not joinable |

Facts marked *(confirm)* are from memory of the version history, not yet
checked against a decompile. Check each one in `reference/` before writing
its rewrite.

---

## 1. What already exists

The client already translates older servers. Read `src/protocol/hop.rs`
(recipe at the top) before anything here.

- **One renderer, one asset set.** The game always runs as 26.3. Older
  servers are translated packet by packet into 26.3 before azalea reads
  them, and our packets back out. Assets, models, block info and GUI stay
  26.3. Nothing outside `src/protocol/` needs to know the server's version,
  except the few places listed in section 4.6.
- **Hops chain.** `Translator::for_protocol` (`hop.rs:419`) picks a list of
  hops: 775 runs `[v775]`, 774 runs `[v774, v775]`. Each hop is a `HopSpec`:
  wire and native packet tables, forward and back id remaps, an item
  component walker, and named inbound and outbound rewrites. Packets with no
  rewrite pass through on the id remap alone.
- **Tables are generated.** `tools/protogen.py` reads a decompiled version
  plus its data generator reports and writes:
  - `packets-<ver>.json`: packet names and ids per phase and direction.
  - `remap-<a>-<b>.json`: id runs for 16 id spaces (blocks, states, items,
    entities, components, serializers, sounds, particles, menus, ...).
    Matching is by **exact name** (and exact property set for states).
- **Version list.** `src/protocol/version.rs`: `VERSIONS`, `NATIVE`,
  `TRANSLATED`, `metadata_matches_native`. The version picker reads
  `selectable()`, so it picks new versions up by itself.
- **Download tooling.** `tools/dump_game_data.py` already fetches and
  SHA-1-checks official server jars from Mojang's manifest.
- **NeoForge.** `src/client/neoforge.rs` runs the 26.3.x handshake;
  `src/protocol/neoforge.rs` holds the payload codecs;
  `src/protocol/registries.rs` holds the name/id tables a frozen registry
  snapshot replaces. Reference: `reference/neoforge-26.3/`.

---

## 2. The shape of the 1.21.1 work

1.21.1 (767) sits seven protocol steps below 1.21.11 (774):

| Version | Protocol |
|---|---|
| 1.21.1 | 767 |
| 1.21.2, 1.21.3 | 768 |
| 1.21.4 | 769 |
| 1.21.5 | 770 |
| 1.21.6 | 771 |
| 1.21.7, 1.21.8 | 772 |
| 1.21.9, 1.21.10 | 773 |
| 1.21.11 | 774 |

**Recommendation: one direct hop, 767 to 774.** `protogen.py` pairs any two
versions, so there is no need to build tables or hops for 768 to 773 when
none of them is a target. The chain becomes `[v767, v774, v775]`.

- For: a third of the hop count, no intermediate tables, no intermediate
  shapes to get right that nobody will ever join with.
- Against: one hop has to absorb seven releases, including the two biggest
  protocol releases in this range (1.21.2 and 1.21.5).
- Middle ground: one `HopSpec`, but group its rewrites by the release that
  introduced each change, with a one-line comment naming it. If 1.21.4 or
  1.21.8 is ever wanted, the hop splits along those lines.

Expect this hop to be larger than `V774` and `v775.rs` combined (about 4K
lines today for two small steps).

---

## 3. Change list: shared plumbing

### 3.1 Version registration

Landed, with 767 in `version::EXPERIMENTAL`: joinable and in the version
picker only while Online Options > "1.21.1 (Experimental)" is on
(`experimentalVersions`, off by default). Move 767 to `TRANSLATED` once
section 4 is done.

Tested against a Velocity + NeoForge 21.1.264 server (Create pack): login,
configuration, the NeoForge handshake and play up to walking around loaded
terrain work. `v767.rs` translates so far:

- Login and configuration: `game_profile` renamed `login_finished` (its
  strict flag dropped), `client_information` (particle setting dropped),
  tag ids.
- Packet renames (`HopSpec::renamed`): `set_carried_item`,
  `horse_screen_open`.
- World: `login`/`respawn` (sea level), chunks (heightmaps, unprefixed
  data arrays, state and block entity ids), block and section updates,
  spawn position (with the last dimension).
- Entities: `player_position`, `teleport_entity`, `set_entity_motion`.
- Ours: `player_input` (sneak changes become `player_command` through
  `HopSpec::outbound_as`), `player_command` actions, `use_item_on`, move
  flags, `move_vehicle`, `container_click` (no claimed slots, so the server
  resends), creative slots (item and count only), recipe packets dropped,
  chat checksums.
- Modded ids keep their distance past the native range in every hop
  (`hop::shifted`), and azalea no longer indexes vanilla-only tables with
  them (`vendor/PATCHES.md`).
- NeoForge: the server's own channels are echoed back, a short list of
  channels mods send unregistered is claimed ad hoc (`AD_HOC`), version
  mismatches are learned in either argument order, and retries wait 4 s for
  Velocity's login rate limit.

Next: `set_entity_data` and `add_entity` (generated layouts, 6.3), item
stacks (`patch_767`), `commands`, recipes, advancements.

| File | Change |
|---|---|
| `src/protocol/version.rs` | Add `v("1.21.1", 767)` (and `1.21`, same protocol) to `VERSIONS`; add `767` to `TRANSLATED`; check `metadata_matches_native` still means what it says; extend the tests (`selectable_is_the_joinable_set`, `lookups`, `a_translated_version_is_joinable...`). |
| `src/protocol/hop.rs` | `Hop::v767()`; arm `767 => [v767, v774, v775]` in `Translator::for_protocol`; update the recipe comment. |
| `src/protocol/v767.rs` (new) | The `HopSpec` and its rewrites, like `v775.rs`. |
| `src/protocol/mod.rs` | `mod v767;` |
| `src/protocol/packets.rs` | Loader `PacketTable::v1_21_1` and its `for_protocol` arm. |
| `src/protocol/remap.rs` | Loaders `Remap::v767_to_v774` / `v774_to_v767`. |
| `src/protocol/data/` | `packets-1.21.1.json`, `remap-767-774.json` (generated). |

### 3.2 Reference sources

- `reference/minecraft-1.21.1/`: decompiled source, `version.json`,
  `generated/reports/{registries,blocks}.json`. 1.21.1 is obfuscated, so the
  decompile needs Mojang's official mappings applied first (26.x ships
  unobfuscated, which is why the existing references didn't need this).
- `reference/neoforge-1.21.1/`: NeoForge's `1.21.1` branch, for section 5.

Both come from `tools/new_version.py` (6.1).

### 3.3 `protogen.py` gaps the 1.21.1 jump exposes

Exact-name matching was enough for one release. Across eight it isn't.
Items 1 to 3 have landed.

1. **Renames.** An entry renamed between versions maps to nothing.
   `RENAMES` in `protogen.py` holds old name to new name per space,
   applied both ways, and `RENAME_PREFIXES` a prefix a whole space dropped.
   Only entries confirmed in the decompile go in. For 767 to 774: `chain`
   to `iron_chain` (block, item), every attribute's `generic.`/`player.`/
   `zombie.` prefix, `OPTIONAL_UUID` to `OPTIONAL_LIVING_ENTITY_REFERENCE`
   (serializer), and three sounds (the screaming goat's horn break merged
   into the plain one; leash knot break/place to `item.lead.untied`/`tied`).
2. **Block states that gained properties.** A block that gained a property
   loses every state under exact matching. `landing_by_state` falls back to
   the same block's closest state: shared properties kept, new ones at the
   block's default. Fallback landings may share a target; exact ones may
   not.
3. **More id spaces.** `command_argument_type` is in `CLIENT_REGISTRIES`.
   Its generated 775-to-777 map matches the hand-written
   `argument_type_775_to_777` exactly (with the `color` to `team_color`
   rename). `Remap::parse` ignores spaces it has no `IdSpace` for, so the
   Rust side picks a space up only when a hop needs it.
4. **Hole report.** Each remap run lists the forward holes by name, with a
   rename guess where one name ends like the other. Turn that into a hard
   failure only if holes start slipping through.

What 767 to 774 still leaves unmapped is shape change, so the hop handles
it, not the table:

| Space | Old entry | What became of it | Hop |
|---|---|---|---|
| data component | `hide_tooltip`, `hide_additional_tooltip` | `tooltip_display` | 4.4 |
| data component | `fire_resistant` | `damage_resistant` (a tag) | 4.4 |
| entity type | `boat`, `chest_boat` | one type per wood | 4.3: pick by the boat's variant metadata |
| entity type | `potion` | `splash_potion`, `lingering_potion` | 4.3: pick by the thrown item |
| serializer | `COMPOUND_TAG` | removed (parrot shoulders) | 4.3 |
| sound | `item.goat_horn.play`, `entity.puffer_fish.ambient`, `entity.wolf.howl` | removed | drop the sound |

### 3.4 Per-connection hop state

Every rewrite today is a pure `fn(&Ids, &[u8])`. Several 767 rewrites need
to remember things across packets (section 4.4: sneak state, recipe display
ids, open container ids). Add a per-connection state struct owned by
`Translator` and passed to the rewrites that need it. Keep stateless
rewrites stateless.

### 3.5 Modded ids past the vanilla range

`Runs::get` (`remap.rs:73`) returns `None` for any id at or past the
table's count. That is correct for vanilla, and wrong for a modded server
over a hop: every modded block, item or entity is dropped or turned to air.
The fix has two parts:

- **Shift, don't drop.** An id at or past the old vanilla count maps to
  `id - old_count + native_count`, so modded ids land past native's vanilla
  range, which is where the 26.3 path already expects them
  (`BlockState::MAX_STATE` handling in `protocol/modded.rs`).
- **Rebuild from names when the server tells us.** See section 5.3.

---

## 4. Change list: the 767 hop

Grouped by area. Each row is one rewrite (or one family of rewrites). The
release that introduced the change is in brackets.

### 4.1 Login and configuration

| Packet | Change |
|---|---|
| `login_finished` | Body shape changed in this range *(confirm)*; check against 1.21.11. |
| `select_known_packs` (ours) | Answer with `minecraft:core` at the **server's** version. The server then sends registry entry names without their data, and the client fills values from its own 26.3 datapack (as `client/dimension.rs` already does for dimension types). Without this, the server sends 1.21.1-shaped registry NBT that every consumer would have to convert. Check how the vendored azalea answers today. |
| `registry_data` | Registries 26.3 expects but 1.21.1 doesn't sync: synthesize them from the bundled datapack. Likely `pig_variant`, `cow_variant`, `chicken_variant`, `frog_variant`, `cat_variant`, `wolf_sound_variant` [1.21.5] *(confirm)*, plus 26.x additions. Registries 1.21.1 syncs that 26.3 dropped: drop them. |
| `update_tags` | Tag registries and their id spaces go through the remap (pattern exists in `hop::update_tags`). |
| `update_enabled_features`, `custom_report_details`, `server_links` | Check; likely pass-through. |

### 4.2 World

| Packet | Change |
|---|---|
| `level_chunk_with_light` | Heightmaps went from NBT to a typed list [1.21.5]; paletted containers dropped the data-array length prefix [1.21.5] *(confirm)*; states through the remap (`hop::remap_sections` exists); block entity types through the remap. Light data then goes through the existing 775 hop. |
| `block_update`, `section_blocks_update` | Existing `hop::block_update` / `section_blocks_update` work once the 767 tables exist. |
| `block_entity_data`, `block_event`, `level_event` | Ids through the remap; check payload NBT for renamed keys. |
| `set_time` | 1.21.1 encodes "not ticking" as a negative day time; 1.21.2+ adds an explicit flag *(confirm)*. `hop::set_time` already writes the 774-to-26.x shape. |
| `explode` | Reworked [1.21.2]: 1.21.1 sends the affected block list and knockback; newer versions send centre, knockback, particle and sound *(confirm)*. |
| `level_particles` | Particle option shapes changed (dust colour packing [1.21.2], new options) *(confirm)*; ids through the remap. |
| `sound`, `sound_entity` | `hop::sound` exists; ids through the remap. |
| `game_event`, `initialize_border` and border packets, `set_default_spawn_position` | Check each; spawn position gained a dimension in 1.21.9 *(confirm)*. |

### 4.3 Entities

| Packet | Change |
|---|---|
| `add_entity` | Entity type through the remap; velocity encoding changed to the 26.x `LpVec3` (the 775 hop handles the last step). |
| `set_entity_data` | Serializer ids through the remap, plus **per-entity index shifts**: indices moved for many types across this range (variant holders for pig/cow/chicken [1.21.5], and others). Generate the per-type layouts (section 6.3) instead of hand-writing them. |
| `teleport_entity`, `player_position` | Reworked [1.21.2]: delta movement and relative flags added *(confirm)*. |
| `entity_position_sync`, `set_cursor_item`, `set_player_inventory`, `move_minecart_along_track` | New after 1.21.1; a 1.21.1 server never sends them. Nothing to do inbound. |
| `set_entity_motion`, `move_entity_*`, `rotate_head`, `animate`, `hurt_animation`, `damage_event` | Check each against 1.21.11; most pass through. |
| `update_attributes` | Attribute ids through the remap; several attributes were renamed (the `generic.` prefix dropped) [1.21.2] *(confirm)*, so they need `RENAMES` entries. |
| `update_mob_effect` | Effect ids through the remap; flags byte check. |

### 4.4 Items and inventory (the biggest area)

| Item | Change |
|---|---|
| Item stacks and component patches | A `patch_767` walker like `patch_774`. Components changed shape all through this range: `food` split into `food` + `consumable` [1.21.2]; `use_remainder`, `equippable`, `item_model`, `damage_resistant`, `tooltip_style` new [1.21.2]; `custom_model_data` reworked [1.21.4]; `hide_tooltip` and every `show_in_tooltip` field replaced by `tooltip_display` [1.21.5] *(confirm all)*. Component type ids go through the remap. |
| Container ids | Byte to VarInt [1.21.2] *(confirm)* in every container packet. |
| `container_set_content`, `container_set_slot`, `set_equipment`, `merchant_offers` | Existing families; they need the 767 stack walker. |
| `container_click` (ours) | 1.21.5+ clients send hashed stacks; 1.21.1 wants full stacks. Simplest correct version: send an empty changed-slot list and an empty carried stack. The 1.21.1 server then sees every slot as changed and resends them (one round trip of visual lag). Exact version: mirror container contents in the hop state and rebuild full stacks. Start simple. |
| `set_creative_mode_slot` (ours) | Full stack back to 1.21.1 shape (`hop::back` pattern). |
| Text components inside items | See 4.6. |

### 4.5 Our packets (outbound)

| Packet | Change |
|---|---|
| `player_input` | 1.21.1 sends two floats plus jump/sneak flags; newer versions send a flags byte [1.21.2] *(confirm)*. Rebuild the floats from the flags. |
| Sneaking | Newer versions take sneak from `player_input` and dropped it from `player_command` [1.21.6] *(confirm)*. The hop must **emit** `player_command` start/stop-sneaking when the sneak flag changes (stateful, 3.4). |
| `client_tick_end`, `player_loaded`, `select_bundle_item`, `pick_item_from_block`/`_entity`, `set_test_block`, `test_instance_block_action`, `change_game_mode`, debug packets | No 1.21.1 counterpart. The table drops them already; check none is load-bearing. Pick block needs a replacement: 1.21.1 has `pick_item` by inventory slot, and creative pick can go through `set_creative_mode_slot`. |
| `use_item` | Rotation fields added [1.21.2] *(confirm)*: drop them. |
| `move_player_*` | Flags byte gained horizontal collision [1.21.2] *(confirm)*: mask it. |
| `interact`, `attack` | Existing 774 rewrites; check the 767 shape. |
| `client_information` | Particle status field added [1.21.2] *(confirm)*: drop it. |
| `chat`, `chat_command_signed` | Checksum byte added [1.21.5] *(confirm)*: drop it. |
| `place_recipe`, `recipe_book_seen_recipe` | 1.21.1 names recipes by id; newer versions by display id. Map back through the hop state (4.7). |
| `container_button_click`, `container_close`, `sign_update`, `edit_book`, `set_structure_block`, `set_jigsaw_block` | Container id width and field checks. |

### 4.6 Text components (cross-cutting)

Text component NBT changed keys and shapes [1.21.5] *(confirm)*:
`clickEvent` to `click_event`, `hoverEvent` to `hover_event`, `value` to
`url`/`command`/`path`/..., hover `contents` split by action, and more.
Every packet that carries a component needs it: chat (`system_chat`,
`player_chat`, `disguised_chat`), `set_title_text` and siblings,
`tab_list`, `player_info_update` (display names), `set_objective`,
`set_score`, `boss_event`, `disconnect`, `set_player_team`, `open_screen`,
`server_data`, `resource_pack_push`, item names and lore, `commands`
suggestions.

Write **one** converter (`fn component_767(nbt) -> nbt`) and call it from
each of those rewrites.

### 4.7 Recipes

1.21.2 replaced the recipe system: 1.21.1 sends full recipe definitions
(`update_recipes`) and unlocks by id (`recipe`); newer versions send
display-only entries (`recipe_book_add`, `recipe_book_remove`) and a reduced
`update_recipes` (property sets, stonecutter).

- Phase 1: drop the 1.21.1 recipe packets and send an empty
  `update_recipes`. The recipe book is empty; crafting still works.
- Phase 2: convert 1.21.1 recipes to displays and keep a display-id to
  recipe-id map in the hop state for `place_recipe`.

### 4.8 Everything else

`commands` (argument type ids, 3.3), `player_info_update` (actions added:
list order [1.21.2], hat [1.21.4] *(confirm)*), `update_advancements`,
`award_stats`, `map_item_data`, `set_player_team`, `boss_event`,
scoreboard packets, `cooldown` (by item group [1.21.2] *(confirm)*),
`open_screen` (menu ids), `custom_payload` (pass-through, NeoForge reads
it), `ping`, `keep_alive`, `resource_pack_push`. Walk the whole 1.21.1
clientbound table against 1.21.11 with the diff report (6.2) and add a row
here for every changed packet.

### 4.9 Outside `src/protocol/`

The game runs as 26.3, but a few things depend on what the server has.

| Area | Change | Files |
|---|---|---|
| Creative inventory | Hide items with no back-mapping (`from_native.map(Item, id).is_none()`): the server would reject them. | `gui/` creative tabs |
| Placement and interaction | Blocks and items absent on the server can't be placed or used; their prediction would desync. | `play/placement.rs`, `play/interaction.rs` |
| Movement and physics | Movement rules changed in this range. Gate each difference on a `version::` predicate, like `metadata_matches_native`. Find them by playtest. | `play/`, vendored `azalea-physics` |
| Version-only HUD and screens | Locator bar/waypoints [1.21.6], bundle selection, newer screens: hide or disable when the server is older. | `gui/` |
| Server resource packs | 1.21.1 packs are pack format 34; `resourcepacks/mod.rs` marks them `TooOld`. They predate item model definitions [1.21.4], so they use `models/item/*.json` `overrides` with `custom_model_data` predicates. Convert those to item model definitions at load. Most 1.21.1 servers with custom items depend on this. | `resourcepacks/mod.rs`, `items/model.rs`, `items/dispatch.rs` |
| Chat | `chat_recv.rs` already reads `protocol::session()`; check signing and the chat type registry on 1.21.1. | `client/chat_recv.rs` |

---

## 5. Change list: NeoForge

### 5.1 26.3 NeoForge

Mostly done. Finish 15d and 15e: one run against a NeoForge 26.3 server
(official installer, on the laptop), fix what breaks, tick the items. Do this
first: it is the cheapest of the four targets.

### 5.2 1.21.1 NeoForge (21.1.x): the handshake

`client/neoforge.rs` follows NeoForge 26.3.x. Diff it against 21.1.x
(`reference/neoforge-1.21.1/`, `network/` and `registries/`):

- Payload names and codecs: `neoforge:register`, `neoforge:network`,
  `neoforge:modded_network_setup_failed`, the frozen registry trio, config
  files, data maps.
- The 1.21.1 branch has every payload the 26.3 handshake handles,
  extensible enums, feature flags and the `c:` channels included
  (`network/payload/`). What's left to compare is their codecs and the
  order the tasks run in.
- `ConnectionProtocol` ordinals (`PLAY = 1`, `CONFIGURATION = 4` in
  `protocol/neoforge.rs`): check they hold in 1.21.1.
- The registry snapshot codec (ids, aliases, any extra fields).
- Choose the offer set by `protocol::session()`: a `NeoForgeVersion` table
  keyed by protocol, not `if` checks scattered through the handshake.

### 5.3 1.21.1 NeoForge: the hop and modded registries

This is the hard part. Over a hop, the server's ids are translated by a
**static** vanilla 1.21.1-to-1.21.11 table before azalea or
`registries.rs` sees them. A NeoForge server adds entries and may renumber.

1. Shift modded ids instead of dropping them (3.5).
2. When `frozen_registry` arrives over a hop, rebuild `to_native` and
   `from_native` for that id space **by name**: each server id, by its
   name, to the native id (vanilla name) or to a slot past native's vanilla
   range (modded name). Rewrite the snapshot into native numbering before
   `registries.rs` applies it, so the rest of the client sees one
   numbering.
3. Block states are not synced; they follow block registration order.
   Vanilla 1.21.1 states keep their ids, and modded states come after. Shift
   them past native `MAX_STATE` (3.5). Raw-id keeping (the 15 decision)
   then applies as on 26.3.
4. Modded data components inside item stacks: the 767 walker can't know
   their shapes. Do the same as the 26.3 path does for unknown components,
   and add a test that one doesn't drop the whole packet.

Static remaps stay for vanilla servers. The dynamic remap only replaces a
space after the server syncs it.

### 5.4 Limits that stay

Same as 26.3: mod-only data in vanilla packets is dropped; mod screens,
custom shapes and mod logic don't run.

---

## 6. Automation

**Short answer: the tables are already automated; the packet shapes are
not, and can't fully be. What can be automated is (a) getting a new version
in, (b) the list of what changed, and (c) two classes of rewrite that are
pure data.**

### 6.1 One-command version intake (fully automatable)

Landed as `tools/new_version.py` (usage in its header):

- `vanilla <ver> [--remap-to V] [--remap-from V]`: official client and
  server jars, client mappings and client libraries from Mojang's manifest,
  each checked against its SHA-1. The server jar's data generator writes
  `generated/reports/`. Obfuscated clients (up to 1.21.11) are renamed with
  AutoRenamingTool; Vineflower decompiles. Both tools are pinned by
  SHA-256. Then `protogen.py packets` and `remap`, which prints the hole
  report (3.3).
- `neoforge <ver>`: shallow clone of NeoForge's branch for that version
  (`1.21.1`, `26.3.x`). NeoForge doesn't tag most releases, so branches,
  not tags.

### 6.2 Codec diff report (automatable, saves the most time)

`tools/codec_diff.py <old> <new>`: for every packet class, every
`DataComponents` entry, every `EntityDataSerializers` entry and every text
component codec, extract the `STREAM_CODEC` / `CODEC` definition (or
`write` / `read` methods) and diff it between versions. Output a markdown
checklist: one row per changed codec, with the diff. That replaces the
*(confirm)* guesses in section 4 with a complete, exact list.

### 6.3 Generated entity metadata layouts (automatable)

Entity data indices come from each class's `defineSynchedData` calls, in
class hierarchy order. Parse those from the decompile for each version and
emit per-entity-type index tables (name, index, serializer). The hop then
remaps metadata generically: old index to name to new index, serializer
through the remap. That removes a whole family of hand-written,
error-prone rewrites, and makes `metadata_matches_native` obsolete.

### 6.4 Generated simple rewrites (partly automatable)

Many packets are a flat `StreamCodec.composite(...)` field list. For those,
parse the field list per version and generate the rewrite when the change
is mechanical: field added (write its default), field removed (skip it),
primitive width changed (byte to VarInt). Worth building only after writing
a dozen by hand and seeing the pattern repeat. Chunks, components, recipes,
explosions, text and input stay hand-written.

### 6.5 Not automatable

Semantic changes: the recipe system, explosions, input and sneaking, text
component meaning, hashed stacks, NeoForge's dynamic registries, movement
rules. These need a person reading both decompiles.

### 6.6 Off-the-shelf alternative: ViaVersion / ViaProxy

ViaVersion translates newer clients to older servers, which is this exact
direction, for every step in section 2. Two ways to use it:

- **As a reference.** Its per-step protocol packages list what changed in
  each release. Read them to check section 4; don't copy them (GPLv3).
- **As a proxy (ViaProxy).** Run it locally; the client stays 26.3-only.
  Works today on PC with no client code. It needs Java, doesn't run on
  Android or web, adds a process, and its support for modded (NeoForge)
  handshakes is limited. Fine as a stopgap or a test oracle (compare its
  output for a packet with our hop's), not as the product.

---

## 7. Testing

- **Unit tests per rewrite**, as now (76 in `hop.rs` and `v775.rs`).
- **Capture and replay.** Record every raw frame from real 1.21.1 vanilla
  and NeoForge sessions (extend the existing `proto` trace in `Hop` to dump
  raw frames). A test replays the corpus through `Translator` and asserts
  azalea decodes every frame. This catches missing rewrites without a
  playtest, and guards against regressions when 26.4 arrives.
- **`every_translated_version_has_a_hop_and_tables`** covers 767 once
  `TRANSLATED` lists it.
- **Live runs** on the laptop against official 1.21.1 and NeoForge 21.1.x
  servers, creative mode after joining, the user positioning.

---

## 8. Suggested order

| Step | Goal | Done when |
|---|---|---|
| 0 | NeoForge 26.3 live run (5.1) | 15d and 15e ticked |
| 1 | Intake tooling (6.1) and 1.21.1 tables; `protogen` renames and state fallback (3.3) | `packets-1.21.1.json`, `remap-767-774.json` with explained holes |
| 2 | Codec diff report (6.2) | Section 4 rows confirmed, missing rows added |
| 3 | Join and stand: login, configuration, known packs, registry data, chunks, player position | Spawn in a 1.21.1 world, terrain correct |
| 4 | World and entities: block updates, entities, metadata layouts (6.3), text components (4.6) | Mobs, players, chat and tab list correct |
| 5 | Inventory: stack walker, container packets, clicks, creative | Inventory works without desyncs |
| 6 | Our packets: input, sneak, use, pick block, chat, commands | Play in survival without kicks or rubber-banding |
| 7 | The rest: particles, sounds, explosions, advancements, maps, scoreboard, recipes phase 1 | Replay corpus decodes cleanly |
| 8 | NeoForge 21.1.x: handshake diffs (5.2), dynamic remap (5.3) | Join a modded 1.21.1 server; modded blocks keep raw ids |
| 9 | Legacy server resource packs, version-gated features (4.9), recipes phase 2 | Custom-item servers look right |

After each step, update `docs/ARCHITECTURE.md` (protocol row) and the hop
recipe at the top of `hop.rs` if the recipe changed.
