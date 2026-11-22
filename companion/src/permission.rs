//! The Screen Recording permission on macOS, without which the strip cannot be read.
//!
//! macOS ties it to the app's signature, and a release is signed anew each time: after
//! an update the permission still shows as given in System Settings, but counts for the
//! old app, and cannot be given again from there.

/// The macOS app's identifier, as `macos-app.sh` writes it.
#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "com.github.norswap.nai-ma-speak";

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Whether the screen can be captured, as it stood when first asked: macOS has the app
/// start again once the permission is given. Always so on other systems.
pub fn granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        static GRANTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        // SAFETY: takes nothing and only reads the permission.
        *GRANTED.get_or_init(|| unsafe { CGPreflightScreenCaptureAccess() })
    }
    #[cfg(not(target_os = "macos"))]
    true
}

/// Clears what macOS has on record for the app and has it ask afresh. Only for the app
/// bundle: a bare program has no record of its own.
#[cfg(target_os = "macos")]
pub fn renew() {
    let bundled = std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"));
    if !bundled {
        return;
    }
    let cleared = std::process::Command::new("/usr/bin/tccutil").args(["reset", "ScreenCapture", BUNDLE_ID]).status();
    crate::diag::log(format!("no Screen Recording permission; cleared the old one: {cleared:?}"));
    // SAFETY: takes nothing and only asks for the permission.
    unsafe { CGRequestScreenCaptureAccess() };
}
