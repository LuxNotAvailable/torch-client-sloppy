use simdnbt::owned::Nbt;

use crate::blockentities::feed::BlockEntityData;

const SPAWN_TIME: i64 = 200;
const COOLDOWN_TIME: i32 = 40;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub age: i64,
}

pub fn decode(tag: &Nbt) -> Data {
    Data {
        age: tag.long("Age").unwrap_or(0),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gateway {
    age: i64,
    teleport_cooldown: i32,
}

impl Gateway {
    pub fn on_data(&mut self, data: &BlockEntityData) {
        if let BlockEntityData::Gateway(data) = data {
            self.age = data.age.min(SPAWN_TIME);
        }
    }

    pub fn tick(&mut self) {
        self.age = (self.age + 1).min(SPAWN_TIME);
        if self.is_cooling_down() {
            self.teleport_cooldown -= 1;
        }
    }

    pub fn event(&mut self, action: u8) {
        if action == 1 {
            self.teleport_cooldown = COOLDOWN_TIME;
        }
    }

    pub fn is_spawning(&self) -> bool {
        self.age < SPAWN_TIME
    }

    pub fn is_cooling_down(&self) -> bool {
        self.teleport_cooldown > 0
    }

    pub fn spawn_percent(&self, partial: f32) -> f32 {
        ((self.age as f32 + partial) / SPAWN_TIME as f32).clamp(0.0, 1.0)
    }

    pub fn cooldown_percent(&self, partial: f32) -> f32 {
        1.0 - ((self.teleport_cooldown as f32 - partial) / COOLDOWN_TIME as f32).clamp(0.0, 1.0)
    }

    #[cfg(test)]
    pub fn cooling(teleport_cooldown: i32) -> Gateway {
        Gateway {
            age: SPAWN_TIME,
            teleport_cooldown,
        }
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::NbtTag;

    use super::*;
    use crate::blockentities::sim::test_nbt::root;

    #[test]
    fn age_decodes_and_seeds_the_spawn_beam() {
        let data = decode(&root(vec![("Age", NbtTag::Long(150))]));
        assert_eq!(data.age, 150);
        let mut gateway = Gateway::default();
        gateway.on_data(&BlockEntityData::Gateway(data));
        assert!(gateway.is_spawning());
        assert!((gateway.spawn_percent(0.0) - 0.75).abs() < 1e-6);
        for _ in 0..50 {
            gateway.tick();
        }
        assert!(!gateway.is_spawning());
        assert_eq!(gateway.spawn_percent(0.5), 1.0);
    }

    #[test]
    fn event_one_runs_a_forty_tick_cooldown() {
        let mut gateway = Gateway::cooling(0);
        gateway.event(1);
        assert!(gateway.is_cooling_down());
        assert_eq!(gateway.cooldown_percent(0.0), 0.0);
        for _ in 0..20 {
            gateway.tick();
        }
        assert!((gateway.cooldown_percent(0.0) - 0.5).abs() < 1e-6);
        for _ in 0..20 {
            gateway.tick();
        }
        assert!(!gateway.is_cooling_down());
    }

    #[test]
    fn a_settled_gateway_compares_equal_as_it_ages() {
        let mut gateway = Gateway::cooling(0);
        let before = gateway;
        gateway.tick();
        assert_eq!(gateway, before);
    }
}
