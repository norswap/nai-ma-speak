//! Any OpenAI-compatible /chat/completions endpoint: Ollama, LM Studio, OpenAI and others.

use crate::config::{Config, ReplyVariant};
use crate::translate::{slang_in, system_prompt, Direction};
use serde_json::{json, Value};
use std::time::Duration;

/// Tencent's HY-MT models want their own prompt and no system message.
fn is_hy_mt(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("hy-mt") || m.contains("hunyuan-mt")
}

/// Builds the request body for either prompt style.
fn request_body(model: &str, text: &str, direction: Direction, variant: ReplyVariant, is_claude: bool) -> Value {
    if !is_hy_mt(model) {
        let mut body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt(text, direction, variant) },
                { "role": "user", "content": text },
            ],
        });
        // Claude models reject a temperature below 1 on Anthropic's OpenAI-compatible
        // endpoint, so it is only sent to other models.
        if !is_claude {
            body["temperature"] = json!(0.2);
        }
        return body;
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

/// Sends one message to the endpoint and returns the model's raw answer.
pub fn translate(config: &Config, text: &str, direction: Direction) -> Result<String, String> {
    let url = completions_url(&config.api_base);
    let is_claude = config.model.trim().to_lowercase().starts_with("claude-") || url.contains("api.anthropic.com");
    let body = request_body(&config.model, text, direction, config.reply_variant, is_claude);

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

    response["choices"][0]["message"]["content"].as_str().map(str::to_string).ok_or_else(|| {
        let detail = response.to_string();
        format!("{url} gave no translation: {}", detail.chars().take(200).collect::<String>())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_server;

    #[test]
    fn calls_openai_compatible_endpoint() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"LF healer for the dungeon"}}]}"#;
        let (base, server) = mock_server(vec![(200, "", body)]);
        let config = Config { api_base: format!("{base}/v1/"), api_key: "secret".into(), ..Config::default() };
        let english = translate(&config, "副本缺奶", Direction::ToEnglish).unwrap();
        assert_eq!(english, "LF healer for the dungeon");
        let request = &server.join().unwrap()[0];
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
        let body = request_body("hf.co/tencent/HY-MT1.5-7B-GGUF:Q4_K_M", "副本来人 缺奶", Direction::ToEnglish, ReplyVariant::Taiwan, false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "HY-MT takes no system prompt");
        let prompt = messages[0]["content"].as_str().unwrap();
        assert!(prompt.starts_with("参考下面的翻译：\n"));
        assert!(prompt.contains("副本 翻译成 dungeon"));
        assert!(prompt.contains("缺奶 翻译成 need a healer"));
        assert!(!prompt.contains("坦克"));
        assert!(prompt.ends_with("不要额外解释：\n副本来人 缺奶"));
        assert_eq!(body["top_k"], 20);

        let plain = request_body("HY-MT1.5-7B", "你好", Direction::ToEnglish, ReplyVariant::Taiwan, false);
        assert_eq!(
            plain["messages"][0]["content"],
            "将以下文本翻译为英语，注意只需要输出翻译后的结果，不要额外解释：\n\n你好"
        );
        let general = request_body("qwen2.5:7b", "缺奶骑", Direction::ToEnglish, ReplyVariant::Taiwan, false);
        assert_eq!(general["messages"][0]["role"], "system");
        let system = general["messages"][0]["content"].as_str().unwrap();
        assert!(system.ends_with("Slang in this line: 缺奶 = need a healer, 奶骑 = holy paladin."), "{system}");
    }

    #[test]
    fn hy_mt_names_the_chinese_to_reply_in() {
        let prompt = |variant| {
            let body = request_body("hy-mt1.5", "need a healer", Direction::ToChinese, variant, false);
            body["messages"][0]["content"].as_str().unwrap().to_string()
        };
        assert!(prompt(ReplyVariant::Taiwan).contains("翻译为繁体中文"));
        assert!(prompt(ReplyVariant::Simplified).contains("翻译为中文"));
    }

    #[test]
    fn claude_models_get_no_temperature() {
        let plain = request_body("gpt-4o-mini", "缺奶", Direction::ToEnglish, ReplyVariant::Taiwan, false);
        assert_eq!(plain["temperature"], 0.2);
        let claude = request_body("claude-haiku-5-5", "缺奶", Direction::ToEnglish, ReplyVariant::Taiwan, true);
        assert!(claude.get("temperature").is_none());
        assert_eq!(claude["messages"][1]["content"], "缺奶");
    }
}
