//! A log file beside the settings, `nai-ma-speak.log`, for what cannot be seen from outside:
//! panics, a window pass that never finishes, passes that run without pause, and what
//! became of the pixel strip and the translations. It holds no chat and no keys, so its
//! end can go into a report of a problem as it is.

use eframe::egui;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The file is started afresh once it has grown past this.
const LARGEST: u64 = 512 * 1024;
/// A pass that takes longer than this is logged as stuck.
const STUCK: Duration = Duration::from_secs(3);
/// More passes per second than this, kept up, is logged with what asked for them. Chat
/// coming in steadily, scrolling and a spinner make 30 to 50; a window that redraws
/// without cause runs at the screen's rate, 60 or more.
const BUSY_PASSES: f32 = 55.0;

fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

fn path() -> std::path::PathBuf {
    crate::config::path().with_file_name("nai-ma-speak.log")
}

/// Appends a line, stamped with the seconds since the companion started.
pub fn log(message: impl AsRef<str>) {
    let line = format!("[{:9.2}] {}\n", started().elapsed().as_secs_f32(), message.as_ref());
    eprint!("{line}");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// A picture of a pixel strip that could not be read, as a PNG in base64; see `keep_strip`.
static UNREAD_STRIP: Mutex<Option<String>> = Mutex::new(None);

/// Keeps what was captured where a strip was found but could not be read, for the
/// report: the picture shows what a log cannot (something drawn over the strip, blurred
/// or shifted blocks, colors run together).
pub fn keep_strip(picture: &xcap::image::RgbaImage) {
    let mut png = Vec::new();
    if picture.write_to(&mut std::io::Cursor::new(&mut png), xcap::image::ImageFormat::Png).is_ok() {
        use base64::Engine;
        *UNREAD_STRIP.lock().unwrap() = Some(base64::engine::general_purpose::STANDARD.encode(png));
    }
}

/// How many of the log's last lines go into a report.
const REPORT_LINES: usize = 150;

/// What to send along when reporting a problem: `state`, the app's own account of how
/// things stand, then the end of the log, then the picture of a strip that could not be
/// read, if there was one. Anything that looks like an API key is cut out, in case a
/// service quoted one back in an error.
pub fn report(state: &str) -> String {
    let log = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let last = lines[lines.len().saturating_sub(REPORT_LINES)..].join("\n");
    let strip = match &*UNREAD_STRIP.lock().unwrap() {
        Some(picture) => format!("\nThe pixel block that could not be read, as a PNG picture in base64:\n{picture}\n"),
        None => String::new(),
    };
    without_keys(&format!("Nai Ma Speak {}\n{state}\n\nLog:\n{last}\n{strip}", env!("CARGO_PKG_VERSION")))
}

/// The text with every word that starts like an API key ("sk-...") replaced.
fn without_keys(text: &str) -> String {
    let key_like = |word: &str| word.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).starts_with("sk-");
    text.split_inclusive(char::is_whitespace)
        .map(|word| if key_like(word) { "(key removed) " } else { word })
        .collect()
}

/// When the pass under way began, in milliseconds since the start; 0 between passes.
static PASS_BEGAN: AtomicU64 = AtomicU64::new(0);

/// Sends egui's and eframe's warnings to the log, e.g. a lost viewport or a full font atlas.
struct Warnings;

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            log(format!("[{}] {}: {}", record.level(), record.target(), record.args()));
        }
    }

    fn flush(&self) {}
}

/// Whether a panic's message is egui's for a window eframe did not open; see `show_window`
/// in app.rs, which catches it.
pub fn is_unopened_window(message: &str) -> bool {
    message.contains("the user callback was never called")
}

/// Starts the log: warnings and panics go into it, and a watcher notes a pass that does
/// not finish.
pub fn start() {
    if log::set_logger(&Warnings).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    if std::fs::metadata(path()).is_ok_and(|file| file.len() > LARGEST) {
        let _ = std::fs::remove_file(path());
    }
    log(format!("started, version {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        if is_unopened_window(&panic.to_string()) {
            return log("a window could not open in this pass; it opens in the next");
        }
        log(format!("PANIC in thread {:?}: {panic}", std::thread::current().name().unwrap_or("unnamed")));
        earlier(panic);
    }));
    std::thread::spawn(|| {
        let mut reported = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let began = PASS_BEGAN.load(Ordering::Relaxed);
            let now = started().elapsed().as_millis() as u64;
            if began != 0 && began != reported && now.saturating_sub(began) > STUCK.as_millis() as u64 {
                reported = began;
                log(format!("STUCK: a window pass has been running for {} ms", now - began));
            }
        }
    });
}

/// Counts window passes, and marks each for the watcher.
pub struct Passes {
    count: u32,
    since: Instant,
    busy_periods: u32,
}

impl Passes {
    pub fn new() -> Self {
        Self { count: 0, since: Instant::now(), busy_periods: 0 }
    }

    pub fn begin(&mut self) {
        PASS_BEGAN.store((started().elapsed().as_millis() as u64).max(1), Ordering::Relaxed);
    }

    /// Ends a pass. Every few seconds, if passes came without pause, logs how many, what
    /// asked for them and `state`, the caller's own account of what the app was doing.
    pub fn end(&mut self, ctx: &egui::Context, state: impl FnOnce() -> String) {
        PASS_BEGAN.store(0, Ordering::Relaxed);
        self.count += 1;
        let period = self.since.elapsed();
        if period < Duration::from_secs(5) {
            return;
        }
        let rate = self.count as f32 / period.as_secs_f32();
        self.busy_periods = if rate > BUSY_PASSES { self.busy_periods + 1 } else { 0 };
        // The first busy period may be a drag or a scroll; say so once it lasts, and then
        // only every so often.
        if self.busy_periods == 2 || (self.busy_periods > 2 && self.busy_periods.is_multiple_of(12)) {
            // With nothing asking, the passes follow the pointer moving over the window.
            let causes: Vec<String> = ctx.repaint_causes().iter().map(|cause| cause.to_string()).collect();
            if !causes.is_empty() {
                log(format!("BUSY: {rate:.0} passes a second for {} s; asked for by {causes:?}; {}", self.busy_periods * 5, state()));
            }
        }
        self.count = 0;
        self.since = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_carries_the_unread_strip() {
        let picture = xcap::image::RgbaImage::from_pixel(8, 2, xcap::image::Rgba([255, 0, 255, 255]));
        keep_strip(&picture);
        use base64::Engine;
        let report = report("state");
        let text = report.lines().last().expect("the picture's line");
        let png = base64::engine::general_purpose::STANDARD.decode(text).expect("base64");
        assert_eq!(xcap::image::load_from_memory(&png).expect("a PNG").to_rgba8(), picture);
    }

    #[test]
    fn a_report_carries_no_keys() {
        let text = "Translation failed: Incorrect API key provided: sk-proj-abc123. Check \"sk-ant-api03-xyz\"\nstrip: found";
        assert_eq!(without_keys(text), "Translation failed: Incorrect API key provided: (key removed) Check (key removed) strip: found");
    }
}
