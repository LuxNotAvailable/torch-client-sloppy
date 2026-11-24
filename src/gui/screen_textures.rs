//! Textures mod screens blit (`modapi::screens`), loaded on demand into a
//! reserved part of the GUI atlas: each one squeezed into a
//! [`CELL`]-pixel square, which stands for its whole UV space.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::gui::atlas::{SCREEN_TEXTURE_PX, SCREEN_TEXTURE_SLOTS};

pub const CELL: u32 = SCREEN_TEXTURE_PX;

#[derive(Default)]
struct Textures {
    slots: HashMap<String, usize>,
    ready: Vec<bool>,
    pending: Vec<(usize, String)>,
}

impl Textures {
    /// The slot `key` has, claiming one; a full set starts over.
    fn request(&mut self, key: &str) -> usize {
        if let Some(slot) = self.slots.get(key) {
            return *slot;
        }
        if self.slots.len() >= SCREEN_TEXTURE_SLOTS {
            *self = Textures::default();
        }
        let slot = self.slots.len();
        self.slots.insert(key.to_owned(), slot);
        self.ready.push(false);
        self.pending.push((slot, key.to_owned()));
        slot
    }

    fn ready(&self, slot: usize) -> bool {
        self.ready.get(slot).copied().unwrap_or(false)
    }
}

static TEXTURES: Mutex<Option<Textures>> = Mutex::new(None);

fn textures() -> MutexGuard<'static, Option<Textures>> {
    TEXTURES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The atlas slot holding `key` (`mymod:textures/gui/press.png`), once
/// it's uploaded; asks for it otherwise.
pub fn slot(key: &str) -> Option<usize> {
    let mut guard = textures();
    let t = guard.get_or_insert_with(Textures::default);
    let slot = t.request(key);
    t.ready(slot).then_some(slot)
}

pub fn take_pending() -> Vec<(usize, String)> {
    textures()
        .as_mut()
        .map(|t| std::mem::take(&mut t.pending))
        .unwrap_or_default()
}

pub fn mark_ready(slot: usize) {
    if let Some(t) = textures().as_mut()
        && let Some(ready) = t.ready.get_mut(slot)
    {
        *ready = true;
    }
}

/// A texture's pixels at [`CELL`] x [`CELL`], from the asset layers
/// (mod packages, then vanilla).
pub fn load(key: &str) -> Option<Vec<u8>> {
    let (ns, path) = key.split_once(':').unwrap_or(("minecraft", key));
    let file = crate::assets_root().parent()?.join(ns).join(path);
    let bytes = crate::platform::assets::read(&file)?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let image = if image.dimensions() == (CELL, CELL) {
        image
    } else {
        image::imageops::resize(&image, CELL, CELL, image::imageops::FilterType::Nearest)
    };
    Some(image.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_texture_keeps_its_slot_until_the_set_is_full() {
        let mut t = Textures::default();
        assert_eq!(t.request("m:a.png"), 0);
        assert_eq!(t.request("m:b.png"), 1);
        assert_eq!(t.request("m:a.png"), 0);
        assert!(!t.ready(0));
        for i in 2..SCREEN_TEXTURE_SLOTS {
            t.request(&format!("m:{i}.png"));
        }
        // Full: the next one starts the set over.
        assert_eq!(t.request("m:new.png"), 0);
        assert_eq!(t.slots.len(), 1);
    }
}
