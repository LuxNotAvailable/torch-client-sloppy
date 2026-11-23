//! `FluidTankRenderer`: a windowed tank's controller draws its fluid as one
//! box, inset by the hull, from the bottom cap up to its fill level. The
//! boiler gauges (a tank with no window) aren't ported.

use torch_mod_api::{BlockEntityView, Geometry, Part, Transform};

/// Create's server config default (`fluidTankCapacity`, buckets a block);
/// a server that changes it would make levels read wrong.
const BUCKETS_PER_BLOCK: f64 = 8.0;
const CAP_HEIGHT: f32 = 0.25;
const HULL_WIDTH: f32 = 0.0703125;
const MIN_PUDDLE: f32 = 0.0625;

/// The sprite and ARGB tint a fluid draws with. Vanilla's water and lava,
/// else the mod's `<ns>:fluid/<path>_still` (Create's own fluids are named
/// that way).
pub fn fluid_sprite(id: &str) -> (String, u32) {
    match id {
        "minecraft:water" | "minecraft:flowing_water" => {
            ("minecraft:block/water_still".into(), 0xFF3F_76E4)
        }
        "minecraft:lava" | "minecraft:flowing_lava" => {
            ("minecraft:block/lava_still".into(), 0xFFFF_FFFF)
        }
        _ => {
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
            let path = path.strip_prefix("flowing_").unwrap_or(path);
            (format!("{ns}:fluid/{path}_still"), 0xFFFF_FFFF)
        }
    }
}

/// The sprites this module may draw that no block model uses.
pub const SPRITES: &[&str] = &[
    "create:fluid/honey_still",
    "create:fluid/chocolate_still",
    "create:fluid/tea_still",
    "create:fluid/milk_still",
    "create:fluid/potion_still",
];

pub fn tank(view: &BlockEntityView) -> Vec<Part> {
    // Only the controller (no `Controller` key) draws, and only behind a
    // window (`Window`, an NBT byte).
    let window = view.nbt.get("Window").and_then(|w| w.as_i64());
    if view.nbt.get("Controller").is_some() || window == Some(0) {
        return Vec::new();
    }
    let width = view.nbt_f64(&["Size"]).unwrap_or(1.0).max(1.0) as f32;
    let height = view.nbt_f64(&["Height"]).unwrap_or(1.0).max(1.0) as f32;
    let Some(fluid) = view.nbt.get("TankContent").and_then(|t| t.get("Fluid")) else {
        return Vec::new();
    };
    let Some(id) = fluid.get("id").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let amount = fluid.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let capacity = (width * width * height) as f64 * BUCKETS_PER_BLOCK * 1000.0;
    let level = (amount / capacity).clamp(0.0, 1.0) as f32;
    let total = height - 2.0 * CAP_HEIGHT - MIN_PUDDLE;
    if level < 1.0 / (512.0 * total) {
        return Vec::new();
    }
    let filled = (level * total).clamp(0.0, total);
    let bottom = CAP_HEIGHT + MIN_PUDDLE;
    let (texture, tint) = fluid_sprite(id);
    let sixteenths = |v: f32| v * 16.0;
    vec![Part {
        geometry: Geometry::Cuboid {
            from: [
                sixteenths(HULL_WIDTH),
                sixteenths(bottom),
                sixteenths(HULL_WIDTH),
            ],
            to: [
                sixteenths(width - HULL_WIDTH),
                sixteenths(bottom + filled),
                sixteenths(width - HULL_WIDTH),
            ],
            texture,
            // Up and the four sides: `renderFluidBox` without its bottom.
            faces: 2 | 4 | 8 | 16 | 32,
            tint,
            uv: None,
        },
        transform: Transform::default(),
        animation: torch_mod_api::Animation::None,
        scroll: None,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(nbt: torch_mod_api::Value) -> BlockEntityView {
        BlockEntityView {
            kind: "create:fluid_tank".into(),
            pos: [0, 0, 0],
            block: "create:fluid_tank".into(),
            nbt,
            ..Default::default()
        }
    }

    #[test]
    fn a_half_full_one_block_tank_fills_half_its_inside() {
        let parts = tank(&view(serde_json::json!({
            "Window": 1, "Size": 1, "Height": 1,
            "TankContent": {"Fluid": {"id": "minecraft:water", "amount": 4000}}})));
        assert_eq!(parts.len(), 1);
        let Geometry::Cuboid {
            from,
            to,
            texture,
            tint,
            ..
        } = &parts[0].geometry
        else {
            panic!()
        };
        assert_eq!(texture, "minecraft:block/water_still");
        assert_eq!(*tint, 0xFF3F_76E4);
        // Inside 1 - 0.5 - 0.0625 tall, half of it filled.
        let filled = (to[1] - from[1]) / 16.0;
        assert!((filled - 0.21875).abs() < 1e-5, "{filled}");
        assert!((from[1] / 16.0 - 0.3125).abs() < 1e-5);
    }

    #[test]
    fn a_tank_part_or_a_windowless_tank_draws_nothing() {
        assert!(tank(&view(serde_json::json!({"Controller": [0, 1, 0]}))).is_empty());
        assert!(
            tank(&view(
                serde_json::json!({"Window": 0, "Size": 1, "Height": 1,
            "TankContent": {"Fluid": {"id": "minecraft:water", "amount": 4000}}})
            ))
            .is_empty()
        );
    }

    #[test]
    fn mod_fluids_use_their_still_sprite() {
        assert_eq!(fluid_sprite("create:honey").0, "create:fluid/honey_still");
        assert_eq!(
            fluid_sprite("create:flowing_honey").0,
            "create:fluid/honey_still"
        );
    }
}
