//! `PipeAttachmentModel`: what Create adds to a pipe's baked model past its
//! blockstate, which has only the core's caps. Per face, the attachment
//! `getRenderedRimAttachment` picks (an arm, a rim, a drain), the casing at
//! a junction, and a bracket. All static; the host re-asks when a face
//! neighbour changes.
//!
//! Gaps: "has a fluid capability" knows only Create's own fluid holders;
//! a neighbour pipe's bracket (`canConnectTo`) is taken as absent.

use std::collections::BTreeMap;

use torch_mod_api::{Animation, BlockAt, BlockEntityView, Geometry, Part, Transform};

use crate::dir::{ALL, Axis, Dir};

/// `FluidTransportBehaviour.AttachmentTypes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attachment {
    None,
    Connection,
    DetailedConnection,
    Rim,
    PartialRim,
    Drain,
    PartialDrain,
}

impl Attachment {
    /// Its `ComponentPartials`, as `fluid_pipe/<name>/<face>` model folders.
    fn partials(self) -> &'static [&'static str] {
        match self {
            Attachment::None => &[],
            Attachment::Connection => &["connection"],
            Attachment::DetailedConnection => &["rim_connector"],
            Attachment::Rim => &["rim_connector", "rim"],
            Attachment::PartialRim => &["rim"],
            Attachment::Drain => &["rim_connector", "drain"],
            Attachment::PartialDrain => &["drain"],
        }
    }

    fn without_connector(self) -> Attachment {
        match self {
            Attachment::Rim => Attachment::PartialRim,
            Attachment::Drain => Attachment::PartialDrain,
            other => other,
        }
    }
}

/// Block entities registered for `FluidHandler.BLOCK` (Create's own).
const FLUID_HOLDERS: &[&str] = &[
    "create:fluid_tank",
    "create:creative_fluid_tank",
    "create:basin",
    "create:spout",
    "create:item_drain",
    "create:portable_fluid_interface",
    "create:hose_pulley",
];

fn prop<'a>(b: &'a BlockAt, name: &str) -> Option<&'a str> {
    b.props.get(name).map(String::as_str)
}

fn is(b: &BlockAt, id: &str) -> bool {
    b.block == id
}

/// `FluidPipeBlock.isOpenAt`.
fn open(b: &BlockAt, d: Dir) -> bool {
    prop(b, d.name()) == Some("true")
}

fn facing(b: &BlockAt) -> Option<Dir> {
    prop(b, "facing").and_then(Dir::parse)
}

/// `FluidValveBlock.getPipeAxis`: the first or second axis that isn't
/// the facing's.
fn valve_axis(b: &BlockAt) -> Option<Axis> {
    let facing = facing(b)?;
    let mut along_first = prop(b, "axis_along_first") != Some("true");
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        if axis != facing.axis() {
            if along_first {
                return Some(axis);
            }
            along_first = true;
        }
    }
    None
}

/// `SmartFluidPipeBlock.getPipeAxis`.
fn smart_axis(b: &BlockAt) -> Option<Axis> {
    if prop(b, "face") == Some("wall") {
        Some(Axis::Y)
    } else {
        facing(b).map(Dir::axis)
    }
}

/// `IAxisPipe.getAxisOf`.
fn axis_of(b: &BlockAt) -> Option<Axis> {
    match b.block.as_str() {
        "create:glass_fluid_pipe" => prop(b, "axis").and_then(Axis::parse),
        "create:fluid_valve" => valve_axis(b),
        "create:smart_fluid_pipe" => smart_axis(b),
        _ => None,
    }
}

/// `canHaveFlowToward` of the block's transport behaviour; `None` for a
/// block without one.
fn flow(b: &BlockAt, d: Dir) -> Option<bool> {
    Some(match b.block.as_str() {
        "create:fluid_pipe" | "create:encased_fluid_pipe" => open(b, d),
        "create:glass_fluid_pipe" => prop(b, "axis").and_then(Axis::parse) == Some(d.axis()),
        "create:fluid_valve" => valve_axis(b) == Some(d.axis()),
        "create:smart_fluid_pipe" => smart_axis(b) == Some(d.axis()),
        "create:mechanical_pump" => facing(b).map(Dir::axis) == Some(d.axis()),
        _ => return None,
    })
}

