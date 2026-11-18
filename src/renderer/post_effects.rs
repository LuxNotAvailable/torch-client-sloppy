//! Server-requested post effects (`ClientboundPostEffects`): vanilla's
//! `creeper`, `spider`, `invert` and `blur` chains, folded into one
//! fullscreen pass over the world camera. The flags live in
//! `client::post_effects`.

use bevy::asset::embedded_asset;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_graph::{
    InternedRenderLabel, InternedRenderSubGraph, RenderLabel, RenderSubGraph,
};
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

use super::systems::WorldCamera;

#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
pub struct PostEffects {
    flags: u32,
    // Scalar pads: a `[u32; 3]` takes array stride in a uniform and no
    // longer matches the shader's 16 bytes.
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

impl FullscreenMaterial for PostEffects {
    fn fragment_shader() -> ShaderRef {
        super::embedded_shader("renderer/post_effects.wgsl")
    }

    fn node_edges() -> Vec<InternedRenderLabel> {
        vec![
            Node3d::Tonemapping.intern(),
            Self::node_label().intern(),
            Node3d::EndMainPassPostProcessing.intern(),
        ]
    }

    fn sub_graph() -> Option<InternedRenderSubGraph> {
        Some(Core3d.intern())
    }
}

pub struct PostEffectsPlugin;

impl Plugin for PostEffectsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "post_effects.wgsl");
        app.add_plugins(FullscreenMaterialPlugin::<PostEffects>::default())
            .add_systems(Update, sync);
    }
}

fn sync(mut commands: Commands, cameras: Query<(Entity, Option<&PostEffects>), With<WorldCamera>>) {
    let flags = crate::client::post_effects::active();
    for (camera, current) in &cameras {
        match (flags, current) {
            (0, Some(_)) => {
                commands.entity(camera).remove::<PostEffects>();
            }
            (0, None) => {}
            (flags, current) if current.map(|c| c.flags) != Some(flags) => {
                commands
                    .entity(camera)
                    .insert(PostEffects { flags, ..default() });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "shader_support")]
    #[test]
    fn the_shader_validates() {
        let module =
            naga::front::wgsl::parse_str(include_str!("post_effects.wgsl")).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }
}
