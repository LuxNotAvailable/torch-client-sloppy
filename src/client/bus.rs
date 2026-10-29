//! The two queues between the game and the rest of the client.
//!
//! - [`ClientRequest`]: what the GUI, input and effects ask the game to do
//!   (respawn, leave a bed, send a packet). The tick drains them in order.
//! - [`ClientEvent`]: what happened in the game that the GUI or effects act
//!   on (open a screen, a totem saved the player, a mod channel spoke). The
//!   frame drains them in order.
//!
//! These replace one-off mailbox fields and side statics, and are meant to
//! become the boundary client plugins see: one typed, ordered stream each
//! way. Custom payloads (mod channels, as NeoForge's networking uses) ride
//! them too, so a mod port talks to its server half through
//! [`ClientRequest::CustomPayload`] and [`ClientEvent::CustomPayload`].
//! Both queues are cleared when a session starts.

use std::sync::Mutex;

use azalea_core::position::BlockPos;

/// Something to do on the game's side, at the next tick.
#[derive(Debug)]
pub enum ClientRequest {
    Quit,
    Disconnect,
    ReloadChunks,
    LeaveBed,
    Respawn,
    RequestStats,
    /// A packet built elsewhere (the game-master block editors).
    Packet(Box<azalea_protocol::packets::game::ServerboundGamePacket>),
    /// Bytes for a server-side mod on a named channel.
    CustomPayload { channel: String, data: Vec<u8> },
}

/// Something that happened in the game, for the frame to act on.
pub enum ClientEvent {
    OpenCommandBlock(crate::session::CommandBlockOpen),
    OpenSignEditor(crate::session::SignEditRequest),
    /// The server opened a written book in this hand.
    OpenBook(crate::session::InteractionHand),
    /// This player started writing a book.
    EditBook(crate::session::BookEditRequest),
    ShowDialog(std::sync::Arc<crate::play::dialog::Dialog>),
    ClearDialog,
    /// The server wants signed chat this client could give: ask.
    SigningPrompt,
    /// Roll the end poem and credits (game event 4).
    ShowCredits,
    /// A game-master block was used: open its editor.
    OpenOpBlock(crate::gui::op_blocks::OpBlock, BlockPos),
    /// A Totem of Undying saved this player: the item flies at the screen.
    TotemActivated,
    /// The elder guardian's curse.
    ElderGuardian,
    /// Bytes from a server-side mod on a named channel the client itself
    /// doesn't handle.
    CustomPayload { channel: String, data: Vec<u8> },
}

/// An ordered queue: pushes from any thread, drains all at once.
pub struct Queue<T>(Mutex<Vec<T>>);

impl<T> Queue<T> {
    const fn new() -> Self {
        Queue(Mutex::new(Vec::new()))
    }

    pub fn push(&self, item: T) {
        self.0.lock().unwrap().push(item);
    }

    pub fn drain(&self) -> Vec<T> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

static REQUESTS: Queue<ClientRequest> = Queue::new();
static EVENTS: Queue<ClientEvent> = Queue::new();

pub fn request(request: ClientRequest) {
    REQUESTS.push(request);
}

pub fn emit(event: ClientEvent) {
    EVENTS.push(event);
}

pub fn take_requests() -> Vec<ClientRequest> {
    REQUESTS.drain()
}

pub fn take_events() -> Vec<ClientEvent> {
    EVENTS.drain()
}

/// A new session: nothing from the last one carries over.
pub fn reset() {
    REQUESTS.clear();
    EVENTS.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_drains_in_order_and_empties() {
        let q: Queue<u32> = Queue::new();
        q.push(1);
        q.push(2);
        assert_eq!(q.drain(), vec![1, 2]);
        assert!(q.drain().is_empty());
    }
}
