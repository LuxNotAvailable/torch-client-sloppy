use azalea_buf::AzBuf;
use azalea_core::position::{Vec3, Vec3f32};
use azalea_entity::particle::Particle;
use azalea_protocol_macros::ClientboundGamePacket;

#[derive(AzBuf, ClientboundGamePacket, Clone, Debug, PartialEq)]
pub struct ClientboundLevelParticles {
    pub particle: Particle,
    pub override_limiter: bool,
    pub always_show: bool,
    pub pos: Vec3,
    pub x_dist: f32,
    pub y_dist: f32,
    pub z_dist: f32,
    pub max_speed: Vec3f32,
    #[var]
    pub count: u32,
    pub randomization: ParticleRandomization,
}

/// How each particle's offset and speed are rolled from the distribution
/// fields.
#[derive(AzBuf, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParticleRandomization {
    #[default]
    Default,
    Alternative,
    AlternativeWithSpeed,
}

impl ParticleRandomization {
    pub fn is_alternative(self) -> bool {
        self != Self::Default
    }
}
