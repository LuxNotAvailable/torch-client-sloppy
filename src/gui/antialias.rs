//! The Antialiasing option: FXAA (a post pass, at a chosen sensitivity),
//! MSAA (2, 4 or 8 samples, as many as the GPU supports) or supersampling
//! (`ssaa`, the scene drawn larger and scaled down). The renderers apply it.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AaMode {
    #[default]
    Off,
    Fxaa,
    Msaa,
    Supersample,
}

impl AaMode {
    pub fn caption(self) -> &'static str {
        match self {
            AaMode::Off => "OFF",
            AaMode::Fxaa => "FXAA",
            AaMode::Msaa => "MSAA",
            AaMode::Supersample => "Supersampling",
        }
    }

    pub fn next(self) -> AaMode {
        match self {
            AaMode::Off => AaMode::Fxaa,
            AaMode::Fxaa => AaMode::Msaa,
            AaMode::Msaa => AaMode::Supersample,
            AaMode::Supersample => AaMode::Off,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AaMode::Off => "off",
            AaMode::Fxaa => "fxaa",
            AaMode::Msaa => "msaa",
            AaMode::Supersample => "ssaa",
        }
    }

    pub fn from_key(key: &str) -> Option<AaMode> {
        match key {
            "off" => Some(AaMode::Off),
            "fxaa" => Some(AaMode::Fxaa),
            "msaa" => Some(AaMode::Msaa),
            "ssaa" => Some(AaMode::Supersample),
            _ => None,
        }
    }
}

/// FXAA's sensitivity names, sharpest first.
pub const FXAA_LEVELS: [&str; 5] = ["Low", "Medium", "High", "Ultra", "Extreme"];
pub const MSAA_SAMPLES: [u8; 3] = [2, 4, 8];
/// Supersampling's scale along each axis.
pub const SSAA_SCALES: [f32; 2] = [1.5, 2.0];

/// The Antialiasing settings: the mode, and a level kept for each mode so
/// switching back and forth keeps what was picked.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Antialiasing {
    pub mode: AaMode,
    /// An index into `FXAA_LEVELS`.
    pub fxaa: usize,
    pub msaa: u8,
    /// An index into `SSAA_SCALES`.
    pub ssaa: usize,
}

impl Default for Antialiasing {
    fn default() -> Antialiasing {
        Antialiasing {
            mode: AaMode::Off,
            fxaa: 2,
            msaa: 4,
            ssaa: 1,
        }
    }
}

impl Antialiasing {
    /// The level's caption for the current mode; `None` when it has none.
    pub fn level_caption(&self) -> Option<String> {
        match self.mode {
            AaMode::Off => None,
            AaMode::Fxaa => Some(FXAA_LEVELS[self.fxaa.min(FXAA_LEVELS.len() - 1)].to_owned()),
            AaMode::Msaa => Some(format!("{}x", self.msaa_samples())),
            AaMode::Supersample => Some(format!(
                "{}x",
                SSAA_SCALES[self.ssaa.min(SSAA_SCALES.len() - 1)]
            )),
        }
    }

    /// Steps the current mode's level, wrapping, skipping sample counts the
    /// GPU cannot do.
    pub fn next_level(&mut self) {
        match self.mode {
            AaMode::Off => {}
            AaMode::Fxaa => self.fxaa = (self.fxaa + 1) % FXAA_LEVELS.len(),
            AaMode::Msaa => {
                let supported: Vec<u8> =
                    MSAA_SAMPLES.into_iter().filter(|&n| supports(n)).collect();
                if supported.is_empty() {
                    return;
                }
                let at = supported.iter().position(|&n| n == self.msaa);
                self.msaa = supported[at.map_or(0, |i| (i + 1) % supported.len())];
            }
            AaMode::Supersample => self.ssaa = (self.ssaa + 1) % SSAA_SCALES.len(),
        }
    }

    /// The MSAA samples to draw with: the chosen count, lowered to one the
    /// GPU supports.
    pub fn msaa_samples(&self) -> u8 {
        MSAA_SAMPLES
            .into_iter()
            .rev()
            .filter(|&n| n <= self.msaa && supports(n))
            .next()
            .unwrap_or(1)
    }

    /// Reads the saved settings. Before modes there was a single
    /// `antialiasing` switch, which was 2x supersampling.
    pub fn read(value: impl Fn(&str) -> Option<serde_json::Value>) -> Antialiasing {
        let mut aa = Antialiasing::default();
        match value("antialiasing") {
            Some(serde_json::Value::Bool(true)) => aa.mode = AaMode::Supersample,
            Some(serde_json::Value::String(key)) => {
                aa.mode = AaMode::from_key(&key).unwrap_or_default()
            }
            _ => {}
        }
        if let Some(i) = value("fxaaLevel").and_then(|v| v.as_u64()) {
            aa.fxaa = (i as usize).min(FXAA_LEVELS.len() - 1);
        }
        if let Some(n) = value("msaaSamples").and_then(|v| v.as_u64())
            && MSAA_SAMPLES.contains(&(n as u8))
        {
            aa.msaa = n as u8;
        }
        if let Some(i) = value("ssaaLevel").and_then(|v| v.as_u64()) {
            aa.ssaa = (i as usize).min(SSAA_SCALES.len() - 1);
        }
        aa
    }

    pub fn write(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        map.insert("antialiasing".into(), self.mode.key().into());
        map.insert("fxaaLevel".into(), self.fxaa.into());
        map.insert("msaaSamples".into(), self.msaa.into());
        map.insert("ssaaLevel".into(), self.ssaa.into());
    }
}

/// The MSAA sample counts this GPU can draw the scene with, as bits
/// (`1 << n`); all of them until the renderer has checked.
static SUPPORTED: AtomicU8 = AtomicU8::new(u8::MAX);

pub fn supports(samples: u8) -> bool {
    samples <= 1 || SUPPORTED.load(Ordering::Relaxed) & (1 << samples.trailing_zeros()) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_switch_reads_as_supersampling() {
        let aa = Antialiasing::read(|k| (k == "antialiasing").then(|| serde_json::json!(true)));
        assert_eq!(aa.mode, AaMode::Supersample);
        assert_eq!(SSAA_SCALES[aa.ssaa], 2.0);
        let aa = Antialiasing::read(|k| (k == "antialiasing").then(|| serde_json::json!(false)));
        assert_eq!(aa.mode, AaMode::Off);
    }

    #[test]
    fn settings_round_trip() {
        let aa = Antialiasing {
            mode: AaMode::Msaa,
            fxaa: 4,
            msaa: 8,
            ssaa: 0,
        };
        let mut map = serde_json::Map::new();
        aa.write(&mut map);
        assert_eq!(Antialiasing::read(|k| map.get(k).cloned()), aa);
    }

    #[test]
    fn levels_wrap() {
        let mut aa = Antialiasing {
            mode: AaMode::Fxaa,
            fxaa: FXAA_LEVELS.len() - 1,
            ..Antialiasing::default()
        };
        aa.next_level();
        assert_eq!(aa.fxaa, 0);
        aa.mode = AaMode::Msaa;
        aa.msaa = 8;
        aa.next_level();
        assert_eq!(aa.msaa, 2);
    }
}
