use crate::worker::{UiEvent, UiSender};
use std::sync::mpsc::Receiver;
use std::thread;

/// Owns the system clipboard and puts there the replies that the player pastes into the
/// game. It only ever writes: nothing the player copies is read.
pub fn spawn(set_requests: Receiver<String>, ui: UiSender) {
    thread::spawn(move || {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                ui.send(UiEvent::Status(format!("Clipboard unavailable: {e}")));
                return;
            }
        };
        for text in set_requests {
            if let Err(e) = clipboard.set_text(text) {
                ui.send(UiEvent::Status(format!("Could not write the clipboard: {e}")));
            }
        }
    });
}
