use crate::config::Config;
use crate::translate::{translate, Direction};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

pub const TEST_LINE: &str = "大家好，今天一起打副本吗？";

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
    /// A chat line for the overlay. A Chinese line arrives without `english` and shows as
    /// it is until `Translated` fills it in, so lines keep the order they were said in.
    Line { id: u64, speaker: String, original: String, english: Option<String> },
    Translated { id: u64, english: String },
    PasteReady { id: u64, english: String },
    ReplyReady { english: String, chinese: String },
    TestResult { result: Result<String, String>, seconds: f32 },
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
/// "<id>\t<speaker>\t<text>". Lines without Chinese are shown as they are and never
/// reach the translator.
pub fn route_frame(frame: &str, next_id: &mut u64) -> Vec<(UiEvent, Option<Job>)> {
    let lines = frame.split('\n').filter_map(crate::strip::parse_payload);
    lines
        .map(|(speaker, text)| {
            let id = *next_id;
            *next_id += 1;
            if crate::text::has_chinese(&text) {
                let job = Job::Incoming { line: id, text: text.clone() };
                (UiEvent::Line { id, speaker, original: text, english: None }, Some(job))
            } else {
                (UiEvent::Line { id, speaker, original: String::new(), english: Some(text) }, None)
            }
        })
        .collect()
}

/// One thread per queue keeps the chat log from delaying click-to-translate.
pub fn spawn(config: Arc<Mutex<Config>>, jobs: Receiver<Job>, ui: UiSender, clipboard: Sender<String>) {
    thread::spawn(move || {
        let mut cache: HashMap<String, String> = HashMap::new();
        for job in jobs {
            let config = config.lock().unwrap().clone();
            if let Job::Test = job {
                let started = Instant::now();
                let result = translate(&config, TEST_LINE, Direction::ToEnglish);
                ui.send(UiEvent::TestResult { result, seconds: started.elapsed().as_secs_f32() });
                continue;
            }
            let (text, direction) = match &job {
                Job::Incoming { text, .. } | Job::Tagged { text, .. } => (text.clone(), Direction::ToEnglish),
                Job::Reply { text } => (text.clone(), Direction::ToChinese),
                Job::Test => unreachable!("handled above"),
            };
            let key = format!("{}|{text}", matches!(direction, Direction::ToEnglish));
            let result = match cache.get(&key) {
                Some(hit) => Ok(hit.clone()),
                None => translate(&config, &text, direction),
            };
            let translated = match result {
                Ok(t) => {
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
                    let _ = clipboard.send(translated.clone());
                    ui.send(UiEvent::ReplyReady { english: text, chinese: translated });
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
        let frame = "12\t[Say] Lili\t副本缺T 速来\n0\t[Say] Bob\tlf tank\nbroken\n0\t[Guild] Ann\thi\tall";
        let routed = route_frame(frame, &mut next_id);
        assert_eq!(next_id, 10);
        assert_eq!(routed.len(), 3);
        let (UiEvent::Line { id: 7, speaker, original, english: None }, Some(Job::Incoming { line: 7, text })) = &routed[0]
        else {
            panic!("the Chinese line shows untranslated and is queued");
        };
        assert_eq!((speaker.as_str(), original.as_str(), text.as_str()), ("[Say] Lili", "副本缺T 速来", "副本缺T 速来"));
        let (UiEvent::Line { id: 8, speaker, english: Some(english), .. }, None) = &routed[1] else {
            panic!("the English line is shown as it is");
        };
        assert_eq!((speaker.as_str(), english.as_str()), ("[Say] Bob", "lf tank"));
        let (UiEvent::Line { id: 9, english: Some(english), .. }, None) = &routed[2] else {
            panic!("the last line is shown as it is");
        };
        assert_eq!(english, "hi\tall");
    }
}
