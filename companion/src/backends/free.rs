//! The services that need no account: Bing Translator and Google Translate, through the
//! endpoints their own web pages use. Unofficial, so either may refuse for a while or
//! change without notice. One backend asks one of them, another both: Bing first, whose
//! translations of game chat are better, and Google when Bing refuses, fails or is slow.
//! They are plain machine translation: they take no instructions and no slang hints, so
//! they explain nothing, and translate a player's name as they would any words.

use super::{bing, google};
use crate::config::{Config, Region};
use crate::translate::Task;
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Shortest time between two requests to one service.
const SPACING: Duration = Duration::from_millis(300);
/// A service that failed is left alone for a wait that doubles from one second up to this.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(30);
/// How many times one line is tried before it is given up: with the doubling wait, a
/// line outlasts about half a minute of rate-limiting.
const ATTEMPTS: u32 = 6;
/// How long a service may take before the next one is asked as well. Bing answers in
/// about a second, so a shorter wait would ask Google for most lines.
const SLOW: Duration = Duration::from_secs(2);

/// Rests ending this close together end together: two services that failed for the
/// same line come due a moment apart.
const NEAR: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Service {
    Bing,
    Google,
}

/// What a line is translated into.
#[derive(Clone, Copy)]
pub enum Target {
    English,
    Taiwanese,
    Simplified,
}

/// Why a service gave no translation. `refused` is a rate limit or the like, which
/// passes: the line is worth trying again after a wait.
#[derive(Debug)]
pub struct Failure {
    pub refused: bool,
    pub why: String,
}

impl Failure {
    pub fn refused(why: impl Into<String>) -> Self {
        Failure { refused: true, why: why.into() }
    }

    pub fn broken(why: impl Into<String>) -> Self {
        Failure { refused: false, why: why.into() }
    }
}

/// When a service may next be asked, and the wait to impose after its next failure.
/// After a failure it is resting until then, and `refused` says what kind it was; after
/// a translation, `next` only spaces the requests.
struct Pace {
    next: Option<Instant>,
    wait: Duration,
    resting: bool,
    refused: bool,
}

impl Default for Pace {
    fn default() -> Self {
        Pace { next: None, wait: FIRST_WAIT, resting: false, refused: false }
    }
}

/// What is remembered between lines, shared by all of them, so a limit met by one line
/// spares the service for the ones after it too.
#[derive(Default)]
pub struct State {
    paces: [Mutex<Pace>; 2],
    pub(super) bing: Mutex<Option<bing::Session>>,
}

pub static STATE: LazyLock<Arc<State>> = LazyLock::new(Arc::default);

impl Service {
    pub fn label(self) -> &'static str {
        match self {
            Service::Bing => "Bing Translator",
            Service::Google => "Google Translate",
        }
    }

    fn base(self, config: &Config) -> &str {
        match self {
            Service::Bing => &config.bing_base,
            Service::Google => &config.google_free_base,
        }
    }

    fn ask(self, state: &State, base: &str, text: &str, target: Target) -> Result<String, Failure> {
        let base = base.trim().trim_end_matches('/');
        match self {
            Service::Bing => bing::ask(&state.bing, base, text, target),
            Service::Google => google::ask(base, text, target),
        }
    }
}

impl State {
    fn pace(&self, service: Service) -> std::sync::MutexGuard<'_, Pace> {
        self.paces[service as usize].lock().unwrap()
    }

    fn record(&self, service: Service, outcome: &Result<String, Failure>) {
        let mut pace = self.pace(service);
        match outcome {
            Ok(_) => *pace = Pace { next: Some(Instant::now() + SPACING), ..Pace::default() },
            Err(failure) => {
                pace.next = Some(Instant::now() + pace.wait);
                pace.wait = (pace.wait * 2).min(LONGEST_WAIT);
                pace.resting = true;
                pace.refused = failure.refused;
            }
        }
    }

    /// How long until `service` may be asked.
    fn left(&self, service: Service) -> Duration {
        self.pace(service).next.map_or(Duration::ZERO, |next| next.saturating_duration_since(Instant::now()))
    }

    /// How long until the first of `services` may be asked.
    fn wait(&self, services: &[Service]) -> Duration {
        services.iter().map(|service| self.left(*service)).min().unwrap_or_default()
    }

    /// Those of `services` to ask now, in order: the ones not resting after a failure,
    /// once the first of them is due. With all resting, the first whose rest ends.
    fn due(&self, services: &[Service]) -> Vec<Service> {
        loop {
            let ready = |service: &Service| !self.pace(*service).resting || self.left(*service) <= NEAR;
            let ready: Vec<_> = services.iter().copied().filter(ready).collect();
            match ready.first() {
                Some(first) => {
                    thread::sleep(self.left(*first));
                    return ready;
                }
                None => thread::sleep(self.wait(services)),
            }
        }
    }
}

