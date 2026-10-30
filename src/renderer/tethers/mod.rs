//! The world-space overlays rebuilt every frame, one system each: rain and
//! snow (`precipitation`), entity shadows (`shadows`), the fire on burning
//! entities (`flames`), what joins entities (`links`: leads, fishing lines,
//! guardian and crystal beams, dragon rays) and lightning (`lightning`).
//! Each kind is one mesh in world space, drawn with the entity material so
//! shader packs draw them too; a system hands its geometry to [`LayerOut`].

mod flames;
mod lightning;
mod links;
mod precipitation;
mod shadows;

use std::collections::HashMap;
use std::f32::consts::PI;

use azalea_registry::builtin::EntityKind;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::entity_material::{EntityMaterial, LightMode, Lit};
use super::frame_view::FrameView;
use crate::entities::feed::EntityAnim;
use crate::util::javarandom::JavaRandom;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Leash,
    Line,
    GuardianBeam,
    CrystalBeam,
    Lightning,
    Rays,
    /// The two fire textures of burning entities' flames.
    Fire0,
    Fire1,
    /// Round shadows under entities.
    Shadow,
    /// Falling rain and snow.
    Rain,
    Snow,
}

const KINDS: [Kind; 11] = [
    Kind::Shadow,
    Kind::Rain,
    Kind::Snow,
    Kind::Leash,
    Kind::Line,
    Kind::GuardianBeam,
    Kind::CrystalBeam,
    Kind::Lightning,
    Kind::Rays,
    Kind::Fire0,
    Kind::Fire1,
];

struct Layer {
    mesh: Handle<Mesh>,
    entity: Entity,
    /// A hash of the geometry last written, to skip rewriting the same.
    written: Option<u64>,
}

#[derive(Resource)]
struct Layers(HashMap<Kind, Layer>);

/// One frame's geometry for a kind: triangles in world space.
#[derive(Default)]
struct Geo {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Geo {
    fn vertex(&mut self, p: Vec3, uv: [f32; 2], color: [f32; 4]) -> u32 {
        self.pos.push(p.to_array());
        self.uv.push(uv);
        self.color.push(color);
        self.pos.len() as u32 - 1
    }

    fn quad(&mut self, v: [(Vec3, [f32; 2], [f32; 4]); 4]) {
        let i = v.map(|(p, uv, c)| self.vertex(p, uv, c));
        self.idx
            .extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    fn tri(&mut self, v: [(Vec3, [f32; 4]); 3]) {
        let i = v.map(|(p, c)| self.vertex(p, [0.0, 0.0], c));
        self.idx.extend_from_slice(&i);
    }

    /// A triangle strip's vertices, as triangles.
    fn strip(&mut self, v: &[(Vec3, [f32; 4])]) {
        let first = self.pos.len() as u32;
        for &(p, c) in v {
            self.vertex(p, [0.0, 0.0], c);
        }
        for i in 0..v.len().saturating_sub(2) as u32 {
            self.idx
                .extend_from_slice(&[first + i, first + i + 1, first + i + 2]);
        }
    }

    /// A hash of the geometry, by bit pattern.
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let bits = |values: &[f32], h: &mut std::collections::hash_map::DefaultHasher| {
            for v in values {
                v.to_bits().hash(h);
            }
        };
        bits(self.pos.as_flattened(), &mut h);
        bits(self.uv.as_flattened(), &mut h);
        bits(self.color.as_flattened(), &mut h);
        self.idx.hash(&mut h);
        h.finish()
    }

    fn into_mesh(self, mesh: &mut Mesh) {
        let normals = vec![[0.0, 1.0, 0.0]; self.pos.len()];
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.color);
        mesh.insert_indices(Indices::U32(self.idx));
    }
}

fn srgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    use crate::util::mth::srgb_to_linear;
    [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), 1.0]
}

/// `Vec3.yRot`: a turn about Y by `radians`.
fn y_rot(v: Vec3, radians: f32) -> Vec3 {
    let (s, c) = radians.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}