/// `FluidPropagator.getStraightPipeAxis`.
fn straight_axis(b: &BlockAt) -> Option<Axis> {
    match b.block.as_str() {
        "create:mechanical_pump" => facing(b).map(Dir::axis),
        "create:glass_fluid_pipe" => prop(b, "axis").and_then(Axis::parse),
        "create:fluid_pipe" => {
            let mut found = None;
            let mut connections = 0;
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                let (a, b2) = (
                    open(b, Dir::from_axis(axis, false)),
                    open(b, Dir::from_axis(axis, true)),
                );
                connections += usize::from(a) + usize::from(b2);
                if a && b2 {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(axis);
                }
            }
            if connections == 2 { found } else { None }
        }
        _ => None,
    }
}

/// `FluidPipeBlock.shouldDrawCasing`: more than two openings around an
/// axis.
fn draws_casing(b: &BlockAt) -> bool {
    is(b, "create:fluid_pipe")
        && [Axis::X, Axis::Y, Axis::Z].into_iter().any(|axis| {
            ALL.into_iter()
                .filter(|d| d.axis() != axis && open(b, *d))
                .count()
                > 2
        })
}

/// `FluidTransportBehaviour.getRenderedRimAttachment`.
fn base(me: &BlockAt, d: Dir, n: &BlockAt) -> Attachment {
    if flow(me, d) != Some(true) {
        return Attachment::None;
    }
    if is(n, "create:mechanical_pump") && facing(n) == Some(d.opposite()) {
        Attachment::None
    } else if is(n, "create:encased_fluid_pipe") && open(n, d.opposite()) {
        Attachment::Rim
    } else if FLUID_HOLDERS.contains(&n.block.as_str()) && !is(n, "create:hose_pulley") {
        Attachment::Drain
    } else {
        Attachment::Rim
    }
}

/// `StandardPipeFluidTransportBehaviour` (plain and encased pipes).
fn standard(me: &BlockAt, d: Dir, n: &BlockAt) -> Attachment {
    let a = base(me, d, n);
    if is(me, "create:encased_fluid_pipe") && a != Attachment::Drain {
        return Attachment::None;
    }
    if a == Attachment::Rim {
        let plain_neighbour = !is(n, "create:fluid_pipe")
            && !is(n, "create:encased_fluid_pipe")
            && !is(n, "create:glass_fluid_pipe");
        if plain_neighbour && flow(n, d.opposite()) == Some(true) {
            return Attachment::DetailedConnection;
        }
        // `shouldDrawRim`: not toward a plain pipe (bracket taken as absent).
        if is(n, "create:fluid_pipe") {
            return if straight_axis(me) == Some(d.axis()) {
                Attachment::Connection
            } else {
                Attachment::DetailedConnection
            };
        }
    }
    if a == Attachment::None && open(me, d) {
        Attachment::DetailedConnection
    } else {
        a
    }
}

/// `StraightPipeFluidTransportBehaviour` (glass pipes, valves, smart pipes).
fn straight(me: &BlockAt, d: Dir, n: &BlockAt) -> Attachment {
    let a = base(me, d, n);
    let axis = axis_of(me);
    if a == Attachment::Rim && is(me, "create:fluid_valve") {
        Attachment::None
    } else if a == Attachment::Rim
        && !is(me, "create:glass_fluid_pipe")
        && is(n, "create:glass_fluid_pipe")
    {
        Attachment::PartialRim
    } else if a == Attachment::Rim && is(n, "create:fluid_pipe") {
        Attachment::None
    } else if axis.is_some() && axis == axis_of(n) {
        Attachment::None
    } else if is(n, "create:fluid_valve") && valve_axis(n) == Some(d.axis()) {
        Attachment::None
    } else {
        a.without_connector()
    }
}

fn attachment(me: &BlockAt, d: Dir, n: &BlockAt) -> Attachment {
    match me.block.as_str() {
        "create:fluid_pipe" | "create:encased_fluid_pipe" => standard(me, d, n),
        "create:glass_fluid_pipe" | "create:fluid_valve" | "create:smart_fluid_pipe" => {
            straight(me, d, n)
        }
        // The pump's behaviour: no plain rims.
        "create:mechanical_pump" => match base(me, d, n) {
            Attachment::Rim => Attachment::None,
            a => a,
        },
        _ => Attachment::None,
    }
}

