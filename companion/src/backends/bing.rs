//! Bing Translator, one of the `free` services, asked the way its web page asks: the
//! page holds a token good for an hour, and each translation is posted with it.

use super::free::{Failure, Target};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A token says it lasts an hour; a new one is fetched some time before.
const SESSION_LASTS: Duration = Duration::from_secs(50 * 60);
/// The longest text the endpoint takes, in characters.
const LONGEST: usize = 1000;
/// The page is made for browsers, so the requests say they come from one.
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// What the translator's page hands out for the requests that follow.
pub struct Session {
    base: String,
    ig: String,
    key: String,
    token: String,
    opened: Instant,
    /// Requests are numbered, as the page numbers its own.
    asked: u32,
}

/// The text between `before` and `after`, at the first `before`.
fn between<'a>(text: &'a str, before: &str, after: &str) -> Option<&'a str> {
    let rest = &text[text.find(before)? + before.len()..];
    Some(&rest[..rest.find(after)?])
}

/// Reads the page's `IG:"…"` and `params_AbusePreventionHelper = [key,"token",lifetime]`.
fn read_page(base: &str, page: &str) -> Option<Session> {
    let ig = between(page, "IG:\"", "\"")?;
    let (key, rest) = between(page, "params_AbusePreventionHelper = [", "]")?.split_once(',')?;
    let token = between(rest, "\"", "\"")?;
    let whole = !ig.is_empty() && !token.is_empty() && key.bytes().all(|byte| byte.is_ascii_digit());
    whole.then(|| Session { base: base.into(), ig: ig.into(), key: key.into(), token: token.into(), opened: Instant::now(), asked: 0 })
}

fn failed(doing: &str, error: ureq::Error) -> Failure {
    match error {
        ureq::Error::Status(429, _) => Failure::refused("Bing Translator is rate-limiting (HTTP 429)"),
        ureq::Error::Status(code, _) => Failure::broken(format!("Bing Translator returned HTTP {code} when {doing}")),
        error => Failure::broken(format!("could not reach Bing Translator: {error}")),
    }
}

fn open(base: &str) -> Result<Session, Failure> {
    let request = ureq::get(&format!("{base}/translator")).timeout(Duration::from_secs(20)).set("User-Agent", AGENT);
    let page = request.call().map_err(|e| failed("opening its page", e))?.into_string();
    let page = page.map_err(|e| Failure::broken(format!("Bing Translator's page could not be read: {e}")))?;
    // A page without a token is one asking to prove a person is there, or a new layout.
    read_page(base, &page).ok_or_else(|| Failure::refused("Bing Translator's page gave no token"))
}

/// What an answer holds: the translation, or the status the endpoint reports inside an
/// answer that arrived as a success.
fn parse(answer: &Value) -> Result<String, Option<u64>> {
    let text = answer.pointer("/0/translations/0/text").and_then(Value::as_str);
    text.map(str::to_string).ok_or_else(|| answer.get("statusCode").and_then(Value::as_u64))
}

