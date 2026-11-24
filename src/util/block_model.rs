use std::collections::HashMap;

use azalea::block::BlockTrait;

use crate::TEXTURE_MAP;
use crate::blocks::rand::ShapeOffset;
use crate::terrain::{BlockGeom, RenderedBlock, TintKind};

fn tex(name: &str) -> u32 {
    TEXTURE_MAP
        .get()
        .and_then(|m| m.get(name).copied())
        .unwrap_or(0)
}

fn bool_prop(props: &HashMap<&str, &str>, key: &str) -> bool {
    props.get(key).map(|v| *v == "true").unwrap_or(false)
}

const IMPLICIT_WATER: &[&str] = &["kelp", "kelp_plant", "seagrass", "tall_seagrass"];

const SKIP_SAME: &[&str] = &[
    "glass",
    "tinted_glass",
    "ice",
    "blue_ice",
    "frosted_ice",
    "honey_block",
    "slime_block",
    "powder_snow",
];

pub const LEAVES_CULL_GROUP: u32 = 1;

/// Dirt paths and farmland: fifteen-sixteenths blocks whose sides meet
/// exactly, so each hides the other's side (vanilla's face occlusion by
/// shape), not only its own kind's.
const LOW_BLOCK_CULL_GROUP: u32 = u32::MAX - 1;

fn cull_group(name: &str, block: &dyn BlockTrait) -> u32 {
    if name.ends_with("_leaves") {
        return LEAVES_CULL_GROUP;
    }
    if name == "dirt_path" || name == "farmland" {
        return LOW_BLOCK_CULL_GROUP;
    }
    if SKIP_SAME.contains(&name)
        || name.ends_with("_stained_glass")
        || name.ends_with("copper_grate")
        // Panes and bars (`IronBarsBlock.skipRendering`): same-kind
        // neighbours always connect, so their facing faces hide.
        || name.ends_with("glass_pane")
        || name.ends_with("_bars")
    {
        return block.as_block_kind() as u32 + 2;
    }
    0
}

pub fn emissive_rendering(name: &str, props: &HashMap<&str, &str>) -> bool {
    match name {
        "magma_block" => true,
        // Not vanilla: fire draws full bright so it reads as a light source.
        "fire" | "soul_fire" => true,
        "sculk_sensor" | "calibrated_sculk_sensor" => {
            props.get("sculk_sensor_phase").copied() == Some("active")
        }
        _ => false,
    }
}

/// A block's `SoundType`: its events, and the volume and pitch they scale.
#[cfg_attr(not(audio), allow(dead_code))]
pub struct SoundGroup {
    pub volume: f32,
    pub pitch: f32,
    pub break_event: &'static str,
    pub place_event: &'static str,
    pub step_event: &'static str,
    /// Each fourth tick of mining it (`ClientLevel.playBreakingSound`).
    pub hit_event: &'static str,
    pub fall_event: &'static str,
}

/// A block's sound type by its id: vanilla's table, or a modded block's
/// own from its package (`blocks::modded`), else stone's.
#[cfg(audio)]
pub fn sound_group(id: &str) -> &'static SoundGroup {
    if id.contains(':')
        && let Some(group) = crate::blocks::modded::sound_group(id)
    {
        return group;
    }
    crate::generated_block_sounds::sound_group(id)
}

#[cfg(all(test, audio))]
mod sound_group_tests {
    use super::*;

    #[test]
    fn a_family_names_its_three_events() {
        assert_eq!(sound_group("oak_planks").place_event, "block.wood.place");
        assert_eq!(sound_group("oak_planks").break_event, "block.wood.break");
        assert_eq!(sound_group("oak_planks").step_event, "block.wood.step");
    }

    #[test]
    fn an_unrecognised_block_falls_back_to_stone() {
        assert_eq!(sound_group("obsidian").break_event, "block.stone.break");
        assert_eq!(sound_group("").break_event, "block.stone.break");
    }

    #[test]
    fn blocks_take_the_game_s_own_sound_types() {
        assert_eq!(
            sound_group("deepslate").break_event,
            "block.deepslate.break"
        );
        assert_eq!(sound_group("melon_stem").place_event, "item.crop.plant");
        assert_eq!(
            sound_group("scaffolding").step_event,
            "block.scaffolding.step"
        );
    }
}

