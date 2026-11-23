//! Create's kinetic renderers (`content/kinetics`): everything turns about
//! its axis at the block entity's `Speed` (rpm), angle =
//! time * speed * 3 / 10 + offset (`KineticBlockEntityRenderer.getAngleForBe`).

use torch_mod_api::{Animation, BlockEntityView, Geometry, Part, Transform};

use crate::dir::{self, ALL, Axis, Dir};

const SHAFT_HALF: &str = "create:block/shaft_half";
const SHAFTLESS_COGWHEEL: &str = "create:block/cogwheel_shaftless";
const SHAFTLESS_LARGE_COGWHEEL: &str = "create:block/large_cogwheel_shaftless";
const COGWHEEL_SHAFT: &str = "create:block/cogwheel_shaft";
const WATER_WHEEL: &str = "create:block/water_wheel/wheel";
const LARGE_WATER_WHEEL: &str = "create:block/large_water_wheel/block";
const LARGE_WATER_WHEEL_EXTENSION: &str = "create:block/large_water_wheel/block_extension";

/// The block entity's speed in rpm (`KineticBlockEntity.speed`, NBT `Speed`).
fn speed(view: &BlockEntityView) -> f32 {
    view.nbt_f64(&["Speed"]).unwrap_or(0.0) as f32
}

/// `KineticBlockEntityVisual.shouldOffset`: Java's `%`, sign and all.
pub fn should_offset(axis: Axis, pos: [i32; 3]) -> bool {
    let x = if axis == Axis::X { 0 } else { pos[0] };
    let y = if axis == Axis::Y { 0 } else { pos[1] };
    let z = if axis == Axis::Z { 0 } else { pos[2] };
    (x + y + z) % 2 == 0
}

/// `KineticBlockEntityVisual.rotationOffset`: cogs mesh by position parity;
/// a large cog's half-tooth sits at 11.25 degrees.
pub fn rotation_offset(large_cog: bool, axis: Axis, pos: [i32; 3]) -> f32 {
    if should_offset(axis, pos) {
        22.5
    } else if large_cog {
        11.25
    } else {
        0.0
    }
}

/// `BracketedKineticBlockEntityRenderer.getShaftAngleOffset`.
fn shaft_offset(axis: Axis, pos: [i32; 3]) -> f32 {
    if should_offset(axis, pos) { 22.5 } else { 0.0 }
}

/// A turn about `axis` through the block's centre at `speed` rpm, scaled by
/// `modifier` (a gearshift's reversal), starting at `offset` degrees.
pub fn spin(axis: Axis, speed: f32, modifier: f32, offset: f32) -> Animation {
    Animation::Spin {
        axis: axis.vector(),
        pivot: [8.0; 3],
        degrees_per_tick: speed * 3.0 / 10.0 * modifier,
        phase: offset,
    }
}

fn model(id: &str, transform: Transform, animation: Animation) -> Part {
    Part::model(id)
        .with_transform(transform)
        .with_animation(animation)
}

/// The block's own model, turning: `KineticBlockEntityRenderer`'s default.
fn own_block(view: &BlockEntityView, animation: Animation) -> Part {
    Part {
        geometry: Geometry::Block {
            block: view.block.clone(),
            props: view.props.clone(),
        },
        transform: Transform::default(),
        animation,
        scroll: None,
    }
}

/// `IRotate.getRotationAxis` for the blocks here: an `axis` property, or
/// a `facing` one's axis, or (a sequenced gearshift) `vertical`.
pub fn rotation_axis(view: &BlockEntityView) -> Axis {
    if view.prop("vertical") == Some("true") {
        return Axis::Y;
    }
    view.prop("axis")
        .and_then(Axis::parse)
        .or_else(|| view.prop("facing").and_then(Dir::parse).map(Dir::axis))
        .unwrap_or(Axis::Y)
}

