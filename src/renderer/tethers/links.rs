//! What joins entities: leads (`LeashFeatureRenderer`), fishing lines
//! (`FishingHookRenderer`), the guardian's laser (`GuardianRenderer.renderBeam`),
//! end crystal beams (`EnderDragonRenderer.submitCrystalBeams`) and the dying
//! dragon's rays (`submitRays`).

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_links(
    view: Res<FrameView>,
    camera: CameraQuery,
    third_person: Res<crate::renderer::input::ThirdPersonState>,
    state: Res<crate::gui::GuiState>,
    time: Res<Time>,
    sampled: Res<crate::renderer::frame_view::SampledEntities>,
    mut out: LayerOut,
    mut guardians: Local<HashMap<i32, (i32, f32)>>,
) {
    let mut leads = Geo::default();
    let mut lines = Geo::default();
    let mut lasers = Geo::default();
    let mut crystals = Geo::default();
    let mut rays = Geo::default();
    if view.in_world {
        let now = time.elapsed_secs();
        let (cam_pos, cam_rot, aspect) = camera_pose(&camera);
        let world = World {
            view: &view,
            sampled: &sampled,
            by_id: view.entities.iter().map(|e| (e.id, e)).collect(),
            local_id: crate::client::tracking::local_entity_id(),
        };
        let partial = view.partial;
        let first_person = !third_person.active;
        let fov = state.options.fov as f32;
        let mut seen_guardians = Vec::new();
        for (anim, st) in view.entities.iter().zip(&sampled.states) {
            let pos = Vec3::from(anim.position(partial));
            if let Some(holder) = st.extras.pet.leash_holder
                && let Some(holder) = world.body(holder)
            {
                leash(&mut leads, pos, st, holder.rope_hold());
            }
            match anim.kind {
                EntityKind::FishingBobber if st.extras.pet.owner_id != 0 => {
                    let hand = if st.extras.pet.owner_id == world.local_id && first_person {
                        Some(first_person_hand(
                            cam_pos,
                            cam_rot,
                            fov,
                            aspect,
                            view.main_hand_left,
                        ))
                    } else {
                        world
                            .body(st.extras.pet.owner_id)
                            .map(|b| third_person_hand(&b))
                    };
                    if let Some(hand) = hand {
                        fishing_line(&mut lines, pos, hand, cam_pos);
                    }
                }
                EntityKind::Guardian | EntityKind::ElderGuardian
                    if st.extras.attack_target != 0 =>
                {
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
                        guardian_beam(&mut lasers, eye, aim, ticks, ticks / 80.0);
                    }
                }
                EntityKind::EndCrystal => {
                    if let Some([x, y, z]) = st.extras.object.beam_target {
                        let target = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                        let delta = pos - target + Vec3::Y * crystal_bob(st.age_ticks);
                        crystal_beam(&mut crystals, target, delta, st.age_ticks);
                    }
                }
                EntityKind::EnderDragon => {
                    if st.death_time > 0.0 {
                        dragon_rays(&mut rays, pos, st.y_rot, st.death_time / 200.0);
                    } else if let Some(crystal) = nearest_crystal(&view.entities, pos, partial) {
                        crystal_beam(&mut crystals, pos, crystal - pos, st.age_ticks);
                    }
                }
                _ => {}
            }
        }
        guardians.retain(|id, _| seen_guardians.contains(id));
    } else {
        guardians.clear();
    }
    out.put(Kind::Leash, leads);
    out.put(Kind::Line, lines);
    out.put(Kind::GuardianBeam, lasers);
    out.put(Kind::CrystalBeam, crystals);
    out.put(Kind::Rays, rays);
}

/// Where an entity is this frame, and what the tethers ask of it.
#[derive(Clone, Copy, Debug)]
struct Body {
    pos: Vec3,
    body_yaw: f32,
    eye_height: f32,
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
    sampled: &'a crate::renderer::frame_view::SampledEntities,
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
                height: if local.crouching { 1.5 } else { 1.8 },
                kind: None,
                crouching: local.crouching,
                left_handed: self.view.main_hand_left,
            });
        }
        if let Some(anim) = self.by_id.get(&id)
            && let Some(st) = self.sampled.get(id)
        {
            return Some(Body {
                pos: Vec3::from(anim.position(partial)),
                body_yaw: st.body_rot,
                eye_height: st.eye_height,
                height: st.bounding_box_height,
                kind: Some(anim.kind),
                crouching: false,
                left_handed: false,
            });
        }
        let at = self.view.players.iter().position(|p| p.id == id)?;
        let player = &self.view.players[at];
        let sample = self.sampled.players.get(at)?;
        Some(Body {
            pos: Vec3::from(player.anim.position(partial)),
            body_yaw: sample.body_yaw,
            eye_height: if sample.crouching { 1.27 } else { 1.62 },
            height: if sample.crouching { 1.5 } else { 1.8 },
            kind: None,
            crouching: sample.crouching,
            left_handed: sample.main_arm_left,
        })
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
        let steps: Vec<u32> = if backwards {
            (0..=24).rev().collect()
        } else {
            (0..=24).collect()
        };
        let mut strip = Vec::with_capacity(50);
        for k in steps {
            let t = k as f32 / 24.0;
            let shade = if k % 2 == backwards as u32 { 0.7 } else { 1.0 };
            let color = srgb(0.5 * shade, 0.4 * shade, 0.3 * shade);
            let y = if d.y > 0.0 {
                d.y * t * t
            } else {
                d.y - d.y * (1.0 - t) * (1.0 - t)
            };
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
    let at =
        |a: f32| hook + Vec3::new(reach.x * a, reach.y * (a * a + a) * 0.5 + 0.25, reach.z * a);
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
    let v_base = if time.floor() as i32 % 2 == 0 {
        0.5
    } else {
        0.0
    };
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
    let overdrive = if death > 0.8 {
        ((death - 0.8) / 0.2).min(1.0)
    } else {
        0.0
    };
    // Additive: the inner colour carries its alpha, the outer is clear.
    let inner = [1.0 - overdrive; 3];
    let inner = [inner[0], inner[1], inner[2], 1.0];
    let outer = [0.0, 0.0, 0.0, 0.0];
    let mut random = JavaRandom::new(432);
    let count = ((death + death * death) / 2.0 * 60.0).floor() as i32;
    let mut turn = Quat::IDENTITY;
    let xyz = |x: f32, y: f32, z: f32| {
        Quat::from_rotation_x(x) * Quat::from_rotation_y(y) * Quat::from_rotation_z(z)
    };
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
    fn rays_grow_with_the_death() {
        let (mut early, mut late) = (Geo::default(), Geo::default());
        dragon_rays(&mut early, Vec3::ZERO, 0.0, 0.2);
        dragon_rays(&mut late, Vec3::ZERO, 0.0, 0.9);
        assert!(late.idx.len() > early.idx.len());
    }
}
