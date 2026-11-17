use crate::backends;
use crate::config::{Backend, Config, ReplyVariant};
use ferrous_opencc::config::BuiltinConfig;
use ferrous_opencc::OpenCC;
use std::sync::OnceLock;

const TO_ENGLISH: &str = "You translate World of Warcraft in-game chat from Chinese into natural, casual English, the way an English-speaking player would write it. \
The server has Taiwanese players typing Traditional Chinese and mainland players typing Simplified Chinese, so slang can come from either side: \
Taiwanese internet slang, Zhuyin shorthand and Hokkien-derived words, or mainland gaming slang. \
Keep player names, numbers and English words as they are. In chat, 1 usually means yes/ready. \
Text in [square brackets] is a link to an item, quest or spell: always keep it, brackets included, wherever it stands in the line, \
with its name in English. \
Reply with the translation only: no quotes, notes or romanization.";

const TO_TAIWANESE: &str = "Translate the user's World of Warcraft chat message into casual Traditional Chinese (繁體中文) with Taiwanese vocabulary, \
the way a Taiwanese player would type it in game chat. Never use Simplified characters. \
Keep player names, numbers and item names as they are. Keep it under 80 characters. Reply with the translation only.";

const TO_SIMPLIFIED: &str = "Translate the user's World of Warcraft chat message into casual Simplified Chinese, the way a mainland Chinese player would type it in game chat. \
Keep player names, numbers and item names as they are. Keep it under 80 characters. Reply with the translation only.";

/// WoW's chat box takes 255 bytes; a Chinese character is 3 of them.
pub const WOW_CHAT_BYTES: usize = 255;

#[derive(Clone, Copy, Debug)]
pub enum Task {
    ToEnglish,
    ToChinese,
    /// Explain a Chinese line word by word; see `explain`.
    Explain(crate::config::Reading),
}

/// Chinese WoW slang as `zh=en` lines: mainland terms in Simplified, and Taiwanese terms
/// the mainland list lacks in Traditional. Only the terms found in a line are sent with
/// it, so long lists cost no extra tokens.
const SLANG_ZH_CN: &str = include_str!("slang/zh_cn.txt");
const SLANG_ZH_TW: &str = include_str!("slang/zh_tw.txt");

/// The `zh=en` pairs of a slang file; blank lines and lines starting with # are skipped.
fn slang_pairs(file: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    file.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).filter_map(|line| line.split_once('='))
}

fn converter(config: BuiltinConfig) -> OpenCC {
    OpenCC::from_config(config).expect("the OpenCC dictionaries are built into the program")
}

/// Rewrites Traditional Chinese in Simplified characters; Simplified text stays as it is.
pub fn to_simplified(text: &str) -> String {
    static T2S: OnceLock<OpenCC> = OnceLock::new();
    T2S.get_or_init(|| converter(BuiltinConfig::T2s)).convert(text)
}

/// Rewrites Simplified Chinese in Traditional characters with Taiwanese wording
/// (软件 becomes 軟體); text already written that way stays as it is.
pub fn to_taiwanese(text: &str) -> String {
    static S2TWP: OnceLock<OpenCC> = OnceLock::new();
    S2TWP.get_or_init(|| converter(BuiltinConfig::S2twp)).convert(text)
}

