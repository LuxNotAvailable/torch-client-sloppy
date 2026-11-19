//! The app: Bevy setup (`systems::run`), input, the per-frame view of the
//! game (`frame_view`), app state and screenshots.

pub mod frame_view;
pub mod input;
pub mod intent;
pub(crate) mod screenshot;
pub mod systems;

pub use frame_view::{FrameView, FrameViewSystems};
pub use systems::{Shared, run};

#[derive(bevy::prelude::States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Menu,
    InGame,
}
