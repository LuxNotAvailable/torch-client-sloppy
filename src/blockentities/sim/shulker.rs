use crate::util::mth::lerp;

const STEP: f32 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    #[default]
    Closed,
    Opening,
    Opened,
    Closing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shulker {
    status: Status,
    progress: f32,
    progress_old: f32,
}

impl Shulker {
    pub fn tick(&mut self) {
        self.progress_old = self.progress;
        match self.status {
            Status::Closed => self.progress = 0.0,
            Status::Opening => {
                self.progress += STEP;
                if self.progress >= 1.0 {
                    self.status = Status::Opened;
                    self.progress = 1.0;
                }
            }
            Status::Opened => self.progress = 1.0,
            Status::Closing => {
                self.progress -= STEP;
                if self.progress <= 0.0 {
                    self.status = Status::Closed;
                    self.progress = 0.0;
                }
            }
        }
    }

    pub fn event(&mut self, action: u8, param: u8) {
        if action != 1 {
            return;
        }
        match param {
            0 => self.status = Status::Closing,
            1 => self.status = Status::Opening,
            _ => {}
        }
    }

    pub fn progress(&self, partial: f32) -> f32 {
        lerp(partial, self.progress_old, self.progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_takes_ten_ticks_and_stops_at_one() {
        let mut shulker = Shulker::default();
        shulker.event(1, 1);
        for _ in 0..10 {
            shulker.tick();
        }
        assert_eq!(shulker.progress(1.0), 1.0);
        shulker.tick();
        assert_eq!(shulker.progress(0.5), 1.0);
        shulker.event(1, 0);
        for _ in 0..10 {
            shulker.tick();
        }
        assert_eq!(shulker.progress(1.0), 0.0);
    }
}
