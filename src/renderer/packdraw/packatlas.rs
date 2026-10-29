use bevy::render::render_resource::{
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewId,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;

use super::GPU_LABEL;
use super::frame::ProcessGpu;
use super::targets::{display_view, extent};

pub(super) struct PackAtlas {
    source: TextureViewId,
    view: TextureView,
    texture: bevy::render::render_resource::Texture,
    /// The tick the animated tiles were last drawn for.
    animated_at: Option<u64>,
}

impl ProcessGpu {
    /// Brings the pack's atlas copy's animated tiles to this tick, every
    /// frame, whether or not the plan is rebuilt.
    pub(super) fn animate_pack_atlas(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        atlas: &GpuImage,
    ) {
        if let Some(pack) = &mut self.pack_atlas
            && pack.source == atlas.texture_view.id()
        {
            animate(pack, device, queue, atlas);
        }
    }

    pub(super) fn pack_atlas(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        atlas: &GpuImage,
    ) -> TextureView {
        let Some(tiles) = crate::renderer::atlas::untinted_tiles() else {
            return display_view(atlas);
        };
        if let Some(existing) = &mut self.pack_atlas
            && existing.source == atlas.texture_view.id()
        {
            animate(existing, device, queue, atlas);
            return existing.view.clone();
        }
        let size = atlas.texture.size();
        let levels = atlas.texture.mip_level_count();
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(&format!("{GPU_LABEL} atlas")),
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: atlas.texture.format(),
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[TextureFormat::Rgba8Unorm],
        });
        let mut encoder =
            device
                .wgpu_device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some(gpu_label!("atlas")),
                });
        for level in 0..levels {
            let at = |texture| wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            };
            encoder.copy_texture_to_texture(
                at(&atlas.texture),
                at(&texture),
                extent((size.width >> level).max(1), (size.height >> level).max(1)),
            );
        }
        queue.submit([encoder.finish()]);
        for (tile, tile_levels) in tiles.iter() {
            let (tx, ty) = (
                tile % crate::renderer::ATLAS_COLS,
                tile / crate::renderer::ATLAS_COLS,
            );
            for (level, bytes) in tile_levels.iter().enumerate().take(levels as usize) {
                let side = (size.width / crate::renderer::ATLAS_COLS) >> level;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: level as u32,
                        origin: wgpu::Origin3d {
                            x: tx * side,
                            y: ty * side,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(side * 4),
                        rows_per_image: Some(side),
                    },
                    extent(side, side),
                );
            }
        }
        let view = texture.create_view(&TextureViewDescriptor {
            format: Some(TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let mut pack = PackAtlas {
            source: atlas.texture_view.id(),
            view: view.clone(),
            texture,
            animated_at: None,
        };
        animate(&mut pack, device, queue, atlas);
        self.pack_atlas = Some(pack);
        view
    }
}

/// Animated block textures: the pack's programs sample the atlas directly,
/// not through the animation table the built-in terrain reads, so each
/// tick every animated tile's current frame is copied over its base tile in
/// the pack's copy of the atlas, at every mip level, as vanilla uploads
/// animation frames. The game's own atlas keeps its frames as they are.
fn animate(pack: &mut PackAtlas, device: &RenderDevice, queue: &RenderQueue, atlas: &GpuImage) {
    use crate::renderer::block_anim;
    let tick = block_anim::tick_now();
    if pack.animated_at == Some(tick) {
        return;
    }
    pack.animated_at = Some(tick);
    let Some(set) = block_anim::current() else { return };
    if set.animations().is_empty() {
        return;
    }
    let size = atlas.texture.size();
    let levels = atlas.texture.mip_level_count();
    let tile_px = size.width / crate::renderer::ATLAS_COLS;
    let mut encoder = device
        .wgpu_device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(gpu_label!("atlas animation")),
        });
    let origin = |tile: u16, side: u32| wgpu::Origin3d {
        x: (u32::from(tile) % crate::renderer::ATLAS_COLS) * side,
        y: (u32::from(tile) / crate::renderer::ATLAS_COLS) * side,
        z: 0,
    };
    for animation in set.animations() {
        let (frame, _, _) = animation.at(tick);
        for level in 0..levels {
            let side = tile_px >> level;
            if side == 0 {
                break;
            }
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas.texture,
                    mip_level: level,
                    origin: origin(frame, side),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &pack.texture,
                    mip_level: level,
                    origin: origin(animation.base, side),
                    aspect: wgpu::TextureAspect::All,
                },
                extent(side, side),
            );
        }
    }
    queue.submit([encoder.finish()]);
}
