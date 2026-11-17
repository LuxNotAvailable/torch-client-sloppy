//! Player actions and prediction: interaction, placement, riding and
//! vehicles, flight, commands, action sounds.

pub(crate) mod boat;
pub(crate) mod commands;
pub(crate) mod dialog;
pub(crate) mod entity_feed;
#[cfg(audio)]
pub(crate) mod entity_sounds;
pub(crate) mod event_particles;
pub(crate) mod flight;
pub(crate) mod gliding;
pub(crate) mod interaction;
pub(crate) mod inventory_bridge;
pub(crate) mod inventory_sort;
#[cfg(audio)]
pub(crate) mod landing;
pub(crate) mod looking;
pub(crate) mod minecart;
pub(crate) mod mob_effects;
#[cfg(audio)]
pub(crate) mod movement_sounds;
pub(crate) mod no_fall;
pub(crate) mod placement;
mod placement_rules;
pub(crate) mod redstone;
pub(crate) mod riding;
pub(crate) mod screen_warp;
pub(crate) mod survival;
#[cfg(audio)]
pub(crate) mod use_sounds;