/// Whether the player `id` has a fishing bobber out, which shows their rod
/// cast (`fishing_rod/cast`).
pub fn casting(view: &FrameView, id: i32) -> bool {
    view.entities
        .iter()
        .any(|e| e.kind == EntityKind::FishingBobber && e.shared().pet.owner_id == id)
}

pub struct TethersPlugin;

impl Plugin for TethersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                setup.run_if(not(resource_exists::<Layers>)),
                (
                    precipitation::draw_precipitation,
                    shadows::draw_shadows,
                    flames::draw_flames,
                    links::draw_links,
                    lightning::draw_lightning,
                )
                    .after(super::frame_view::FrameViewSystems)
                    .run_if(resource_exists::<Layers>),
                update_item_context.after(super::frame_view::FrameViewSystems),
            )
                .run_if(resource_exists::<super::systems::AssetsReady>),
        );
    }
}

/// An entity texture sampled with wrapping, for the scrolling beams.
fn tiled(path: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let handle = crate::entities::load_entity_image(path, images)?;
    if let Some(image) = images.get_mut(&handle) {
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::nearest()
        });
    }
    Some(handle)
}

/// A layer a shader pack draws in its weather pass, keeping its vertex
/// alpha: rain, snow and shadows.
#[derive(Component)]
pub struct WeatherLayer;

/// The holder and world item models read: the local player, now.
fn update_item_context(view: Res<FrameView>) {
    let dimension = crate::renderer::dimension::current()
        .registry_id()
        .to_owned();
    let dimension = if dimension.contains(':') {
        dimension
    } else {
        format!("minecraft:{dimension}")
    };
    let local = crate::client::tracking::local_entity_id();
    crate::items::dispatch::set_context(crate::items::dispatch::WorldContext {
        in_world: view.in_world,
        game_time: view.game_time as i64,
        day_ticks: view.day_ticks,
        pos: view.player_lerp.map(f64::from),
        yaw: view.camera_yaw,
        dimension,
        spawn: crate::items::dispatch::spawn(),
        death: crate::items::dispatch::death(),
        using: crate::play::interaction::using_item(),
        left_handed: view.main_hand_left,
        casting: casting(&view, local),
    });
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
) {
    let lit = Lit::full_bright();
    let mut layers = HashMap::new();
    for kind in KINDS {
        let (texture, mode, alpha) = match kind {
            Kind::Leash | Kind::Line => (None, LightMode::Flat, AlphaMode::Opaque),
            Kind::GuardianBeam => (
                tiled("entity/guardian/guardian_beam", &mut images),
                LightMode::Emissive,
                AlphaMode::Mask(0.1),
            ),
            Kind::CrystalBeam => (
                tiled("entity/end_crystal/end_crystal_beam", &mut images),
                LightMode::Emissive,
                AlphaMode::Mask(0.1),
            ),
            Kind::Lightning | Kind::Rays => (None, LightMode::Emissive, AlphaMode::Add),
            // Flames light themselves (`withBlock(light, 15)`).
            Kind::Rain => (
                tiled("environment/rain", &mut images),
                LightMode::Text,
                AlphaMode::Blend,
            ),
            Kind::Snow => (
                tiled("environment/snow", &mut images),
                LightMode::Text,
                AlphaMode::Blend,
            ),
            Kind::Shadow => (
                crate::entities::load_entity_image("misc/shadow", &mut images),
                // Text mode keeps the vertex alpha the fade lives in.
                LightMode::Text,
                AlphaMode::Blend,
            ),
            Kind::Fire0 => (
                crate::entities::load_entity_image("block/fire_0", &mut images),
                LightMode::Emissive,
                AlphaMode::Mask(0.1),
            ),
            Kind::Fire1 => (
                crate::entities::load_entity_image("block/fire_1", &mut images),
                LightMode::Emissive,
                AlphaMode::Mask(0.1),
            ),
        };
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        Geo::default().into_mesh(&mut mesh);
        let mesh = meshes.add(mesh);
        // Shadows lie on block tops; a nudge toward the camera keeps them
        // above the ground (`entityShadow`'s layering offset).
        // and faint edges must not be cut away.
        let (bias, cutoff) = match kind {
            Kind::Shadow => (2, 0.0),
            Kind::Rain | Kind::Snow => (0, 0.0),
            _ => (0, 0.1),
        };
        let material = materials.add(EntityMaterial::new(texture, mode, alpha, cutoff, lit, bias));
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Visibility::Hidden,
                NoFrustumCulling,
                Name::new(format!("tethers_{kind:?}")),
            ))
            .id();
        if matches!(kind, Kind::Shadow | Kind::Rain | Kind::Snow) {
            // Neither casts the sun's shadow: rain sheets would darken the
            // world and streak the light shafts.
            commands.entity(entity).insert((
                WeatherLayer,
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        }
        layers.insert(
            kind,
            Layer {
                mesh,
                entity,
                written: None,
            },
        );
    }
    commands.insert_resource(Layers(layers));
}

