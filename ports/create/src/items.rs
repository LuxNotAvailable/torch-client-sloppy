//! Item stacks as Create's belts and depots keep them (`TransportedItemStack`)
//! and the copies their renderers stack up. Each copy is a part with two
//! transforms: Java picks by `BakedModel.isGui3d()`, the host by whether
//! the item's model is a flat sprite.

use torch_mod_api::{Animation, Part, Value};

use crate::dir::{Axis, Dir};
use crate::pose::{JavaRandom, Pose};

/// A `TransportedItemStack` from its NBT.
#[derive(Clone, Debug, PartialEq)]
pub struct Transported {
    pub id: String,
    pub count: u32,
    /// `beltPosition`: blocks along the belt (a depot: 0 at the edge, 0.5
    /// at the centre).
    pub pos: f32,
    /// `sideOffset`.
    pub side: f32,
    pub angle: i32,
    /// `insertedFrom`.
    pub from: Dir,
    /// Held by processing (a press, a fan): it doesn't move.
    pub locked: bool,
}

/// An `ItemStack`'s id and count (`{"id": ..., "count": ...}`).
pub fn stack(nbt: &Value) -> Option<(String, u32)> {
    let id = nbt.get("id")?.as_str()?.to_owned();
    let count = nbt.get("count").and_then(Value::as_u64).unwrap_or(1) as u32;
    (count > 0).then_some((id, count))
}

pub fn transported(nbt: &Value) -> Option<Transported> {
    let (id, count) = stack(nbt.get("Item")?)?;
    let f = |key: &str| nbt.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let from = nbt.get("InDirection").and_then(Value::as_u64).unwrap_or(0) as usize;
    Some(Transported {
        id,
        count,
        pos: f("Pos"),
        side: f("Offset"),
        angle: nbt.get("Angle").and_then(Value::as_i64).unwrap_or(0) as i32,
        from: crate::dir::ALL.get(from).copied().unwrap_or(Dir::Down),
        locked: nbt.get("Locked").and_then(Value::as_i64).unwrap_or(0) != 0,
    })
}

/// Extra copies drawn for a stack: `Mth.log2(count) / 2`.
pub fn copies(count: u32) -> u32 {
    if count == 0 {
        0
    } else {
        (31 - count.leading_zeros()) / 2
    }
}

/// The copy loop both renderers run, as parts: `block` for a 3D model,
/// `flat` for a sprite, each called with the copy's index and the pose
/// so far, returning the copy's pose.
pub fn stacked(
    id: &str,
    count: u32,
    mut block: Pose,
    mut flat: Pose,
    mut block_copy: impl FnMut(u32, Pose) -> Pose,
    flat_copy: impl Fn(Pose) -> Pose,
    animation: &Animation,
) -> Vec<Part> {
    let mut parts = Vec::new();
    for i in 0..=copies(count) {
        let mut part = Part::item(id).with_animation(animation.clone());
        part.transform = block_copy(i, block).transform();
        if let torch_mod_api::Geometry::Item { flat: f, .. } = &mut part.geometry {
            *f = Some(flat_copy(flat).transform());
        }
        parts.push(part);
        block = block.translate(0.0, 1.0 / 64.0, 0.0);
        flat = flat.rotate(Axis::Y, 10.0).translate(0.0, 1.0 / 16.0, 0.0);
    }
    parts
}

/// The scatter a 3D stack's copies get: `nextFloat() / 16 * i` on X and Z.
pub fn scatter(random: &mut JavaRandom, i: u32, pose: Pose) -> Pose {
    let x = random.next_float() * 0.0625 * i as f32;
    let z = random.next_float() * 0.0625 * i as f32;
    pose.translate(x, 0.0, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_of_sixty_four_draws_four() {
        assert_eq!(copies(1), 0);
        assert_eq!(copies(3), 0);
        assert_eq!(copies(4), 1);
        assert_eq!(copies(64), 3);
    }

    #[test]
    fn a_transported_stack_reads_its_nbt() {
        let t = transported(&serde_json::json!({
            "Item": {"id": "minecraft:iron_ingot", "count": 5},
            "Pos": 1.25, "Offset": 0.1, "Angle": 30, "InDirection": 2
        }))
        .unwrap();
        assert_eq!(
            (t.id.as_str(), t.count, t.pos, t.angle),
            ("minecraft:iron_ingot", 5, 1.25, 30)
        );
        assert_eq!(t.from, Dir::North);
        assert!(!t.locked);
    }
}
