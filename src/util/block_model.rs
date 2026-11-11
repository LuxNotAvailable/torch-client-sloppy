use std::collections::HashMap;

use azalea::block::BlockTrait;

use crate::TEXTURE_MAP;
use crate::blocks::rand::ShapeOffset;
use crate::renderer::{BlockGeom, RenderedBlock, TintKind};

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
    {
        return block.as_block_kind() as u32 + 2;
    }
    0
}

pub fn emissive_rendering(name: &str, props: &HashMap<&str, &str>) -> bool {
    match name {
        "magma_block" => true,
        "sculk_sensor" | "calibrated_sculk_sensor" => {
            props.get("sculk_sensor_phase").copied() == Some("active")
        }
        _ => false,
    }
}

/// A block's `SoundType`: its events, and the volume and pitch they scale.
#[cfg(audio)]
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

#[cfg(audio)]
pub use crate::generated_block_sounds::sound_group;

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
        return modded_visual(state);
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
        };
    }

    let baked = crate::blocks::registry().baked(state.id(), name, &props);
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
        offset: ShapeOffset::for_block(name),
        state: state.id(),
        translucent: translucent_layer(name),
    }
}

fn translucent_layer(name: &str) -> bool {
    matches!(
        name,
        "ice" | "frosted_ice" | "slime_block" | "honey_block" | "tinted_glass" | "nether_portal"
    ) || name.ends_with("_stained_glass")
        || name.ends_with("_stained_glass_pane")
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
        let to =
            crate::blocks::bake::tile_uv_rect(tex(crate::renderer::atlas::MISSING_TEXTURE), rows);
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
    }
}

/// A modded block's tint: a bright colour picked by hashing its raw id.
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
    // A modded state is solid, as stone (`blocks::info::stand_in`).
    let state = crate::blocks::info::stand_in(state);
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