pub fn block_visual(state: azalea::block::BlockState) -> RenderedBlock {
    let block: Box<dyn BlockTrait> = Box::<dyn BlockTrait>::from(state);
    let name = block.id();
    let props = block.property_map();
    let cull_group = cull_group(name, block.as_ref());
    let tint_kind = if crate::util::biome_color::GRASS_TINTED_BLOCKS.contains(&name) {
        TintKind::Grass
    } else if name == "vine" {
        TintKind::Foliage
    } else if name == "leaf_litter" {
        TintKind::DryFoliage
    } else if name == "water_cauldron" {
        TintKind::Water
    } else if name == "melon_stem" || name == "pumpkin_stem" {
        let age: u8 = props.get("age").and_then(|v| v.parse().ok()).unwrap_or(0);
        TintKind::Stem(age)
    } else if name == "attached_melon_stem" || name == "attached_pumpkin_stem" {
        TintKind::Stem(crate::util::biome_color::STEM_ATTACHED)
    } else if name == "redstone_wire" {
        let power: u8 = props.get("power").and_then(|v| v.parse().ok()).unwrap_or(0);
        TintKind::Redstone(power)
    } else if matches!(
        name,
        "spruce_leaves"
            | "birch_leaves"
            | "pale_oak_leaves"
            | "cherry_leaves"
            | "azalea_leaves"
            | "flowering_azalea_leaves"
            | "red_poplar_leaves"
            | "orange_poplar_leaves"
            | "yellow_poplar_leaves"
            | "lily_pad"
    ) {
        TintKind::None
    } else if name.ends_with("_leaves") {
        TintKind::Foliage
    } else {
        TintKind::None
    };

    if !state.is_vanilla() {
        return dumped_visual(state).unwrap_or_else(|| modded_visual(state));
    }

    if matches!(name, "air" | "cave_air" | "void_air") {
        return RenderedBlock {
            geom: BlockGeom::Model(Default::default()),
            is_solid: false,
            waterlogged: false,
            full_cube: false,
            emission: 0,
            emissive: false,
            cull_group,
            tint_kind,
            offset: ShapeOffset::None,
            state: state.id(),
            translucent: false,
            shape: 0,
            torch: false,
        };
    }

    if matches!(name, "water" | "lava" | "bubble_column") {
        let is_lava = name == "lava";
        let still = tex(if is_lava { "lava_still" } else { "water_still" });
        let flow = tex(if is_lava { "lava_flow" } else { "water_flow" });
        let level: u32 = props.get("level").and_then(|v| v.parse().ok()).unwrap_or(0);
        let amount: u8 = if name == "bubble_column" || level == 0 || level >= 8 {
            8
        } else {
            (8 - level) as u8
        };
        let light = crate::lighting::props::of(state);
        return RenderedBlock {
            geom: BlockGeom::Fluid {
                amount,
                still,
                flow,
                lava: is_lava,
                kind: if is_lava {
                    crate::terrain::FLUID_LAVA
                } else {
                    crate::terrain::FLUID_WATER
                },
                tint: 0xFF_FFFF,
            },
            is_solid: false,
            waterlogged: false,
            full_cube: false,
            emission: light.emission,
            emissive: false,
            cull_group,
            tint_kind: TintKind::None,
            offset: ShapeOffset::None,
            state: state.id(),
            translucent: false,
            shape: 0,
            torch: false,
        };
    }

    let baked = crate::blocks::registry().baked(state.id(), name, &props);
    let offset = ShapeOffset::for_block(name);
    // What its sides cover of a neighbour's faces; a block drawn off its
    // cell (an offset flower) or in a see-through layer covers nothing.
    let shape = if offset == ShapeOffset::None && !translucent_layer(name) {
        crate::blocks::shape::intern(crate::blocks::shape::sides_of(&baked))
    } else {
        0
    };
    let is_solid = baked.is_solid;
    let full_cube = baked.full_cube;
    let light = crate::lighting::props::of(state);
    RenderedBlock {
        geom: BlockGeom::Model(baked),
        is_solid,
        full_cube,
        emission: light.emission,
        emissive: emissive_rendering(name, &props),
        waterlogged: !is_solid
            && (IMPLICIT_WATER.contains(&name) || bool_prop(&props, "waterlogged")),
        cull_group,
        tint_kind,
        offset,
        state: state.id(),
        translucent: translucent_layer(name),
        shape,
        torch: name.ends_with("torch"),
    }
}

