//! Falling rain and snow: `WeatherEffectRenderer`. A camera-facing sheet of
//! `environment/rain.png` or `environment/snow.png` stands on every column
//! within the weather radius, from the ground (or below the camera) up past
//! it, scrolling down; it fades toward the edge and with the rain level.

use bevy::math::Vec3;

use crate::util::javarandom::JavaRandom;

/// `weatherRadius`'s default.
pub const RADIUS: i32 = 10;

/// What falls in a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fall {
    Rain,
    Snow,
}

/// One column's sheet: `ColumnInstance`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub fall: Fall,
    pub x: i32,
    pub z: i32,
    pub bottom: i32,
    pub top: i32,
    pub u_offset: f32,
    pub v_offset: f32,
}

/// One corner of a sheet: position relative to the camera, uv, alpha.
pub type Corner = (Vec3, [f32; 2], f32);

fn seed(x: i32, z: i32) -> i32 {
    x.wrapping_mul(x)
        .wrapping_mul(3121)
        .wrapping_add(x.wrapping_mul(45238971))
        ^ z.wrapping_mul(z).wrapping_mul(418711).wrapping_add(z.wrapping_mul(13761))
}

/// The columns about `camera` (a block position) where something falls.
/// `terrain` gives the `MOTION_BLOCKING` height of a column and `fall` what
/// falls there, if anything.
pub fn columns(
    camera: [i32; 3],
    game_time: i64,
    partial: f32,
    terrain: impl Fn(i32, i32) -> i32,
    fall: impl Fn(i32, i32) -> Option<Fall>,
) -> Vec<Column> {
    let mut out = Vec::new();
    let wrapped = (game_time & 131071) as i32;
    for z in camera[2] - RADIUS..=camera[2] + RADIUS {
        for x in camera[0] - RADIUS..=camera[0] + RADIUS {
            let height = terrain(x, z);
            let bottom = (camera[1] - RADIUS).max(height);
            let top = (camera[1] + RADIUS).max(height);
            if top == bottom {
                continue;
            }
            let Some(kind) = fall(x, z) else { continue };
            let mut random = JavaRandom::new(seed(x, z) as i64);
            let (u_offset, v_offset) = match kind {
                Fall::Rain => {
                    let tick_offset = x
                        .wrapping_mul(x)
                        .wrapping_mul(3121)
                        .wrapping_add(x.wrapping_mul(45238971))
                        .wrapping_add(z.wrapping_mul(z).wrapping_mul(418711))
                        .wrapping_add(z.wrapping_mul(13761))
                        & 0xFF;
                    let speed = 3.0 + random.next_f32();
                    let offset = -((wrapped + tick_offset) as f32 + partial) / 32.0 * speed;
                    (0.0, offset % 32.0)
                }
                Fall::Snow => {
                    let time = wrapped as f32 + partial;
                    let u = (random.next_f64() + (time * 0.01 * random.next_gaussian() as f32) as f64) as f32;
                    let v = (random.next_f64() + (time * random.next_gaussian() as f32 * 0.001) as f64) as f32;
                    let drop = -((game_time & 511) as f32 + partial) / 512.0;
                    (u, drop + v)
                }
            };
            out.push(Column { fall: kind, x, z, bottom, top, u_offset, v_offset });
        }
    }
    out
}

/// A column's sheet as a quad facing the camera's column, relative to the
/// camera at `cam`. `intensity` is the rain level.
pub fn sheet(column: &Column, cam: Vec3, intensity: f32) -> [Corner; 4] {
    let max_alpha = match column.fall {
        Fall::Rain => 1.0,
        Fall::Snow => 0.8,
    };
    let rx = column.x as f32 + 0.5 - cam.x;
    let rz = column.z as f32 + 0.5 - cam.z;
    let radius_sq = (RADIUS * RADIUS) as f32;
    let t = ((rx * rx + rz * rz) / radius_sq).min(1.0);
    let alpha = (max_alpha + (0.5 - max_alpha) * t) * intensity;
    // `columnSizeX/Z`: square to the line from the camera's column.
    let dx = (column.x - cam.x.floor() as i32) as f32;
    let dz = (column.z - cam.z.floor() as i32) as f32;
    let length = (dx * dx + dz * dz).sqrt();
    let (hx, hz) = if length > 0.0 { (-dz / length / 2.0, dx / length / 2.0) } else { (f32::NAN, f32::NAN) };
    // Vanilla's centre column divides by zero and draws nothing.
    let (hx, hz) = if hx.is_nan() { (0.0, 0.0) } else { (hx, hz) };
    let (y0, y1) = (column.bottom as f32 - cam.y, column.top as f32 - cam.y);
    let (u0, u1) = (column.u_offset, column.u_offset + 1.0);
    let v0 = column.bottom as f32 * 0.25 + column.v_offset;
    let v1 = column.top as f32 * 0.25 + column.v_offset;
    [
        (Vec3::new(rx - hx, y1, rz - hz), [u0, v0], alpha),
        (Vec3::new(rx + hx, y1, rz + hz), [u1, v0], alpha),
        (Vec3::new(rx + hx, y0, rz + hz), [u1, v1], alpha),
        (Vec3::new(rx - hx, y0, rz - hz), [u0, v1], alpha),
    ]
}

