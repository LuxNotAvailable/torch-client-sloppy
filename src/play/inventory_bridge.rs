use crate::client::chat_text;
use crate::items::potions;
use crate::session::{Book, ContainerKind, FurnaceKind, HorseMenu, InvAction, SlotStack};
use azalea::inventory::ContainerClickEvent;
use azalea::prelude::*;
use azalea_inventory::{ItemStack, Menu};

pub(crate) fn container_kind(menu: &Menu) -> ContainerKind {
    match menu {
        Menu::Generic9x1 { .. } => ContainerKind::Chest(1),
        Menu::Generic9x2 { .. } => ContainerKind::Chest(2),
        Menu::Generic9x3 { .. } => ContainerKind::Chest(3),
        Menu::Generic9x4 { .. } => ContainerKind::Chest(4),
        Menu::Generic9x5 { .. } => ContainerKind::Chest(5),
        Menu::Generic9x6 { .. } => ContainerKind::Chest(6),
        Menu::Crafting { .. } => ContainerKind::Crafting,
        Menu::Furnace { .. } => ContainerKind::Furnace(FurnaceKind::Furnace),
        Menu::BlastFurnace { .. } => ContainerKind::Furnace(FurnaceKind::BlastFurnace),
        Menu::Smoker { .. } => ContainerKind::Furnace(FurnaceKind::Smoker),
        Menu::Hopper { .. } => ContainerKind::Hopper,
        Menu::Grindstone { .. } => ContainerKind::Grindstone,
        Menu::Enchantment { .. } => ContainerKind::Enchantment,
        Menu::Loom { .. } => ContainerKind::Loom,
        Menu::Stonecutter { .. } => ContainerKind::Stonecutter,
        Menu::CartographyTable { .. } => ContainerKind::Cartography,
        Menu::Smithing { .. } => ContainerKind::Smithing,
        Menu::Beacon { .. } => ContainerKind::Beacon,
        Menu::BrewingStand { .. } => ContainerKind::BrewingStand,
        Menu::Merchant { .. } => ContainerKind::Merchant,
        Menu::Lectern { .. } => ContainerKind::Lectern,
        Menu::Anvil { .. } => ContainerKind::Anvil,
        _ => ContainerKind::None,
    }
}

/// `ItemStack.hasFoil`: the override when an item sets one, otherwise whether
/// it carries enchantments.
/// The worn trim, by registry key names (vanilla palettes share them).
pub(crate) fn armor_trim(
    data: &azalea_inventory::ItemStackData,
    registries: &azalea_core::registry_holder::RegistryHolder,
) -> Option<crate::entities::render::humanoid::armor::Trim> {
    use azalea_core::data_registry::DataRegistryWithKey;
    let trim = data.get_component::<azalea_inventory::components::Trim>()?;
    fn name<K: azalea_registry::DataRegistryKey>(key: Option<K>) -> Option<String> {
        Some(key?.into_ident().path().to_owned())
    }
    let pattern = name(trim.pattern.key_owned(registries))?;
    let material = name(trim.material.key_owned(registries))?;
    Some(crate::entities::render::humanoid::armor::Trim::new(
        &pattern, &material,
    ))
}

pub(crate) fn has_glint(data: &azalea_inventory::ItemStackData) -> bool {
    use azalea_inventory::components as comp;
    match data.get_component::<comp::EnchantmentGlintOverride>() {
        Some(o) => o.show_glint,
        None => data
            .get_component::<comp::Enchantments>()
            .is_some_and(|e| !e.levels.is_empty()),
    }
}

/// A stack's cooldown, in percent: the one `SlotStack` field that moves
/// while the stack itself stays the same.
pub(crate) fn slot_cooldown(stack: &ItemStack) -> u8 {
    use azalea_inventory::components as comp;
    let ItemStack::Present(data) = stack else {
        return 0;
    };
    if !crate::client::cooldowns::any_active() {
        return 0;
    }
    let item = data.kind.to_str().trim_start_matches("minecraft:");
    let use_cooldown = data.get_component::<comp::UseCooldown>();
    let group = use_cooldown
        .as_ref()
        .and_then(|c| c.cooldown_group.as_ref());
    crate::client::cooldowns::percent(group, item)
}

