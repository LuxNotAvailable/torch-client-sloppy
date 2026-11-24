use std::borrow::Cow;
use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::items::mesh::{ItemMeshes, Transform as ItemTransform};
use crate::items::model::{DisplayContext, DisplayTransforms};

#[derive(Clone)]
pub struct ItemGpu {
    pub mesh: Handle<Mesh>,
    pub material: Handle<crate::draw::SimpleMaterial>,
    pub texture: Handle<Image>,
    pub display: DisplayTransforms,
    pub bounds: (Vec3, Vec3),
    /// A flat sprite model (`item/generated`): not `isGui3d`.
    pub flat: bool,
}

impl ItemGpu {
    /// A named display slot, as vanilla resolves it: a missing left hand
    /// takes the right hand's, and left hands are mirrored
    /// (`ItemTransform.apply`'s left-hand fix).
    pub fn transform(&self, slot: &str) -> ItemTransform {
        let Some(ctx) = DisplayContext::parse(slot) else {
            return ItemTransform::NONE;
        };
        let mut t = self.display.for_context(ctx);
        if matches!(
            ctx,
            DisplayContext::ThirdPersonLeftHand | DisplayContext::FirstPersonLeftHand
        ) {
            t.translation[0] = -t.translation[0];
            t.rotation[1] = -t.rotation[1];
            t.rotation[2] = -t.rotation[2];
        }
        t
    }

    pub fn for_context(&self, ctx: DisplayContext) -> ItemTransform {
        self.display.for_context(ctx)
    }
}

#[derive(Resource)]
pub struct ItemAssets {
    builder: ItemMeshes,
    built: HashMap<String, Option<ItemGpu>>,
    contextual: HashMap<String, bool>,
    using_sensitive: HashMap<String, bool>,
}

impl ItemAssets {
    pub fn new() -> ItemAssets {
        ItemAssets {
            builder: ItemMeshes::new(crate::items::model::shared()),
            built: HashMap::new(),
            contextual: HashMap::new(),
            using_sensitive: HashMap::new(),
        }
    }

    pub fn bake_pattern_icon(&self, key: &str, edge: u32) -> Option<image::RgbaImage> {
        self.builder.bake_pattern_icon(key, edge)
    }

    pub fn get(
        &mut self,
        id: &str,
        ctx: &str,
        using: bool,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<crate::draw::SimpleMaterial>,
    ) -> Option<&ItemGpu> {
        if id.is_empty() {
            return None;
        }
        let contextual = match self.contextual.get(id).copied() {
            Some(flag) => flag,
            None => {
                let flag = self.builder.context_sensitive(id);
                self.contextual.insert(id.to_string(), flag);
                flag
            }
        };
        let using_sensitive = match self.using_sensitive.get(id).copied() {
            Some(flag) => flag,
            None => {
                let flag = self.builder.depends_on_using_item(id);
                self.using_sensitive.insert(id.to_string(), flag);
                flag
            }
        };
        let key = match (contextual, using_sensitive && using) {
            (false, false) => Cow::Borrowed(id),
            (true, false) => Cow::Owned(format!("{id}@{ctx}")),
            (false, true) => Cow::Owned(format!("{id}@using")),
            (true, true) => Cow::Owned(format!("{id}@{ctx}@using")),
        };
        if !self.built.contains_key(key.as_ref()) {
            let built = self.build(id, ctx, using, meshes, images, materials);
            self.built.insert(key.to_string(), built);
        }
        self.built
            .get(key.as_ref())
            .and_then(|entry| entry.as_ref())
    }

    fn build(
        &self,
        id: &str,
        ctx: &str,
        using: bool,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<crate::draw::SimpleMaterial>,
    ) -> Option<ItemGpu> {
        let built = self.builder.build(id, ctx, using)?;
        if built.vertices.is_empty() {
            return None;
        }

        let texture = images.add(crate::scene::rgba_image(
            built.image.width(),
            built.image.height(),
            built.image.into_raw(),
            RenderAssetUsages::RENDER_WORLD,
        ));

        let material = materials.add(crate::draw::SimpleMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(texture.clone()),
            alpha_mode: crate::draw::Alpha::Mask(0.1),
            ..default()
        });

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            built.vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            built.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            built.vertices.iter().map(|v| v.normal).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_COLOR,
            built
                .vertices
                .iter()
                .map(|v| {
                    [
                        crate::util::mth::srgb_to_linear(v.color[0]),
                        crate::util::mth::srgb_to_linear(v.color[1]),
                        crate::util::mth::srgb_to_linear(v.color[2]),
                        1.0,
                    ]
                })
                .collect::<Vec<_>>(),
        );
        mesh.insert_indices(Indices::U32(built.indices));

        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in &built.vertices {
            min = min.min(Vec3::from(v.pos));
            max = max.max(Vec3::from(v.pos));
        }

        Some(ItemGpu {
            mesh: meshes.add(mesh),
            material,
            texture,
            display: built.display,
            bounds: (min, max),
            flat: built.generated,
        })
    }
}
