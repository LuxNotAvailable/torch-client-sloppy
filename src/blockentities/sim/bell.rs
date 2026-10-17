use azalea_core::direction::Direction;

use crate::blocks::facing;

const DURATION: u32 = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bell {
    ticks: u32,
    shaking: bool,
    click_direction: Direction,
}

impl Bell {
    pub fn tick(&mut self) {
        if self.shaking {
            self.ticks += 1;
        }
        if self.ticks >= DURATION {
            self.shaking = false;
            self.ticks = 0;
        }
    }

    pub fn event(&mut self, action: u8, param: u8) {
        if action == 1 {
            self.click_direction = facing::from_3d_data_value(param.into());
            self.ticks = 0;
            self.shaking = true;
        }
    }

    pub fn shake_direction(&self) -> Option<Direction> {
        self.shaking.then_some(self.click_direction)
    }

    pub fn ticks(&self, partial: f32) -> f32 {
        self.ticks as f32 + partial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_shakes_for_fifty_ticks() {
        let mut bell = Bell::default();
        assert_eq!(bell.shake_direction(), None);
        bell.event(1, 5);
        assert_eq!(bell.shake_direction(), Some(Direction::East));
        for _ in 0..49 {
            bell.tick();
        }
        assert!(bell.shake_direction().is_some());
        bell.tick();
        assert_eq!(bell.shake_direction(), None);
        assert_eq!(bell.ticks(0.0), 0.0);
    }
}
