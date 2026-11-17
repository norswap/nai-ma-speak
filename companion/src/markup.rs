//! WoW's color codes in chat text: `|cAARRGGBB` starts a color, `|cnNAME:` starts a
//! named one and `|r` ends the innermost.

pub type Rgb = [u8; 3];

/// A stretch of text in one color; `None` is the color of the line it is in.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub text: String,
    pub color: Option<Rgb>,
}

/// A chat line as the game shows it: `prefix` then `body`, in `color` where the spans
/// name none. The body is the part that gets translated.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChatLine {
    /// The speaker's name as the game shows it; empty for a line nobody said.
    pub speaker: String,
    pub color: Rgb,
    pub prefix: Vec<Span>,
    pub body: Vec<Span>,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), color: None }
    }
}

/// Reads "rrggbb".
pub fn rgb(hex: &str) -> Option<Rgb> {
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Splits markup into spans. `open` holds the colors still open, innermost last, and is
/// carried from one piece of a line to the next. Named colors are not known here, so
/// their text keeps the color around it. A `|` that starts no color code is kept.
pub fn parse(markup: &str, open: &mut Vec<Option<Rgb>>) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut push = |text: &str, color: Option<Rgb>| match spans.last_mut() {
        _ if text.is_empty() => {}
        Some(last) if last.color == color => last.text.push_str(text),
        _ => spans.push(Span { text: text.to_string(), color }),
    };
    let mut rest = markup;
    while let Some(bar) = rest.find('|') {
        let current = open.last().copied().flatten();
        push(&rest[..bar], current);
        let code = &rest[bar..];
        let hex = code.get(2..10).filter(|hex| code.starts_with("|c") && hex.bytes().all(|b| b.is_ascii_hexdigit()));
        let named = code.strip_prefix("|cn").and_then(|name| name.find([':', '|'])).filter(|end| code.as_bytes()[3 + end] == b':');
        if let Some(hex) = hex {
            open.push(rgb(&hex[2..]));
            rest = &code[10..];
        } else if let Some(end) = named {
            open.push(current);
            rest = &code[3 + end + 1..];
        } else if let Some(after) = code.strip_prefix("|r") {
            open.pop();
            rest = after;
        } else {
            push("|", current);
            rest = &code[1..];
        }
    }
    push(rest, open.last().copied().flatten());
    spans
}

/// The text of the spans without their colors.
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// Byte ranges of the `[...]` groups in a text, in order. A group may hold others, as in
/// "[[31] Centaur Bounty]", and is then one group.
pub fn bracketed(text: &str) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let (mut start, mut depth) = (0, 0usize);
    for (at, c) in text.char_indices() {
        match c {
            '[' => {
                if depth == 0 {
                    start = at;
                }
                depth += 1;
            }
            ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups.push((start, at + 1));
                }
            }
            _ => {}
        }
    }
    groups
}

