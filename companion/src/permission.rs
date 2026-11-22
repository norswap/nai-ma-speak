//! The Screen Recording permission on macOS, without which the strip cannot be read.
//!
//! macOS ties it to the app's signature, and a release is signed anew each time: after
//! an update the permission still shows as given in System Settings, but counts for the
//! old app, and cannot be given again from there. So a companion that starts without
//! the permission clears what is on record for it and has macOS ask afresh, which also
//! puts it back in the list in System Settings, switched off.
//!
//! macOS does not pass on the question of a program it has already told no, so the one
//! that finds the permission missing does not ask: it starts a second copy that asks
//! before checking anything, and quits.

/// Set for the copy started to ask for the permission.
#[cfg(target_os = "macos")]
const ASKING: &str = "NAIMASPEAK_ASK_PERMISSION";

/// The macOS app's identifier, as `macos-app.sh` writes it.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "com.github.norswap.nai-ma-speak";

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
fn asking() -> bool {
    std::env::var_os(ASKING).is_some()
}

/// Whether the screen can be captured, as it stood at the start: macOS has the app
/// start again once the permission is given. Always so on other systems.
pub fn granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        static GRANTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        // SAFETY: takes nothing and only reads the permission.
        *GRANTED.get_or_init(|| !asking() && unsafe { CGPreflightScreenCaptureAccess() })
    }
    #[cfg(not(target_os = "macos"))]
    true
}

/// At the start: without the permission, clears what macOS has on record for the app
/// and starts the copy that asks. True if it did, and this copy is to quit. Only for
/// the app bundle: a bare program has no record of its own.
#[cfg(target_os = "macos")]
pub fn renew() -> bool {
    let bundled = std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"));
    if asking() || !bundled || granted() {
        return false;
    }
    let cleared = std::process::Command::new("/usr/bin/tccutil").args(["reset", "ScreenCapture", BUNDLE_ID]).status();
    crate::diag::log(format!("no Screen Recording permission; cleared the old one: {cleared:?}"));
    let mut marks = vec![ASKING];
    if std::env::var_os(crate::config::JUST_UPDATED).is_some() {
        marks.push(crate::config::JUST_UPDATED);
    }
    crate::config::start_again(&marks).is_ok()
}

/// In the copy started to ask: has macOS ask for the permission, once the companion's
/// window is up, which would otherwise cover the question.
#[cfg(target_os = "macos")]
pub fn ask() {
    if asking() {
        std::thread::sleep(std::time::Duration::from_secs(2));
        // SAFETY: takes nothing and only asks for the permission.
        let given = unsafe { CGRequestScreenCaptureAccess() };
        crate::diag::log(format!("asked for the Screen Recording permission; given: {given}"));
    }
}