/// The block entity's `Source` (the block driving it), if it has one.
fn source(view: &BlockEntityView) -> Option<[i32; 3]> {
    let s = view.nbt.get("Source")?.as_array()?;
    let at = |i: usize| s.get(i)?.as_i64().map(|v| v as i32);
    Some([at(0)?, at(1)?, at(2)?])
}

/// `KineticBlockEntity.getSourceFacing`.
fn source_facing(view: &BlockEntityView) -> Option<Dir> {
    let s = source(view)?;
    Some(Dir::nearest([
        s[0] - view.pos[0],
        s[1] - view.pos[1],
        s[2] - view.pos[2],
    ]))
}

/// `simple_kinetic`: shafts and cogwheels turn their own model; a large
/// cogwheel turns its shaftless wheel and its shaft at their own offsets
/// (`BracketedKineticBlockEntityRenderer`).
pub fn simple(view: &BlockEntityView) -> Vec<Part> {
    let axis = rotation_axis(view);
    let speed = speed(view);
    if view.block == "create:large_cogwheel" {
        let facing = dir::to_face_vertical(Dir::from_axis(axis, true));
        return vec![
            model(
                SHAFTLESS_LARGE_COGWHEEL,
                facing,
                spin(axis, speed, 1.0, rotation_offset(true, axis, view.pos)),
            ),
            model(
                COGWHEEL_SHAFT,
                facing,
                spin(axis, speed, 1.0, shaft_offset(axis, view.pos)),
            ),
        ];
    }
    vec![own_block(
        view,
        spin(axis, speed, 1.0, rotation_offset(false, axis, view.pos)),
    )]
}

/// `encased_shaft` and the like (`ShaftRenderer`): a shaft inside the
/// casing, which stays the block's own model.
pub fn encased_shaft(view: &BlockEntityView) -> Vec<Part> {
    let axis = rotation_axis(view);
    let shaft = Part {
        geometry: Geometry::Block {
            block: "create:shaft".into(),
            props: [("axis".to_owned(), axis_name(axis).to_owned())].into(),
        },
        transform: Transform::default(),
        animation: spin(
            axis,
            speed(view),
            1.0,
            rotation_offset(false, axis, view.pos),
        ),
        scroll: None,
    };
    vec![shaft]
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
}

/// `EncasedCogRenderer`: the shaftless cog, and a half shaft on each side
/// its `top_shaft` / `bottom_shaft` says.
pub fn encased_cog(view: &BlockEntityView, large: bool) -> Vec<Part> {
    let axis = rotation_axis(view);
    let speed = speed(view);
    let cog = if large {
        SHAFTLESS_LARGE_COGWHEEL
    } else {
        SHAFTLESS_COGWHEEL
    };
    let mut parts = vec![model(
        cog,
        dir::to_face_vertical(Dir::from_axis(axis, true)),
        spin(axis, speed, 1.0, rotation_offset(large, axis, view.pos)),
    )];
    let shaft_angle = if large {
        shaft_offset(axis, view.pos)
    } else {
        rotation_offset(false, axis, view.pos)
    };
    for positive in [false, true] {
        let flag = if positive {
            "top_shaft"
        } else {
            "bottom_shaft"
        };
        if view.prop(flag) == Some("true") {
            let d = Dir::from_axis(axis, positive);
            parts.push(model(
                SHAFT_HALF,
                dir::to_face(d),
                spin(axis, speed, 1.0, shaft_angle),
            ));
        }
    }
    parts
}

