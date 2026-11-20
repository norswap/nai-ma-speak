//! Google Translate's free web endpoint, one of the `free` services.

use super::free::{Failure, Target};
use serde_json::Value;
use std::time::Duration;

/// Reads the translation out of the endpoint's answer, nested arrays in which each
/// `[0][i][0]` is one translated stretch of the text.
fn parse(answer: &Value) -> Option<String> {
    let parts = answer.get(0)?.as_array()?;
    let text: String = parts.iter().filter_map(|part| part.get(0)?.as_str()).collect();
    (!text.is_empty()).then_some(text)
}

pub fn ask(base: &str, text: &str, target: Target) -> Result<String, Failure> {
    let (source, target) = match target {
        Target::English => ("auto", "en"),
        Target::Taiwanese => ("en", "zh-TW"),
        Target::Simplified => ("en", "zh-CN"),
    };
    let request = ureq::get(&format!("{base}/translate_a/single"))
        .timeout(Duration::from_secs(20))
        .query("client", "gtx")
        .query("sl", source)
        .query("tl", target)
        .query("dt", "t")
        .query("q", text);
    match request.call() {
        Ok(response) => {
            let answer: Value = response.into_json().map_err(|e| Failure::broken(format!("Google Translate sent an answer that is not JSON: {e}")))?;
            parse(&answer).ok_or_else(|| {
                let detail: String = answer.to_string().chars().take(120).collect();
                Failure::broken(format!("Google Translate's free service answered in a form this app does not know: {detail}"))
            })
        }
        Err(ureq::Error::Status(code @ (429 | 503), _)) => Err(Failure::refused(format!("Google Translate is rate-limiting (HTTP {code})"))),
        Err(ureq::Error::Status(code, _)) => Err(Failure::broken(format!("Google Translate's free service returned HTTP {code}"))),
        Err(e) => Err(Failure::broken(format!("could not reach Google Translate: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_server;

    #[test]
    fn joins_the_translated_stretches() {
        let one: Value = serde_json::from_str(r#"[[["Need a healer","缺補",null,null,10]],null,"zh-TW"]"#).unwrap();
        assert_eq!(parse(&one).as_deref(), Some("Need a healer"));
        let two: Value =
            serde_json::from_str(r#"[[["Need a healer. ","缺補。",null,null,10],["Whisper me","密我",null,null,3]],null,"zh-TW"]"#).unwrap();
        assert_eq!(parse(&two).as_deref(), Some("Need a healer. Whisper me"));
        assert_eq!(parse(&serde_json::json!({"error": "no"})), None);
        assert_eq!(parse(&serde_json::json!([[]])), None);
    }

    #[test]
    fn asks_google_without_a_key() {
        let (base, server) = mock_server(vec![(200, "", r#"[[["Need a healer","缺補",null,null,10]],null,"zh-TW"]"#)]);
        assert_eq!(ask(&base, "缺補 密我", Target::English).unwrap(), "Need a healer");
        let request = &server.join().unwrap()[0];
        let line = request.lines().next().unwrap();
        assert!(line.starts_with("GET /translate_a/single?"), "{line}");
        for wanted in ["client=gtx", "sl=auto", "tl=en", "dt=t", "q=%E7%BC%BA%E8%A3%9C+%E5%AF%86%E6%88%91"] {
            assert!(line.contains(wanted), "{wanted} missing from {line}");
        }
        assert!(!request.to_lowercase().contains("authorization"));
    }

    #[test]
    fn replies_ask_for_the_chosen_chinese() {
        let reply = r#"[[["需要治療","need a healer",null,null,10]],null,"en"]"#;
        for (target, wanted) in [(Target::Taiwanese, "tl=zh-TW"), (Target::Simplified, "tl=zh-CN")] {
            let (base, server) = mock_server(vec![(200, "", reply)]);
            ask(&base, "need a healer", target).unwrap();
            let request = &server.join().unwrap()[0];
            assert!(request.contains("sl=en") && request.contains(wanted), "{request}");
        }
    }

    #[test]
    fn tells_a_rate_limit_from_a_failure() {
        let (base, _server) = mock_server(vec![(429, "", "Too Many Requests")]);
        assert!(ask(&base, "缺坦", Target::English).unwrap_err().refused);
        let (base, _server) = mock_server(vec![(404, "", "gone")]);
        assert!(!ask(&base, "缺坦", Target::English).unwrap_err().refused);
    }
}
