//! What joins entities, rebuilt every frame: leads (`LeashFeatureRenderer`),
//! fishing lines (`FishingHookRenderer`), the guardian's laser
//! (`GuardianRenderer.renderBeam`), end crystal beams
//! (`EnderDragonRenderer.submitCrystalBeams`), the dying dragon's rays
//! (`submitRays`) and lightning bolts (`LightningBoltRenderer`). Each kind is
//! one mesh in world space, drawn with the entity material so shader packs
//! draw them too.

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
}

const KINDS: [Kind; 6] = [
    Kind::Leash,
    Kind::Line,
    Kind::GuardianBeam,
    Kind::CrystalBeam,
    Kind::Lightning,
    Kind::Rays,
];

struct Layer {
    mesh: Handle<Mesh>,
    entity: Entity,
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
        self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
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
            self.idx.extend_from_slice(&[first + i, first + i + 1, first + i + 2]);
        }
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
        .any(|e| e.kind == EntityKind::FishingBobber && e.shared().owner_id == id)
}

pub struct TethersPlugin;

impl Plugin for TethersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                setup.run_if(not(resource_exists::<Layers>)),
                draw.after(super::frame_view::FrameViewSystems)
                    .run_if(resource_exists::<Layers>),
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
        };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        Geo::default().into_mesh(&mut mesh);
        let mesh = meshes.add(mesh);
        let material = materials.add(EntityMaterial::new(texture, mode, alpha, 0.1, lit, 0));
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
        layers.insert(kind, Layer { mesh, entity });
    }
    commands.insert_resource(Layers(layers));
}

/// Where an entity is this frame, and what the tethers ask of it.
#[derive(Clone, Copy, Debug)]
struct Body {
    pos: Vec3,
    body_yaw: f32,
    eye_height: f32,
    width: f32,
    height: f32,
    kind: Option<EntityKind>,
    /// For players: crouching, and the main arm on the left.
    crouching: bool,
    left_handed: bool,
}

impl Body {
    /// `getRopeHoldPosition`: where a lead held by this entity starts.
    fn rope_hold(&self) -> Vec3 {
        let yaw = self.body_yaw.to_radians();
        match self.kind {
            None => {
                let x = 0.22 * if self.left_handed { 1.0 } else { -1.0 };
                let z = if self.crouching { -0.2 } else { 0.07 };
                self.pos + y_rot(Vec3::new(x, self.height - 1.0, z), -yaw)
            }
            Some(EntityKind::LeashKnot) => self.pos + Vec3::new(0.0, 0.2, 0.0),
            Some(EntityKind::Villager | EntityKind::WanderingTrader) => {
                self.pos + y_rot(Vec3::new(0.0, self.height - 1.0, 0.2), -yaw)
            }
            Some(_) => self.pos + Vec3::new(0.0, self.eye_height * 0.7, 0.0),
        }
    }
}

struct World<'a> {
    view: &'a FrameView,
    by_id: HashMap<i32, &'a EntityAnim>,
    local_id: i32,
}

