//! What the addon sends through the pixel strip. The text of a frame holds one or more
//! lines separated by "\n", and each line is fields separated by tabs. The first field
//! says what the line is:
//!
//! - `0`, `1`, `T` or `E`, then prefix, body and `rrggbb`: a chat line. Prefix and body
//!   carry WoW color codes and together make the line as the game shows it, in that
//!   color. With `0` the line is shown as it is; with `1` its body is to be translated.
//!   `T` and `E` are a line the player clicked a link on, handled ahead of the others:
//!   [T] to translate it again, [?] to explain it word by word.
//! - `L`, then one record per chat window separated by byte 30, each
//!   "name, shown, left, top, width, height, font size" separated by byte 31.
//! - `S`, then a chat window's name and the mouse wheel steps it has had since login,
//!   up counted positive.
//! - `C`, then `reply` or `settings`: a click on one of the addon's buttons.
//! - `R`, then English the player typed after "/tr", to translate and copy.
//!
//! The addon side is in `addon/Translate`: Chat.lua (chat lines), Layout.lua (`L`, `S`)
//! and Buttons.lua (`C`, `R`).

use crate::layout::ChatWindow;
use crate::markup::{self, ChatLine};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Chat { line: ChatLine, handling: Handling },
    Windows(Vec<ChatWindow>),
    Wheel { name: String, count: i64 },
    Command(Command),
}

/// What to do with a chat line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handling {
    Show,
    Translate,
    /// Asked for by the player: translated before the lines already waiting, and marked.
    TranslateFirst,
    /// Asked for by the player: explained word by word, for learning Chinese.
    Explain,
}

impl Handling {
    /// Whether the player asked for this line and is waiting on it.
    pub fn asked(self) -> bool {
        matches!(self, Handling::TranslateFirst | Handling::Explain)
    }
}

/// Something the player asked for in the game.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    OpenReply,
    OpenSettings,
    Reply(String),
}

/// Reads one line of a frame. None for a line that is none of the kinds above or is
/// missing fields.
pub fn parse(line: &str) -> Option<Message> {
    let (kind, rest) = line.split_once('\t')?;
    match kind {
        "L" => Some(Message::Windows(rest.split('\u{1e}').filter_map(window).collect())),
        "S" => {
            let (name, count) = rest.split_once('\t')?;
            Some(Message::Wheel { name: name.to_string(), count: count.parse().ok()? })
        }
        "C" if rest == "reply" => Some(Message::Command(Command::OpenReply)),
        "C" if rest == "settings" => Some(Message::Command(Command::OpenSettings)),
        "R" if !rest.trim().is_empty() => Some(Message::Command(Command::Reply(rest.trim().to_string()))),
        "0" | "1" | "T" | "E" => {
            let handling = match kind {
                "0" => Handling::Show,
                "1" => Handling::Translate,
                "T" => Handling::TranslateFirst,
                _ => Handling::Explain,
            };
            let mut fields = rest.splitn(3, '\t');
            let (prefix, body) = (fields.next()?, fields.next()?);
            let color = fields.next().and_then(markup::rgb).unwrap_or([255; 3]);
            let mut open = Vec::new();
            let line = ChatLine { color, prefix: markup::parse(prefix, &mut open), body: markup::parse(body, &mut open) };
            Some(Message::Chat { line, handling })
        }
        _ => None,
    }
}

/// One chat window of a layout line. Records that do not parse are left out.
fn window(record: &str) -> Option<ChatWindow> {
    let mut fields = record.split('\u{1f}');
    let name = fields.next()?.to_string();
    let shown = fields.next()? == "1";
    let mut number = || fields.next()?.parse::<f32>().ok().filter(|n| n.is_finite());
    let rect = [number()?, number()?, number()?, number()?];
    Some(ChatWindow { name, shown, rect, font_size: number()? })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const LAYOUT: &str =
        "L\tGeneral\u{1f}1\u{1f}33\u{1f}760\u{1f}440\u{1f}180\u{1f}14\u{1e}翻譯\u{1f}0\u{1f}33\u{1f}760\u{1f}440\u{1f}180\u{1f}16.5\u{1e}bad\u{1f}1\u{1f}x";

    /// The windows of `LAYOUT`.
    pub fn windows() -> Vec<ChatWindow> {
        match parse(LAYOUT) {
            Some(Message::Windows(windows)) => windows,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_chat_lines() {
        let Some(Message::Chat { line, handling: Handling::Translate }) = parse("1\t[|cff3fc7ebLili|r]: \t缺 |cffa335ee[劍]|r\t40ff40") else {
            panic!("a chat line to translate");
        };
        assert_eq!(line.color, [0x40, 0xff, 0x40]);
        assert_eq!(line.prefix[1], markup::Span { text: "Lili".into(), color: Some([0x3f, 0xc7, 0xeb]) });
        assert_eq!(markup::plain(&line.body), "缺 [劍]");
        // Without a color the line is white, and 0 asks for no translation.
        let Some(Message::Chat { line, handling: Handling::Show }) = parse("0\t[Party] Lili: \t副本缺T 速来") else {
            panic!("a chat line shown as it is");
        };
        assert_eq!((markup::plain(&line.prefix), line.color), ("[Party] Lili: ".into(), [255; 3]));
        assert!(matches!(parse("T\t\t缺坦\tffffff"), Some(Message::Chat { handling: Handling::TranslateFirst, .. })));
        assert!(matches!(parse("E\t\t缺坦\tffffff"), Some(Message::Chat { handling: Handling::Explain, .. })));
        assert_eq!(parse("broken"), None);
        assert_eq!(parse("7\t\t缺坦"), None);
        assert_eq!(parse("1\tno body"), None);
        assert_eq!(parse("X\ta\tb"), None);
    }

    #[test]
    fn reads_the_windows_of_a_layout_line() {
        let windows = windows();
        assert_eq!(windows.len(), 2, "the broken record is left out");
        let general =
            ChatWindow { name: "General".into(), shown: true, rect: [33.0, 760.0, 440.0, 180.0], font_size: 14.0 };
        assert_eq!(windows[0], general);
        assert_eq!((windows[1].name.as_str(), windows[1].shown, windows[1].font_size), ("翻譯", false, 16.5));
        assert_eq!(parse("L\t"), Some(Message::Windows(Vec::new())));
    }

    #[test]
    fn reads_wheel_counts_and_commands() {
        assert_eq!(parse("S\t翻譯\t-4"), Some(Message::Wheel { name: "翻譯".into(), count: -4 }));
        assert_eq!(parse("S\t翻譯\tup"), None);
        assert_eq!(parse("C\treply"), Some(Message::Command(Command::OpenReply)));
        assert_eq!(parse("C\tsettings"), Some(Message::Command(Command::OpenSettings)));
        assert_eq!(parse("C\tother"), None);
        assert_eq!(parse("R\t sure, I can heal "), Some(Message::Command(Command::Reply("sure, I can heal".into()))));
        assert_eq!(parse("R\t "), None);
    }
}