pub fn ask(held: &Mutex<Option<Session>>, base: &str, text: &str, target: Target) -> Result<String, Failure> {
    if text.chars().count() > LONGEST {
        return Err(Failure::broken(format!("Bing Translator takes no more than {LONGEST} characters")));
    }
    let (from, to) = match target {
        Target::English => ("auto-detect", "en"),
        Target::Taiwanese => ("en", "zh-Hant"),
        Target::Simplified => ("en", "zh-Hans"),
    };
    // A token turned down gets one more try with a new one.
    for renewed in [false, true] {
        let (url, key, token) = {
            let mut held = held.lock().unwrap();
            let usable = |session: &Session| session.base == base && session.opened.elapsed() < SESSION_LASTS;
            if renewed || !held.as_ref().is_some_and(usable) {
                *held = None;
                *held = Some(open(base)?);
            }
            let session = held.as_mut().expect("opened above");
            session.asked += 1;
            let url = format!("{base}/ttranslatev3?isVertical=1&IG={}&IID=translator.5023.{}", session.ig, session.asked);
            (url, session.key.clone(), session.token.clone())
        };
        let request = ureq::post(&url).timeout(Duration::from_secs(20)).set("User-Agent", AGENT);
        let form = [("fromLang", from), ("text", text), ("to", to), ("token", token.as_str()), ("key", key.as_str())];
        let status = match request.send_form(&form) {
            Ok(response) => {
                let answer: Value = response.into_json().map_err(|e| Failure::refused(format!("Bing Translator sent an answer that is not JSON: {e}")))?;
                match parse(&answer) {
                    Ok(text) => return Ok(text),
                    Err(status) => status,
                }
            }
            Err(ureq::Error::Status(401, _)) => Some(401),
            Err(e) => return Err(failed("translating", e)),
        };
        match status {
            Some(205 | 401) => continue,
            Some(429) | None => return Err(Failure::refused("Bing Translator is rate-limiting")),
            Some(status) => return Err(Failure::broken(format!("Bing Translator answered with status {status}"))),
        }
    }
    Err(Failure::refused("Bing Translator turns down its own tokens"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_server;

    const PAGE: &str = r#"<script>x={Ver:"47",IG:"ABC123",y:1}; var params_AbusePreventionHelper = [123,"TOKEN",3600000];</script>"#;
    const ANSWER: &str = r#"[{"translations":[{"text":"Need a healer","to":"en"}],"usedLLM":true,"detectedLanguage":{"language":"zh-Hant"}}]"#;

    #[test]
    fn reads_the_token_off_the_page() {
        let session = read_page("b", PAGE).unwrap();
        assert_eq!((session.ig.as_str(), session.key.as_str(), session.token.as_str()), ("ABC123", "123", "TOKEN"));
        assert!(read_page("b", "<html>Prove you are a person</html>").is_none());
    }

    #[test]
    fn opens_the_page_once_and_posts_each_line_with_its_token() {
        let (base, server) = mock_server(vec![(200, "", PAGE), (200, "", ANSWER), (200, "", ANSWER)]);
        let held = Mutex::new(None);
        assert_eq!(ask(&held, &base, "缺補 密我", Target::English).unwrap(), "Need a healer");
        ask(&held, &base, "need a healer", Target::Taiwanese).unwrap();
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /translator "), "{}", requests[0]);
        assert!(requests[1].starts_with("POST /ttranslatev3?isVertical=1&IG=ABC123&IID=translator.5023.1 "), "{}", requests[1]);
        for wanted in ["fromLang=auto-detect", "to=en", "token=TOKEN", "key=123", "text=%E7%BC%BA%E8%A3%9C+%E5%AF%86%E6%88%91"] {
            assert!(requests[1].contains(wanted), "{wanted} missing from {}", requests[1]);
        }
        assert!(requests[1].contains(&format!("User-Agent: {AGENT}")), "{}", requests[1]);
        assert!(requests[2].contains("IID=translator.5023.2 ") && requests[2].contains("fromLang=en") && requests[2].contains("to=zh-Hant"));
    }

    #[test]
    fn a_token_turned_down_is_replaced_once() {
        let rejected = r#"{"statusCode":205,"errorMessage":""}"#;
        let (base, server) = mock_server(vec![(200, "", PAGE), (200, "", rejected), (200, "", PAGE), (200, "", ANSWER)]);
        assert_eq!(ask(&Mutex::new(None), &base, "缺補", Target::English).unwrap(), "Need a healer");
        assert_eq!(server.join().unwrap().len(), 4);

        let (base, server) = mock_server(vec![(200, "", PAGE), (200, "", rejected), (200, "", PAGE), (200, "", rejected)]);
        assert!(ask(&Mutex::new(None), &base, "缺補", Target::English).unwrap_err().refused);
        assert_eq!(server.join().unwrap().len(), 4);
    }

    #[test]
    fn tells_a_rate_limit_from_a_failure() {
        let (base, _server) = mock_server(vec![(200, "", "<html>Prove you are a person</html>")]);
        assert!(ask(&Mutex::new(None), &base, "缺坦", Target::English).unwrap_err().refused);
        let (base, _server) = mock_server(vec![(200, "", PAGE), (200, "", r#"{"statusCode":400,"errorMessage":""}"#)]);
        assert!(!ask(&Mutex::new(None), &base, "缺坦", Target::English).unwrap_err().refused);
        let long = "坦".repeat(LONGEST + 1);
        assert!(!ask(&Mutex::new(None), "http://127.0.0.1:9", &long, Target::English).unwrap_err().refused);
    }
}
