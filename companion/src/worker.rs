use crate::config::Config;
use crate::markup::ChatLine;
use crate::translate::{translate, Direction};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

pub const TEST_LINE: &str = "大家好，今天一起打副本嗎？";

pub enum Job {
    /// A Chinese line read from the addon's pixel strip, already in the overlay as `line`.
    Incoming { line: u64, text: String },
    /// A line copied from the addon window as "TR#<id> <text>".
    Tagged { id: u64, text: String },
    /// English the player typed in the companion, to send as Chinese.
    Reply { text: String },
    /// The Test model button: a fixed line, never cached.
    Test,
}

pub enum UiEvent {
    /// A chat line for the overlay. A line with `chinese` set shows as it is until
    /// `Translated` brings its English, so lines keep the order they were said in.
    Line { id: u64, line: ChatLine, chinese: bool },
    Translated { id: u64, english: String },
    PasteReady { id: u64, english: String },
    /// A reply in Chinese. It is copied for pasting unless it is too long for WoW's chat
    /// box, which would cut it off.
    ReplyReady { english: String, chinese: String, copied: bool },
    TestResult { result: Result<String, String>, seconds: f32 },
    /// The game's chat windows, in screen points, whenever the addon reports them.
    ChatWindows(Vec<crate::layout::ChatWindow>),
    /// Pixel strip state: searching, or where it was found.
    Strip(String),
    /// Whether WoW has focus; None when the focused window cannot be read.
    WowView(crate::focus::WowView),
    Status(String),
}

/// Sends to the UI and wakes it so the event shows without mouse movement.
#[derive(Clone)]
pub struct UiSender {
    tx: Sender<UiEvent>,
    ctx: eframe::egui::Context,
}

impl UiSender {
    pub fn new(tx: Sender<UiEvent>, ctx: eframe::egui::Context) -> Self {
        Self { tx, ctx }
    }

    pub fn send(&self, event: UiEvent) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

/// Turns the text of one strip frame into overlay lines, numbered from `next_id`, and the
/// translation jobs for the Chinese ones. A frame holds one or more lines, each
/// "<id>\t<prefix>\t<body>\t<color>". Lines whose body has no Chinese are shown as they
/// are and never reach the translator, which gets the body without its color codes.
pub fn route_frame(frame: &str, next_id: &mut u64) -> Vec<(UiEvent, Option<Job>)> {
    let lines = frame.split('\n').filter_map(crate::strip::parse_payload);
    lines
        .map(|line| {
            let id = *next_id;
            *next_id += 1;
            let text = crate::markup::plain(&line.body);
            let chinese = crate::text::has_chinese(&text);
            let job = chinese.then(|| Job::Incoming { line: id, text: text.trim().to_string() });
            (UiEvent::Line { id, line, chinese }, job)
        })
        .collect()
}

/// One thread per queue keeps the chat log from delaying click-to-translate.
pub fn spawn(config: Arc<Mutex<Config>>, jobs: Receiver<Job>, ui: UiSender, clipboard: Sender<String>) {
    thread::spawn(move || {
        let mut cache: HashMap<String, String> = HashMap::new();
        for job in jobs {
            let config = config.lock().unwrap().clone();
            // Notes from the backend, such as a wait; cleared once the line gets through.
            let noted = std::cell::Cell::new(false);
            let notify = |note: String| {
                noted.set(true);
                ui.send(UiEvent::Status(note));
            };
            if let Job::Test = job {
                let started = Instant::now();
                let result = translate(&config, TEST_LINE, Direction::ToEnglish, &notify);
                ui.send(UiEvent::TestResult { result, seconds: started.elapsed().as_secs_f32() });
                continue;
            }
            let (text, direction) = match &job {
                Job::Incoming { text, .. } | Job::Tagged { text, .. } => (text.clone(), Direction::ToEnglish),
                Job::Reply { text } => (text.clone(), Direction::ToChinese),
                Job::Test => unreachable!("handled above"),
            };
            let key =
                format!("{:?}|{:?}|{}|{text}", config.backend, config.reply_variant, matches!(direction, Direction::ToEnglish));
            let result = match cache.get(&key) {
                Some(hit) => Ok(hit.clone()),
                None => translate(&config, &text, direction, &notify),
            };
            let translated = match result {
                Ok(t) => {
                    if noted.get() {
                        ui.send(UiEvent::Status("Ready".into()));
                    }
                    if cache.len() > 2000 {
                        cache.clear();
                    }
                    cache.insert(key, t.clone());
                    t
                }
                Err(e) => {
                    ui.send(UiEvent::Status(format!("Translation failed: {e}")));
                    continue;
                }
            };
            match job {
                Job::Incoming { line, .. } => ui.send(UiEvent::Translated { id: line, english: translated }),
                Job::Tagged { id, .. } => {
                    let _ = clipboard.send(format!("TR#{id} {translated}"));
                    ui.send(UiEvent::PasteReady { id, english: translated });
                }
                Job::Reply { text } => {
                    let copied = translated.len() <= crate::translate::WOW_CHAT_BYTES;
                    if copied {
                        let _ = clipboard.send(translated.clone());
                    }
                    ui.send(UiEvent::ReplyReady { english: text, chinese: translated, copied });
                }
                Job::Test => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_chinese_lines_go_to_the_translator() {
        let mut next_id = 7;
        let frame = "12\t[Say] [Lili]: \t副本缺T |cffa335ee[劍]|r \t40ff40\n0\t[小明] says: \tlf tank\tffffff\nbroken\n0\t\tBob has come online.";
        let routed = route_frame(frame, &mut next_id);
        assert_eq!(next_id, 10);
        assert_eq!(routed.len(), 3);
        let (UiEvent::Line { id: 7, line, chinese: true }, Some(Job::Incoming { line: 7, text })) = &routed[0] else {
            panic!("the Chinese line shows untranslated and is queued");
        };
        assert_eq!((crate::markup::plain(&line.prefix).as_str(), text.as_str()), ("[Say] [Lili]: ", "副本缺T [劍]"));
        assert_eq!(line.color, [0x40, 0xff, 0x40]);
        // A Chinese name alone does not send a line to the translator.
        assert!(matches!(&routed[1], (UiEvent::Line { id: 8, chinese: false, .. }, None)));
        let (UiEvent::Line { id: 9, line, chinese: false }, None) = &routed[2] else {
            panic!("the system line is shown as it is");
        };
        assert_eq!((line.prefix.len(), crate::markup::plain(&line.body).as_str()), (0, "Bob has come online."));
    }
}
