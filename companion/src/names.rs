//! The Chinese names of players seen speaking, so the translator can be told which words
//! in a line are someone's name and should stay as written.

use std::sync::Mutex;

/// The names, oldest first. A busy server has thousands of speakers; beyond this many the
/// oldest are forgotten.
static KNOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());
const MOST: usize = 5000;
/// How many names at most are pointed out in one line.
const MOST_IN_LINE: usize = 8;

/// Notes a speaker's name. Names without Chinese need no pointing out, and a single
/// character is too often an ordinary word.
pub fn remember(name: &str) {
    let name = name.trim();
    if !crate::text::has_chinese(name) || name.chars().count() < 2 {
        return;
    }
    let mut known = KNOWN.lock().unwrap();
    if !known.iter().any(|seen| seen == name) {
        if known.len() >= MOST {
            known.remove(0);
        }
        known.push(name.to_string());
    }
}

/// The known names that occur in a line, longest first.
pub fn in_line(text: &str) -> Vec<String> {
    let known = KNOWN.lock().unwrap();
    let mut found: Vec<String> = known.iter().filter(|name| text.contains(name.as_str())).cloned().collect();
    found.sort_by_key(|name| std::cmp::Reverse(name.chars().count()));
    found.truncate(MOST_IN_LINE);
    found
}

/// What to tell a chat model about the names in a line; empty when there are none. A
/// name can be made of everyday words, so the model is left to judge which is meant.
pub fn hint(text: &str) -> String {
    let names = in_line(text);
    if names.is_empty() {
        return String::new();
    }
    format!(
        " Players on this server are named: {}. Where the line means the player, keep the name in Chinese characters \
exactly as written; where the same characters are plainly ordinary words, translate them.",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_out_the_known_names_in_a_line() {
        for name in ["測試黑熊", "測試黑熊王", " 測試小明 ", "Bob", "熊", "測試黑熊"] {
            remember(name);
        }
        assert_eq!(in_line("跟著測試黑熊王走，測試小明在哪"), ["測試黑熊王", "測試黑熊", "測試小明"]);
        assert_eq!(in_line("Bob 熊 來了"), Vec::<String>::new());
        assert!(hint("測試小明 來").contains("named: 測試小明."));
        assert_eq!(hint("沒有人"), "");
    }
}
