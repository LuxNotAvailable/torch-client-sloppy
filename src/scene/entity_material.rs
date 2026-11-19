//! The entity material as plain data: texture, light and blend, read by
//! the draw-list collector (`entity_draw`).

use bevy::asset::Asset;
use bevy::prelude::*;

use crate::client::dimension::Dimension;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightMode {
    Cardinal = 0,
    Flat = 1,
    Emissive = 2,
    Text = 3,
}

const DIFFUSE_LIGHT: [Vec3; 2] = [Vec3::new(0.2, 1.0, -0.7), Vec3::new(-0.2, 1.0, 0.7)];
const NETHER_DIFFUSE_LIGHT: [Vec3; 2] = [Vec3::new(0.2, 1.0, -0.7), Vec3::new(-0.2, -1.0, 0.7)];

pub fn diffuse_lights(dim: Dimension) -> [Vec3; 2] {
    let pair = match dim {
        Dimension::Nether => NETHER_DIFFUSE_LIGHT,
        _ => DIFFUSE_LIGHT,
    };
    [pair[0].normalize(), pair[1].normalize()]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EntityParams {
    pub tint: Vec4,
    pub light: Vec4,
    pub light0: Vec4,
    pub light1: Vec4,
    /// x: 1 to take the light from the lightmap at the levels below, 0 to
    /// stay full bright; w: 1 for the enchantment glint.
    pub switches: Vec4,
    /// w: block light level × 16.
    pub light_block: Vec4,
    /// w: sky light level × 16.
    pub light_sky: Vec4,
    pub mode: u32,
}

impl Default for EntityParams {
    fn default() -> Self {
        EntityParams {
            tint: Vec4::ONE,
            light: Vec4::new(1.0, 1.0, 1.0, 0.0),
            light0: DIFFUSE_LIGHT[0].normalize().extend(0.0),
            light1: DIFFUSE_LIGHT[1].normalize().extend(1.0),
            switches: Vec4::ZERO,
            light_block: Vec4::ZERO,
            light_sky: Vec4::new(0.0, 0.0, 0.0, 240.0),
            mode: LightMode::Cardinal as u32,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lit {
    pub tint: [f32; 4],
    pub light: crate::lighting::lightmap::CellLight,
    pub dirs: [Vec3; 2],
    pub overlay_alpha: f32,
    pub overlay_white: bool,
    /// Enchantment shimmer over this layer.
    pub glint: bool,
}

impl Lit {
    pub fn full_bright() -> Lit {
        Lit {
            tint: [1.0; 4],
            light: crate::lighting::lightmap::CellLight::full_bright(),
            dirs: diffuse_lights(Dimension::Overworld),
            overlay_alpha: 1.0,
            overlay_white: false,
            glint: false,
        }
    }
}

impl EntityParams {
    pub fn new(mode: LightMode, cutoff: f32, lit: Lit) -> EntityParams {
        let mut params = EntityParams {
            light: Vec4::new(1.0, 1.0, 1.0, cutoff),
            mode: mode as u32,
            ..default()
        };
        params.set(lit);
        params
    }

    /// Holds only what stays put while an entity keeps its light levels.
    /// The light's colour changes every frame (torch flicker, the time of
    /// day), so the shader reads it from the shared lightmap texture rather
    /// than each material holding a copy that would need rewriting, and its
    /// bind group rebuilding, every frame.
    pub fn set(&mut self, lit: Lit) {
        self.tint = Vec4::from(lit.tint);
        self.light0 = lit.dirs[0].extend(if lit.overlay_white { 1.0 } else { 0.0 });
        self.light1 = lit.dirs[1].extend(lit.overlay_alpha);
        let from_lightmap = lit.light != crate::lighting::lightmap::CellLight::full_bright();
        self.switches = Vec4::new(
            if from_lightmap { 1.0 } else { 0.0 },
            0.0,
            0.0,
            if lit.glint { 1.0 } else { 0.0 },
        );
        self.light_block.w = f32::from(lit.light.levels[0]) * 16.0;
        self.light_sky.w = f32::from(lit.light.levels[1]) * 16.0;
    }
}

#[derive(Asset, TypePath, Clone, Debug)]
pub struct EntityMaterial {
    pub texture: Option<Handle<Image>>,
    pub params: EntityParams,
    pub alpha_mode: crate::draw::Alpha,
    pub depth_bias: i32,
    pub see_through: bool,
}

impl EntityMaterial {
    pub fn new(
        texture: Option<Handle<Image>>,
        mode: LightMode,
        alpha_mode: crate::draw::Alpha,
        cutoff: f32,
        lit: Lit,
        depth_bias: i32,
    ) -> EntityMaterial {
        EntityMaterial {
            texture,
            params: EntityParams::new(mode, cutoff, lit),
            alpha_mode,
            depth_bias,
            see_through: false,
        }
    }
}

/// The armor glint texture.
pub const GLINT: Handle<Image> = bevy::asset::uuid_handle!("2b7e4c19-8d5a-4f63-b0e2-91c7a6d34f85");

/// The item glint texture, for the GUI and hand glints.
pub const GLINT_ITEM: Handle<Image> =
    bevy::asset::uuid_handle!("c41f0a7e-5b2d-4e98-a3c6-7d18e9b25f04");

/// Kept as stored, without the sRGB decode: the glint blend works on the
/// encoded values, as vanilla's does on its sRGB framebuffer.
fn glint_image(name: &str) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use wgpu_types::{Extent3d, TextureDimension, TextureFormat};

    let file = crate::assets_root().join(format!("textures/misc/{name}.png"));
    let (w, h, data) = match crate::platform::assets::open_image(&file) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            (rgba.width(), rgba.height(), rgba.into_raw())
        }
        Err(e) => {
            warn!("glint texture {name} not found ({e}); that glint is off");
            (1, 1, vec![0, 0, 0, 255])
        }
    };
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

fn load_glint(mut images: ResMut<Assets<Image>>) {
    for (handle, name) in [
        (&GLINT, "enchanted_glint_armor"),
        (&GLINT_ITEM, "enchanted_glint_item"),
    ] {
        images
            .insert(handle, glint_image(name))
            .expect("a uuid handle has no generation to go stale");
    }
}

pub struct EntityMaterialPlugin;

impl Plugin for EntityMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<EntityMaterial>()
            .init_asset::<crate::draw::SimpleMaterial>()
            .init_resource::<crate::scene::rig_parts::RigParts>()
            .add_systems(
            Update,
            load_glint.run_if(resource_added::<crate::app::systems::AssetsReady>),
        );
        crate::scene::entity_draw::plugin(app);
        #[cfg(feature = "vulkan")]
        crate::vk::entity_dump::plugin(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(light: [f32; 2]) -> f32 {
        let clamped = [light[0].max(0.0), light[1].max(0.0)];
        (1.0f32).min((clamped[0] + clamped[1]) * 0.6 + 0.4)
    }

    fn dots(normal: Vec3, dirs: [Vec3; 2]) -> [f32; 2] {
        [dirs[0].dot(normal), dirs[1].dot(normal)]
    }

    #[test]
    fn the_top_of_a_mob_is_lit_and_its_underside_is_not() {
        let dirs = diffuse_lights(Dimension::Overworld);
        assert_eq!(mix(dots(Vec3::Y, dirs)), 1.0);
        assert_eq!(mix(dots(Vec3::NEG_Y, dirs)), 0.4);
    }

    #[test]
    fn the_sides_of_a_mob_match_across_each_axis() {
        let dirs = diffuse_lights(Dimension::Overworld);
        let north = mix(dots(Vec3::NEG_Z, dirs));
        let south = mix(dots(Vec3::Z, dirs));
        let east = mix(dots(Vec3::X, dirs));
        let west = mix(dots(Vec3::NEG_X, dirs));
        assert!((north - south).abs() < 1e-6, "{north} vs {south}");
        assert!((east - west).abs() < 1e-6, "{east} vs {west}");
        for side in [north, east] {
            assert!((0.4..1.0).contains(&side), "{side}");
        }
    }

    #[test]
    fn the_nether_lights_the_underside_of_a_mob() {
        let dirs = diffuse_lights(Dimension::Nether);
        let below = mix(dots(Vec3::NEG_Y, dirs));
        assert!(below > 0.4, "{below}");
        assert!(below < mix(dots(Vec3::Y, diffuse_lights(Dimension::Overworld))));
    }

    #[test]
    fn the_end_lights_like_the_overworld() {
        assert_eq!(
            diffuse_lights(Dimension::End),
            diffuse_lights(Dimension::Overworld)
        );
    }

    #[test]
    fn a_new_light_colour_at_the_same_levels_leaves_the_material_alone() {
        use crate::lighting::lightmap::{State, cell_light};
        let noon = State {
            sky_factor: 1.0,
            block_factor: 1.5,
            sky_light_color: [1.0; 3],
            block_light_tint: [1.0, 0.85, 0.3],
            ..State::default()
        };
        let flickered = State {
            block_factor: 1.3,
            sky_factor: 0.6,
            ..noon
        };
        let lit = |state: &State| Lit {
            light: cell_light(state, 9, 12),
            ..Lit::full_bright()
        };
        assert_ne!(lit(&noon).light.color, lit(&flickered).light.color);
        let params = EntityParams::new(LightMode::Cardinal, 0.1, lit(&noon));
        let mut after = params;
        after.set(lit(&flickered));
        assert_eq!(after, params);
        assert_eq!(params.light_block.w, 9.0 * 16.0);
        assert_eq!(params.light_sky.w, 12.0 * 16.0);
        assert_eq!(params.switches.x, 1.0);
    }

    #[test]
    fn full_bright_skips_the_lightmap() {
        let params = EntityParams::new(LightMode::Cardinal, 0.1, Lit::full_bright());
        assert_eq!(params.switches.x, 0.0);
    }
}
