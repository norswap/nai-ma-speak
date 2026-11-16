use crate::config::{Config, ReplyVariant};
use serde_json::{json, Value};
use ferrous_opencc::config::BuiltinConfig;
use ferrous_opencc::OpenCC;
use std::sync::OnceLock;
use std::time::Duration;

const TO_ENGLISH: &str = "You translate World of Warcraft in-game chat from Chinese into natural, casual English, the way an English-speaking player would write it. \
The server has Taiwanese players typing Traditional Chinese and mainland players typing Simplified Chinese, so slang can come from either side: \
Taiwanese internet slang, Zhuyin shorthand and Hokkien-derived words, or mainland gaming slang. \
Keep player names, numbers, item names in brackets, and English words as they are. In chat, 1 usually means yes/ready. \
Reply with the translation only: no quotes, notes or romanization.";

const TO_TAIWANESE: &str = "Translate the user's World of Warcraft chat message into casual Traditional Chinese (繁體中文) with Taiwanese vocabulary, \
the way a Taiwanese player would type it in game chat. Never use Simplified characters. \
Keep player names, numbers and item names as they are. Keep it under 80 characters. Reply with the translation only.";

const TO_SIMPLIFIED: &str = "Translate the user's World of Warcraft chat message into casual Simplified Chinese, the way a mainland Chinese player would type it in game chat. \
Keep player names, numbers and item names as they are. Keep it under 80 characters. Reply with the translation only.";

/// WoW's chat box takes 255 bytes; a Chinese character is 3 of them.
pub const WOW_CHAT_BYTES: usize = 255;