/// `sheet` cut wherever its texture repeats, each piece's uv brought into
/// 0 to 1: a sampler that clamps (a shader pack's) then draws it the same as
/// one that wraps.
pub fn sheet_pieces(column: &Column, cam: Vec3, intensity: f32) -> Vec<[Corner; 4]> {
    let [tl, tr, _br, bl] = sheet(column, cam, intensity);
    let alpha = tl.2;
    let (u0, v0, v1) = (tl.1[0], tl.1[1], bl.1[1]);
    // Cuts along the width (u runs u0 to u0 + 1) and down the height.
    let cuts = |from: f32, to: f32| {
        let (lo, hi) = (from.min(to), from.max(to));
        let mut at = vec![0.0f32];
        let mut k = lo.floor() + 1.0;
        while k < hi {
            at.push((k - from) / (to - from));
            k += 1.0;
        }
        at.push(1.0);
        at.sort_by(f32::total_cmp);
        at
    };
    let across = cuts(u0, u0 + 1.0);
    let down = if (v1 - v0).abs() > 0.0 { cuts(v0, v1) } else { vec![0.0, 1.0] };
    let mut out = Vec::new();
    for w in across.windows(2) {
        for h in down.windows(2) {
            let (s0, s1, t0, t1) = (w[0], w[1], h[0], h[1]);
            let u = |s: f32| u0 + s;
            let v = |t: f32| v0 + (v1 - v0) * t;
            let shift_u = u((s0 + s1) * 0.5).floor();
            let shift_v = v((t0 + t1) * 0.5).floor();
            // Across from the left edge to the right, down from the top.
            let at = |s: f32, t: f32| {
                let top = tl.0 + (tr.0 - tl.0) * s;
                let bottom = bl.0 + (tr.0 - tl.0) * s;
                top + (bottom - top) * t
            };
            let uv = |s: f32, t: f32| [u(s) - shift_u, v(t) - shift_v];
            out.push([
                (at(s0, t0), uv(s0, t0), alpha),
                (at(s1, t0), uv(s1, t0), alpha),
                (at(s1, t1), uv(s1, t1), alpha),
                (at(s0, t1), uv(s0, t1), alpha),
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_stand_on_open_ground_and_not_under_roofs() {
        let open = columns([0, 64, 0], 1000, 0.0, |_, _| 60, |_, _| Some(Fall::Rain));
        assert_eq!(open.len(), 21 * 21);
        assert!(open.iter().all(|c| c.bottom == 60 && c.top == 74));
        // A roof above the radius: nothing falls.
        assert!(columns([0, 64, 0], 1000, 0.0, |_, _| 100, |_, _| Some(Fall::Rain)).is_empty());
        assert!(columns([0, 64, 0], 1000, 0.0, |_, _| 60, |_, _| None).is_empty());
    }

    #[test]
    fn sheets_fade_toward_the_edge() {
        let c = |x| Column { fall: Fall::Rain, x, z: 0, bottom: 60, top: 74, u_offset: 0.0, v_offset: 0.0 };
        let cam = Vec3::new(0.5, 64.0, 0.5);
        assert!(sheet(&c(1), cam, 1.0)[0].2 > sheet(&c(9), cam, 1.0)[0].2);
        assert_eq!(sheet(&c(1), cam, 0.0)[0].2, 0.0);
    }

    #[test]
    fn pieces_keep_their_uv_in_one_repeat() {
        let column = Column { fall: Fall::Snow, x: 3, z: 1, bottom: 60, top: 74, u_offset: 2.3, v_offset: -7.6 };
        let pieces = sheet_pieces(&column, Vec3::new(0.5, 64.0, 0.5), 1.0);
        assert!(pieces.len() > 4);
        for piece in &pieces {
            for (_, uv, _) in piece {
                assert!((-1e-4..=1.0001).contains(&uv[0]) && (-1e-4..=1.0001).contains(&uv[1]), "{uv:?}");
            }
        }
    }
}
