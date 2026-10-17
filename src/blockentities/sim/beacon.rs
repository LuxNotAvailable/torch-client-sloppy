use std::sync::Arc;

use azalea::block::BlockState;
use azalea_core::heightmap_kind::HeightmapKind;
use azalea_core::position::ChunkPos;
use azalea_registry::builtin::BlockKind;
use azalea_registry::tags::blocks::BEACON_BASE_BLOCKS;

use super::TickCtx;
use crate::util::dye::{self, DYES};

const BLOCKS_CHECK_PER_TICK: usize = 10;
const MAX_LEVELS: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub color: u32,
    pub height: i32,
}

#[derive(Debug, Default)]
pub struct Scan {
    checking: Vec<Section>,
    last_check_y: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Beacon {
    sections: Arc<[Section]>,
    levels: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    Tint(u32),
    Pass,
    Block,
}

impl Beacon {
    pub fn sections(&self) -> &[Section] {
        if self.levels == 0 {
            &[]
        } else {
            &self.sections
        }
    }

    pub fn tick(&mut self, ctx: &TickCtx, scan: &mut Scan) {
        let [x, y, z] = ctx.pos;
        let min_y = ctx.world.chunks.min_y();
        let surface = world_surface(ctx, min_y);
        let mut check_y = if scan.last_check_y < y {
            scan.checking.clear();
            scan.last_check_y = y - 1;
            y
        } else {
            scan.last_check_y + 1
        };
        for _ in 0..BLOCKS_CHECK_PER_TICK {
            if check_y > surface {
                break;
            }
            let state = ctx.block([x, check_y, z]).unwrap_or(BlockState::AIR);
            if !step(&mut scan.checking, probe(state)) {
                scan.last_check_y = surface;
                break;
            }
            check_y += 1;
            scan.last_check_y += 1;
        }
        if ctx.game_time % 80 == 0 && !self.sections.is_empty() {
            self.levels = update_base(ctx, min_y);
        }
        if scan.last_check_y >= surface {
            scan.last_check_y = min_y - 1;
            if *self.sections != *scan.checking {
                self.sections = Arc::from(scan.checking.as_slice());
            }
        }
    }