/// `GearboxRenderer`: a half shaft on each side off the box's axis, each
/// turning one way or the other by where the drive comes from.
pub fn gearbox(view: &BlockEntityView) -> Vec<Part> {
    let box_axis = rotation_axis(view);
    let speed = speed(view);
    let from = if speed != 0.0 {
        source_facing(view)
    } else {
        None
    };
    ALL.into_iter()
        .filter(|d| d.axis() != box_axis)
        .map(|d| {
            let modifier = match from {
                Some(s) if s.axis() == d.axis() => {
                    if s == d {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Some(s) if s.positive() == d.positive() => -1.0,
                _ => 1.0,
            };
            let offset = rotation_offset(false, d.axis(), view.pos);
            model(
                SHAFT_HALF,
                dir::to_face(d),
                spin(d.axis(), speed, modifier, offset),
            )
        })
        .collect()
}

/// `SplitShaftRenderer` (clutch, gearshift, sequenced gearshift): a half
/// shaft each way along the axis; the far side of a powered gearshift
/// turns backwards, of a powered clutch not at all.
pub fn split_shaft(view: &BlockEntityView) -> Vec<Part> {
    let axis = rotation_axis(view);
    let speed = speed(view);
    let powered = view.prop("powered") == Some("true");
    let from = source(view).and(source_facing(view));
    [false, true]
        .into_iter()
        .map(|positive| {
            let d = Dir::from_axis(axis, positive);
            let driven_side = from.is_some_and(|s| s != d);
            let modifier = match view.kind.as_str() {
                "create:gearshift" if driven_side && powered => -1.0,
                "create:clutch" if driven_side && powered => 0.0,
                // The sequenced gearshift's own modifier (from its running
                // instruction) isn't ported.
                _ => 1.0,
            };
            let offset = rotation_offset(false, axis, view.pos);
            model(
                SHAFT_HALF,
                dir::to_face(d),
                spin(axis, speed, modifier, offset),
            )
        })
        .collect()
}

/// `CreativeMotorRenderer`: a half shaft out of its face.
pub fn motor(view: &BlockEntityView) -> Vec<Part> {
    let facing = view
        .prop("facing")
        .and_then(Dir::parse)
        .unwrap_or(Dir::North);
    let axis = facing.axis();
    vec![model(
        SHAFT_HALF,
        dir::to_face(facing),
        spin(
            axis,
            speed(view),
            1.0,
            rotation_offset(false, axis, view.pos),
        ),
    )]
}

/// `KineticBlockEntityRenderer` itself (turntable, gantry shaft, powered
/// shaft, crushing wheel): the block's own model turning. A powered
/// shaft's angle adds 180 on even coordinates
/// (`PoweredShaftBlockEntity.getRotationAngleOffset`).
pub fn plain(view: &BlockEntityView) -> Vec<Part> {
    let axis = rotation_axis(view);
    let mut offset = rotation_offset(false, axis, view.pos);
    if view.kind == "create:powered_shaft" && axis.choose(view.pos) % 2 == 0 {
        offset += 180.0;
    }
    vec![own_block(view, spin(axis, speed(view), 1.0, offset))]
}

/// `PumpRenderer`: the cog, faced like the pump, turning.
pub fn pump(view: &BlockEntityView) -> Vec<Part> {
    let Some(facing) = view.prop("facing").and_then(Dir::parse) else {
        return Vec::new();
    };
    let axis = facing.axis();
    vec![model(
        "create:block/mechanical_pump/cog",
        dir::to_face(facing),
        spin(
            axis,
            speed(view),
            1.0,
            rotation_offset(false, axis, view.pos),
        ),
    )]
}

/// `HandCrankRenderer`: the block's own model turning, and the handle
/// (faced away from the crank's facing) at the crank's own angle, which
/// chases the speed: here, the speed it settles at.
pub fn hand_crank(view: &BlockEntityView) -> Vec<Part> {
    let facing = view.prop("facing").and_then(Dir::parse).unwrap_or(Dir::Up);
    let axis = facing.axis();
    let speed = speed(view);
    vec![
        own_block(
            view,
            spin(axis, speed, 1.0, rotation_offset(false, axis, view.pos)),
        ),
        model(
            "create:block/hand_crank/handle",
            dir::to_face(facing.opposite()),
            spin(axis, speed, 1.0, 0.0),
        ),
    ]
}

/// `SpeedControllerRenderer`: a shaft along the block's horizontal axis.
/// The bracket (a client-side check for a large cog above) isn't ported.
pub fn speed_controller(view: &BlockEntityView) -> Vec<Part> {
    let axis = view
        .prop("horizontal_axis")
        .and_then(Axis::parse)
        .unwrap_or(Axis::X);
    vec![Part {
        geometry: Geometry::Block {
            block: "create:shaft".into(),
            props: [("axis".to_owned(), axis_name(axis).to_owned())].into(),
        },
        transform: Transform::default(),
        animation: spin(
            axis,
            speed(view),
            1.0,
            rotation_offset(false, axis, view.pos),
        ),
        scroll: None,
    }]
}

/// `BearingRenderer`: a half shaft behind, turning, and the top plate at
/// the bearing's own angle (`Angle`), which moves with the contraption at
/// the block's angular speed while `Running`. The contraption isn't drawn.
pub fn bearing(view: &BlockEntityView) -> Vec<Part> {
    let Some(facing) = view.prop("facing").and_then(Dir::parse) else {
        return Vec::new();
    };
    let axis = facing.axis();
    let speed = speed(view);
    let shaft = model(
        SHAFT_HALF,
        dir::to_face(facing.opposite()),
        spin(axis, speed, 1.0, rotation_offset(false, axis, view.pos)),
    );
    let top = if view.kind == "create:windmill_bearing" {
        "create:block/bearing/top_wooden"
    } else {
        "create:block/bearing/top"
    };
    let mut calls = Vec::new();
    if axis != Axis::Y {
        calls.push((Axis::Y, facing.opposite().horizontal_angle()));
    }
    calls.push((Axis::X, -90.0 - facing.vertical_angle()));
    let angle = view.nbt_f64(&["Angle"]).unwrap_or(0.0) as f32;
    let running = view.nbt.get("Running").and_then(|r| r.as_i64()) == Some(1);
    // `convertToAngular`: rpm to degrees a tick.
    let per_tick = if running { speed * 0.3 } else { 0.0 };
    let plate = Part::model(top)
        .with_transform(Transform {
            rotate: dir::java_rotations(&calls),
            ..Transform::default()
        })
        .with_animation(Animation::Spin {
            axis: axis.vector(),
            pivot: [8.0; 3],
            degrees_per_tick: per_tick,
            phase: angle - per_tick * view.time as f32,
        });
    vec![shaft, plate]
}

/// `WaterWheelRenderer`: the wheel (its wood swap isn't ported: oak).
pub fn water_wheel(view: &BlockEntityView, large: bool) -> Vec<Part> {
    let speed = speed(view);
    let (wheel, facing) = if large {
        let axis = view.prop("axis").and_then(Axis::parse).unwrap_or(Axis::Y);
        let wheel = if view.prop("extension") == Some("true") {
            LARGE_WATER_WHEEL_EXTENSION
        } else {
            LARGE_WATER_WHEEL
        };
        (wheel, Dir::from_axis(axis, true))
    } else {
        let facing = view
            .prop("facing")
            .and_then(Dir::parse)
            .unwrap_or(Dir::North);
        (WATER_WHEEL, facing)
    };
    let axis = facing.axis();
    vec![model(
        wheel,
        dir::to_face_vertical(facing),
        spin(axis, speed, 1.0, rotation_offset(false, axis, view.pos)),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        kind: &str,
        block: &str,
        props: &[(&str, &str)],
        nbt: torch_mod_api::Value,
    ) -> BlockEntityView {
        BlockEntityView {
            kind: kind.into(),
            pos: [3, 64, -5],
            block: block.into(),
            props: props
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            nbt,
            time: 0,
        }
    }

    fn rate(part: &Part) -> (f32, f32) {
        match part.animation {
            Animation::Spin {
                degrees_per_tick,
                phase,
                ..
            } => (degrees_per_tick, phase),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn offsets_follow_position_parity_with_javas_remainder() {
        // x + z = -2 along Y: even.
        assert!(should_offset(Axis::Y, [3, 64, -5]));
        // -3 % 2 is -1 in Java, not 1: odd.
        assert!(!should_offset(Axis::Y, [0, 0, -3]));
        assert_eq!(rotation_offset(true, Axis::Y, [0, 0, -3]), 11.25);
    }

    #[test]
    fn a_shaft_turns_its_own_model_at_three_tenths_of_its_rpm_a_tick() {
        let v = view(
            "create:simple_kinetic",
            "create:shaft",
            &[("axis", "x")],
            serde_json::json!({"Speed": 32.0}),
        );
        let parts = simple(&v);
        assert_eq!(parts.len(), 1);
        assert!(
            matches!(parts[0].geometry, Geometry::Block { ref block, .. } if block == "create:shaft")
        );
        let (dpt, phase) = rate(&parts[0]);
        assert!((dpt - 9.6).abs() < 1e-5);
        // Along X: y + z = 59, odd.
        assert_eq!(phase, 0.0);
    }

    #[test]
    fn a_large_cog_turns_its_wheel_and_shaft_apart() {
        let v = view(
            "create:simple_kinetic",
            "create:large_cogwheel",
            &[("axis", "y")],
            serde_json::json!({"Speed": 16.0}),
        );
        let parts = simple(&v);
        assert_eq!(parts.len(), 2);
        assert_eq!(rate(&parts[0]).1, 22.5);
        assert_eq!(rate(&parts[1]).1, 22.5);
        assert_eq!(parts[0].transform.rotate, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_gearbox_turns_its_far_side_backwards() {
        // Driven from the west: the west half turns forwards, the east one
        // backwards, the north and south ones (negative / positive) by sign.
        let v = view(
            "create:gearbox",
            "create:gearbox",
            &[("axis", "y")],
            serde_json::json!({"Speed": 10.0, "Source": [2, 64, -5]}),
        );
        let parts = gearbox(&v);
        assert_eq!(parts.len(), 4);
        let by_face: Vec<f32> = parts.iter().map(|p| rate(p).0).collect();
        // Order: north, south, west, east.
        assert_eq!(by_face, [-3.0, 3.0, 3.0, -3.0]);
    }

    #[test]
    fn a_powered_clutch_stops_its_far_side() {
        let v = view(
            "create:clutch",
            "create:clutch",
            &[("axis", "x"), ("powered", "true")],
            serde_json::json!({"Speed": 10.0, "Source": [2, 64, -5]}),
        );
        let parts = split_shaft(&v);
        let rates: Vec<f32> = parts.iter().map(|p| rate(p).0).collect();
        // West (the source) turns; east doesn't.
        assert_eq!(rates, [3.0, 0.0]);
    }

    #[test]
    fn an_encased_cog_shows_only_the_shafts_it_has() {
        let v = view(
            "create:encased_cogwheel",
            "create:andesite_encased_cogwheel",
            &[
                ("axis", "z"),
                ("top_shaft", "true"),
                ("bottom_shaft", "false"),
            ],
            serde_json::json!({"Speed": 8.0}),
        );
        let parts = encased_cog(&v, false);
        assert_eq!(parts.len(), 2);
        // The half shaft faces south (the positive end).
        assert_eq!(parts[1].transform.rotate, dir::to_face(Dir::South).rotate);
    }

    #[test]
    fn a_powered_shaft_adds_half_a_turn_on_even_coordinates() {
        let mut v = view(
            "create:powered_shaft",
            "create:powered_shaft",
            &[("axis", "x")],
            serde_json::json!({"Speed": 8.0}),
        );
        v.pos = [4, 64, 0];
        // Along X: y + z = 64, even: 22.5, and x = 4 even: + 180.
        assert_eq!(rate(&plain(&v)[0]).1, 202.5);
    }
}