/// The attachment parts for the block entity at `view`. `look` answers the
/// block on each face (`None` where unloaded: taken as air).
pub fn attachments(view: &BlockEntityView, look: impl Fn(Dir) -> Option<BlockAt>) -> Vec<Part> {
    let me = BlockAt {
        block: view.block.clone(),
        props: view.props.clone(),
    };
    let mut parts = Vec::new();
    for d in ALL {
        let n = look(d).unwrap_or_default();
        for partial in attachment(&me, d, &n).partials() {
            parts.push(Part::model(format!(
                "create:block/fluid_pipe/{partial}/{}",
                d.name()
            )));
        }
    }
    if draws_casing(&me) {
        parts.push(Part::model("create:block/fluid_pipe/casing"));
    }
    // `BracketedBlockEntityBehaviour`: the bracket's block state.
    if let Some(bracket) = view.nbt.get("Bracket") {
        if let Some(name) = bracket.get("Name").and_then(|n| n.as_str()) {
            let props: BTreeMap<String, String> = bracket
                .get("Properties")
                .and_then(|p| p.as_object())
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default();
            parts.push(Part {
                geometry: Geometry::Block {
                    block: name.to_owned(),
                    props,
                },
                transform: Transform::default(),
                animation: Animation::None,
                scroll: None,
            });
        }
    }
    parts
}

/// The block on face `d` of `pos`, from the host.
pub fn neighbour(pos: [i32; 3], d: Dir) -> Option<BlockAt> {
    let [dx, dy, dz] = d.normal();
    torch_mod_api::host::block(pos[0] + dx, pos[1] + dy, pos[2] + dz)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, props: &[(&str, &str)]) -> BlockAt {
        BlockAt {
            block: id.to_owned(),
            props: props
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn pipe(open: &[Dir]) -> BlockAt {
        let props: Vec<(&str, &str)> = ALL
            .iter()
            .map(|d| (d.name(), if open.contains(d) { "true" } else { "false" }))
            .collect();
        block("create:fluid_pipe", &props)
    }

    fn view(b: &BlockAt) -> BlockEntityView {
        BlockEntityView {
            kind: b.block.clone(),
            block: b.block.clone(),
            props: b.props.clone(),
            ..Default::default()
        }
    }

    fn models(parts: &[Part]) -> Vec<String> {
        parts
            .iter()
            .filter_map(|p| match &p.geometry {
                Geometry::Model { model, .. } => Some(model.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_straight_run_draws_plain_arms_and_an_open_end_a_rim() {
        let me = pipe(&[Dir::North, Dir::South]);
        let next = pipe(&[Dir::North, Dir::South]);
        let parts = attachments(&view(&me), |d| (d == Dir::North).then(|| next.clone()));
        assert_eq!(
            models(&parts),
            [
                "create:block/fluid_pipe/connection/north",
                "create:block/fluid_pipe/rim_connector/south",
                "create:block/fluid_pipe/rim/south",
            ]
        );
    }

    #[test]
    fn a_bend_uses_detailed_arms_and_a_tank_gets_a_drain() {
        let me = pipe(&[Dir::Up, Dir::East]);
        let above = pipe(&[Dir::Down]);
        let tank = block("create:fluid_tank", &[]);
        let parts = attachments(&view(&me), |d| match d {
            Dir::Up => Some(above.clone()),
            Dir::East => Some(tank.clone()),
            _ => None,
        });
        assert_eq!(
            models(&parts),
            [
                "create:block/fluid_pipe/rim_connector/up",
                "create:block/fluid_pipe/rim_connector/east",
                "create:block/fluid_pipe/drain/east",
            ]
        );
    }

    #[test]
    fn a_junction_is_cased_and_a_pump_facing_in_gets_a_bare_connector() {
        let me = pipe(&[Dir::North, Dir::South, Dir::East, Dir::West]);
        let pump = block("create:mechanical_pump", &[("facing", "south")]);
        let parts = attachments(&view(&me), |d| (d == Dir::North).then(|| pump.clone()));
        let names = models(&parts);
        let north: Vec<_> = names.iter().filter(|m| m.ends_with("/north")).collect();
        assert_eq!(north, ["create:block/fluid_pipe/rim_connector/north"]);
        assert!(names.contains(&"create:block/fluid_pipe/casing".to_owned()));
    }

    #[test]
    fn a_glass_pipe_between_pipes_draws_nothing_and_a_pump_rims_nowhere() {
        let glass = block("create:glass_fluid_pipe", &[("axis", "z")]);
        let ends = pipe(&[Dir::North, Dir::South]);
        let parts = attachments(&view(&glass), |d| {
            matches!(d, Dir::North | Dir::South).then(|| ends.clone())
        });
        assert!(parts.is_empty(), "{:?}", models(&parts));
        let pump = block("create:mechanical_pump", &[("facing", "up")]);
        assert!(attachments(&view(&pump), |_| None).is_empty());
    }
}
