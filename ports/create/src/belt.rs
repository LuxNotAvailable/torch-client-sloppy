//! `BeltRenderer`: each belt block draws its belt partial (top and bottom
//! halves, or one diagonal piece) with the texture scrolling at the belt's
//! speed, and a turning pulley at each end. Items on the belt aren't
//! ported (no item rendering in the API yet).

use torch_mod_api::{BlockEntityView, Part, Scroll, Transform};

use crate::dir::{Axis, Dir, java_rotations};
use crate::kinetic;

/// The scroll targets no model names: plain, then each dye's.
pub fn sprites() -> Vec<String> {
    let mut out = vec![
        "create:block/belt_scroll".to_owned(),
        "create:block/belt_diagonal_scroll".to_owned(),
    ];
    for dye in DYES {
        out.push(format!("create:block/belt/{dye}_scroll"));
        out.push(format!("create:block/belt/{dye}_diagonal_scroll"));
    }
    out
}

const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `BeltRenderer.getBeltPartial`.
fn partial(diagonal: bool, start: bool, end: bool, bottom: bool) -> &'static str {
    match (diagonal, bottom, start, end) {
        (true, _, true, _) => "create:block/belt/diagonal_start",
        (true, _, _, true) => "create:block/belt/diagonal_end",
        (true, ..) => "create:block/belt/diagonal_middle",
        (false, true, true, _) => "create:block/belt/start_bottom",
        (false, true, _, true) => "create:block/belt/end_bottom",
        (false, true, ..) => "create:block/belt/middle_bottom",
        (false, false, true, _) => "create:block/belt/start",
        (false, false, _, true) => "create:block/belt/end",
        (false, false, ..) => "create:block/belt/middle",
    }
}

/// `BeltRenderer.getSpriteShiftEntry`: (original, scroll target).
fn sprite_shift(dye: Option<&str>, diagonal: bool, bottom: bool) -> (&'static str, String) {
    let from = if diagonal {
        "create:block/belt_diagonal"
    } else if bottom {
        "create:block/belt_offset"
    } else {
        "create:block/belt"
    };
    let to = match (dye, diagonal) {
        (Some(d), true) => format!("create:block/belt/{d}_diagonal_scroll"),
        (Some(d), false) => format!("create:block/belt/{d}_scroll"),
        (None, true) => "create:block/belt_diagonal_scroll".to_owned(),
        (None, false) => "create:block/belt_scroll".to_owned(),
    };
    (from, to)
}

