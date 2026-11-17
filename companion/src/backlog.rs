//! The overlay's recent lines, kept in `backlog.json` beside the settings, so that
//! restarting the companion does not empty the overlay.

use crate::markup::{ChatLine, Span};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Lines older than this when the companion starts are not brought back.
const KEEP: Duration = Duration::from_secs(10 * 60);

/// One overlay line as kept: when it arrived, in seconds since 1970, and its body in
/// English or explained, if that had arrived.
#[derive(Serialize, Deserialize)]
pub struct Kept {
    pub seen: u64,
    pub line: ChatLine,
    pub done: Option<Vec<Span>>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn path() -> std::path::PathBuf {
    crate::config::path().with_file_name("backlog.json")
}

/// The kept lines that are still recent, oldest first. None if there is no file or it
/// cannot be read, which is not worth a word: the overlay just starts empty.
pub fn load() -> Vec<Kept> {
    let kept: Vec<Kept> = std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    recent(kept, now())
}

fn recent(kept: Vec<Kept>, now: u64) -> Vec<Kept> {
    kept.into_iter().filter(|line| now.saturating_sub(line.seen) < KEEP.as_secs()).collect()
}

pub fn save(lines: &[Kept]) {
    if let Ok(text) = serde_json::to_string(lines) {
        let _ = std::fs::write(path(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recent_lines_come_back() {
        let line = |seen| Kept { seen, line: ChatLine { speaker: String::new(), color: [255; 3], prefix: vec![], body: vec![Span::plain("hi")], number: None }, done: None };
        let kept = recent(vec![line(1000), line(1500), line(1601)], 1000 + 601);
        assert_eq!(kept.iter().map(|line| line.seen).collect::<Vec<_>>(), [1500, 1601]);
        // What is written reads back the same.
        let text = serde_json::to_string(&kept).unwrap();
        let back: Vec<Kept> = serde_json::from_str(&text).unwrap();
        assert_eq!(back[0].line, kept[0].line);
    }
}
