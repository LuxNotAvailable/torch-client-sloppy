//! Round shadows under entities (`EntityRenderDispatcher.renderShadow`),
//! laid on the block tops under each caster.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_shadows(
    view: Res<FrameView>,
    camera: CameraQuery,
    third_person: Res<crate::renderer::input::ThirdPersonState>,
    state: Res<crate::gui::GuiState>,
    sampled: Res<crate::renderer::frame_view::SampledEntities>,
    mut out: LayerOut,
) {
    let mut geo = Geo::default();
    let opacity = if state.options.entity_shadows {
        state.options.shadow_opacity as f32 / 100.0
    } else {
        0.0
    };
    let level = crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone();
    if view.in_world
        && opacity > 0.0
        && let Some(level) = level
    {
        let level = level.read();
        let (cam_pos, _, _) = camera_pose(&camera);
        let partial = view.partial;
        let solid_top = |p: azalea_core::position::BlockPos| {
            use azalea::physics::collision::BlockWithShape;
            level
                .get_block_state(p)
                .is_some_and(|s| !s.is_air() && s.is_collision_shape_full())
        };
        let light = |p: azalea_core::position::BlockPos| {
            let (block, sky) =
                crate::renderer::lightmap::levels_at([p.x as f32, p.y as f32, p.z as f32]);
            block.max(sky)
        };
        let mut cast = |pos: Vec3, radius: f32, strength: f32| {
            let dist_sq = (pos - cam_pos).length_squared();
            for piece in
                crate::renderer::shadows::pieces(pos, radius, strength, dist_sq, &solid_top, &light)
            {
                let uv = |x: f32, z: f32| {
                    [
                        -(x - pos.x) / 2.0 / radius + 0.5,
                        -(z - pos.z) / 2.0 / radius + 0.5,
                    ]
                };
                // Vanilla's alpha tops out at a half: 50% is vanilla, 100%
                // solid black.
                let c = [1.0, 1.0, 1.0, (piece.alpha * 2.0 * opacity).min(1.0)];
                // Lifted off the block top so it never fights the face.
                let y = piece.y + 0.005;
                geo.quad([
                    (Vec3::new(piece.x0, y, piece.z0), uv(piece.x0, piece.z0), c),
                    (Vec3::new(piece.x0, y, piece.z1), uv(piece.x0, piece.z1), c),
                    (Vec3::new(piece.x1, y, piece.z1), uv(piece.x1, piece.z1), c),
                    (Vec3::new(piece.x1, y, piece.z0), uv(piece.x1, piece.z0), c),
                ]);
            }
        };
        if third_person.active {
            cast(Vec3::from(view.player_lerp), 0.5, 1.0);
        }
        for player in view.players.iter() {
            cast(Vec3::from(player.anim.position(partial)), 0.5, 1.0);
        }
        for (anim, st) in view.entities.iter().zip(&sampled.states) {
            let facts = crate::entities::facts::facts(anim.kind);
            if facts.shadow_radius > 0.0 && !anim.is_invisible() {
                // `MobRenderer`: babies' shadows shrink with them.
                cast(
                    Vec3::from(anim.position(partial)),
                    facts.shadow_radius * st.age_scale * st.scale,
                    facts.shadow_strength,
                );
            }
        }
    }
    out.put(Kind::Shadow, geo);
}