/// Asks `services` in order, each one when the one before has failed or been `SLOW`.
/// The first translation to arrive is the answer.
fn ask(state: &Arc<State>, services: &[Service], config: &Config, text: &str, target: Target) -> Result<String, Vec<Failure>> {
    let (answers, arrived) = mpsc::channel();
    let mut unasked = services.iter().copied();
    let mut start = || {
        let Some(service) = unasked.next() else {
            return false;
        };
        let (state, base, text, answers) = (state.clone(), service.base(config).to_string(), text.to_string(), answers.clone());
        thread::spawn(move || {
            let outcome = service.ask(&state, &base, &text, target);
            state.record(service, &outcome);
            // Nobody listens once another service has answered.
            let _ = answers.send(outcome);
        });
        true
    };
    let mut asked = usize::from(start());
    let mut failures = Vec::new();
    while failures.len() < asked {
        match arrived.recv_timeout(SLOW) {
            Ok(Ok(text)) => return Ok(text),
            Ok(Err(failure)) => {
                failures.push(failure);
                asked += usize::from(start());
            }
            Err(_) => asked += usize::from(start()),
        }
    }
    Err(failures)
}

/// "Bing Translator and Google Translate are", "Google Translate is".
fn named(services: &[Service]) -> String {
    let names: Vec<_> = services.iter().map(|service| service.label()).collect();
    format!("{} {}", names.join(" and "), if names.len() > 1 { "are" } else { "is" })
}