/// The world camera, as the overlays query it.
type CameraQuery<'w, 's> = Query<
    'w,
    's,
    (&'static GlobalTransform, &'static Projection),
    With<super::systems::WorldCamera>,
>;

/// The camera's position, rotation and aspect ratio this frame.
fn camera_pose(camera: &CameraQuery) -> (Vec3, Quat, f32) {
    camera
        .single()
        .map_or((Vec3::ZERO, Quat::IDENTITY, 16.0 / 9.0), |(t, p)| {
            let aspect = match p {
                Projection::Perspective(p) => p.aspect_ratio,
                _ => 16.0 / 9.0,
            };
            (t.translation(), t.rotation(), aspect)
        })
}

/// Where an overlay system puts its frame's geometry: the layer's mesh,
/// shown only while it has triangles.
#[derive(bevy::ecs::system::SystemParam)]
struct LayerOut<'w, 's> {
    layers: ResMut<'w, Layers>,
    meshes: ResMut<'w, Assets<Mesh>>,
    visibility: Query<'w, 's, &'static mut Visibility>,
}

impl LayerOut<'_, '_> {
    fn put(&mut self, kind: Kind, geo: Geo) {
        let Some(layer) = self.layers.0.get_mut(&kind) else {
            return;
        };
        let empty = geo.idx.is_empty();
        if let Ok(mut vis) = self.visibility.get_mut(layer.entity) {
            let wanted = if empty {
                Visibility::Hidden
            } else {
                Visibility::Visible
            };
            if *vis != wanted {
                *vis = wanted;
            }
        }
        if empty {
            return;
        }
        // The same geometry as last frame (still mobs' shadows, a resting
        // lead) needn't be rewritten: every write re-uploads the mesh.
        let fingerprint = geo.fingerprint();
        if layer.written == Some(fingerprint) {
            return;
        }
        if let Some(mesh) = self.meshes.get_mut(&layer.mesh) {
            geo.into_mesh(mesh);
            layer.written = Some(fingerprint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(at: f32) -> Geo {
        let mut geo = Geo::default();
        let c = [1.0, 1.0, 1.0, 0.5];
        geo.quad([
            (Vec3::new(at, 0.0, 0.0), [0.0, 0.0], c),
            (Vec3::new(at, 0.0, 1.0), [0.0, 1.0], c),
            (Vec3::new(at + 1.0, 0.0, 1.0), [1.0, 1.0], c),
            (Vec3::new(at + 1.0, 0.0, 0.0), [1.0, 0.0], c),
        ]);
        geo
    }

    #[test]
    fn the_same_geometry_has_the_same_fingerprint_and_a_moved_one_does_not() {
        assert_eq!(quad(2.0).fingerprint(), quad(2.0).fingerprint());
        assert_ne!(quad(2.0).fingerprint(), quad(2.001).fingerprint());
        assert_ne!(quad(2.0).fingerprint(), Geo::default().fingerprint());
    }
}