#[derive(Clone, Copy)]
pub enum Direction {
    ToEnglish,
    ToChinese,
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
fn slang_in(text: &str) -> Vec<(String, &'static str)> {
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

/// Tencent's HY-MT models want their own prompt and no system message.
fn is_hy_mt(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("hy-mt") || m.contains("hunyuan-mt")
}

/// Builds the request body for either prompt style.
fn request_body(model: &str, text: &str, direction: Direction, variant: ReplyVariant) -> Value {
    if !is_hy_mt(model) {
        let system = match direction {
            Direction::ToEnglish => {
                let slang: Vec<String> = slang_in(text).iter().map(|(zh, en)| format!("{zh} = {en}")).collect();
                if slang.is_empty() {
                    TO_ENGLISH.to_string()
                } else {
                    format!("{TO_ENGLISH} Slang in this line: {}.", slang.join(", "))
                }
            }
            Direction::ToChinese => match variant {
                ReplyVariant::Taiwan => TO_TAIWANESE.to_string(),
                ReplyVariant::Simplified => TO_SIMPLIFIED.to_string(),
            },
        };
        return json!({
            "model": model,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": text },
            ],
        });
    }

    // Templates and sampling settings from the HY-MT1.5 model card.
    let target = match direction {
        Direction::ToEnglish => "英语",
        // The model card's names for Chinese and Traditional Chinese.
        Direction::ToChinese => match variant {
            ReplyVariant::Taiwan => "繁体中文",
            ReplyVariant::Simplified => "中文",
        },
    };
    let terms: Vec<String> = match direction {
        Direction::ToEnglish => slang_in(text)
            .iter()
            .map(|(zh, en)| format!("{zh} 翻译成 {en}"))
            .collect(),
        Direction::ToChinese => Vec::new(),
    };
    let prompt = if terms.is_empty() {
        format!("将以下文本翻译为{target}，注意只需要输出翻译后的结果，不要额外解释：\n\n{text}")
    } else {
        format!(
            "参考下面的翻译：\n{}\n\n将以下文本翻译为{target}，注意只需要输出翻译后的结果，不要额外解释：\n{text}",
            terms.join("\n")
        )
    };
    json!({
        "model": model,
        "temperature": 0.7,
        "top_p": 0.6,
        // Read by llama.cpp and Ollama; other servers ignore them.
        "top_k": 20,
        "repeat_penalty": 1.05,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

/// Local servers (LM Studio, Ollama) serve the API under /v1; a bare
/// `http://host:port` gets it added so a missing /v1 is not a silent failure.
fn completions_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let after_scheme = base.split_once("://").map_or(base, |(_, rest)| rest);
    if after_scheme.contains('/') {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

/// Sends one message to an OpenAI-compatible /chat/completions endpoint.
pub fn translate(config: &Config, text: &str, direction: Direction) -> Result<String, String> {
    let url = completions_url(&config.api_base);
    let body = request_body(&config.model, text, direction, config.reply_variant);

    let mut request = ureq::post(&url).timeout(Duration::from_secs(60));
    if !config.api_key.is_empty() {
        request = request.set("Authorization", &format!("Bearer {}", config.api_key));
    }
    let response: Value = match request.send_json(body) {
        Ok(r) => r.into_json().map_err(|e| format!("bad JSON from {url}: {e}"))?,
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            return Err(format!("{url} returned {code}: {}", detail.chars().take(200).collect::<String>()));
        }
        Err(e) => return Err(format!("could not reach {url}: {e}")),
    };

    response["choices"][0]["message"]["content"]
        .as_str()
        .map(clean_reply)
        .filter(|s| !s.is_empty())
        .map(|reply| finish(reply, direction, config.reply_variant))
        .ok_or_else(|| {
            let detail = response.to_string();
            format!("{url} gave no translation: {}", detail.chars().take(200).collect::<String>())
        })
}

/// The last step for every translation. A Taiwanese reply is passed through the
/// Simplified-to-Taiwanese conversion, so no Simplified character reaches the clipboard
/// even when the model drifts.
fn finish(reply: String, direction: Direction, variant: ReplyVariant) -> String {
    match (direction, variant) {
        (Direction::ToChinese, ReplyVariant::Taiwan) => to_taiwanese(&reply),
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
    use std::io::{Read, Write};
    use std::net::TcpListener;

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
        assert_eq!(finish(drifted.clone(), Direction::ToChinese, ReplyVariant::Taiwan), "需要治療，組我");
        assert_eq!(finish(drifted.clone(), Direction::ToChinese, ReplyVariant::Simplified), drifted);
        assert_eq!(finish("LF healer 治疗".into(), Direction::ToEnglish, ReplyVariant::Taiwan), "LF healer 治疗");

        let system = |model: &str, variant| {
            let body = request_body(model, "need a healer", Direction::ToChinese, variant);
            body["messages"][0]["content"].as_str().unwrap().to_string()
        };
        assert!(system("gpt-4o-mini", ReplyVariant::Taiwan).contains("繁體中文"));
        assert!(system("gpt-4o-mini", ReplyVariant::Taiwan).contains("Never use Simplified"));
        assert!(system("gpt-4o-mini", ReplyVariant::Simplified).contains("Simplified Chinese"));
        assert!(system("hy-mt1.5", ReplyVariant::Taiwan).contains("翻译为繁体中文"));
        assert!(system("hy-mt1.5", ReplyVariant::Simplified).contains("翻译为中文"));
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

    /// Reads one whole HTTP request, headers and body. The client may send them in separate
    /// writes; replying and closing after only the first one resets the connection under it.
    fn read_request(stream: &mut impl Read) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let text = String::from_utf8_lossy(&data);
            if let Some(head_end) = text.find("\r\n\r\n") {
                let length = text[..head_end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if data.len() >= head_end + 4 + length {
                    return text.into_owned();
                }
            }
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "connection closed before the request was complete");
            data.extend_from_slice(&buf[..n]);
        }
    }

    #[test]
    fn calls_openai_compatible_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"LF healer for the dungeon"}}]}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            request
        });
        let config = Config {
            api_base: format!("http://127.0.0.1:{port}/v1/"),
            api_key: "secret".into(),
            ..Config::default()
        };
        let english = translate(&config, "副本缺奶", Direction::ToEnglish).unwrap();
        assert_eq!(english, "LF healer for the dungeon");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions"));
        assert!(request.contains("Bearer secret"));
    }

    #[test]
    fn adds_v1_to_bare_host() {
        assert_eq!(completions_url("http://localhost:1234"), "http://localhost:1234/v1/chat/completions");
        assert_eq!(completions_url("http://localhost:1234/"), "http://localhost:1234/v1/chat/completions");
        assert_eq!(completions_url("http://localhost:1234/v1/"), "http://localhost:1234/v1/chat/completions");
        assert_eq!(completions_url("https://api.openai.com/v1"), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn hy_mt_uses_model_card_prompt_and_glossary() {
        let body = request_body("hf.co/tencent/HY-MT1.5-7B-GGUF:Q4_K_M", "副本来人 缺奶", Direction::ToEnglish, ReplyVariant::Taiwan);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "HY-MT takes no system prompt");
        let prompt = messages[0]["content"].as_str().unwrap();
        assert!(prompt.starts_with("参考下面的翻译：\n"));
        assert!(prompt.contains("副本 翻译成 dungeon"));
        assert!(prompt.contains("缺奶 翻译成 need a healer"));
        assert!(!prompt.contains("坦克"));
        assert!(prompt.ends_with("不要额外解释：\n副本来人 缺奶"));
        assert_eq!(body["top_k"], 20);

        let plain = request_body("HY-MT1.5-7B", "你好", Direction::ToEnglish, ReplyVariant::Taiwan);
        assert_eq!(
            plain["messages"][0]["content"],
            "将以下文本翻译为英语，注意只需要输出翻译后的结果，不要额外解释：\n\n你好"
        );
        let general = request_body("qwen2.5:7b", "缺奶骑", Direction::ToEnglish, ReplyVariant::Taiwan);
        assert_eq!(general["messages"][0]["role"], "system");
        let system = general["messages"][0]["content"].as_str().unwrap();
        assert!(system.ends_with("Slang in this line: 缺奶 = need a healer, 奶骑 = holy paladin."), "{system}");
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
