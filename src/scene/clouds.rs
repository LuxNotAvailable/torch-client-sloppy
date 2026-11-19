//! The cloud layer, as `draw::Env::clouds`.

use std::sync::Arc;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::app::AppState;
use crate::app::systems::WorldCamera;
use crate::client::timeline::CLOUD_HEIGHT;
use crate::direction::Direction;
use crate::gui::GuiState;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CloudStatus {
    Off,
    Fast,
    #[default]
    Fancy,
}

impl CloudStatus {
    pub fn caption(self) -> &'static str {
        match self {
            CloudStatus::Off => "OFF",
            CloudStatus::Fast => "Fast",
            CloudStatus::Fancy => "Fancy",
        }
    }

    pub fn serialized_name(self) -> &'static str {
        match self {
            CloudStatus::Off => "false",
            CloudStatus::Fast => "fast",
            CloudStatus::Fancy => "true",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<CloudStatus> {
        match name {
            "false" => Some(CloudStatus::Off),
            "fast" => Some(CloudStatus::Fast),
            "true" => Some(CloudStatus::Fancy),
            _ => None,
        }
    }

    pub fn next(self) -> CloudStatus {
        match self {
            CloudStatus::Off => CloudStatus::Fast,
            CloudStatus::Fast => CloudStatus::Fancy,
            CloudStatus::Fancy => CloudStatus::Off,
        }
    }
}

const CELL_SIZE: f32 = 12.0;
const CELL_HEIGHT: f32 = 4.0;
const TICKS_PER_CELL: i64 = 400;
const DRIFT_PER_TICK: f64 = 0.030_000_001;
const Z_OFFSET: f64 = 3.960_000_038_146_972_7;
const EMPTY_ALPHA: u8 = 10;

const CLOUD_RANGE_CHUNKS: i32 = 32;

const CLOUD_FOG_END: f32 = (CLOUD_RANGE_CHUNKS * 16) as f32;

const FACE_SHADE: [f32; 6] = [0.7, 1.0, 0.8, 0.8, 0.9, 0.9];

const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
    [
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ],
    [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ],
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
];

const DOWN: usize = Direction::Down.index();
const UP: usize = Direction::Up.index();
const NORTH: usize = Direction::North.index();
const SOUTH: usize = Direction::South.index();
const WEST: usize = Direction::West.index();
const EAST: usize = Direction::East.index();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RelativeCameraPos {
    AboveClouds,
    InsideClouds,
    BelowClouds,
}

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            setup.run_if(resource_added::<crate::app::systems::AssetsReady>),
        )
        .add_systems(
            PostUpdate,
            update_clouds
                .in_set(crate::draw::DrawSystems)
                .before(TransformSystems::Propagate)
                .run_if(in_state(AppState::InGame))
                .run_if(resource_exists::<CloudAssets>),
        );
    }
}

struct CloudTexture {
    width: usize,
    height: usize,
    cells: Vec<u8>,
}

const NORTH_OPEN: u8 = 1 << 3;
const EAST_OPEN: u8 = 1 << 2;
const SOUTH_OPEN: u8 = 1 << 1;
const WEST_OPEN: u8 = 1 << 0;
const SOLID: u8 = 1 << 4;

#[derive(Resource)]
struct CloudAssets {
    texture: Option<CloudTexture>,
    mesh: Arc<crate::draw::MeshData>,
    built: Option<(i32, i32, RelativeCameraPos, CloudStatus)>,
}

fn setup(mut commands: Commands) {
    let path = crate::textures_dir().join("environment/clouds.png");
    let texture = match crate::platform::assets::open_image(&path) {
        Ok(img) => Some(CloudTexture::new(&img.to_rgba8())),
        Err(e) => {
            eprintln!("clouds: {} could not be read ({e})", path.display());
            None
        }
    };
    commands.insert_resource(CloudAssets {
        texture,
        mesh: crate::draw::MeshData::new(Vec::new(), Vec::new(), false),
        built: None,
    });
}

impl CloudTexture {
    fn new(image: &image::RgbaImage) -> CloudTexture {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let empty = |x: usize, y: usize| image.get_pixel(x as u32, y as u32).0[3] < EMPTY_ALPHA;
        let mut cells = vec![0u8; width * height];
        for y in 0..height {
            for x in 0..width {
                if empty(x, y) {
                    continue;
                }
                let mut flags = SOLID;
                if empty(x, (y + height - 1) % height) {
                    flags |= NORTH_OPEN;
                }
                if empty((x + 1) % width, y) {
                    flags |= EAST_OPEN;
                }
                if empty(x, (y + 1) % height) {
                    flags |= SOUTH_OPEN;
                }
                if empty((x + width - 1) % width, y) {
                    flags |= WEST_OPEN;
                }
                cells[x + y * width] = flags;
            }
        }
        CloudTexture {
            width,
            height,
            cells,
        }
    }

    fn cell(&self, x: i32, y: i32) -> u8 {
        let ix = x.rem_euclid(self.width as i32) as usize;
        let iy = y.rem_euclid(self.height as i32) as usize;
        self.cells[ix + iy * self.width]
    }
}

struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    fn face(&mut self, x: i32, z: i32, direction: usize, inside: bool, top_color: bool) {
        let base = self.positions.len() as u32;
        let shade =
            crate::util::mth::srgb_to_linear(FACE_SHADE[if top_color { UP } else { direction }]);
        for corner in FACE_VERTICES[direction] {
            self.positions.push([
                (corner[0] + x as f32) * CELL_SIZE,
                corner[1] * CELL_HEIGHT,
                (corner[2] + z as f32) * CELL_SIZE,
            ]);
            self.colors.push([shade, shade, shade, 1.0]);
        }
        if inside {
            self.indices.extend_from_slice(&[
                base + 3,
                base + 2,
                base + 1,
                base + 3,
                base + 1,
                base,
            ]);
        } else {
            self.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

fn build_mesh(
    texture: &CloudTexture,
    center_x: i32,
    center_z: i32,
    relative: RelativeCameraPos,
    status: CloudStatus,
) -> Arc<crate::draw::MeshData> {
    let radius_cells = (CLOUD_RANGE_CHUNKS * 16) as f32 / CELL_SIZE;
    let radius_cells = radius_cells.ceil() as i32;
    let extrude = status == CloudStatus::Fancy;

    let mut b = MeshBuilder {
        positions: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    for ring in 0..=2 * radius_cells {
        for rel_x in -ring..=ring {
            let rel_z = ring - rel_x.abs();
            if rel_z < 0 || rel_z > radius_cells {
                continue;
            }
            if rel_x * rel_x + rel_z * rel_z > radius_cells * radius_cells {
                continue;
            }
            if rel_z != 0 {
                try_cell(
                    &mut b, texture, center_x, center_z, relative, extrude, rel_x, -rel_z,
                );
            }
            try_cell(
                &mut b, texture, center_x, center_z, relative, extrude, rel_x, rel_z,
            );
        }
    }

    for (position, color) in b.positions.iter().zip(b.colors.iter_mut()) {
        let distance = (position[0] * position[0] + position[2] * position[2]).sqrt();
        color[3] = 1.0 - (distance / CLOUD_FOG_END).clamp(0.0, 1.0);
    }

    let verts = b
        .positions
        .iter()
        .zip(&b.colors)
        .map(|(&pos, &color)| crate::draw::EntityVertex {
            pos,
            normal: [0.0, 1.0, 0.0],
            color,
            ..Default::default()
        })
        .collect();
    crate::draw::MeshData::new(verts, b.indices, false)
}

#[allow(clippy::too_many_arguments)]
fn try_cell(
    b: &mut MeshBuilder,
    texture: &CloudTexture,
    center_x: i32,
    center_z: i32,
    relative: RelativeCameraPos,
    extrude: bool,
    x: i32,
    z: i32,
) {
    let cell = texture.cell(center_x + x, center_z + z);
    if cell == 0 {
        return;
    }
    if !extrude {
        b.face(x, z, DOWN, false, true);
        return;
    }

    if relative != RelativeCameraPos::BelowClouds {
        b.face(x, z, UP, false, false);
    }
    if relative != RelativeCameraPos::AboveClouds {
        b.face(x, z, DOWN, false, false);
    }
    if cell & NORTH_OPEN != 0 && z > 0 {
        b.face(x, z, NORTH, false, false);
    }
    if cell & SOUTH_OPEN != 0 && z < 0 {
        b.face(x, z, SOUTH, false, false);
    }
    if cell & WEST_OPEN != 0 && x > 0 {
        b.face(x, z, WEST, false, false);
    }
    if cell & EAST_OPEN != 0 && x < 0 {
        b.face(x, z, EAST, false, false);
    }
    if relative == RelativeCameraPos::InsideClouds && x.abs() <= 1 && z.abs() <= 1 {
        for direction in 0..6 {
            b.face(x, z, direction, true, false);
        }
    }
}

fn update_clouds(
    gui: Res<GuiState>,
    view: Res<crate::app::frame_view::FrameView>,
    env: Res<crate::scene::environment::Environment>,
    mut assets: ResMut<CloudAssets>,
    mut out: ResMut<crate::draw::Env>,
    camera: Query<&Transform, With<WorldCamera>>,
) {
    crate::prof_span!("render:update_clouds");
    out.clouds.clear();
    let status = gui.options.clouds;

    let partial = view.partial;
    let color = env.sky.cloud_color;

    let Some((texture_cells_w, texture_cells_h)) =
        assets.texture.as_ref().map(|t| (t.width, t.height))
    else {
        return;
    };
    // Under water the clouds sit past the water fog's end (vanilla's
    // `cloudEnd`), so they're fogged out entirely.
    let [eye_x, eye_y, eye_z] = view.tick_eye();
    let underwater = crate::util::block_model::eye_fluid_at(eye_x, eye_y, eye_z)
        == Some(crate::util::block_model::EyeFluid::Water);
    if status == CloudStatus::Off || color.alpha() == 0 || underwater || gui.in_menu() {
        return;
    }

    let Ok(camera) = camera.single() else { return };
    let (cam_x, cam_y, cam_z) = (
        camera.translation.x as f64,
        camera.translation.y as f64,
        camera.translation.z as f64,
    );

    let relative_bottom = CLOUD_HEIGHT as f64 - cam_y;
    let relative_top = relative_bottom + CELL_HEIGHT as f64;
    let relative = if relative_top < 0.0 {
        RelativeCameraPos::AboveClouds
    } else if relative_bottom > 0.0 {
        RelativeCameraPos::BelowClouds
    } else {
        RelativeCameraPos::InsideClouds
    };

    let period = texture_cells_w as i64 * TICKS_PER_CELL;
    let offset = crate::client::tracking::game_time().rem_euclid(period) as f64 + partial as f64;
    let texture_w = texture_cells_w as f64 * CELL_SIZE as f64;
    let texture_h = texture_cells_h as f64 * CELL_SIZE as f64;
    let mut cloud_x = cam_x + offset * DRIFT_PER_TICK;
    let mut cloud_z = cam_z + Z_OFFSET;
    cloud_x -= (cloud_x / texture_w).floor() * texture_w;
    cloud_z -= (cloud_z / texture_h).floor() * texture_h;
    let cell_x = (cloud_x / CELL_SIZE as f64).floor() as i32;
    let cell_z = (cloud_z / CELL_SIZE as f64).floor() as i32;
    let x_in_cell = cloud_x - cell_x as f64 * CELL_SIZE as f64;
    let z_in_cell = cloud_z - cell_z as f64 * CELL_SIZE as f64;

    let key = (cell_x, cell_z, relative, status);
    if assets.built != Some(key) {
        assets.built = Some(key);
        let Some(texture) = assets.texture.as_ref() else {
            return;
        };
        assets.mesh = build_mesh(texture, cell_x, cell_z, relative, status);
    }

    let at = Vec3::new(
        (cam_x - x_in_cell) as f32,
        CLOUD_HEIGHT,
        (cam_z - z_in_cell) as f32,
    );
    out.clouds.push(crate::draw::EnvDraw {
        alpha: crate::draw::ALPHA_BLEND,
        bias: crate::scene::sky::BIAS_CLOUDS,
        tint: color.to_color().to_linear().to_f32_array(),
        ..crate::draw::EnvDraw::new(assets.mesh.clone(), 1, Mat4::from_translation(at))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard() -> CloudTexture {
        let mut image = image::RgbaImage::new(4, 4);
        image.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        CloudTexture::new(&image)
    }

    #[test]
    fn neighbours_are_packed_per_cell() {
        let t = checkerboard();
        assert_eq!(
            t.cell(1, 1),
            SOLID | NORTH_OPEN | EAST_OPEN | SOUTH_OPEN | WEST_OPEN
        );
        assert_eq!(t.cell(0, 0), 0);
        assert_eq!(t.cell(5, 5), t.cell(1, 1));
    }

    #[test]
    fn fast_clouds_are_one_quad_per_cell() {
        let t = checkerboard();
        let fast = build_mesh(&t, 0, 0, RelativeCameraPos::BelowClouds, CloudStatus::Fast);
        let fancy = build_mesh(&t, 0, 0, RelativeCameraPos::BelowClouds, CloudStatus::Fancy);
        assert!(fast.verts.len() > 0);
        assert!(fancy.verts.len() > fast.verts.len());
        assert_eq!(fast.verts.len() % 4, 0);
    }

    #[test]
    fn the_far_side_of_the_layer_is_not_built() {
        let t = checkerboard();
        let below = build_mesh(&t, 0, 0, RelativeCameraPos::BelowClouds, CloudStatus::Fancy);
        let inside = build_mesh(
            &t,
            0,
            0,
            RelativeCameraPos::InsideClouds,
            CloudStatus::Fancy,
        );
        assert!(inside.verts.len() > below.verts.len());
    }

    #[test]
    fn the_fade_reaches_zero_at_the_rim() {
        let t = checkerboard();
        let mesh = build_mesh(&t, 0, 0, RelativeCameraPos::BelowClouds, CloudStatus::Fast);
        for (p, c) in mesh.verts.iter().map(|v| (v.pos, v.color)) {
            let d = (p[0] * p[0] + p[2] * p[2]).sqrt();
            assert!(c[3] >= 0.0 && c[3] <= 1.0);
            if d >= CLOUD_FOG_END {
                assert_eq!(c[3], 0.0);
            }
        }
    }

    #[test]
    fn the_option_cycles_and_round_trips() {
        let mut status = CloudStatus::Off;
        for expected in [CloudStatus::Fast, CloudStatus::Fancy, CloudStatus::Off] {
            status = status.next();
            assert_eq!(status, expected);
        }
        for status in [CloudStatus::Off, CloudStatus::Fast, CloudStatus::Fancy] {
            assert_eq!(
                CloudStatus::from_serialized_name(status.serialized_name()),
                Some(status)
            );
        }
    }
}
