//! A log file beside the settings, `translate.log`, for what cannot be seen from outside:
//! panics, a window pass that never finishes, and passes that run without pause.

use eframe::egui;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The file is started afresh once it has grown past this.
const LARGEST: u64 = 512 * 1024;
/// A pass that takes longer than this is logged as stuck.
const STUCK: Duration = Duration::from_secs(3);
/// More passes per second than this, kept up, is logged with what asked for them.
const BUSY_PASSES: f32 = 25.0;

fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

fn path() -> std::path::PathBuf {
    crate::config::path().with_file_name("translate.log")
}

/// Appends a line, stamped with the seconds since the companion started.
pub fn log(message: impl AsRef<str>) {
    let line = format!("[{:9.2}] {}\n", started().elapsed().as_secs_f32(), message.as_ref());
    eprint!("{line}");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
        let _ = file.write_all(line.as_bytes());
    }
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

/// Starts the log: warnings and panics go into it, and a watcher notes a pass that does
/// not finish.
pub fn start() {
    if log::set_logger(&Warnings).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    if std::fs::metadata(path()).is_ok_and(|file| file.len() > LARGEST) {
        let _ = std::fs::remove_file(path());
    }
    log(format!("started, version {}", env!("CARGO_PKG_VERSION")));
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
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
            let causes: Vec<String> = ctx.repaint_causes().iter().map(|cause| cause.to_string()).collect();
            log(format!("BUSY: {rate:.0} passes a second for {} s; asked for by {causes:?}; {}", self.busy_periods * 5, state()));
        }
        self.count = 0;
        self.since = Instant::now();
    }
}