pub fn belt(view: &BlockEntityView) -> Vec<Part> {
    let Some(facing) = view.prop("facing").and_then(Dir::parse) else {
        return Vec::new();
    };
    let slope = view.prop("slope").unwrap_or("horizontal");
    let part = view.prop("part").unwrap_or("middle");
    let downward = slope == "downward";
    let upward = slope == "upward";
    let diagonal = downward || upward;
    let sideways = slope == "sideways";
    let along_x = facing.axis() == Axis::X;
    let step = if facing.positive() { 1.0 } else { -1.0 };
    let (mut start, mut end) = (part == "start", part == "end");
    if downward || (slope == "vertical" && facing.positive()) {
        std::mem::swap(&mut start, &mut end);
    }
    let rotate = java_rotations(&[
        (
            Axis::Y,
            facing.horizontal_angle()
                + if upward { 180.0 } else { 0.0 }
                + if sideways { 270.0 } else { 0.0 },
        ),
        (Axis::Z, if sideways { 90.0 } else { 0.0 }),
        (
            Axis::X,
            if !diagonal && slope != "horizontal" {
                90.0
            } else {
                0.0
            },
        ),
    ]);
    let transform = Transform {
        rotate,
        ..Transform::default()
    };
    // `NBTHelper.writeEnum`: the dye's name, any case.
    let dye = view
        .nbt
        .get("Dye")
        .and_then(|d| d.as_str())
        .map(str::to_ascii_lowercase);
    let dye = dye
        .as_deref()
        .and_then(|d| DYES.iter().find(|n| **n == d).copied());
    let mut speed = view.nbt_f64(&["Speed"]).unwrap_or(0.0) as f32;
    if (diagonal && (downward ^ along_x))
        || (!sideways && !diagonal && along_x)
        || (sideways && !facing.positive())
    {
        speed = -speed;
    }

    let mut parts = Vec::new();
    // `Iterate.trueAndFalse`: the bottom first; a diagonal draws only it.
    for bottom in [true, false] {
        let mut part = Part::model(partial(diagonal, start, end, bottom)).with_transform(transform);
        if speed != 0.0 || dye.is_some() {
            let (from, to) = sprite_shift(dye, diagonal, bottom);
            part = part.with_scroll(Scroll {
                from: from.to_owned(),
                to,
                per_tick: speed * step / 504.0,
                phase: if bottom { 0.5 } else { 0.0 },
                span: if diagonal { 0.375 } else { 0.5 },
            });
        }
        parts.push(part);
        if diagonal {
            break;
        }
    }

    // `hasPulley`: every part but a middle.
    if part != "middle" {
        let dir = if sideways {
            Dir::Up
        } else {
            facing.clockwise()
        };
        let mut calls = Vec::new();
        match dir.axis() {
            Axis::X => calls.push((Axis::Y, 90.0)),
            Axis::Y => calls.push((Axis::X, 90.0)),
            Axis::Z => {}
        }
        calls.push((Axis::X, 90.0));
        // `BeltBlock.getRotationAxis`.
        let axis = if sideways {
            Axis::Y
        } else {
            facing.clockwise().axis()
        };
        let speed = view.nbt_f64(&["Speed"]).unwrap_or(0.0) as f32;
        parts.push(
            Part::model("create:block/belt_pulley")
                .with_transform(Transform {
                    rotate: java_rotations(&calls),
                    ..Transform::default()
                })
                .with_animation(kinetic::spin(
                    axis,
                    speed,
                    1.0,
                    kinetic::rotation_offset(false, axis, view.pos),
                )),
        );
    }
    parts.extend(items(view));
    parts
}

/// Blocks a belt hands items to (`DirectBeltInputBehaviour`).
const INPUTS: &[&str] = &[
    "create:belt",
    "create:depot",
    "create:weighted_ejector",
    "create:chute",
    "create:smart_chute",
];

/// `BeltInventory.Ending`'s margin past the belt's end: into another
/// input 0.25, against a block 0.45, off the end 0. Out of the plugin's
/// sight (a long belt's far end), an input is assumed.
fn end_margin(
    view: &BlockEntityView,
    facing: Dir,
    verticality: i32,
    length: i32,
    positive: bool,
) -> f32 {
    let offset = if positive { length } else { -1 };
    let n = facing.normal();
    let at = [
        view.pos[0] + offset * n[0],
        view.pos[1] + offset.clamp(0, (length - 1).max(0)) * verticality,
        view.pos[2] + offset * n[2],
    ];
    match torch_mod_api::host::block(at[0], at[1], at[2]) {
        None => 0.25,
        Some(b) if INPUTS.contains(&b.block.as_str()) => 0.25,
        Some(b) if b.block.ends_with("air") => 0.0,
        Some(_) => 0.45,
    }
}

