//! Firework explosions: `FireworkParticles.Starter`, its shapes, sparks and flash.

#![allow(unused_imports)]

use super::*;

/// A rocket's explosion: `FireworkExplosion`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FireworkExplosion {
    /// 0 small ball, 1 large ball, 2 star, 3 creeper, 4 burst.
    pub shape: u8,
    pub colors: Vec<i32>,
    pub fade_colors: Vec<i32>,
    pub trail: bool,
    pub twinkle: bool,
}

/// `FireworkParticles.Starter`: sets off a rocket's explosions one every
/// other tick, with the blast and the twinkle.
pub(super) struct Starter {
    pub(super) pos: Vec3,
    pub(super) velocity: Vec3,
    pub(super) explosions: Vec<FireworkExplosion>,
    pub(super) life: u32,
    pub(super) lifetime: u32,
    pub(super) twinkle_delay: bool,
    pub(super) tick_acc: f32,
}

pub(super) static STARTERS: std::sync::Mutex<Vec<Starter>> = std::sync::Mutex::new(Vec::new());

/// A new session: no fireworks carry over.
pub(crate) fn reset_fireworks() {
    STARTERS.lock().unwrap().clear();
}

/// A rocket exploded (entity event 17) at `pos` moving at `velocity`.
pub(crate) fn start_firework(
    pos: [f64; 3],
    velocity: [f64; 3],
    explosions: Vec<FireworkExplosion>,
) {
    let pos = Vec3::new(pos[0] as f32, pos[1] as f32, pos[2] as f32);
    let velocity = Vec3::new(velocity[0] as f32, velocity[1] as f32, velocity[2] as f32);
    if explosions.is_empty() {
        // `ClientLevel.createFireworks`: a rocket with none puffs smoke.
        if let Some(shared) = crate::SHARED.get() {
            let mut s = shared.lock().unwrap();
            for _ in 0..8 {
                crate::session::push_particle_emit(
                    &mut s.session.world.particle_emits,
                    crate::session::ParticleEmit::Level(crate::session::LevelParticle {
                        particle: azalea::entity::particle::Particle::Poof,
                        pos: [pos.x as f64, pos.y as f64, pos.z as f64],
                        dist: [0.0; 3],
                        max_speed: [1.0; 3],
                        count: 0,
                        randomization: Default::default(),
                        override_limiter: false,
                        always_show: false,
                    }),
                );
            }
        }
        return;
    }
    let twinkle_delay = explosions.iter().any(|e| e.twinkle);
    let lifetime = explosions.len() as u32 * 2 - 1 + if twinkle_delay { 15 } else { 0 };
    STARTERS.lock().unwrap().push(Starter {
        pos,
        velocity,
        explosions,
        life: 0,
        lifetime,
        twinkle_delay,
        tick_acc: TICK,
    });
}

// @generated begin firework shapes: tools/gen_firework_shapes.py
/// The creeper face's outline (`FireworkParticles.Starter.CREEPER_PARTICLE_COORDS`).
pub(super) const CREEPER_COORDS: [[f32; 2]; 12] = [
    [0.0, 0.2],
    [0.2, 0.2],
    [0.2, 0.6],
    [0.6, 0.6],
    [0.6, 0.2],
    [0.2, 0.2],
    [0.2, 0.0],
    [0.4, 0.0],
    [0.4, -0.6],
    [0.2, -0.6],
    [0.2, -0.4],
    [0.0, -0.4],
];

/// The star's outline (`FireworkParticles.Starter.STAR_PARTICLE_COORDS`).
pub(super) const STAR_COORDS: [[f32; 2]; 6] = [
    [0.0, 1.0],
    [0.3455, 0.309],
    [0.9511, 0.309],
    [0.37959182, -0.12653062],
    [0.6122449, -0.8040816],
    [0.0, -0.35918367],
];
// @generated end firework shapes

pub(super) fn rgb(color: i32) -> [f32; 3] {
    [
        ((color >> 16) & 0xFF) as f32 / 255.0,
        ((color >> 8) & 0xFF) as f32 / 255.0,
        (color & 0xFF) as f32 / 255.0,
    ]
}

