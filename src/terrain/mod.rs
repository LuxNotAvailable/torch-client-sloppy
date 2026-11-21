//! Section meshing, shared by the client mesher and the native renderer:
//! block geometry, light, connected textures, vertex layout, cave visibility,
//! and the per-frame section stream (`stream`).

pub(crate) mod block_anim;
pub mod cave;
pub(crate) mod ctm;
pub mod hiz;
pub mod lighter;
pub(crate) mod mesh;
pub(crate) mod occupancy;
pub mod stream;
pub mod textures;
pub mod visgraph;
pub mod voxels;

pub use mesh::{
    MeshBuf, biome_blend, build_section_mesh, lighting_enabled, set_biome_blend,
    set_lighting_enabled, set_smooth_lighting,
};
pub use occupancy::Occupancy;

/// Greedy meshing (Video Settings > Greedy Meshing): the native renderer
/// merges whole faces into rectangles (`vk::packed::merge`). One of
/// `GREEDY_OFF`, `GREEDY_STANDARD` (same texture) and `GREEDY_AGGRESSIVE`
/// (any texture).
pub static GREEDY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(GREEDY_OFF);
pub const GREEDY_OFF: u8 = 0;
pub const GREEDY_STANDARD: u8 = 1;
pub const GREEDY_AGGRESSIVE: u8 = 2;

/// Triangle outlines over the terrain (Debug Options > Triangle Outlines),
/// tinted by how each quad is stored.
pub static OUTLINES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Culling frozen at the camera it was turned on at (Debug Options >
/// Freeze Culling), so freecam can look at what it keeps.
pub static FREEZE_CULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The overdraw heat map (Debug Options > Overdraw): every drawn terrain
/// fragment, hidden ones included.
pub static OVERDRAW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Torches farther than this many blocks aren't drawn (Video Settings >
/// Torch Distance); 0 for no limit.
pub static TORCH_DISTANCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Away-face culling for the cutout layer (Advanced Performance > Cutout
/// Face Culling).
pub static CUTOUT_FACES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Away-face culling for angled cutout faces: cross plants, turned signs
/// (Advanced Performance > Angled Face Culling).
pub static ANGLED_FACES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Away-face culling by plane band within a section (Advanced Performance >
/// Exact Face Culling).
pub static FACE_BANDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Per-quad away-face culling on the GPU (Advanced Performance > Per-Face
/// Culling).
pub static FACE_FILTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The terrain cull walks sections nearest the camera first (Advanced
/// Performance > Near-First).
pub static NEAR_FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The Hi-Z pyramid is rebuilt every second frame (Advanced Performance >
/// Half-Rate Occlusion).
pub static HIZ_EVERY_OTHER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(target_os = "android"));

/// Hi-Z level 1 from one depth texel per 2 x 2 (Advanced Performance >
/// Quarter-Res Occlusion).
pub static HIZ_QUARTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Section origins per row of the terrain origins texture (`terrain.wgsl`
/// and the pack preludes read it at this width).
pub const ORIGIN_ROW: u32 = 256;

/// Block atlas width in tiles.
pub const ATLAS_COLS: u32 = 16;
pub const TILE_PX: u32 = 16;

#[derive(Clone, Debug)]
pub enum BlockGeom {
    Model(std::sync::Arc<crate::blocks::BakedBlock>),
    Fluid {
        amount: u8,
        still: u32,
        flow: u32,
        lava: bool,
    },
}

pub fn fluid_surface(amount: u8) -> f32 {
    amount as f32 / 9.0
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TintKind {
    #[default]
    None,
    Grass,
    Foliage,
    DryFoliage,
    Redstone(u8),
    /// A melon or pumpkin stem by its age (0 to 7), or `ATTACHED` for an
    /// attached stem (vanilla's `BlockTintSources.stem` and its constant).
    Stem(u8),
    Water,
    /// A modded block, coloured by its raw state id so blocks tell apart.
    Modded(u32),
}

#[derive(Clone)]
pub struct RenderedBlock {
    pub geom: BlockGeom,
    pub is_solid: bool,
    pub waterlogged: bool,
    pub full_cube: bool,
    pub emission: u8,
    pub emissive: bool,
    pub cull_group: u32,
    pub tint_kind: TintKind,
    pub offset: crate::blocks::rand::ShapeOffset,
    pub state: u16,
    pub translucent: bool,
    /// Its sides' coverage, an index into `blocks::shape::shapes`; 0 for
    /// none.
    pub shape: u32,
    /// A torch (any kind): its quads go in the section's torch range.
    pub torch: bool,
}

pub struct PendingSection {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub sec_y: i32,
    pub opaque: MeshBuf,
    pub water: MeshBuf,
    pub vis: visgraph::VisibilitySet,
    /// Voxel records for traced reflections (`voxels`), when a pack wants
    /// them.
    pub voxels: Option<std::sync::Arc<voxels::SectionVoxels>>,
}