fn translucent_layer(name: &str) -> bool {
    matches!(
        name,
        "ice" | "frosted_ice" | "slime_block" | "honey_block" | "tinted_glass" | "nether_portal"
    ) || name.ends_with("_stained_glass")
        || name.ends_with("_stained_glass_pane")
}

/// A plugin's tint as the mesher's kind.
fn tint_kind_of(tint: torch_mod_api::Tint) -> TintKind {
    match tint {
        torch_mod_api::Tint::None => TintKind::None,
        torch_mod_api::Tint::Grass => TintKind::Grass,
        torch_mod_api::Tint::Foliage => TintKind::Foliage,
        torch_mod_api::Tint::DryFoliage => TintKind::DryFoliage,
        torch_mod_api::Tint::Water => TintKind::Water,
        torch_mod_api::Tint::Color(rgb) => TintKind::Color(rgb & 0xFF_FFFF),
    }
}

/// The tint of a modded block whose model has tinted faces. A mod's block
/// colour handler is client code no dump reaches, so this goes by name the
/// way vanilla's handlers are assigned: leaves and vines take the foliage
/// map, grass-like plants the grass map, water the water colour; anything
/// else tinted, foliage. Untinted models stay untinted.
fn dumped_tint(name: &str, baked: &crate::blocks::BakedBlock) -> TintKind {
    let tinted = baked
        .parts
        .iter()
        .flat_map(|p| &p.choices)
        .any(|m| m.quads.iter().any(|q| q.tint >= 0));
    if !tinted {
        return TintKind::None;
    }
    let path = name.rsplit(':').next().unwrap_or(name);
    let has = |words: &[&str]| words.iter().any(|w| path.contains(w));
    if has(&["water"]) {
        TintKind::Water
    } else if has(&["grass", "fern", "sugar_cane", "lily", "moss_carpet"]) && !has(&["leaves"]) {
        TintKind::Grass
    } else {
        TintKind::Foliage
    }
}

/// A modded state a dump named (`blocks::modded`): its mod's own model.
/// `None` when the state isn't dumped or its model didn't load.
fn dumped_visual(state: azalea::block::BlockState) -> Option<RenderedBlock> {
    use crate::blocks::modded::Render;
    let (name, props, light, render, block_entity, offset, fluid) =
        crate::blocks::modded::with_state(state.id(), |s| {
            let props: HashMap<&str, &str> = s.props.iter().copied().collect();
            (
                s.name,
                props,
                s.fact.light,
                s.fact.render,
                s.block_entity,
                s.fact.offset,
                s.fact.fluid,
            )
        })?;
    let empty = |state: azalea::block::BlockState| RenderedBlock {
        geom: BlockGeom::Model(Default::default()),
        is_solid: false,
        waterlogged: false,
        full_cube: false,
        emission: light,
        emissive: false,
        cull_group: 0,
        tint_kind: TintKind::None,
        offset: ShapeOffset::None,
        state: state.id(),
        translucent: false,
        shape: 0,
        torch: false,
    };
    // A fluid block (`LiquidBlock` renders invisible): its fluid draws.
    if render == Render::Invisible
        && let Some(geom) = fluid.and_then(fluid_geom)
    {
        return Some(RenderedBlock {
            geom,
            ..empty(state)
        });
    }
    let drawn_by_plugin = block_entity.is_some_and(crate::modapi::replaces_block_model);
    if render == Render::Invisible || drawn_by_plugin {
        return Some(empty(state));
    }
    let baked = crate::blocks::registry().baked(state.id(), name, &props);
    if baked.parts.is_empty() {
        // A block entity renderer draws it in the mod; nothing to show here.
        return (render == Render::Animated).then(|| empty(state));
    }
    // A plugin's tint (`Plugin::block_tint`) wins over the name guess.
    let tint_kind = {
        let props: Vec<(&str, &str)> = props.iter().map(|(k, v)| (*k, *v)).collect();
        match crate::modapi::block_tint(state.id(), name, &props) {
            Some(tint) => tint_kind_of(tint),
            None => dumped_tint(name, &baked),
        }
    };
    // A see-through layer, or a block drawn off its cell, covers none of
    // its neighbours' faces.
    let translucent = baked.translucent;
    let shape = if translucent || offset != ShapeOffset::None {
        0
    } else {
        crate::blocks::shape::intern(crate::blocks::shape::sides_of(&baked))
    };
    Some(RenderedBlock {
        is_solid: baked.is_solid && !translucent,
        full_cube: baked.full_cube,
        geom: BlockGeom::Model(baked),
        emission: light,
        emissive: false,
        waterlogged: bool_prop(&props, "waterlogged") || fluid.is_some_and(|f| is_water(f.kind)),
        cull_group: 0,
        tint_kind,
        offset,
        state: state.id(),
        translucent,
        shape,
        torch: false,
    })
}