/// Gives a translation the colors of the line it translates, as far as they can be
/// placed: a line in a single color keeps it throughout, item and other links (colored
/// text in square brackets) keep their colors in order, and any other colored text keeps
/// its color where the translation repeats it unchanged.
pub fn carry_colors(translated: &str, original: &[Span]) -> Vec<Span> {
    let has_text = |span: &&Span| !span.text.trim().is_empty();
    let colored: Vec<&Span> = original.iter().filter(|span| span.color.is_some()).filter(has_text).collect();
    let Some(first) = colored.first() else {
        return vec![Span::plain(translated)];
    };
    let all_colored = original.iter().filter(has_text).all(|span| span.color == first.color);
    if all_colored {
        return vec![Span { text: translated.to_string(), color: first.color }];
    }

    let is_link = |span: &&&Span| span.text.trim().starts_with('[') && span.text.trim().ends_with(']');
    let links: Vec<&&Span> = colored.iter().filter(is_link).collect();
    let groups = bracketed(translated);
    let mut marks: Vec<(usize, usize, Option<Rgb>)> = Vec::new();
    let links_placed = !links.is_empty() && links.len() == groups.len();
    if links_placed {
        marks.extend(groups.iter().zip(&links).map(|(&(from, to), link)| (from, to, link.color)));
    }
    for span in colored.iter().filter(|span| !(links_placed && is_link(span))) {
        let wanted = span.text.trim();
        let free = |from: usize| !marks.iter().any(|&(a, b, _)| from < b && a < from + wanted.len());
        let found = translated.match_indices(wanted).map(|(from, _)| from).find(|&from| free(from));
        if let Some(from) = found.filter(|_| wanted.chars().count() >= 2) {
            marks.push((from, from + wanted.len(), span.color));
        }
    }
    marks.sort_by_key(|&(from, _, _)| from);

    let mut spans = Vec::new();
    let mut at = 0;
    for (from, to, color) in marks {
        if from > at {
            spans.push(Span::plain(&translated[at..from]));
        }
        spans.push(Span { text: translated[from..to].to_string(), color });
        at = to;
    }
    if at < translated.len() {
        spans.push(Span::plain(&translated[at..]));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    const PURPLE: Option<Rgb> = Some([0xa3, 0x35, 0xee]);
    const GREEN: Option<Rgb> = Some([0x33, 0xff, 0x99]);

    fn spans(markup: &str) -> Vec<Span> {
        parse(markup, &mut Vec::new())
    }

    fn span(text: &str, color: Option<Rgb>) -> Span {
        Span { text: text.into(), color }
    }

    #[test]
    fn reads_color_codes() {
        assert_eq!(spans("plain"), [span("plain", None)]);
        assert_eq!(
            spans("wts |cffa335ee[劍]|r 50g"),
            [span("wts ", None), span("[劍]", PURPLE), span(" 50g", None)]
        );
        // The innermost color wins and `|r` goes back to the one around it.
        assert_eq!(
            spans("|cff33ff99a|cffa335eeb|rc|rd"),
            [span("a", GREEN), span("b", PURPLE), span("c", GREEN), span("d", None)]
        );
        // A named color keeps the color around it; stray codes do no harm.
        assert_eq!(spans("|cff33ff99a|cnRED_FONT_COLOR:b|r|r|r c"), [span("ab", GREEN), span(" c", None)]);
        assert_eq!(spans("a|b |cffzz |c"), [span("a|b |cffzz |c", None)]);
        assert_eq!(plain(&spans("|cff33ff99DBM|r: pull")), "DBM: pull");
        assert_eq!(rgb("40ff40"), Some([0x40, 0xff, 0x40]));
        assert_eq!(rgb("40ff4"), None);
        assert_eq!(rgb("繁體"), None);
    }

    #[test]
    fn colors_carry_from_prefix_to_body() {
        let mut open = Vec::new();
        assert_eq!(parse("[|cff33ff99Bob", &mut open), [span("[", None), span("Bob", GREEN)]);
        assert_eq!(parse(" says|r: hi", &mut open), [span(" says", GREEN), span(": hi", None)]);
    }

    #[test]
    fn translations_keep_the_colors_that_can_be_placed() {
        assert_eq!(carry_colors("need a healer", &spans("缺補")), [span("need a healer", None)]);
        assert_eq!(carry_colors("pull now", &spans("|cff33ff99開怪|r ")), [span("pull now", GREEN)]);
        // Links keep their colors in order, whatever their names became.
        let links = spans("賣 |cffa335ee[劍]|r 和 |cff33ff99[盾]|r");
        assert_eq!(
            carry_colors("Selling [Sword] and [Shield]", &links),
            [span("Selling ", None), span("[Sword]", PURPLE), span(" and ", None), span("[Shield]", GREEN)]
        );
        // A link with brackets inside it, as some addons write quest levels, is one link.
        assert_eq!(
            carry_colors("[[31] Centaur Bounty] ears", &spans("|cffa335ee[[31] Centaur Bounty]|r耳朵")),
            [span("[[31] Centaur Bounty]", PURPLE), span(" ears", None)]
        );
        // A link the translation dropped: nothing is guessed.
        assert_eq!(carry_colors("Selling sword and [Shield]", &links), [span("Selling sword and [Shield]", None)]);
        // Other colored text keeps its color where it comes back unchanged.
        assert_eq!(
            carry_colors("DBM says pull", &spans("|cff33ff99DBM|r 說開怪")),
            [span("DBM", GREEN), span(" says pull", None)]
        );
    }
}