impl World<'_> {
    fn body(&self, id: i32) -> Option<Body> {
        let partial = self.view.partial;
        if id == self.local_id {
            let local = &self.view.local;
            return Some(Body {
                pos: Vec3::from(self.view.player_lerp),
                body_yaw: local.body_yaw,
                eye_height: self.view.eye_height,
                width: 0.6,
                height: if local.crouching { 1.5 } else { 1.8 },
                kind: None,
                crouching: local.crouching,
                left_handed: self.view.main_hand_left,
            });
        }
        if let Some(anim) = self.by_id.get(&id) {
            let st = anim.sample(partial);
            return Some(Body {
                pos: Vec3::from(anim.position(partial)),
                body_yaw: st.body_rot,
                eye_height: st.eye_height,
                width: st.bounding_box_width,
                height: st.bounding_box_height,
                kind: Some(anim.kind),
                crouching: false,
                left_handed: false,
            });
        }
        let player = self.view.players.iter().find(|p| p.id == id)?;
        let sample = player.anim.sample(partial);
        Some(Body {
            pos: Vec3::from(player.anim.position(partial)),
            body_yaw: sample.body_yaw,
            eye_height: if sample.crouching { 1.27 } else { 1.62 },
            width: 0.6,
            height: if sample.crouching { 1.5 } else { 1.8 },
            kind: None,
            crouching: sample.crouching,
            left_handed: sample.main_arm_left,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    view: Res<FrameView>,
    layers: Res<Layers>,
    camera: Query<(&GlobalTransform, &Projection), With<super::systems::WorldCamera>>,
    third_person: Res<super::input::ThirdPersonState>,
    state: Res<crate::gui::GuiState>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut visibility: Query<&mut Visibility>,
    mut guardians: Local<HashMap<i32, (i32, f32)>>,
    mut bolts: Local<HashMap<i32, f32>>,
) {
    let mut geo: HashMap<Kind, Geo> = KINDS.iter().map(|k| (*k, Geo::default())).collect();
    let now = time.elapsed_secs();
    let (cam_pos, cam_rot, aspect) = camera.single().map_or((Vec3::ZERO, Quat::IDENTITY, 16.0 / 9.0), |(t, p)| {
        let aspect = match p {
            Projection::Perspective(p) => p.aspect_ratio,
            _ => 16.0 / 9.0,
        };
        (t.translation(), t.rotation(), aspect)
    });

    if view.in_world {
        let world = World {
            view: &view,
            by_id: view.entities.iter().map(|e| (e.id, e)).collect(),
            local_id: crate::client::tracking::local_entity_id(),
        };
        let partial = view.partial;
        let first_person = !third_person.active;
        let fov = state.options.fov as f32;

        let mut seen_guardians = Vec::new();
        let mut seen_bolts = Vec::new();
        for anim in view.entities.iter() {
            let st = anim.sample(partial);
            let pos = Vec3::from(anim.position(partial));
            if let Some(holder) = st.extras.leash_holder
                && let Some(holder) = world.body(holder)
            {
                leash(geo.get_mut(&Kind::Leash).unwrap(), pos, &st, holder.rope_hold());
            }
            match anim.kind {
                EntityKind::FishingBobber if st.extras.owner_id != 0 => {
                    let hand = if st.extras.owner_id == world.local_id && first_person {
                        Some(first_person_hand(cam_pos, cam_rot, fov, aspect, view.main_hand_left))
                    } else {
                        world.body(st.extras.owner_id).map(|b| third_person_hand(&b))
                    };
                    if let Some(hand) = hand {
                        fishing_line(geo.get_mut(&Kind::Line).unwrap(), pos, hand, cam_pos);
                    }
                }
                EntityKind::Guardian | EntityKind::ElderGuardian if st.extras.attack_target != 0 => {
                    let target = st.extras.attack_target;
                    let entry = guardians.entry(anim.id).or_insert((target, now));
                    if entry.0 != target {
                        *entry = (target, now);
                    }
                    seen_guardians.push(anim.id);
                    if let Some(body) = world.body(target) {
                        // `clientSideAttackTime`, counting to the 80-tick attack.
                        let ticks = ((now - entry.1) * 20.0).min(80.0);
                        let eye = pos + Vec3::Y * st.eye_height;
                        let aim = body.pos + Vec3::Y * (body.height * 0.5);
                        guardian_beam(geo.get_mut(&Kind::GuardianBeam).unwrap(), eye, aim, ticks, ticks / 80.0);
                    }
                }
                EntityKind::EndCrystal => {
                    if let Some([x, y, z]) = st.extras.beam_target {
                        let target = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                        let delta = pos - target + Vec3::Y * crystal_bob(st.age_ticks);
                        crystal_beam(geo.get_mut(&Kind::CrystalBeam).unwrap(), target, delta, st.age_ticks);
                    }
                }
                EntityKind::EnderDragon => {
                    if st.death_time > 0.0 {
                        dragon_rays(geo.get_mut(&Kind::Rays).unwrap(), pos, st.y_rot, st.death_time / 200.0);
                    } else if let Some(crystal) = nearest_crystal(&view.entities, pos, partial) {
                        let delta = crystal - pos;
                        crystal_beam(geo.get_mut(&Kind::CrystalBeam).unwrap(), pos, delta, st.age_ticks);
                    }
                }
                EntityKind::LightningBolt => {
                    bolts.entry(anim.id).or_insert(now);
                    seen_bolts.push(anim.id);
                    lightning(geo.get_mut(&Kind::Lightning).unwrap(), pos, anim.id as i64);
                }
                _ => {}
            }
        }
        guardians.retain(|id, _| seen_guardians.contains(id));
        bolts.retain(|id, _| seen_bolts.contains(id));
    } else {
        guardians.clear();
        bolts.clear();
    }

    // `LightningBolt.tick` sets the sky flash for the first ticks of a bolt.
    let flash = bolts
        .values()
        .map(|start| (3.0 - (now - start) * 20.0).max(0.0))
        .fold(0.0f32, f32::max);
    super::environment::set_sky_flash(flash);

    for (kind, layer) in &layers.0 {
        let built = geo.remove(kind).unwrap_or_default();
        let empty = built.idx.is_empty();
        if let Ok(mut vis) = visibility.get_mut(layer.entity) {
            let wanted = if empty { Visibility::Hidden } else { Visibility::Visible };
            if *vis != wanted {
                *vis = wanted;
            }
        }
        if !empty && let Some(mesh) = meshes.get_mut(&layer.mesh) {
            built.into_mesh(mesh);
        }
    }
}

/// A lead from `entity` (at `pos`) to `end`: two crossed ribbons of 24 steps
/// in alternating browns, sagging (`slack`).
fn leash(geo: &mut Geo, pos: Vec3, st: &crate::entities::state::EntityState, end: Vec3) {
    // `getLeashOffset`, turned with the body.
    let offset = y_rot(
        Vec3::new(0.0, st.eye_height, st.bounding_box_width * 0.4),
        -st.body_rot.to_radians(),
    );
    let start = pos + offset;
    let d = end - start;
    let horizontal = (d.x * d.x + d.z * d.z).sqrt().max(1e-4);
    let factor = 0.05 / 2.0 / horizontal;
    let (x_off, z_off) = (d.z * factor, d.x * factor);
    for backwards in [false, true] {
        let fudge = if backwards { 0.0 } else { 0.05 };
        let steps: Vec<u32> = if backwards { (0..=24).rev().collect() } else { (0..=24).collect() };
        let mut strip = Vec::with_capacity(50);
        for k in steps {
            let t = k as f32 / 24.0;
            let shade = if k % 2 == backwards as u32 { 0.7 } else { 1.0 };
            let color = srgb(0.5 * shade, 0.4 * shade, 0.3 * shade);
            let y = if d.y > 0.0 { d.y * t * t } else { d.y - d.y * (1.0 - t) * (1.0 - t) };
            let at = start + Vec3::new(d.x * t, y, d.z * t);
            strip.push((at + Vec3::new(-x_off, fudge, z_off), color));
            strip.push((at + Vec3::new(x_off, 0.05 - fudge, -z_off), color));
        }
        geo.strip(&strip);
    }
}

/// `getPlayerHandPos` in first person: a point on the near plane, scaled
/// out by `960 / fov`.
fn first_person_hand(cam: Vec3, rot: Quat, fov: f32, aspect: f32, left_handed: bool) -> Vec3 {
    const NEAR: f32 = 0.05;
    let invert = if left_handed { -1.0 } else { 1.0 };
    let height = (fov.to_radians() / 2.0).tan() * NEAR;
    let width = height * aspect;
    let local = Vec3::new(invert * 0.525 * width, -0.1 * height, -NEAR);
    cam + rot * local * (960.0 / fov)
}

/// `getPlayerHandPos` from outside: ahead of the body and to the rod side.
fn third_person_hand(body: &Body) -> Vec3 {
    let invert = if body.left_handed { -1.0 } else { 1.0 };
    let (sin, cos) = body.body_yaw.to_radians().sin_cos();
    let right = invert * 0.35;
    let forward = 0.8;
    let y = if body.crouching { -0.1875 } else { 0.0 };
    body.pos
        + Vec3::new(
            -cos * right - sin * forward,
            body.eye_height + y - 0.45,
            -sin * right + cos * forward,
        )
}

/// The line from a bobber at `hook` to `hand`: 16 segments along vanilla's
/// curve, a few pixels wide, facing the camera.
fn fishing_line(geo: &mut Geo, hook: Vec3, hand: Vec3, cam: Vec3) {
    let reach = hand - (hook + Vec3::new(0.0, 0.25, 0.0));
    let at = |a: f32| hook + Vec3::new(reach.x * a, reach.y * (a * a + a) * 0.5 + 0.25, reach.z * a);
    let black = [0.0, 0.0, 0.0, 1.0];
    for i in 0..16 {
        let (p0, p1) = (at(i as f32 / 16.0), at((i + 1) as f32 / 16.0));
        // About two pixels at 1080 lines and the default FOV.
        let width = |p: Vec3| (p - cam).length() * 0.0013;
        let side = (p1 - p0).cross(cam - p0).normalize_or_zero();
        geo.quad([
            (p0 - side * width(p0), [0.0, 0.0], black),
            (p1 - side * width(p1), [0.0, 0.0], black),
            (p1 + side * width(p1), [0.0, 0.0], black),
            (p0 + side * width(p0), [0.0, 0.0], black),
        ]);
    }
}

/// `renderBeam`: the laser from a guardian's eye to its target, two crossed
/// planes turning as the attack charges, a cap at the end.
fn guardian_beam(geo: &mut Geo, eye: Vec3, target: Vec3, time: f32, scale: f32) {
    let v = target - eye;
    let length = v.length() + 1.0;
    let n = v.normalize_or_zero();
    let x_rot = n.y.clamp(-1.0, 1.0).acos();
    let y_rot = PI / 2.0 - n.z.atan2(n.x);
    let rotation = Quat::from_rotation_y(y_rot) * Quat::from_rotation_x(x_rot);
    let rot = time * 0.05 * -1.5;
    let c = scale * scale;
    let color = srgb(
        (64.0 + c * 191.0).floor() / 255.0,
        (32.0 + c * 191.0).floor() / 255.0,
        (128.0 - c * 64.0).floor() / 255.0,
    );
    let ring = |angle: f32, r: f32| (angle.cos() * r, angle.sin() * r);
    let (wx, wz) = ring(rot + PI, 0.2);
    let (ex, ez) = ring(rot, 0.2);
    let (nx, nz) = ring(rot + PI / 2.0, 0.2);
    let (sx, sz) = ring(rot + PI * 3.0 / 2.0, 0.2);
    let (wnx, wnz) = ring(rot + PI * 3.0 / 4.0, 0.282);
    let (enx, enz) = ring(rot + PI / 4.0, 0.282);
    let (wsx, wsz) = ring(rot + PI * 5.0 / 4.0, 0.282);
    let (esx, esz) = ring(rot + PI * 7.0 / 4.0, 0.282);
    let min_v = -1.0 + (time * 0.5) % 1.0;
    let max_v = min_v + length * 2.5;
    let p = |x: f32, y: f32, z: f32| eye + rotation * Vec3::new(x, y, z);
    geo.quad([
        (p(wx, length, wz), [0.4999, max_v], color),
        (p(wx, 0.0, wz), [0.4999, min_v], color),
        (p(ex, 0.0, ez), [0.0, min_v], color),
        (p(ex, length, ez), [0.0, max_v], color),
    ]);
    geo.quad([
        (p(nx, length, nz), [0.4999, max_v], color),
        (p(nx, 0.0, nz), [0.4999, min_v], color),
        (p(sx, 0.0, sz), [0.0, min_v], color),
        (p(sx, length, sz), [0.0, max_v], color),
    ]);
    let v_base = if time.floor() as i32 % 2 == 0 { 0.5 } else { 0.0 };
    geo.quad([
        (p(wnx, length, wnz), [0.5, v_base + 0.5], color),
        (p(enx, length, enz), [1.0, v_base + 0.5], color),
        (p(esx, length, esz), [1.0, v_base], color),
        (p(wsx, length, wsz), [0.5, v_base], color),
    ]);
}

/// `EndCrystalRenderer.getY`: the crystal's bob.
fn crystal_bob(time: f32) -> f32 {
    let mut h = (time * 0.2).sin() / 2.0 + 0.5;
    h = (h * h + h) * 0.4;
    h - 1.4
}

/// `checkCrystals`: the crystal nearest a dragon, within 32 blocks of its
/// box, at its bob.
fn nearest_crystal(entities: &[EntityAnim], dragon: Vec3, partial: f32) -> Option<Vec3> {
    entities
        .iter()
        .filter(|e| e.kind == EntityKind::EndCrystal)
        .map(|e| {
            let st = e.sample(partial);
            Vec3::from(e.position(partial)) + Vec3::Y * crystal_bob(st.age_ticks)
        })
        .filter(|p| (*p - dragon).length() < 40.0)
        .min_by(|a, b| (*a - dragon).length().total_cmp(&(*b - dragon).length()))
}

/// `submitCrystalBeams`: an eight-sided cone from `origin` (lifted two
/// blocks) along `delta`, dark at the start, scrolling with `time`.
fn crystal_beam(geo: &mut Geo, origin: Vec3, delta: Vec3, time: f32) {
    let origin = origin + Vec3::Y * 2.0;
    let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
    let length = delta.length();
    let rotation = Quat::from_rotation_y(-delta.z.atan2(delta.x) - PI / 2.0)
        * Quat::from_rotation_x(-horizontal.atan2(delta.y) - PI / 2.0);
    let (v0, v1) = (-time * 0.01, length / 32.0 - time * 0.01);
    let (black, white) = ([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    let p = |x: f32, y: f32, z: f32| origin + rotation * Vec3::new(x, y, z);
    let (mut last_sin, mut last_cos, mut last_u) = (0.0f32, 0.75f32, 0.0f32);
    for i in 1..=8 {
        let a = i as f32 * PI * 2.0 / 8.0;
        let (sin, cos) = (a.sin() * 0.75, a.cos() * 0.75);
        let u = i as f32 / 8.0;
        geo.quad([
            (p(last_sin * 0.2, last_cos * 0.2, 0.0), [last_u, v0], black),
            (p(last_sin, last_cos, length), [last_u, v1], white),
            (p(sin, cos, length), [u, v1], white),
            (p(sin * 0.2, cos * 0.2, 0.0), [u, v0], black),
        ]);
        (last_sin, last_cos, last_u) = (sin, cos, u);
    }
}

/// `submitRays`: the dying dragon's light, more and longer rays as the death
/// runs on (`death` from 0 to 1), from inside its body.
fn dragon_rays(geo: &mut Geo, pos: Vec3, y_rot: f32, death: f32) {
    const HALF_SQRT_3: f32 = 0.866_025_4;
    // The dragon's pose up to the rays: turn, step forward, mirror, drop.
    let base = Mat4::from_translation(pos)
        * Mat4::from_rotation_y((-y_rot).to_radians())
        * Mat4::from_translation(Vec3::new(0.0, 0.0, 1.0))
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.501 - 1.0, -2.0));
    let overdrive = if death > 0.8 { ((death - 0.8) / 0.2).min(1.0) } else { 0.0 };
    // Additive: the inner colour carries its alpha, the outer is clear.
    let inner = [1.0 - overdrive; 3];
    let inner = [inner[0], inner[1], inner[2], 1.0];
    let outer = [0.0, 0.0, 0.0, 0.0];
    let mut random = JavaRandom::new(432);
    let count = ((death + death * death) / 2.0 * 60.0).floor() as i32;
    let mut turn = Quat::IDENTITY;
    let xyz = |x: f32, y: f32, z: f32| Quat::from_rotation_x(x) * Quat::from_rotation_y(y) * Quat::from_rotation_z(z);
    for _ in 0..count {
        let mut f = || random.next_f32() * PI * 2.0;
        let (a, b, c, d, e) = (f(), f(), f(), f(), f());
        let g = f() + death * PI / 2.0;
        // The pose turns again each ray, never turning back.
        turn = turn * xyz(a, b, c) * xyz(d, e, g);
        let length = random.next_f32() * 20.0 + 5.0 + overdrive * 10.0;
        let width = random.next_f32() * 2.0 + 1.0 + overdrive * 2.0;
        let m = base * Mat4::from_quat(turn);
        let at = |v: Vec3| m.transform_point3(v);
        let origin = at(Vec3::ZERO);
        let left = at(Vec3::new(-HALF_SQRT_3 * width, length, -0.5 * width));
        let right = at(Vec3::new(HALF_SQRT_3 * width, length, -0.5 * width));
        let bottom = at(Vec3::new(0.0, length, width));
        geo.tri([(origin, inner), (left, outer), (right, outer)]);
        geo.tri([(origin, inner), (right, outer), (bottom, outer)]);
        geo.tri([(origin, inner), (bottom, outer), (left, outer)]);
    }
}

/// `LightningBoltRenderer`: a jagged trunk of eight 16-block segments and two
/// side branches, four nested layers wide, from a seed.
fn lightning(geo: &mut Geo, pos: Vec3, seed: i64) {
    // Additive: the bolt's colour times its alpha of 0.3.
    let color = [0.45 * 0.3, 0.45 * 0.3, 0.5 * 0.3, 1.0];
    let mut x_offsets = [0.0f32; 8];
    let mut z_offsets = [0.0f32; 8];
    let (mut x, mut z) = (0.0f32, 0.0f32);
    let mut random = JavaRandom::new(seed);
    for i in (0..8).rev() {
        x_offsets[i] = x;
        z_offsets[i] = z;
        x += random.next_int(11) as f32 - 5.0;
        z += random.next_int(11) as f32 - 5.0;
    }
    let (final_x, final_z) = (x, z);
    for layer in 0..4 {
        let mut random = JavaRandom::new(seed);
        for branch in 0..3usize {
            let trunk = branch == 0;
            let start = 7 - branch;
            let end = if trunk { 0 } else { start - 2 };
            let mut sx = x_offsets[start] - final_x;
            let mut sz = z_offsets[start] - final_z;
            for segment in (end..=start).rev() {
                let (ex, ez) = (sx, sz);
                if trunk {
                    sx += random.next_int(11) as f32 - 5.0;
                    sz += random.next_int(11) as f32 - 5.0;
                } else {
                    sx += random.next_int(31) as f32 - 15.0;
                    sz += random.next_int(31) as f32 - 15.0;
                }
                let mut top = 0.1 + layer as f32 * 0.2;
                let mut bottom = top;
                if trunk {
                    top *= segment as f32 * 0.1 + 1.0;
                    bottom *= (segment as f32 - 1.0) * 0.1 + 1.0;
                }
                for (rx, rz, lx, lz) in [
                    (false, false, true, false),
                    (true, false, true, true),
                    (true, true, false, true),
                    (false, true, false, false),
                ] {
                    let s = |positive: bool, r: f32| if positive { r } else { -r };
                    let y0 = segment as f32 * 16.0;
                    let y1 = (segment + 1) as f32 * 16.0;
                    geo.quad([
                        (pos + Vec3::new(sx + s(rx, bottom), y0, sz + s(rz, bottom)), [0.0, 0.0], color),
                        (pos + Vec3::new(ex + s(rx, top), y1, ez + s(rz, top)), [0.0, 0.0], color),
                        (pos + Vec3::new(ex + s(lx, top), y1, ez + s(lz, top)), [0.0, 0.0], color),
                        (pos + Vec3::new(sx + s(lx, bottom), y0, sz + s(lz, bottom)), [0.0, 0.0], color),
                    ]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lead_sags_between_its_ends() {
        let mut geo = Geo::default();
        let st = crate::entities::state::EntityState::new(0, EntityKind::Cow);
        leash(&mut geo, Vec3::ZERO, &st, Vec3::new(4.0, 0.0, 0.0));
        // Two ribbons of 25 pairs, as strips.
        assert_eq!(geo.pos.len(), 100);
        assert_eq!(geo.idx.len(), 2 * 48 * 3);
    }

    #[test]
    fn a_guardian_beam_reaches_past_its_target() {
        let mut geo = Geo::default();
        guardian_beam(&mut geo, Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0), 10.0, 0.5);
        let far = geo.pos.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        // The beam runs a block past the target (`length + 1`).
        assert!((far - 6.0).abs() < 0.3, "{far}");
    }

    #[test]
    fn the_same_seed_draws_the_same_bolt() {
        let (mut a, mut b) = (Geo::default(), Geo::default());
        lightning(&mut a, Vec3::ZERO, 7);
        lightning(&mut b, Vec3::ZERO, 7);
        assert_eq!(a.pos, b.pos);
        assert!(!a.pos.is_empty());
    }

    #[test]
    fn rays_grow_with_the_death() {
        let (mut early, mut late) = (Geo::default(), Geo::default());
        dragon_rays(&mut early, Vec3::ZERO, 0.0, 0.2);
        dragon_rays(&mut late, Vec3::ZERO, 0.0, 0.9);
        assert!(late.idx.len() > early.idx.len());
    }
}