/// Both slang lists keyed by the term in Simplified characters and lower-case Latin
/// letters, so one lookup serves lines in either script and either case. A term in both
/// lists keeps its mainland entry.
fn slang() -> &'static [(String, &'static str)] {
    static SLANG: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    SLANG.get_or_init(|| {
        let mut terms: Vec<(String, &'static str)> = Vec::new();
        for (zh, en) in slang_pairs(SLANG_ZH_CN).chain(slang_pairs(SLANG_ZH_TW)) {
            let key = to_simplified(zh).to_ascii_lowercase();
            if !terms.iter().any(|(known, _)| *known == key) {
                terms.push((key, en));
            }
        }
        terms
    })
}

/// Where a slang term stands in a line, if it does. Chinese has no spaces, so a term may
/// sit anywhere. Latin letters and digits do form words: where a term starts or ends
/// with one, the line must not carry on with another there, or DD would be found in
/// "adds" and 缺T in 缺Tank.
fn find_term(line: &str, term: &str) -> Option<usize> {
    let word_char = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    line.match_indices(term).map(|(start, _)| start).find(|&start| {
        let (before, after) = (line[..start].chars().next_back(), line[start + term.len()..].chars().next());
        !(word_char(term.chars().next()) && word_char(before)) && !(word_char(term.chars().next_back()) && word_char(after))
    })
}

/// The slang terms in a line, longest first, skipping any term that is
/// part of a longer match (奶 inside 奶骑). At most 12 to keep prompts short.
/// The line may be in Traditional or Simplified characters: it is matched through a
/// Simplified copy, and each term is returned the way the line writes it.
pub fn slang_in(text: &str) -> Vec<(String, &'static str)> {
    let simplified = to_simplified(text).to_ascii_lowercase();
    // Conversion is character for character almost always; when it is, a match can be
    // cut out of the original line.
    let aligned = simplified.chars().count() == text.chars().count();
    let as_written = |key: &str, start: usize| {
        if !aligned {
            return key.to_string();
        }
        let skipped = simplified[..start].chars().count();
        text.chars().skip(skipped).take(key.chars().count()).collect()
    };
    let mut found: Vec<(&(String, &str), usize)> =
        slang().iter().filter_map(|term| Some((term, find_term(&simplified, &term.0)?))).collect();
    found.sort_by_key(|((key, _), _)| std::cmp::Reverse(key.chars().count()));
    let mut kept: Vec<(&(String, &str), usize)> = Vec::new();
    for term in found {
        if !kept.iter().any(|((longer, _), _)| longer.contains(term.0 .0.as_str())) {
            kept.push(term);
        }
    }
    kept.truncate(12);
    kept.into_iter().map(|((key, en), start)| (as_written(key, start), *en)).collect()
}

/// The instructions for a chat model: how to translate, and for a Chinese line the slang
/// found in it.
pub fn system_prompt(text: &str, task: Task, variant: ReplyVariant) -> String {
    match task {
        Task::ToEnglish => {
            let slang: Vec<String> = slang_in(text).iter().map(|(zh, en)| format!("{zh} = {en}")).collect();
            if slang.is_empty() {
                TO_ENGLISH.to_string()
            } else {
                format!("{TO_ENGLISH} Slang in this line: {}.", slang.join(", "))
            }
        }
        Task::ToChinese => match variant {
            ReplyVariant::Taiwan => TO_TAIWANESE.to_string(),
            ReplyVariant::Simplified => TO_SIMPLIFIED.to_string(),
        },
        Task::Explain(reading) => {
            let slang: Vec<String> = slang_in(text).iter().map(|(zh, en)| format!("{zh} = {en}")).collect();
            let prompt = crate::explain::prompt(reading);
            if slang.is_empty() {
                prompt
            } else {
                format!("{prompt} Slang in this line: {}.", slang.join(", "))
            }
        }
    }
}

/// Translates or explains one line with the backend chosen in the settings. `notify` receives short
/// progress notes for the status line, such as a wait imposed by the service.
pub fn translate(config: &Config, text: &str, task: Task, notify: &dyn Fn(String)) -> Result<String, String> {
    let raw = match config.backend {
        Backend::GoogleFree => backends::google::translate(config, text, task, notify),
        Backend::Claude => backends::claude::translate(config, text, task, notify),
        Backend::OpenAiCompatible => backends::openai::translate(config, text, task),
    }?;
    if let Task::Explain(_) = task {
        // Structured, and read by `explain::parse`, not a line of chat to tidy.
        return Ok(raw);
    }
    let reply = clean_reply(&raw);
    if reply.is_empty() {
        return Err(format!("{} gave an empty translation", config.backend.label()));
    }
    let reply = match task {
        Task::ToEnglish => restore_links(text, reply),
        _ => reply,
    };
    Ok(finish(reply, task, config.reply_variant))
}

/// Puts back links that a translation left out. A model asked to keep what is in square
/// brackets still drops a link now and then, mostly one already in English at the start
/// of a Chinese line, which it folds into "that quest". When the translation has fewer
/// links than the line, as many of the line's links as are missing are added, from
/// those the translation does not have word for word: at the front if the line began
/// with one, else at the end.
fn restore_links(original: &str, translated: String) -> String {
    let original = original.trim();
    let links = crate::markup::bracketed(original);
    let missing = links.len().saturating_sub(crate::markup::bracketed(&translated).len());
    let mut absent: Vec<(usize, usize)> =
        links.into_iter().filter(|&(from, to)| !translated.contains(&original[from..to])).collect();
    // A link in Chinese may be there under its English name; one without Chinese cannot.
    absent.sort_by_key(|&(from, to)| crate::text::has_chinese(&original[from..to]));
    let (mut front, mut back) = (String::new(), String::new());
    for (from, to) in absent.into_iter().take(missing) {
        if from == 0 {
            front = format!("{} ", &original[from..to]);
        } else {
            back.push_str(&format!(" {}", &original[from..to]));
        }
    }
    format!("{front}{translated}{back}")
}

/// The last step for every translation. A Taiwanese reply is passed through the
/// Simplified-to-Taiwanese conversion, so no Simplified character reaches the clipboard
/// even when the model drifts.
fn finish(reply: String, task: Task, variant: ReplyVariant) -> String {
    match (task, variant) {
        (Task::ToChinese, ReplyVariant::Taiwan) => to_taiwanese(&reply),
        _ => reply,
    }
}

/// Models sometimes wrap the answer in quotes or add a thinking block; drop both.
fn clean_reply(raw: &str) -> String {
    let text = match raw.rfind("</think>") {
        Some(end) => &raw[end + "</think>".len()..],
        None => raw,
    };
    let text = text.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    // WoW chat is single-line.
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_a_translation_left_out_are_put_back() {
        let restore = |original: &str, translated: &str| restore_links(original, translated.to_string());
        assert_eq!(
            restore("[[31] Centaur Bounty] 換我打你那個任務=.=", "Let me do that quest for you =.="),
            "[[31] Centaur Bounty] Let me do that quest for you =.="
        );
        assert_eq!(restore("賣 [劍] 和 [Shield of Truth]", "Selling [Sword]"), "Selling [Sword] [Shield of Truth]");
        // Nothing is added to a translation that has the line's links, renamed or not.
        assert_eq!(restore("[科多獸圍捕] 我也要做", "I want to do [Kodo Roundup] too"), "I want to do [Kodo Roundup] too");
        assert_eq!(restore("缺補", "need a healer"), "need a healer");
    }

    fn slang_of(text: &str) -> Vec<(String, &'static str)> {
        slang_in(text)
    }

    fn term(zh: &str, en: &'static str) -> (String, &'static str) {
        (zh.to_string(), en)
    }

    #[test]
    fn quest_words_use_wow_meanings() {
        let found = slang_of("我不太熟悉任务路线，谁能带我做任务");
        assert!(found.contains(&term("做任务", "questing")));
        assert!(found.contains(&term("任务路线", "quest route")));
        assert_eq!(slang_of("还差一个任务"), vec![term("任务", "quest")]);
    }

    #[test]
    fn converts_between_scripts() {
        assert_eq!(to_simplified("副本缺補 密我"), "副本缺补 密我");
        assert_eq!(to_simplified("黑上缺奶 金团"), "黑上缺奶 金团");
        assert_eq!(to_taiwanese("软件"), "軟體");
        assert_eq!(to_taiwanese("需要治疗，组我"), "需要治療，組我");
        let taiwanese = "大家好，今天一起打副本嗎？";
        assert_eq!(to_taiwanese(taiwanese), taiwanese, "Traditional text is left alone");
    }

    #[test]
    fn latin_terms_match_whole_words_only() {
        assert_eq!(slang_of("缺DD 速來"), vec![term("速來", "come quick"), term("DD", "DPS")]);
        assert_eq!(slang_of("來個dd"), vec![term("dd", "DPS")]);
        assert_eq!(slang_of("2 adds 來了"), vec![]);
        assert_eq!(slang_of("DDoS"), vec![]);
        assert_eq!(slang_of("缺T 來"), vec![term("缺T", "need a tank")]);
        assert_eq!(slang_of("缺Tank"), vec![]);
        assert_eq!(slang_of("PM我"), vec![], "M我 does not start inside another word");
        assert_eq!(slang_of("要的M我"), vec![term("M我", "whisper me")]);
        assert_eq!(find_term("dd add dd", "dd"), Some(0));
        assert_eq!(find_term("add dd", "dd"), Some(4));
    }

    #[test]
    fn replies_come_out_in_the_chosen_chinese() {
        let drifted = "需要治疗，组我".to_string();
        assert_eq!(finish(drifted.clone(), Task::ToChinese, ReplyVariant::Taiwan), "需要治療，組我");
        assert_eq!(finish(drifted.clone(), Task::ToChinese, ReplyVariant::Simplified), drifted);
        assert_eq!(finish("LF healer 治疗".into(), Task::ToEnglish, ReplyVariant::Taiwan), "LF healer 治疗");

        assert!(system_prompt("need a healer", Task::ToChinese, ReplyVariant::Taiwan).contains("繁體中文"));
        assert!(system_prompt("need a healer", Task::ToChinese, ReplyVariant::Taiwan).contains("Never use Simplified"));
        assert!(system_prompt("need a healer", Task::ToChinese, ReplyVariant::Simplified).contains("Simplified Chinese"));
        let hinted = system_prompt("副本缺補", Task::ToEnglish, ReplyVariant::Taiwan);
        assert!(hinted.ends_with("Slang in this line: 副本 = dungeon, 缺補 = need a healer."), "{hinted}");
        assert!(!system_prompt("你好", Task::ToEnglish, ReplyVariant::Taiwan).contains("Slang in this line"));
    }

    #[test]
    fn slang_matches_traditional_and_simplified_lines() {
        // Traditional: 副本 comes from the mainland list, 缺補 from the Taiwan list.
        let found = slang_of("副本缺補 密我");
        assert!(found.contains(&term("副本", "dungeon")), "{found:?}");
        assert!(found.contains(&term("缺補", "need a healer")), "{found:?}");
        assert!(found.contains(&term("密我", "whisper me")), "{found:?}");
        // A mainland term typed in Traditional is found, and shown as typed.
        assert_eq!(slang_of("奶騎來"), vec![term("奶騎", "holy paladin")]);
        // Simplified still works, with the mainland additions.
        let found = slang_of("黑上缺奶 金团");
        for wanted in [term("黑上", "UBRS"), term("缺奶", "need a healer"), term("金团", "GDKP")] {
            assert!(found.contains(&wanted), "{found:?}");
        }
        // Longer Taiwan terms win over the short mainland ones inside them.
        assert_eq!(slang_of("惡魔獵人"), vec![term("惡魔獵人", "Demon Hunter")]);
        assert_eq!(slang_of("評級戰場"), vec![term("評級戰場", "rated battleground")]);
    }

    #[test]
    fn cleans_reply() {
        assert_eq!(clean_reply("\"Need a healer\"\n"), "Need a healer");
        assert_eq!(clean_reply("<think>hmm</think>\nLF tank"), "LF tank");
    }

    #[test]
    fn slang_prefers_longest_terms() {
        assert_eq!(slang_in("奶骑来"), vec![term("奶骑", "holy paladin")]);
        assert_eq!(slang_in("hello"), vec![]);
        for file in [SLANG_ZH_CN, SLANG_ZH_TW] {
            let lines = file.lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#'));
            assert_eq!(lines.count(), slang_pairs(file).count(), "every line that is not a comment is a zh=en pair");
            assert!(slang_pairs(file).all(|(zh, en)| !zh.is_empty() && !en.is_empty()));
        }
        assert_eq!(slang_pairs(SLANG_ZH_CN).count(), 221);
        assert_eq!(slang_pairs(SLANG_ZH_TW).count(), 89);
    }
}