fn is_water(fluid: &str) -> bool {
    matches!(fluid, "minecraft:water" | "minecraft:flowing_water")
}

/// A dumped fluid state's geometry: vanilla's water and lava as their own,
/// a modded fluid by its look (`blocks::modded_fluids`).
fn fluid_geom(fluid: crate::blocks::modded::FluidFact) -> Option<BlockGeom> {
    use crate::terrain::{FLUID_LAVA, FLUID_MODDED, FLUID_WATER};
    let (still, flow, kind, tint) = match fluid.kind {
        k if is_water(k) => (
            tex("water_still"),
            tex("water_flow"),
            FLUID_WATER,
            0xFF_FFFF,
        ),
        "minecraft:lava" | "minecraft:flowing_lava" => {
            (tex("lava_still"), tex("lava_flow"), FLUID_LAVA, 0xFF_FFFF)
        }
        k => {
            use crate::blocks::modded_fluids::{atlas_key, look, slot};
            let look = look(k)?;
            (
                tex(atlas_key(&look.still)),
                tex(atlas_key(&look.flowing)),
                FLUID_MODDED + slot(k),
                look.tint,
            )
        }
    };
    Some(BlockGeom::Fluid {
        amount: fluid.amount,
        still,
        flow,
        lava: kind == FLUID_LAVA,
        kind,
        tint,
    })
}

/// A modded block (VULKAN_REWRITE_CHECKLIST.md 3.8): a full cube in the
/// missing texture, tinted by its raw id, solid as stone for collision and
/// lighting (`blocks::info::stand_in`).
fn modded_visual(state: azalea::block::BlockState) -> RenderedBlock {
    static CUBE: std::sync::OnceLock<std::sync::Arc<crate::blocks::BakedBlock>> =
        std::sync::OnceLock::new();
    let cube = CUBE.get_or_init(|| {
        let stone = block_visual(azalea::block::BlockState::from(
            azalea_registry::builtin::BlockKind::Stone,
        ));
        let BlockGeom::Model(stone) = stone.geom else {
            return std::sync::Arc::default();
        };
        let rows = crate::blocks::registry().atlas_rows();
        let from = crate::blocks::bake::tile_uv_rect(tex("stone"), rows);
        let to = crate::blocks::bake::tile_uv_rect(tex(crate::scene::atlas::MISSING_TEXTURE), rows);
        let remap = |uv: [f32; 2]| {
            let u = (uv[0] - from[0]) / (from[2] - from[0]);
            let v = (uv[1] - from[1]) / (from[3] - from[1]);
            [to[0] + u * (to[2] - to[0]), to[1] + v * (to[3] - to[1])]
        };
        let parts = stone
            .parts
            .iter()
            .map(|slot| crate::blocks::bake::BakedSlot {
                choices: slot
                    .choices
                    .iter()
                    .map(|model| {
                        let mut quads = model.quads.clone();
                        for quad in &mut quads {
                            quad.uv = quad.uv.map(remap);
                            quad.tint = 0;
                        }
                        std::sync::Arc::new(crate::blocks::BakedModel {
                            quads,
                            had_elements: model.had_elements,
                            occludes: model.occludes,
                            full_cube: model.full_cube,
                            ambient_occlusion: model.ambient_occlusion,
                            translucent: false,
                            missing_textures: 0,
                            particle_uv: Some(to),
                        })
                    })
                    .collect(),
                weights: slot.weights.clone(),
                total: slot.total,
            })
            .collect();
        std::sync::Arc::new(crate::blocks::BakedBlock {
            parts,
            is_solid: stone.is_solid,
            full_cube: stone.full_cube,
            ambient_occlusion: stone.ambient_occlusion,
            randomized: false,
            translucent: false,
        })
    });
    RenderedBlock {
        geom: BlockGeom::Model(cube.clone()),
        is_solid: true,
        waterlogged: false,
        full_cube: true,
        emission: 0,
        emissive: false,
        cull_group: 0,
        tint_kind: TintKind::Modded(state.id() as u32),
        offset: ShapeOffset::None,
        state: state.id(),
        translucent: false,
        // A stand-in cube is solid, which culls its neighbours already.
        shape: 0,
        torch: false,
    }
}

