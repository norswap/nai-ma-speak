//! Explaining a Chinese chat line to someone learning the language: each word with how
//! it is read and what it means, then the whole line in English, then a note on its
//! grammar when there is something to say.

use crate::config::Reading;
use crate::markup::Span;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Explanation {
    words: Vec<Word>,
    translation: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Word {
    zh: String,
    #[serde(default)]
    reading: String,
    #[serde(default)]
    en: String,
}

/// The color of everything around the Chinese words and the translation.
const ASIDE: [u8; 3] = [170, 170, 170];

/// The instructions for a chat model.
pub fn prompt(reading: Reading) -> String {
    let system = match reading {
        Reading::Pinyin => "Hanyu Pinyin with tone marks (fùběn)",
        Reading::Bopomofo => "Zhuyin, also called Bopomofo, with its tone marks (ㄈㄨˋ ㄅㄣˇ)",
    };
    format!(
        "You help an English speaker learn Chinese from World of Warcraft in-game chat. \
The user sends one chat line. Reply with JSON only, in this shape: \
{{\"words\":[{{\"zh\":\"...\",\"reading\":\"...\",\"en\":\"...\"}}],\"translation\":\"...\",\"note\":\"...\"}}. \
\"words\" lists the line's words in order, split as a Chinese speaker would, keeping names and slang terms whole. \
\"zh\" is the word exactly as written in the line; \"reading\" is how it is read, in {system}; \
\"en\" is what it means here, in a word or a few. Every word written in Chinese characters gets a reading and a meaning, \
game slang included. A word in Latin letters or digits gets an empty \"reading\", and a meaning only if it is slang \
(T for tank, 5=1 for four of five players found). Leave out punctuation. \
\"translation\" is the whole line in natural, casual English, as a player would say it. \
\"note\" is one short sentence on a point of grammar or usage in this line that a learner would want to know and \
that you are sure of; an empty string when the line has nothing of the kind, which is often."
    )
}

/// Reads a model's answer. Models may put the JSON in a code block or say something
/// around it, so it is taken from the first brace to the last.
pub fn parse(answer: &str) -> Result<Explanation, String> {
    let json = answer.find('{').zip(answer.rfind('}')).filter(|(from, to)| from < to).map(|(from, to)| &answer[from..=to]);
    let json = json.ok_or("the model did not answer in the form asked for")?;
    serde_json::from_str(json).map_err(|e| format!("the model's answer could not be read: {e}"))
}

/// The explanation as the overlay shows it: each word followed by its reading and
/// meaning in parentheses, then the translation and the note on lines of their own.
pub fn spans(explanation: &Explanation) -> Vec<Span> {
    let aside = |text: String| Span { text, color: Some(ASIDE) };
    let mut spans = Vec::new();
    for word in &explanation.words {
        spans.push(Span::plain(word.zh.as_str()));
        // A model sometimes repeats the word as its own meaning; that says nothing.
        let said = |part: &&str| !part.is_empty() && *part != word.zh.trim();
        let details: Vec<&str> = [word.reading.trim(), word.en.trim()].into_iter().filter(said).collect();
        spans.push(aside(if details.is_empty() { " ".into() } else { format!(" ({}) ", details.join(", ")) }));
    }
    spans.push(Span::plain(format!("\n{}", explanation.translation.trim())));
    if !explanation.note.trim().is_empty() {
        spans.push(aside(format!("\n{}", explanation.note.trim())));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::plain;

    #[test]
    fn reads_an_answer_and_lays_it_out() {
        let answer = "```json\n{\"words\":[{\"zh\":\"副本\",\"reading\":\"fùběn\",\"en\":\"dungeon\"},{\"zh\":\"缺\",\"reading\":\"quē\",\"en\":\"lacking\"},\
{\"zh\":\"T\",\"reading\":\"\",\"en\":\"tank\"},{\"zh\":\"5\",\"en\":\"5\"}],\"translation\":\" Dungeon needs a tank \",\"note\":\"缺 before a role means the group lacks it.\"}\n```";
        let explanation = parse(answer).unwrap();
        assert_eq!(
            plain(&spans(&explanation)),
            "副本 (fùběn, dungeon) 缺 (quē, lacking) T (tank) 5 \nDungeon needs a tank\n缺 before a role means the group lacks it."
        );
        assert_eq!(spans(&explanation)[0].color, None);
        assert_eq!(spans(&explanation)[1].color, Some(ASIDE));
    }

    #[test]
    fn the_note_is_optional_and_a_bad_answer_is_an_error() {
        let explanation = parse("{\"words\":[],\"translation\":\"hi\"}").unwrap();
        assert_eq!(plain(&spans(&explanation)), "\nhi");
        assert!(parse("I cannot do that").is_err());
        assert!(parse("} {").is_err());
        assert!(parse("{\"words\": 3}").is_err());
    }

    #[test]
    fn asks_for_the_chosen_reading() {
        assert!(prompt(Reading::Pinyin).contains("Pinyin") && !prompt(Reading::Pinyin).contains("Zhuyin"));
        assert!(prompt(Reading::Bopomofo).contains("Zhuyin"));
    }
}
