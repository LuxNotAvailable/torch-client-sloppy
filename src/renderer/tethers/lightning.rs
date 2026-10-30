//! Lightning bolts (`LightningBoltRenderer`) and the sky flash they set.

use super::*;

pub(super) fn draw_lightning(
    view: Res<FrameView>,
    time: Res<Time>,
    mut out: LayerOut,
    mut bolts: Local<HashMap<i32, f32>>,
) {
    let now = time.elapsed_secs();
    let mut geo = Geo::default();
    if view.in_world {
        let mut seen = Vec::new();
        for anim in view.entities.iter() {
            if anim.kind == EntityKind::LightningBolt {
                bolts.entry(anim.id).or_insert(now);
                seen.push(anim.id);
                let pos = Vec3::from(anim.position(view.partial));
                lightning(&mut geo, pos, anim.id as i64);
            }
        }
        bolts.retain(|id, _| seen.contains(id));
    } else {
        bolts.clear();
    }

    // `LightningBolt.tick` sets the sky flash for the first ticks of a bolt.
    let flash = bolts
        .values()
        .map(|start| (3.0 - (now - start) * 20.0).max(0.0))
        .fold(0.0f32, f32::max);
    crate::renderer::environment::set_sky_flash(flash);
    out.put(Kind::Lightning, geo);
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
                        (
                            pos + Vec3::new(sx + s(rx, bottom), y0, sz + s(rz, bottom)),
                            [0.0, 0.0],
                            color,
                        ),
                        (
                            pos + Vec3::new(ex + s(rx, top), y1, ez + s(rz, top)),
                            [0.0, 0.0],
                            color,
                        ),
                        (
                            pos + Vec3::new(ex + s(lx, top), y1, ez + s(lz, top)),
                            [0.0, 0.0],
                            color,
                        ),
                        (
                            pos + Vec3::new(sx + s(lx, bottom), y0, sz + s(lz, bottom)),
                            [0.0, 0.0],
                            color,
                        ),
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
    fn the_same_seed_draws_the_same_bolt() {
        let (mut a, mut b) = (Geo::default(), Geo::default());
        lightning(&mut a, Vec3::ZERO, 7);
        lightning(&mut b, Vec3::ZERO, 7);
        assert_eq!(a.pos, b.pos);
        assert!(!a.pos.is_empty());
    }
}