    #[cfg(test)]
    pub fn lit(sections: &[Section]) -> Beacon {
        Beacon {
            sections: Arc::from(sections),
            levels: 1,
        }
    }
}

fn step(checking: &mut Vec<Section>, probe: Probe) -> bool {
    let last = checking.len().checked_sub(1);
    match (probe, last) {
        (Probe::Tint(color), _) if checking.len() <= 1 => {
            checking.push(Section { color, height: 1 })
        }
        (Probe::Tint(color), Some(i)) if checking[i].color == color => checking[i].height += 1,
        (Probe::Tint(color), Some(i)) => {
            let color = average(checking[i].color, color);
            checking.push(Section { color, height: 1 });
        }
        (Probe::Pass, Some(i)) => checking[i].height += 1,
        _ => {
            checking.clear();
            return false;
        }
    }
    true
}

fn probe(state: BlockState) -> Probe {
    probe_kind(
        BlockKind::from(state),
        crate::lighting::props::of(state).dampening,
    )
}

fn probe_kind(kind: BlockKind, dampening: u8) -> Probe {
    if let Some(color) = beam_tint(kind) {
        Probe::Tint(color)
    } else if dampening >= 15 && kind != BlockKind::Bedrock {
        Probe::Block
    } else {
        Probe::Pass
    }
}

fn beam_tint(kind: BlockKind) -> Option<u32> {
    let id = if kind == BlockKind::Beacon {
        0
    } else {
        let name = kind.to_str().strip_prefix("minecraft:")?;
        let color = name
            .strip_suffix("_stained_glass_pane")
            .or_else(|| name.strip_suffix("_stained_glass"))?;
        dye::id_by_name(color)?
    };
    Some(0xFF00_0000 | DYES[id].texture_diffuse)
}

fn average(a: u32, b: u32) -> u32 {
    let channel = |shift: u32| (((a >> shift) & 0xFF) + ((b >> shift) & 0xFF)) / 2 << shift;
    channel(24) | channel(16) | channel(8) | channel(0)
}

fn world_surface(ctx: &TickCtx, min_y: i32) -> i32 {
    let [x, _, z] = ctx.pos;
    let Some(chunk) = ctx
        .world
        .chunks
        .get(&ChunkPos::new(x.div_euclid(16), z.div_euclid(16)))
    else {
        return min_y;
    };
    let chunk = chunk.read();
    match chunk.heightmaps.get(&HeightmapKind::WorldSurface) {
        Some(map) => map.get_first_available(x.rem_euclid(16) as u8, z.rem_euclid(16) as u8),
        None => min_y + ctx.world.chunks.height() as i32,
    }
}

fn update_base(ctx: &TickCtx, min_y: i32) -> i32 {
    let [x, y, z] = ctx.pos;
    let is_base = |pos: [i32; 3]| {
        ctx.block(pos)
            .is_some_and(|state| BEACON_BASE_BLOCKS.contains(&BlockKind::from(state)))
    };
    let mut levels = 0;
    for step in 1..=MAX_LEVELS {
        let ly = y - step;
        if ly < min_y {
            break;
        }
        let ok =
            (x - step..=x + step).all(|lx| (z - step..=z + step).all(|lz| is_base([lx, ly, lz])));
        if !ok {
            break;
        }
        levels = step;
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(probes: &[Probe]) -> Vec<Section> {
        let mut checking = Vec::new();
        for probe in probes {
            if !step(&mut checking, *probe) {
                break;
            }
        }
        checking
    }

    const WHITE: u32 = 0xFF00_0000 | 16383998;
    const RED: u32 = 0xFF00_0000 | 11546150;
    const BLUE: u32 = 0xFF00_0000 | 3949738;

    #[test]
    fn the_first_glass_replaces_the_beacon_white_and_later_glass_averages() {
        let sections = scan(&[
            Probe::Tint(WHITE),
            Probe::Pass,
            Probe::Tint(RED),
            Probe::Tint(RED),
            Probe::Pass,
            Probe::Tint(BLUE),
        ]);
        assert_eq!(
            sections,
            vec![
                Section {
                    color: WHITE,
                    height: 2
                },
                Section {
                    color: RED,
                    height: 3
                },
                Section {
                    color: average(RED, BLUE),
                    height: 1
                },
            ]
        );
    }

    #[test]
    fn an_opaque_block_clears_the_beam() {
        assert!(scan(&[Probe::Tint(WHITE), Probe::Pass, Probe::Block]).is_empty());
        assert!(scan(&[Probe::Pass]).is_empty());
    }

    #[test]
    fn glass_and_the_beacon_tint_and_nothing_else_does() {
        assert_eq!(beam_tint(BlockKind::Beacon), Some(WHITE));
        assert_eq!(beam_tint(BlockKind::RedStainedGlass), Some(RED));
        assert_eq!(beam_tint(BlockKind::BlueStainedGlassPane), Some(BLUE));
        assert_eq!(beam_tint(BlockKind::Glass), None);
        assert_eq!(beam_tint(BlockKind::Stone), None);
    }

    #[test]
    fn bedrock_lets_the_beam_through() {
        assert_eq!(probe_kind(BlockKind::Bedrock, 15), Probe::Pass);
        assert_eq!(probe_kind(BlockKind::Stone, 15), Probe::Block);
        assert_eq!(probe_kind(BlockKind::Air, 0), Probe::Pass);
    }

    #[test]
    fn average_halves_each_channel() {
        assert_eq!(average(0xFF00_00FF, 0xFFFF_0000), 0xFF7F_007F);
    }

    #[test]
    fn an_inactive_beacon_has_no_sections() {
        let beacon = Beacon {
            sections: Arc::from(
                &[Section {
                    color: WHITE,
                    height: 1,
                }][..],
            ),
            ..Beacon::default()
        };
        assert!(beacon.sections().is_empty());
        assert_eq!(Beacon::lit(&beacon.sections).sections().len(), 1);
    }
}
