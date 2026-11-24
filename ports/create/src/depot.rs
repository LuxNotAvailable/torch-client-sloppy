//! `DepotRenderer.renderItemsOf`: the held item (and any still coming in)
//! on the depot's top, the processing outputs in a ring. Items coming in
//! are drawn where the NBT has them; their ease to the centre isn't
//! played. Skipped: upright items and packages.

use torch_mod_api::{Animation, BlockEntityView, Part, Value};

use crate::dir::Axis;
use crate::items::{Transported, scatter, stack, stacked, transported};
use crate::pose::{JavaRandom, Pose};

/// `DepotRenderer.renderItem`.
fn item(pose: Pose, id: &str, count: u32, angle: f32, random: JavaRandom) -> Vec<Part> {
    let pose = pose.rotate(Axis::Y, angle);
    let mut random = random;
    stacked(
        id,
        count,
        pose,
        pose,
        |i, p| scatter(&mut random, i, p).scale(0.5),
        |p| {
            p.scale(0.5)
                .translate(0.0, -0.1875, 0.0)
                .rotate(Axis::X, 90.0)
        },
        &Animation::None,
    )
}

pub fn depot(view: &BlockEntityView) -> Vec<Part> {
    let base = Pose::default().translate(0.5, 0.9375, 0.5);
    let mut parts = Vec::new();

    let held = view.nbt.get("HeldItem").and_then(transported);
    let incoming = view
        .nbt
        .get("Incoming")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(transported);
    for t in held.into_iter().chain(incoming) {
        parts.extend(item(
            placed(base, &t),
            &t.id,
            t.count,
            t.angle as f32,
            JavaRandom::new(0),
        ));
    }

    let outputs = view
        .nbt
        .get("OutputBuffer")
        .and_then(|b| b.get("Items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for out in outputs {
        let Some((id, count)) = stack(out) else {
            continue;
        };
        let slot = out.get("Slot").and_then(Value::as_i64).unwrap_or(0);
        let pose = base
            .rotate(Axis::Y, 45.0 * slot as f32)
            .translate(0.35, 0.0, 0.0);
        let mut random = JavaRandom::new(slot + 1);
        let angle = (360.0 * random.next_float()) as i32 as f32;
        parts.extend(item(pose, &id, count, angle, random));
    }
    parts
}

/// Where an item coming in from a side sits: `0.5 - pos` back toward
/// where it came from, and its side offset.
fn placed(base: Pose, t: &Transported) -> Pose {
    if t.from.axis() == Axis::Y {
        return base;
    }
    let n = t.from.opposite().normal();
    let back = 0.5 - t.pos;
    let along_x = t.from.clockwise().axis() == Axis::X;
    let side = if along_x { t.side } else { -t.side };
    base.translate(n[0] as f32 * back, 0.0, n[2] as f32 * back)
        .translate(
            if along_x { side } else { 0.0 },
            0.0,
            if along_x { 0.0 } else { side },
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use torch_mod_api::Geometry;

    #[test]
    fn a_depot_draws_its_held_stack_and_its_outputs() {
        let view = BlockEntityView {
            kind: "create:depot".into(),
            block: "create:depot".into(),
            nbt: serde_json::json!({
                "HeldItem": {"Item": {"id": "minecraft:iron_ingot", "count": 16},
                             "Pos": 0.5, "Angle": 12, "InDirection": 1},
                "OutputBuffer": {"Items": [{"Slot": 2, "id": "create:iron_sheet", "count": 1}],
                                 "Size": 8}
            }),
            ..Default::default()
        };
        let parts = depot(&view);
        // Sixteen ingots: three copies; one sheet.
        assert_eq!(parts.len(), 4);
        let Geometry::Item { item, flat } = &parts[0].geometry else {
            panic!("{:?}", parts[0].geometry);
        };
        assert_eq!(item, "minecraft:iron_ingot");
        assert!(flat.is_some());
        // The held stack sits at the top's centre.
        let t = parts[0].transform.translate;
        assert!(
            (t[0] - 8.0).abs() < 1e-3 && (t[1] - 15.0).abs() < 1e-3,
            "{t:?}"
        );
        assert!(
            matches!(&parts[3].geometry, Geometry::Item { item, .. } if item == "create:iron_sheet")
        );
    }
}