pub(crate) fn slot_stack(
    stack: &ItemStack,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
    advanced_tooltips: bool,
) -> SlotStack {
    use azalea_buf::AzBuf;
    use azalea_inventory::components as comp;

    let ItemStack::Present(data) = stack else {
        return SlotStack::default();
    };

    let get = |f: fn(&azalea_inventory::ItemStackData) -> Option<i32>| f(data).unwrap_or(0);
    let damage = get(|d| d.get_component::<comp::Damage>().map(|c| c.amount)) as u16;
    let max_damage = get(|d| d.get_component::<comp::MaxDamage>().map(|c| c.amount)) as u16;

    let custom_name = data
        .get_component::<comp::CustomName>()
        .map(|c| chat_text::to_spans(&c.name));
    let lore = data
        .get_component::<comp::Lore>()
        .map(|c| c.lines.iter().map(chat_text::to_spans).collect())
        .unwrap_or_default();
    let name_for = |e: &azalea_registry::data::Enchantment| -> String {
        use azalea_core::data_registry::DataRegistryWithKey;
        use azalea_registry::DataRegistry;
        registries
            .and_then(|r| e.key_owned(r))
            .map(|k| {
                azalea_registry::DataRegistryKey::into_ident(k)
                    .path()
                    .to_string()
            })
            .unwrap_or_else(|| format!("#{}", e.protocol_id()))
    };
    let mut enchantments: Vec<(String, i32)> = data
        .get_component::<comp::StoredEnchantments>()
        .map(|c| {
            c.enchantments
                .iter()
                .map(|(e, lvl)| (name_for(e), *lvl))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    enchantments.extend(
        data.get_component::<comp::Enchantments>()
            .map(|c| {
                c.levels
                    .iter()
                    .map(|(e, lvl)| (name_for(e), *lvl))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    enchantments.sort();

    let nbt_bytes = if advanced_tooltips {
        let mut buf = Vec::new();
        let _ = stack.azalea_write(&mut buf);
        buf.len()
    } else {
        0
    };

    let item = data.kind.to_str().trim_start_matches("minecraft:");
    let cooldown = if crate::client::cooldowns::any_active() {
        let use_cooldown = data.get_component::<comp::UseCooldown>();
        let group = use_cooldown
            .as_ref()
            .and_then(|c| c.cooldown_group.as_ref());
        crate::client::cooldowns::percent(group, item)
    } else {
        0
    };

    SlotStack {
        item,
        count: data.count.clamp(0, 255) as u8,
        cooldown,
        damage,
        max_damage,
        custom_name,
        lore,
        enchantments,
        potion: potion_contents(data),
        unbreakable: data.get_component::<comp::Unbreakable>().is_some(),
        charged: data
            .get_component::<comp::ChargedProjectiles>()
            .is_some_and(|c| !c.items.is_empty()),
        map_id: data.get_component::<comp::MapId>().map(|m| m.id),
        component_count: data.component_patch.iter().count() as u16,
        nbt_bytes,
        banner_patterns: banner_patterns(data, registries),
        banner_layers: banner_layers(data, registries),
        book: book(data).map(Box::new),
        glint: has_glint(data),
        model_data: model_data(data, registries).map(Box::new),
    }
}

/// A held item's model variant (`item@path`), when its model varies: the
/// few fields a variant reads, not a whole `SlotStack`.
pub(crate) fn variant_key(data: &azalea_inventory::ItemStackData) -> Option<std::sync::Arc<str>> {
    use azalea_inventory::components as comp;
    let item = data.kind.to_str().trim_start_matches("minecraft:");
    let has_model = data.get_component::<comp::ItemModel>().is_some();
    if !has_model && !crate::items::icons::is_dynamic(item) {
        return None;
    }
    let get = |f: fn(&azalea_inventory::ItemStackData) -> Option<i32>| f(data).unwrap_or(0);
    let stack = SlotStack {
        item,
        count: data.count.clamp(0, 255) as u8,
        damage: get(|d| d.get_component::<comp::Damage>().map(|c| c.amount)) as u16,
        max_damage: get(|d| d.get_component::<comp::MaxDamage>().map(|c| c.amount)) as u16,
        charged: data
            .get_component::<comp::ChargedProjectiles>()
            .is_some_and(|c| !c.items.is_empty()),
        model_data: model_data(data, None).map(Box::new),
        ..Default::default()
    };
    let key = stack.model_key();
    (key != item).then(|| std::sync::Arc::from(key.as_ref()))
}

/// What the item model reads from the stack's components; `None` when it
/// reads nothing.
fn model_data(
    data: &azalea_inventory::ItemStackData,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
) -> Option<crate::items::dispatch::ModelData> {
    use azalea_inventory::components as comp;
    let mut out = crate::items::dispatch::ModelData::default();
    if let Some(cmd) = data.get_component::<comp::CustomModelData>() {
        out.cmd_floats = cmd.floats.clone();
        out.cmd_flags = cmd.flags.clone();
        out.cmd_strings = cmd.strings.clone();
    }
    if let (Some(trim), Some(registries)) = (data.get_component::<comp::Trim>(), registries) {
        use azalea_core::data_registry::DataRegistryWithKey;
        out.trim_material = trim
            .material
            .key_owned(registries)
            .map(|k| azalea_registry::DataRegistryKey::into_ident(k).to_string());
    }
    if let Some(charged) = data.get_component::<comp::ChargedProjectiles>() {
        out.charge = charged.items.first().map(|stack| match stack.kind() {
            azalea_registry::builtin::ItemKind::FireworkRocket => "rocket",
            _ => "arrow",
        });
    }
    if let Some(bundle) = data.get_component::<comp::BundleContents>() {
        // `BundleContents.weight`: each item takes 1/64 of the bundle, or
        // 1/16 when it stacks to 16, or the whole of it when it can't stack.
        let weight: f32 = bundle
            .items
            .iter()
            .map(|stack| {
                let max = stack
                    .as_present()
                    .and_then(|d| d.get_component::<comp::MaxStackSize>().map(|m| m.count))
                    .unwrap_or(64)
                    .max(1);
                stack.count() as f32 / max as f32
            })
            .sum();
        out.bundle_fullness = weight.clamp(0.0, 1.0);
    }
    if let Some(tracker) = data.get_component::<comp::LodestoneTracker>()
        && let Some(target) = &tracker.target
    {
        out.lodestone = Some((
            [target.pos.x, target.pos.y, target.pos.z],
            target.dimension.to_string(),
        ));
    }
    if let Some(model) = data.get_component::<comp::ItemModel>() {
        let id = model.resource_location.to_string();
        if id.trim_start_matches("minecraft:")
            != data.kind.to_str().trim_start_matches("minecraft:")
        {
            out.item_model = Some(id);
        }
    }
    out.quick_charge = data
        .get_component::<comp::Enchantments>()
        .and_then(|e| {
            registries.and_then(|r| {
                use azalea_core::data_registry::DataRegistryWithKey;
                e.levels.iter().find_map(|(e, level)| {
                    let key = e.key_owned(r)?;
                    (azalea_registry::DataRegistryKey::into_ident(key).path() == "quick_charge")
                        .then_some(*level as u8)
                })
            })
        })
        .unwrap_or(0);
    out.components = data
        .component_patch
        .iter()
        .map(|(kind, _)| kind.to_str().trim_start_matches("minecraft:").to_owned())
        .collect();
    (out != crate::items::dispatch::ModelData::default()).then_some(out)
}

pub(crate) fn slot_stack_nbt(item: &simdnbt::owned::NbtCompound) -> Option<SlotStack> {
    use crate::items::model::strip_namespace;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

    fn component<'a>(components: Option<&'a NbtCompound>, key: &str) -> Option<&'a NbtTag> {
        let components = components?;
        components
            .get(&format!("minecraft:{key}"))
            .or_else(|| components.get(key))
    }

    let raw = item.string("id")?.to_str();
    let id = crate::generated_items::item(strip_namespace(&raw))?.id;
    let components = item.compound("components");
    let spans = |tag: Option<&NbtTag>| -> Option<Vec<crate::text::Span>> {
        let text = chat_text::from_nbt_tag(tag?)?;
        Some(chat_text::to_spans(&text))
    };
    let int = |key: &str| component(components, key).and_then(NbtTag::int);

    let mut enchantments: Vec<(String, i32)> = ["enchantments", "stored_enchantments"]
        .iter()
        .filter_map(|key| component(components, key)?.compound())
        .flat_map(|map| {
            map.iter()
                .filter_map(|(name, level)| {
                    Some((strip_namespace(&name.to_string()).to_string(), level.int()?))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    enchantments.sort();

    Some(SlotStack {
        item: id,
        count: item.int("count").unwrap_or(1).clamp(1, 99) as u8,
        damage: int("damage").unwrap_or(0).clamp(0, u16::MAX as i32) as u16,
        max_damage: int("max_damage").unwrap_or(0).clamp(0, u16::MAX as i32) as u16,
        custom_name: spans(component(components, "custom_name")),
        lore: match component(components, "lore") {
            Some(NbtTag::List(NbtList::String(lines))) => lines
                .iter()
                .filter_map(|line| spans(Some(&NbtTag::String(line.clone()))))
                .collect(),
            Some(NbtTag::List(NbtList::Compound(lines))) => lines
                .iter()
                .filter_map(|line| spans(Some(&NbtTag::Compound(line.clone()))))
                .collect(),
            _ => Vec::new(),
        },
        enchantments,
        unbreakable: component(components, "unbreakable").is_some(),
        component_count: components.map(|c| c.len()).unwrap_or(0) as u16,
        ..SlotStack::default()
    })
}

fn banner_patterns(
    data: &azalea_inventory::ItemStackData,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
) -> Option<Box<[Box<str>]>> {
    use azalea_core::data_registry::DataRegistryWithKey;
    use azalea_inventory::components as comp;

    let component = data.get_component::<comp::ProvidesBannerPatterns>()?;
    let azalea_registry::HolderSet::Direct { contents } = &component.key else {
        return None;
    };
    let registries = registries?;
    let out: Vec<Box<str>> = contents
        .iter()
        .filter_map(|k| {
            let key = k.key_owned(registries)?;
            Some(Box::from(
                azalea_registry::DataRegistryKey::into_ident(key).path(),
            ))
        })
        .collect();
    (!out.is_empty()).then(|| out.into_boxed_slice())
}

fn banner_layers(
    data: &azalea_inventory::ItemStackData,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
) -> Option<Box<[crate::blockentities::banner::BannerLayer]>> {
    use azalea_inventory::components as comp;

    use crate::blockentities::banner::{BannerLayer, MAX_PATTERNS, asset_for};

    let mut out: Vec<BannerLayer> = Vec::new();
    if let Some(base) = data.get_component::<comp::BaseColor>() {
        out.push(BannerLayer {
            asset: Box::from("base"),
            color: base.color as u8,
        });
    }
    if let Some(component) = data.get_component::<comp::BannerPatterns>() {
        use azalea_registry::{DataRegistry, Holder};

        out.extend(
            component
                .patterns
                .iter()
                .take(MAX_PATTERNS)
                .filter_map(|layer| {
                    let asset = match &layer.pattern {
                        Holder::Reference(kind) => {
                            let id = registries?.protocol_id_to_identifier(
                                azalea::Identifier::from("banner_pattern"),
                                kind.protocol_id(),
                            )?;
                            asset_for(id.path())
                        }
                        Holder::Direct(pattern) => Box::from(pattern.asset_id.path()),
                    };
                    Some(BannerLayer {
                        asset,
                        color: layer.color.clamp(0, 15) as u8,
                    })
                }),
        );
    }
    (!out.is_empty()).then(|| out.into_boxed_slice())
}

fn book(data: &azalea_inventory::ItemStackData) -> Option<Book> {
    use azalea_inventory::components as comp;

    if let Some(written) = data.get_component::<comp::WrittenBookContent>() {
        return Some(Book {
            pages: written
                .pages
                .iter()
                .map(|p| chat_text::to_spans(&p.raw))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            title: written.title.raw.clone(),
            author: written.author.clone(),
            generation: written.generation.clamp(0, MAX_GENERATION as i32) as u8,
            signed: true,
        });
    }
    let writable = data.get_component::<comp::WritableBookContent>()?;
    Some(Book {
        pages: writable
            .pages
            .iter()
            .map(|p| {
                vec![crate::text::Span {
                    text: p.raw.clone(),
                    style: Default::default(),
                }]
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        title: String::new(),
        author: String::new(),
        generation: 0,
        signed: false,
    })
}

const MAX_GENERATION: u8 = 3;

pub(crate) fn potion_contents(
    data: &azalea_inventory::ItemStackData,
) -> Option<potions::PotionContents> {
    use azalea_inventory::components as comp;

    let bare = |id: &str| id.trim_start_matches("minecraft:").to_string();
    if !potions::is_potion_item(&bare(data.kind.to_str())) {
        return None;
    }
    let c = data.get_component::<comp::PotionContents>()?;
    Some(potions::PotionContents {
        potion: c.potion.as_ref().map(|p| bare(p.to_str())),
        custom_color: c.custom_color.map(|v| v as u32 & 0xFF_FFFF),
        custom_effects: c
            .custom_effects
            .iter()
            .map(|e| potions::PotionEffectInstance {
                effect: bare(e.id.to_str()),
                duration: e.details.duration,
                amplifier: e.details.amplifier,
            })
            .collect(),
        custom_name: c.custom_name.clone(),
    })
}

fn resolve_enchantments(
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
    enchantments: &[(String, i32)],
) -> std::collections::HashMap<azalea_registry::data::Enchantment, i32> {
    use azalea_registry::DataRegistry;
    use azalea_registry::identifier::Identifier;

    let Some(registries) = registries else {
        return std::collections::HashMap::new();
    };
    enchantments
        .iter()
        .filter_map(|(name, level)| {
            let id = registries
                .enchantment
                .map
                .get_index_of(&Identifier::new(name.clone()))?;
            Some((azalea_registry::Enchantment::new_raw(id as u32), *level))
        })
        .collect()
}

fn creative_stack(
    stack: &SlotStack,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
) -> ItemStack {
    use azalea_registry::builtin::ItemKind;
    use std::str::FromStr;

    if stack.is_empty() {
        return ItemStack::Empty;
    }
    let Ok(kind) = ItemKind::from_str(stack.item) else {
        return ItemStack::Empty;
    };
    let mut out = ItemStack::new(kind, stack.count as i32);
    if !stack.enchantments.is_empty() {
        let levels = resolve_enchantments(registries, &stack.enchantments);
        if !levels.is_empty() {
            use azalea_inventory::components as comp;
            out = out.with_component(comp::StoredEnchantments {
                enchantments: levels,
            });
        }
    }
    if let Some(contents) = &stack.potion {
        use azalea_inventory::components as comp;
        use azalea_registry::builtin::{MobEffect, Potion};

        let potion = contents
            .potion
            .as_deref()
            .and_then(|id| Potion::from_str(id).ok());
        let custom_effects = contents
            .custom_effects
            .iter()
            .filter_map(|e| {
                Some(comp::MobEffectInstance {
                    id: MobEffect::from_str(&e.effect).ok()?,
                    details: comp::MobEffectDetails {
                        duration: e.duration,
                        amplifier: e.amplifier,
                        ..comp::MobEffectDetails::new()
                    },
                })
            })
            .collect::<Vec<_>>();
        if potion.is_some()
            || contents.custom_color.is_some()
            || !custom_effects.is_empty()
            || contents.custom_name.is_some()
        {
            out = out.with_component(comp::PotionContents {
                potion,
                custom_color: contents.custom_color.map(|v| v as i32),
                custom_effects,
                custom_name: contents.custom_name.clone(),
            });
        }
    }
    out
}

pub(crate) fn apply_inv_actions(bot: &Client, actions: Vec<InvAction>, horse: Option<HorseMenu>) {
    use azalea_protocol::packets::game::s_container_button_click::ServerboundContainerButtonClick;
    use azalea_protocol::packets::game::s_container_close::ServerboundContainerClose;
    use azalea_protocol::packets::game::s_rename_item::ServerboundRenameItem;
    use azalea_protocol::packets::game::s_select_trade::ServerboundSelectTrade;
    use azalea_protocol::packets::game::s_set_beacon::ServerboundSetBeacon;
    use azalea_protocol::packets::game::s_set_creative_mode_slot::ServerboundSetCreativeModeSlot;

    let world = bot.world().ok();
    let world_guard = world.as_ref().map(|w| w.read());
    let registries = world_guard.as_ref().map(|g| &g.registries);

    for action in actions {
        match action {
            InvAction::Click(operation) => {
                if let Some(horse) = horse.as_ref() {
                    send_horse_click(bot, horse, &operation, registries);
                    continue;
                }
                let window_id = bot
                    .component::<azalea::entity::inventory::Inventory>()
                    .map(|i| i.id)
                    .unwrap_or(0);
                bot.ecs.write().trigger(ContainerClickEvent {
                    entity: bot.entity,
                    window_id,
                    operation,
                });
            }
            InvAction::CreativeSet { slot, stack } => {
                let stack = creative_stack(&stack, registries);
                bot.write_packet(ServerboundSetCreativeModeSlot {
                    slot_num: slot,
                    item_stack: stack.clone().into(),
                });
                let mut ecs = bot.ecs.write();
                if let Some(mut inv) =
                    ecs.get_mut::<azalea::entity::inventory::Inventory>(bot.entity)
                    && let Some(dst) = inv.inventory_menu.slot_mut(slot as usize)
                {
                    *dst = stack;
                }
            }
            InvAction::CreativeDrop { stack } => {
                let stack = creative_stack(&stack, registries);
                if stack.is_empty() {
                    continue;
                }
                bot.write_packet(ServerboundSetCreativeModeSlot {
                    slot_num: u16::MAX,
                    item_stack: stack.into(),
                });
            }
            InvAction::ButtonClick(button_id) => {
                let window_id = bot
                    .component::<azalea::entity::inventory::Inventory>()
                    .map(|i| i.id)
                    .unwrap_or(0);
                bot.write_packet(ServerboundContainerButtonClick {
                    container_id: window_id,
                    button_id: button_id as u32,
                });
            }
            InvAction::SelectTrade(item) => {
                bot.write_packet(ServerboundSelectTrade { item });
            }
            InvAction::SetBeacon { primary, secondary } => {
                bot.write_packet(ServerboundSetBeacon { primary, secondary });
            }
            InvAction::RenameItem(name) => {
                bot.write_packet(ServerboundRenameItem { name });
            }
            InvAction::Sort(request) => {
                if horse.is_some() {
                    continue;
                }
                let Ok(inventory) = bot.component::<azalea::entity::inventory::Inventory>() else {
                    continue;
                };
                let abilities = bot
                    .component::<azalea::entity::PlayerAbilities>()
                    .map(|a| a.clone())
                    .unwrap_or_default();
                let ops = crate::play::inventory_sort::plan(&inventory, &abilities, request);
                let window_id = inventory.id;
                drop(inventory);
                for operation in ops {
                    bot.ecs.write().trigger(ContainerClickEvent {
                        entity: bot.entity,
                        window_id,
                        operation,
                    });
                }
            }
            InvAction::Close => {
                if let Some(horse) = horse.as_ref() {
                    bot.write_packet(ServerboundContainerClose {
                        container_id: horse.container_id,
                    });
                    continue;
                }
                let id = bot
                    .component::<azalea::entity::inventory::Inventory>()
                    .map(|i| i.id)
                    .unwrap_or(0);
                bot.ecs
                    .write()
                    .trigger(azalea::inventory::CloseContainerEvent {
                        entity: bot.entity,
                        id,
                    });
            }
        }
    }
}

fn send_horse_click(
    bot: &Client,
    horse: &HorseMenu,
    operation: &azalea_inventory::operations::ClickOperation,
    registries: Option<&azalea_core::registry_holder::RegistryHolder>,
) {
    use azalea_protocol::packets::game::s_container_click::{
        HashedStack, ServerboundContainerClick,
    };

    let Some(registries) = registries else { return };
    let carried = bot
        .component::<azalea::entity::inventory::Inventory>()
        .map(|i| i.carried.clone())
        .unwrap_or_default();
    let carried_item = HashedStack::from_item_stack(&carried, registries);

    bot.write_packet(ServerboundContainerClick {
        container_id: horse.container_id,
        state_id: horse.state_id,
        slot_num: operation.slot_num().map(|n| n as i16).unwrap_or(-999),
        button_num: operation.button_num(),
        click_type: operation.click_type(),
        changed_slots: Default::default(),
        carried_item,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use azalea_buf::AzBuf;
    use azalea_inventory::UntrustedItemStack;
    use azalea_registry::builtin::DataComponentKind;

    fn water_bottle() -> SlotStack {
        SlotStack {
            item: "potion",
            count: 1,
            potion: Some(potions::PotionContents {
                potion: Some("water".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_creative_stack_length_delimits_its_components() {
        let stack = UntrustedItemStack(creative_stack(&water_bottle(), None));
        let mut bytes = Vec::new();
        stack.azalea_write(&mut bytes).unwrap();

        let mut kind = Vec::new();
        DataComponentKind::PotionContents
            .azalea_write(&mut kind)
            .unwrap();
        let head_len = bytes.len() - kind.len() - 6;
        assert_eq!(&bytes[head_len..head_len + kind.len()], &kind[..]);

        let payload = &bytes[head_len + kind.len()..];
        assert_eq!(payload, &[5, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn an_untrusted_stack_round_trips() {
        for stack in [
            water_bottle(),
            SlotStack {
                potion: Some(potions::PotionContents {
                    potion: Some("strong_healing".into()),
                    custom_color: Some(0x123456),
                    custom_effects: vec![potions::PotionEffectInstance {
                        effect: "poison".into(),
                        duration: 200,
                        amplifier: 1,
                    }],
                    custom_name: None,
                }),
                ..water_bottle()
            },
            SlotStack {
                item: "diamond_sword",
                count: 1,
                ..Default::default()
            },
        ] {
            let built = UntrustedItemStack(creative_stack(&stack, None));
            let mut bytes = Vec::new();
            built.azalea_write(&mut bytes).unwrap();
            let read =
                UntrustedItemStack::azalea_read(&mut std::io::Cursor::new(&bytes[..])).unwrap();
            assert_eq!(read, built, "{}", stack.item);
        }
    }
}
