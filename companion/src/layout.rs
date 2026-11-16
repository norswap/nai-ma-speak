//! The chat windows of the game, as the addon reports them through the strip, so the
//! overlay can sit on one of them.

use crate::focus::Rect;

/// One chat window. Lengths are in game pixels from the top-left corner of the game's
/// picture when parsed, and in screen points after `on_screen`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatWindow {
    /// The name on the window's tab.
    pub name: String,
    /// False while another tab of the same dock is selected, or the interface is hidden.
    pub shown: bool,
    pub rect: Rect,
    pub font_size: f32,
    /// Opacity of the window's background as set in the game, 0 to 1.
    pub alpha: f32,
}

/// Parses a layout line: "L", a tab, then one record per window separated by byte 30,
/// each "name, shown, left, top, width, height, font size, alpha" separated by byte 31.
/// None for any other line; records that do not parse are left out.
pub fn parse(line: &str) -> Option<Vec<ChatWindow>> {
    let records = line.strip_prefix("L\t")?;
    let window = |record: &str| {
        let mut fields = record.split('\u{1f}');
        let name = fields.next()?.to_string();
        let shown = fields.next()? == "1";
        let mut number = || fields.next()?.parse::<f32>().ok().filter(|n| n.is_finite());
        let rect = [number()?, number()?, number()?, number()?];
        Some(ChatWindow { name, shown, rect, font_size: number()?, alpha: number()? })
    };
    Some(records.split('\u{1e}').filter_map(window).collect())
}

/// Parses a scroll line: "S", the name of a chat window and how many mouse wheel steps it
/// has had since login, up counted positive. None for any other line.
pub fn parse_scroll(line: &str) -> Option<(String, i64)> {
    let (name, count) = line.strip_prefix("S\t")?.split_once('\t')?;
    Some((name.to_string(), count.parse().ok()?))
}

/// Something the player asked for in the game: a click on one of the addon's buttons, or
/// English typed after "/tr" to translate and copy.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    OpenReply,
    OpenSettings,
    Reply(String),
}

/// Parses a command line: "C", a tab and the button's name, or "R", a tab and the English.
pub fn parse_command(line: &str) -> Option<Command> {
    match line.split_once('\t')? {
        ("C", "reply") => Some(Command::OpenReply),
        ("C", "settings") => Some(Command::OpenSettings),
        ("R", english) if !english.trim().is_empty() => Some(Command::Reply(english.trim().to_string())),
        _ => None,
    }
}

/// Where the game's picture is on screen, worked out from the strip, which the addon
/// draws at the picture's top-left corner with blocks of 4 game pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picture {
    /// The monitor's top-left corner in screen points.
    pub monitor: (f32, f32),
    /// Captured pixels per screen point on that monitor.
    pub scale: f32,
    /// The strip's top-left corner in captured pixels, and its block size.
    pub strip: (f32, f32),
    pub block: f32,
}

impl Picture {
    /// Converts a window from game pixels to screen points.
    pub fn on_screen(&self, window: &ChatWindow) -> ChatWindow {
        let captured = self.block / 4.0; // captured pixels per game pixel
        let points = captured / self.scale;
        let [x, y, width, height] = window.rect;
        ChatWindow {
            rect: [
                self.monitor.0 + (self.strip.0 + x * captured) / self.scale,
                self.monitor.1 + (self.strip.1 + y * captured) / self.scale,
                width * points,
                height * points,
            ],
            font_size: window.font_size * points,
            ..window.clone()
        }
    }
}

/// The window the overlay is anchored to: the one whose tab is named `tab`, ignoring case
/// and surrounding spaces. None when no name is set or no window has it.
pub fn anchor<'a>(windows: &'a [ChatWindow], tab: &str) -> Option<&'a ChatWindow> {
    let tab = tab.trim();
    if tab.is_empty() {
        return None;
    }
    windows.iter().find(|window| window.name.trim().to_lowercase() == tab.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str =
        "L\tGeneral\u{1f}1\u{1f}33\u{1f}760\u{1f}440\u{1f}180\u{1f}14\u{1f}0.25\u{1e}翻譯\u{1f}0\u{1f}33\u{1f}760\u{1f}440\u{1f}180\u{1f}16.5\u{1f}0\u{1e}bad\u{1f}1\u{1f}x";

    #[test]
    fn parses_the_windows_of_a_layout_line() {
        let windows = parse(LINE).unwrap();
        assert_eq!(windows.len(), 2, "the broken record is left out");
        let general =
            ChatWindow { name: "General".into(), shown: true, rect: [33.0, 760.0, 440.0, 180.0], font_size: 14.0, alpha: 0.25 };
        assert_eq!(windows[0], general);
        assert_eq!((windows[1].name.as_str(), windows[1].shown, windows[1].font_size), ("翻譯", false, 16.5));
        assert_eq!(parse("12\t[Say] Lili\t你好"), None);
        assert_eq!(parse("L\t"), Some(Vec::new()));
        assert_eq!(parse_scroll("S\t翻譯\t-4"), Some(("翻譯".into(), -4)));
        assert_eq!(parse_scroll("S\t翻譯\tup"), None);
        assert_eq!(parse_scroll("3\ta\tb"), None);
        assert_eq!(parse_command("C\treply"), Some(Command::OpenReply));
        assert_eq!(parse_command("C\tsettings"), Some(Command::OpenSettings));
        assert_eq!(parse_command("R\t sure, I can heal "), Some(Command::Reply("sure, I can heal".into())));
        assert_eq!(parse_command("R\t "), None);
        assert_eq!(parse_command("C\tother"), None);
        assert_eq!(parse_command("3\ta\tb"), None);
    }

    #[test]
    fn places_a_window_on_screen() {
        let window = &parse(LINE).unwrap()[0];
        // A plain display: the game fills a window whose picture starts at 76,30.
        let plain = Picture { monitor: (0.0, 0.0), scale: 1.0, strip: (76.0, 30.0), block: 4.0 };
        assert_eq!(plain.on_screen(window).rect, [109.0, 790.0, 440.0, 180.0]);
        assert_eq!(plain.on_screen(window).font_size, 14.0);
        // A HiDPI display to the right: captures have two pixels per point.
        let hidpi = Picture { monitor: (1920.0, -554.0), scale: 2.0, strip: (0.0, 66.0), block: 4.0 };
        assert_eq!(hidpi.on_screen(window).rect, [1936.5, -141.0, 220.0, 90.0]);
        assert_eq!(hidpi.on_screen(window).font_size, 7.0);
    }

    #[test]
    fn finds_the_anchor_by_tab_name() {
        let windows = parse(LINE).unwrap();
        assert_eq!(anchor(&windows, " general ").map(|w| w.shown), Some(true));
        assert_eq!(anchor(&windows, "翻譯").map(|w| w.shown), Some(false));
        assert_eq!(anchor(&windows, "Trade"), None);
        assert_eq!(anchor(&windows, "  "), None);
    }
}
