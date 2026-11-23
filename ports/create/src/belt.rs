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