/// A spark: `SparkParticle`, gravity 0.1, three quarters the size.
pub(super) fn spark(
    atlas: &Atlas,
    rng: &mut JavaRandom,
    pos: Vec3,
    velocity: Vec3,
    explosion: &FireworkExplosion,
    colors: &[i32],
) -> Option<Particle> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index("firework")?), rng);
    let color = colors[rng.next_int(colors.len() as u32) as usize];
    let fade = (!explosion.fade_colors.is_empty()).then(|| {
        rgb(explosion.fade_colors[rng.next_int(explosion.fade_colors.len() as u32) as usize])
    });
    r.size *= 0.75;
    let [cr, cg, cb] = rgb(color);
    Some(Particle {
        prev: pos,
        pos,
        velocity,
        gravity: 0.1,
        friction: 0.91,
        tick_acc: 0.0,
        age: 0,
        lifetime: 48 + rng.next_int(12),
        tint: [cr, cg, cb, 0.99],
        size: r.size,
        grow: Grow::Fixed,
        sprite: r.sprite,
        flags: TRANSLUCENT | FULLBRIGHT | FADE_OUT,
        behavior: Behavior::Spark(Spark {
            fade,
            trail: explosion.trail,
            twinkle: explosion.twinkle,
        }),
    })
}

pub(super) fn explode(
    starter: &Starter,
    explosion: &FireworkExplosion,
    atlas: &Atlas,
    rng: &mut JavaRandom,
    out: &mut Vec<Particle>,
) {
    let black = [0x1E1B1B];
    let colors: &[i32] = if explosion.colors.is_empty() {
        &black
    } else {
        &explosion.colors
    };
    let at = starter.pos;
    let add = |out: &mut Vec<Particle>, rng: &mut JavaRandom, v: Vec3| {
        if let Some(p) = spark(atlas, rng, at, v, explosion, colors) {
            out.push(p);
        }
    };
    match explosion.shape {
        0 | 1 => {
            let (speed, steps) = if explosion.shape == 0 {
                (0.25f64, 2i32)
            } else {
                (0.5, 4)
            };
            for y in -steps..=steps {
                for x in -steps..=steps {
                    let mut z = -steps;
                    while z <= steps {
                        let xa = x as f64 + (rng.next_f64() - rng.next_f64()) * 0.5;
                        let ya = y as f64 + (rng.next_f64() - rng.next_f64()) * 0.5;
                        let za = z as f64 + (rng.next_f64() - rng.next_f64()) * 0.5;
                        let len = (xa * xa + ya * ya + za * za).sqrt() / speed
                            + rng.next_gaussian() * 0.05;
                        add(
                            out,
                            rng,
                            Vec3::new((xa / len) as f32, (ya / len) as f32, (za / len) as f32),
                        );
                        if y != -steps && y != steps && x != -steps && x != steps {
                            z += steps * 2 - 1;
                        }
                        z += 1;
                    }
                }
            }
        }
        2 | 3 => {
            let coords: &[[f32; 2]] = if explosion.shape == 2 {
                &STAR_COORDS
            } else {
                &CREEPER_COORDS
            };
            let flat = explosion.shape == 3;
            let speed = 0.5f64;
            let (sx, sy) = (coords[0][0] as f64, coords[0][1] as f64);
            add(
                out,
                rng,
                Vec3::new((sx * speed) as f32, (sy * speed) as f32, 0.0),
            );
            let base = rng.next_f32() as f64 * std::f64::consts::PI;
            let modifier = if flat { 0.034 } else { 0.34 };
            for step in 0..3 {
                let angle = base + step as f64 * std::f64::consts::PI * modifier;
                let (mut ox, mut oy) = (sx, sy);
                for c in &coords[1..] {
                    let (tx, ty) = (c[0] as f64, c[1] as f64);
                    let mut sub = 0.25;
                    while sub <= 1.0 {
                        let xa = (ox + (tx - ox) * sub) * speed;
                        let ya = (oy + (ty - oy) * sub) * speed;
                        let za = xa * angle.sin();
                        let xa = xa * angle.cos();
                        for flip in [-1.0, 1.0] {
                            add(
                                out,
                                rng,
                                Vec3::new((xa * flip) as f32, ya as f32, (za * flip) as f32),
                            );
                        }
                        sub += 0.25;
                    }
                    (ox, oy) = (tx, ty);
                }
            }
        }
        _ => {
            let off_x = rng.next_gaussian() * 0.05;
            let off_z = rng.next_gaussian() * 0.05;
            for _ in 0..70 {
                let xa = starter.velocity.x as f64 * 0.5 + rng.next_gaussian() * 0.15 + off_x;
                let za = starter.velocity.z as f64 * 0.5 + rng.next_gaussian() * 0.15 + off_z;
                let ya = starter.velocity.y as f64 * 0.5 + rng.next_f64() * 0.5;
                add(out, rng, Vec3::new(xa as f32, ya as f32, za as f32));
            }
        }
    }
    // The flash, in the first colour.
    if let Some(index) = atlas.sheet_index("flash") {
        let [cr, cg, cb] = rgb(colors[0]);
        out.push(Particle {
            prev: at,
            pos: at,
            velocity: Vec3::ZERO,
            gravity: 0.0,
            friction: 1.0,
            tick_acc: 0.0,
            age: 0,
            lifetime: 4,
            tint: [cr, cg, cb, 0.6],
            size: 1.0,
            grow: Grow::Flash,
            sprite: Sprite::Animated(index),
            flags: TRANSLUCENT | FULLBRIGHT,
            behavior: Behavior::None,
        });
    }
}