/// A modded block's tint: a bright colour picked by hashing its raw id.
/// `0xRRGGBB` as the 0..1 multipliers a tint is.
pub fn rgb_ratio(rgb: u32) -> [f32; 3] {
    let c = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    [c(16), c(8), c(0)]
}

pub fn modded_tint(id: u32) -> [f32; 3] {
    let h = id
        .wrapping_mul(0x9E37_79B9)
        .rotate_left(13)
        .wrapping_mul(0x85EB_CA6B);
    let channel = |shift: u32| 0.45 + ((h >> shift) & 0xFF) as f32 / 255.0 * 0.55;
    [channel(0), channel(8), channel(16)]
}

/// A block state's block id, `stone` (see [`crate::blocks::info`]).
pub fn block_id(state: azalea::block::BlockState) -> &'static str {
    crate::blocks::info::info(state).id
}

pub fn is_solid(state: azalea::block::BlockState) -> bool {
    use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
    // A modded state by its dump; one no dump names is solid, as stone.
    if !state.is_vanilla() {
        return crate::blocks::modded::with_state(state.id(), |s| s.fact.solid_render)
            .unwrap_or(true);
    }
    const UNKNOWN: u8 = 0;
    const SOLID: u8 = 1;
    const OPEN: u8 = 2;
    const STATES: usize = azalea::block::BlockState::MAX_STATE as usize + 1;
    static CACHE: [AtomicU8; STATES] = [const { AtomicU8::new(UNKNOWN) }; STATES];

    let Some(slot) = CACHE.get(state.id() as usize) else {
        return block_visual(state).is_solid;
    };
    match slot.load(Relaxed) {
        SOLID => true,
        OPEN => false,
        _ => {
            let solid = block_visual(state).is_solid;
            slot.store(if solid { SOLID } else { OPEN }, Relaxed);
            solid
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EyeFluid {
    Water,
    Lava,
}

fn fluid_at(block: &crate::blocks::info::StateInfo) -> Option<(EyeFluid, u8)> {
    let name = block.id;
    let level_amount = || {
        let level: u32 = block.get("level").and_then(|v| v.parse().ok()).unwrap_or(0);
        if level == 0 || level >= 8 {
            8
        } else {
            (8 - level) as u8
        }
    };
    match name {
        "water" | "bubble_column" => Some((
            EyeFluid::Water,
            if name == "bubble_column" {
                8
            } else {
                level_amount()
            },
        )),
        "lava" => Some((EyeFluid::Lava, level_amount())),
        "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => Some((EyeFluid::Water, 8)),
        _ if block.flag("waterlogged") => Some((EyeFluid::Water, 8)),
        _ => None,
    }
}

#[cfg(feature = "shader_support")]
pub fn powder_snow_at(x: f64, y: f64, z: f64) -> bool {
    let Some(world) = crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone()
    else {
        return false;
    };
    let world = crate::client::tracking::read_world(&world);
    let pos =
        azalea_core::position::BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
    world
        .get_block_state(pos)
        .is_some_and(|state| block_id(state) == "powder_snow")
}

pub fn eye_fluid_at(x: f64, y: f64, z: f64) -> Option<EyeFluid> {
    let world = crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone()?;
    let world = crate::client::tracking::read_world(&world);
    let pos =
        azalea_core::position::BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let state = world.get_block_state(pos)?;
    let (kind, amount) = fluid_at(crate::blocks::info::info(state))?;

    let same_above = world
        .get_block_state(pos.up(1))
        .and_then(|s| fluid_at(crate::blocks::info::info(s)))
        .is_some_and(|(above_kind, _)| above_kind == kind);

    let height = if amount == 8 || same_above {
        1.0
    } else {
        amount as f32 / 9.0
    };
    let frac_y = (y - pos.y as f64) as f32;
    crate::log_debug!(
        "fog",
        "eye {pos:?} frac_y={frac_y:.3} name={} amount={amount} height={height:.3} same_above={same_above}",
        block_id(state)
    );
    (frac_y < height).then_some(kind)
}
