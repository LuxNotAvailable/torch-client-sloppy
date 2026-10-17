use azalea_registry::builtin::BlockEntityKind;
use simdnbt::owned::{Nbt, NbtCompound};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    None,
    Box,
    BoxAndInvisibleBlocks,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Turn {
    #[default]
    None,
    Cw90,
    Cw180,
    Ccw90,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Flip {
    #[default]
    None,
    LeftRight,
    FrontBack,
}

const STRUCTURE_OFFSET: [i32; 3] = [0, 1, 1];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub mode: Mode,
    pub local: [i32; 3],
    pub size: [i32; 3],
    pub test_instance: bool,
    pub beam: Option<u32>,
    pub errors: Vec<[i32; 3]>,
}

pub fn decode(kind: BlockEntityKind, tag: &Nbt) -> Data {
    match kind {
        BlockEntityKind::StructureBlock => structure(tag),
        BlockEntityKind::TestInstanceBlock => test_instance(tag),
        _ => Data::default(),
    }
}

pub fn beam_color(data: &Data) -> Option<u32> {
    data.beam
}

fn vec3(tag: &NbtCompound, key: &str, default: [i32; 3], range: (i32, i32)) -> [i32; 3] {
    let mut out = [0; 3];
    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
        out[i] = tag
            .int(&format!("{key}{axis}"))
            .unwrap_or(default[i])
            .clamp(range.0, range.1);
    }
    out
}

fn structure(tag: &Nbt) -> Data {
    let tag: &NbtCompound = tag;
    let mode = tag.string("mode").map(|s| s.to_string());
    let show_air = tag.byte("showair").unwrap_or(0) != 0;
    let show_box = tag.byte("showboundingbox").unwrap_or(1) != 0;
    let mode = match mode.as_deref() {
        Some("SAVE") if show_air => Mode::BoxAndInvisibleBlocks,
        Some("SAVE") => Mode::Box,
        Some("LOAD") if show_box => Mode::Box,
        _ => Mode::None,
    };
    let pos = vec3(tag, "pos", [0, 1, 0], (-48, 48));
    let size = vec3(tag, "size", [0; 3], (0, 48));
    let turn = match tag.string("rotation").map(|s| s.to_string()).as_deref() {
        Some("CLOCKWISE_90") => Turn::Cw90,
        Some("CLOCKWISE_180") => Turn::Cw180,
        Some("COUNTERCLOCKWISE_90") => Turn::Ccw90,
        _ => Turn::None,
    };
    let flip = match tag.string("mirror").map(|s| s.to_string()).as_deref() {
        Some("LEFT_RIGHT") => Flip::LeftRight,
        Some("FRONT_BACK") => Flip::FrontBack,
        _ => Flip::None,
    };
    let (local, size) = structure_box(pos, size, turn, flip);
    Data {
        mode,
        local,
        size,
        ..Data::default()
    }
}

fn structure_box(pos: [i32; 3], size: [i32; 3], turn: Turn, flip: Flip) -> ([i32; 3], [i32; 3]) {
    let [xo, y0, zo] = pos;
    let y1 = y0 + size[1];
    let (xd, zd) = match flip {
        Flip::LeftRight => (size[0], -size[2]),
        Flip::FrontBack => (-size[0], size[2]),
        Flip::None => (size[0], size[2]),
    };
    let (x0, z0, x1, z1) = match turn {
        Turn::Cw90 => {
            let x0 = if zd < 0 { xo } else { xo + 1 };
            let z0 = if xd < 0 { zo + 1 } else { zo };
            (x0, z0, x0 - zd, z0 + xd)
        }
        Turn::Cw180 => {
            let x0 = if xd < 0 { xo } else { xo + 1 };
            let z0 = if zd < 0 { zo } else { zo + 1 };
            (x0, z0, x0 - xd, z0 - zd)
        }
        Turn::Ccw90 => {
            let x0 = if zd < 0 { xo + 1 } else { xo };
            let z0 = if xd < 0 { zo } else { zo + 1 };
            (x0, z0, x0 + zd, z0 - xd)
        }
        Turn::None => {
            let x0 = if xd < 0 { xo + 1 } else { xo };
            let z0 = if zd < 0 { zo + 1 } else { zo };
            (x0, z0, x0 + xd, z0 + zd)
        }
    };
    (
        [x0.min(x1), y0.min(y1), z0.min(z1)],
        [(x1 - x0).abs(), (y1 - y0).abs(), (z1 - z0).abs()],
    )
}

fn test_instance(tag: &Nbt) -> Data {
    let mut out = Data {
        test_instance: true,
        ..Data::default()
    };
    if let Some(data) = tag.compound("data") {
        let size = data
            .int_array("size")
            .filter(|a| a.len() == 3)
            .map_or([0; 3], |a| [a[0], a[1], a[2]]);
        let turn = match data.string("rotation").map(|s| s.to_string()).as_deref() {
            Some("clockwise_90") => Turn::Cw90,
            Some("180") => Turn::Cw180,
            Some("counterclockwise_90") => Turn::Ccw90,
            _ => Turn::None,
        };
        let size = if matches!(turn, Turn::Cw90 | Turn::Ccw90) {
            [size[2], size[1], size[0]]
        } else {
            size
        };
        out.mode = Mode::Box;
        out.local = STRUCTURE_OFFSET;
        out.size = size;
        let failed = data.get("error_message").is_some();
        out.beam = match data.string("status").map(|s| s.to_string()).as_deref() {
            Some("running") => Some(0xFF80_8080),
            Some("finished") if failed => Some(0xFFFF_0000),
            Some("finished") => Some(0xFF00_FF00),
            _ => None,
        };
    }
    if let Some(list) = tag.list("errors").and_then(|l| l.compounds()) {
        out.errors = list
            .iter()
            .filter_map(|c| c.int_array("pos"))
            .filter(|a| a.len() == 3)
            .map(|a| [a[0], a[1], a[2]])
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrotated_box_starts_at_the_offset() {
        let (local, size) = structure_box([2, 3, 4], [5, 6, 7], Turn::None, Flip::None);
        assert_eq!((local, size), ([2, 3, 4], [5, 6, 7]));
    }

    #[test]
    fn clockwise_90_swaps_the_horizontal_axes() {
        let (local, size) = structure_box([0, 0, 0], [3, 2, 5], Turn::Cw90, Flip::None);
        assert_eq!(size, [5, 2, 3]);
        assert_eq!(local, [-4, 0, 0]);
    }

    #[test]
    fn mirror_extends_the_box_backwards() {
        let (local, size) = structure_box([0, 0, 0], [3, 2, 5], Turn::None, Flip::FrontBack);
        assert_eq!((local, size), ([-2, 0, 0], [3, 2, 5]));
    }

    #[test]
    fn default_data_draws_nothing() {
        assert_eq!(Data::default().mode, Mode::None);
        assert_eq!(beam_color(&Data::default()), None);
    }
}