pub(super) fn tick_fireworks(
    time: Res<Time>,
    atlas: Res<ParticleAtlas>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut live: ResMut<LiveParticles>,
    mut particle_rng: ResMut<ParticleRng>,
) {
    let mut starters = STARTERS.lock().unwrap();
    if starters.is_empty() {
        return;
    }
    let Some(atlas) = atlas.0.as_ref() else {
        return;
    };
    let eye = camera
        .iter()
        .next()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    let dt = time.delta_secs();
    let rng = &mut particle_rng.0;
    starters.retain_mut(|starter| {
        starter.tick_acc += dt;
        while starter.tick_acc >= TICK {
            starter.tick_acc -= TICK;
            let far = starter.pos.distance_squared(eye) >= 256.0;
            let at = starter.pos.to_array();
            if starter.life == 0 {
                let large = starter.explosions.len() >= 3
                    || starter.explosions.iter().any(|e| e.shape == 1);
                let sound = match (large, far) {
                    (true, true) => "entity.firework_rocket.large_blast_far",
                    (true, false) => "entity.firework_rocket.large_blast",
                    (false, true) => "entity.firework_rocket.blast_far",
                    (false, false) => "entity.firework_rocket.blast",
                };
                #[cfg(audio)]
                crate::audio::play_at(
                    sound,
                    crate::audio::category::SoundCategory::Ambient,
                    at,
                    20.0,
                    0.95 + rng.next_f32() * 0.1,
                );
                #[cfg(not(audio))]
                let _ = (sound, at);
            }
            if starter.life % 2 == 0 && ((starter.life / 2) as usize) < starter.explosions.len() {
                let explosion = starter.explosions[(starter.life / 2) as usize].clone();
                if live.0.len() < MAX_LIVE_PARTICLES {
                    explode(starter, &explosion, atlas, rng, &mut live.0);
                }
            }
            starter.life += 1;
            if starter.life > starter.lifetime {
                if starter.twinkle_delay {
                    let sound = if far {
                        "entity.firework_rocket.twinkle_far"
                    } else {
                        "entity.firework_rocket.twinkle"
                    };
                    #[cfg(audio)]
                    crate::audio::play_at(
                        sound,
                        crate::audio::category::SoundCategory::Ambient,
                        at,
                        20.0,
                        0.9 + rng.next_f32() * 0.15,
                    );
                    #[cfg(not(audio))]
                    let _ = sound;
                }
                return false;
            }
        }
        true
    });
}
