use crate::gui::ScreenCtx;
use crate::gui::painter::Painter;
use crate::gui::widgets::{Button, WIDGET_HEIGHT};
use crate::session::SharedMutex;
use std::sync::Arc;

const LEAVE_BED: &str = "Leave Bed";

const BUTTON_WIDTH: f32 = 200.0;

/// `InBedChatScreen`'s Leave Bed button, above the chat box; the chat itself
/// is drawn as in the chat screen.
pub fn draw(p: &mut Painter, ctx: &ScreenCtx, shared: &Arc<SharedMutex>) {
    let x = (ctx.vw / 2.0 - BUTTON_WIDTH / 2.0).floor();
    let y = (ctx.vh - 40.0).floor();

    if Button::new(x, y, BUTTON_WIDTH, WIDGET_HEIGHT, LEAVE_BED).draw(p, ctx) {
        crate::client::bus::request(crate::client::bus::ClientRequest::LeaveBed);
    }
}