pub fn translate(state: &Arc<State>, services: &[Service], config: &Config, text: &str, task: Task, notify: &dyn Fn(String)) -> Result<String, String> {
    let named = named(services);
    let target = match (task, config.reply_variant) {
        // A name is translated like any text: its words, with no telling a name from a phrase.
        (Task::ToEnglish | Task::Name, _) => Target::English,
        (Task::ToChinese | Task::ToChineseAgain, Region::Taiwan) => Target::Taiwanese,
        (Task::ToChinese | Task::ToChineseAgain, Region::Mainland) => Target::Simplified,
        (Task::Explain(_) | Task::ExplainName, _) => {
            return Err(format!("{named} unable to explain lines or names. Choose Claude or an OpenAI-compatible model on the Backend tab."))
        }
    };
    let advice = "Try again later, or pick another backend in Settings.";
    for attempt in 1..=ATTEMPTS {
        let failures = match ask(state, &state.due(services), config, text, target) {
            Ok(text) => return Ok(text),
            Err(failures) => failures,
        };
        // Nothing passes by waiting unless a service refused, now or for an earlier line.
        if !services.iter().any(|service| state.pace(*service).refused) {
            let whys: Vec<_> = failures.into_iter().map(|failure| failure.why).collect();
            return Err(format!("{}. {advice}", whys.join("; ")));
        }
        if attempt < ATTEMPTS {
            notify(format!("{named} rate-limiting; waiting {} s", state.wait(services).as_secs_f32().ceil()));
        }
    }
    Err(format!("{named} still rate-limiting. {advice}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_server;
    use std::cell::RefCell;

    const GOOGLE_OK: &str = r#"[[["Need a tank","缺坦",null,null,10]],null,"zh-TW"]"#;
    const BING_PAGE: &str = r#"<script>x={Ver:"47",IG:"ABC123",y:1}; var params_AbusePreventionHelper = [123,"TOKEN",3600000];</script>"#;

    fn google(base: String) -> Config {
        Config { google_free_base: base, ..Config::default() }
    }

    fn fresh() -> Arc<State> {
        Arc::default()
    }

    #[test]
    fn waits_and_tries_again_when_rate_limited() {
        let (base, server) = mock_server(vec![(429, "", "Too Many Requests"), (200, "", GOOGLE_OK)]);
        let notes = RefCell::new(Vec::new());
        let english =
            translate(&fresh(), &[Service::Google], &google(base), "缺坦", Task::ToEnglish, &|note| notes.borrow_mut().push(note)).unwrap();
        assert_eq!(english, "Need a tank");
        assert_eq!(server.join().unwrap().len(), 2);
        let notes = notes.into_inner();
        assert_eq!(notes, ["Google Translate is rate-limiting; waiting 1 s"]);
    }

    #[test]
    fn other_errors_fail_at_once_with_advice() {
        let (base, server) = mock_server(vec![(404, "", "gone")]);
        let error = translate(&fresh(), &[Service::Google], &google(base), "缺坦", Task::ToEnglish, &|_| ()).unwrap_err();
        assert!(error.contains("HTTP 404") && error.contains("another backend"), "{error}");
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn explaining_is_left_to_the_chat_models() {
        let error = translate(&fresh(), &[Service::Bing, Service::Google], &Config::default(), "缺坦", Task::ExplainName, &|_| ()).unwrap_err();
        assert!(error.starts_with("Bing Translator and Google Translate are unable to explain"), "{error}");
    }

    #[test]
    fn google_answers_when_bing_refuses_and_bing_is_then_left_alone() {
        let (bing, bing_server) = mock_server(vec![(429, "", "Too Many Requests")]);
        let (google, google_server) = mock_server(vec![(200, "", GOOGLE_OK), (200, "", GOOGLE_OK)]);
        let config = Config { bing_base: bing, google_free_base: google, ..Config::default() };
        let state = fresh();
        let both = [Service::Bing, Service::Google];
        let notes = RefCell::new(Vec::new());
        for _ in 0..2 {
            let english = translate(&state, &both, &config, "缺坦", Task::ToEnglish, &|note| notes.borrow_mut().push(note)).unwrap();
            assert_eq!(english, "Need a tank");
        }
        // The second line came within Bing's wait, so only Google was asked.
        assert_eq!(bing_server.join().unwrap().len(), 1);
        assert_eq!(google_server.join().unwrap().len(), 2);
        assert!(notes.into_inner().is_empty());
    }

    #[test]
    fn bing_is_asked_first() {
        let answer = r#"[{"translations":[{"text":"Need a tank","to":"en"}]}]"#;
        let (bing, bing_server) = mock_server(vec![(200, "", BING_PAGE), (200, "", answer)]);
        let config = Config { bing_base: bing, google_free_base: "http://127.0.0.1:9".into(), ..Config::default() };
        let english = translate(&fresh(), &[Service::Bing, Service::Google], &config, "缺坦", Task::ToEnglish, &|_| ()).unwrap();
        assert_eq!(english, "Need a tank");
        assert_eq!(bing_server.join().unwrap().len(), 2);
    }

    #[test]
    fn both_refusing_is_waited_out() {
        let (bing, bing_server) = mock_server(vec![(429, "", "Too Many Requests")]);
        let (google, google_server) = mock_server(vec![(429, "", "Too Many Requests"), (200, "", GOOGLE_OK)]);
        let config = Config { bing_base: bing, google_free_base: google, ..Config::default() };
        let notes = RefCell::new(Vec::new());
        let english =
            translate(&fresh(), &[Service::Bing, Service::Google], &config, "缺坦", Task::ToEnglish, &|note| notes.borrow_mut().push(note)).unwrap();
        assert_eq!(english, "Need a tank");
        assert_eq!(notes.into_inner(), ["Bing Translator and Google Translate are rate-limiting; waiting 1 s"]);
        // Google's wait ended first and it answered; Bing was not needed again.
        assert_eq!(bing_server.join().unwrap().len(), 1);
        assert_eq!(google_server.join().unwrap().len(), 2);
    }
}