/// `BeltRenderer.renderItems`: the controller draws every item on the
/// belt. Items glide at the belt's speed (`BeltInventory.tick`) until they
/// stop at the end, each a block behind the one in front; the server says
/// when one leaves. Skipped: upright items (fluid holders), packages, the
/// shadow, and light per segment (the controller's is used).
pub fn items(view: &BlockEntityView) -> Vec<Part> {
    use crate::items::{scatter, stacked, transported};
    use crate::pose::{JavaRandom, Pose};
    use torch_mod_api::{Animation, Value};

    if view.nbt.get("IsController").and_then(Value::as_i64) != Some(1) {
        return Vec::new();
    }
    let length = view.nbt.get("Length").and_then(Value::as_i64).unwrap_or(0) as i32;
    let (Some(facing), Some(slope)) =
        (view.prop("facing").and_then(Dir::parse), view.prop("slope"))
    else {
        return Vec::new();
    };
    if length <= 0 || slope == "vertical" || slope == "sideways" {
        return Vec::new();
    }
    let len = length as f32;
    let verticality = match slope {
        "downward" => -1,
        "upward" => 1,
        _ => 0,
    };
    let n = facing.normal();
    let (dx, dz) = (n[0] as f32, n[2] as f32);
    let world = |o: f32| {
        let up = if o < 0.5 {
            0.0
        } else {
            verticality as f32 * (o.min(len - 0.5) - 0.5)
        };
        [dx * o, up, dz * o]
    };
    // `getDirectionAwareBeltMovementSpeed`: blocks a tick, along the offset.
    let flip = if facing.axis() == Axis::X { -1.0 } else { 1.0 };
    let step = if facing.positive() { 1.0 } else { -1.0 };
    let v = view.nbt_f64(&["Speed"]).unwrap_or(0.0) as f32 / 480.0 * step * flip;
    let mut list: Vec<_> = view
        .nbt
        .get("Inventory")
        .and_then(|i| i.get("Items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(transported)
        .collect();
    // Front first.
    list.sort_by(|a, b| {
        if v >= 0.0 {
            b.pos.total_cmp(&a.pos)
        } else {
            a.pos.total_cmp(&b.pos)
        }
    });
    let end = if v == 0.0 {
        0.0
    } else {
        let margin = end_margin(view, facing, verticality, length, v > 0.0);
        if v > 0.0 { len - margin } else { margin }
    };
    let t0 = view.nbt_time as f32;
    let start = [-0.5 * dx + 0.5, 0.9375, -0.5 * dz + 0.5];
    let tilt_forward = ((slope == "downward") ^ facing.positive()) == (facing.axis() == Axis::Z);
    let side_along_x = facing.clockwise().axis() == Axis::X;

    let mut parts = Vec::new();
    for (k, item) in list.iter().enumerate() {
        let from = item.pos;
        let stop = if item.locked || v == 0.0 {
            from
        } else if v > 0.0 {
            (end - k as f32).max(from)
        } else {
            (end + k as f32).min(from)
        };
        let animation = if stop == from {
            Animation::None
        } else {
            let at = |o: f32| {
                let (w, w0) = (world(o), world(from));
                [
                    t0 + (o - from) / v,
                    (w[0] - w0[0]) * 16.0,
                    (w[1] - w0[1]) * 16.0,
                    (w[2] - w0[2]) * 16.0,
                ]
            };
            let mut points = vec![[t0, 0.0, 0.0, 0.0]];
            let mut bends = [0.5, len - 0.5];
            if v < 0.0 {
                bends.reverse();
            }
            for o in bends {
                if (o - from) * v.signum() > 0.0 && (stop - o) * v.signum() > 0.0 {
                    points.push(at(o));
                }
            }
            points.push(at(stop));
            Animation::Path { points }
        };

        let p = world(from);
        let on_slope = verticality != 0 && (0.5..=len - 0.5).contains(&from);
        let slope_angle = match (on_slope, tilt_forward) {
            (false, _) => 0.0,
            (true, true) => -45.0,
            (true, false) => 45.0,
        };
        let side = if side_along_x { item.side } else { -item.side };
        let mut pose = Pose::default()
            .translate(start[0] + p[0], start[1] + p[1], start[2] + p[2])
            .translate(
                if side_along_x { side } else { 0.0 },
                0.0,
                if side_along_x { 0.0 } else { side },
            )
            .rotate(
                if facing.axis() == Axis::X {
                    Axis::Z
                } else {
                    Axis::X
                },
                slope_angle,
            );
        if on_slope {
            pose = pose.translate(0.0, 0.125, 0.0);
        }
        let angle = item.angle as f32;
        let mut random = JavaRandom::new(i64::from(item.angle));
        parts.extend(stacked(
            &item.id,
            item.count,
            pose,
            pose,
            |i, p| scatter(&mut random, i, p.rotate(Axis::Y, angle)).scale(0.5),
            |p| {
                p.rotate(Axis::Y, angle)
                    .translate(0.0, -0.09375, 0.0)
                    .rotate(Axis::X, 90.0)
                    .scale(0.5)
            },
            &animation,
        ));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(props: &[(&str, &str)], nbt: torch_mod_api::Value) -> BlockEntityView {
        BlockEntityView {
            kind: "create:belt".into(),
            block: "create:belt".into(),
            props: props
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            nbt,
            ..Default::default()
        }
    }

    #[test]
    fn a_flat_running_belt_end_draws_two_scrolling_halves_and_a_pulley() {
        let parts = belt(&view(
            &[
                ("facing", "north"),
                ("slope", "horizontal"),
                ("part", "start"),
            ],
            serde_json::json!({"Speed": 32.0}),
        ));
        assert_eq!(parts.len(), 3);
        let bottom = parts[0].scroll.as_ref().unwrap();
        assert_eq!(bottom.from, "create:block/belt_offset");
        assert_eq!(bottom.to, "create:block/belt_scroll");
        assert_eq!(bottom.phase, 0.5);
        // North is negative: time runs backwards.
        assert!((bottom.per_tick + 32.0 / 504.0).abs() < 1e-6);
        assert_eq!(parts[1].scroll.as_ref().unwrap().from, "create:block/belt");
        assert!(matches!(
            parts[2].animation,
            torch_mod_api::Animation::Spin { .. }
        ));
    }

    #[test]
    fn a_still_plain_middle_neither_scrolls_nor_has_a_pulley() {
        let parts = belt(&view(
            &[
                ("facing", "east"),
                ("slope", "horizontal"),
                ("part", "middle"),
            ],
            serde_json::json!({"Speed": 0.0}),
        ));
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.scroll.is_none()));
    }

    #[test]
    fn items_glide_to_the_end_and_queue_a_block_apart() {
        let mut v = view(
            &[
                ("facing", "east"),
                ("slope", "horizontal"),
                ("part", "start"),
            ],
            serde_json::json!({
                "IsController": 1, "Length": 3, "Speed": 32.0,
                "Inventory": {"Items": [
                    {"Item": {"id": "minecraft:iron_ingot", "count": 1}, "Pos": 1.6},
                    {"Item": {"id": "minecraft:stone", "count": 1}, "Pos": 0.4},
                ]}
            }),
        );
        v.nbt_time = 100;
        let parts = items(&v);
        assert_eq!(parts.len(), 2);
        // East flips: positive speed runs toward offset 0. The stone is in
        // front and stops at 0.25 (an input assumed past the end); the
        // ingot a block behind, at 1.25.
        let last = |p: &Part| match &p.animation {
            torch_mod_api::Animation::Path { points } => *points.last().unwrap(),
            other => panic!("{other:?}"),
        };
        let speed = 32.0 / 480.0;
        let stone = last(&parts[0]);
        assert!(
            (stone[0] - (100.0 + 0.15 / speed)).abs() < 1e-3,
            "{stone:?}"
        );
        assert!((stone[1] + 0.15 * 16.0).abs() < 1e-3, "{stone:?}");
        let ingot = last(&parts[1]);
        assert!((ingot[1] + 0.35 * 16.0).abs() < 1e-3, "{ingot:?}");
        // A middle segment draws none: only the controller has the items.
        v.nbt["IsController"] = serde_json::json!(0);
        assert!(items(&v).is_empty());
    }

    #[test]
    fn a_dyed_diagonal_draws_one_piece_from_its_dyes_strip() {
        let parts = belt(&view(
            &[("facing", "south"), ("slope", "upward"), ("part", "middle")],
            serde_json::json!({"Speed": 0.0, "Dye": "LIGHT_BLUE"}),
        ));
        assert_eq!(parts.len(), 1);
        let scroll = parts[0].scroll.as_ref().unwrap();
        assert_eq!(scroll.to, "create:block/belt/light_blue_diagonal_scroll");
        assert_eq!(scroll.span, 0.375);
    }
}
