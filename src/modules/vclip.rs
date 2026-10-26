use super::registry::{Id, vclip as setting};
use super::store;

const ALLOWANCE: f64 = 100.0;

const BATCH: u32 = 4;

const BLOCKS_PER_PACKET: f64 = 10.0;

const MAX_PACKETS: u32 = 20;

pub fn take() -> Option<f64> {
    let s = store();
    if !s.armed(Id::VClip) {
        return None;
    }
    let allowed = s.enabled(Id::VClip);
    s.set_enabled(Id::VClip, false);
    let blocks = s.num(setting::DISTANCE).round() as f64;
    (allowed && blocks != 0.0).then_some(blocks)
}

pub fn packets(blocks: f64) -> u32 {
    let vanilla = (blocks * blocks / ALLOWANCE).ceil() as u32;
    if vanilla <= BATCH {
        vanilla.max(1)
    } else {
        ((blocks.abs() / BLOCKS_PER_PACKET).ceil() as u32).min(MAX_PACKETS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_count_follows_the_squared_allowance() {
        assert_eq!(packets(1.0), 1);
        assert_eq!(packets(-10.0), 1, "100 is not over 100");
        assert_eq!(packets(11.0), 2);
        assert_eq!(packets(-20.0), 4);
        assert_eq!(packets(21.0), 3, "past the batch, Meteor's count");
        assert_eq!(packets(-500.0), MAX_PACKETS);
    }
}
