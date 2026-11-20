//! The mouse wheel over the overlay, on macOS. Clicks and scrolling pass through the
//! overlay to the game, and the game there reports only some wheel steps to addons (a
//! slow single notch reaches it as a fraction of a step, which it drops). So the
//! companion listens to the system's scroll events itself: only listening, the game
//! still gets them all.

use crate::worker::UiSender;

/// Starts listening and returns whether that worked; call it on the main thread. Each
/// event is sent on as `UiEvent::Scrolled`, with where the pointer was.
#[cfg(target_os = "macos")]
pub fn watch(ui: UiSender) -> bool {
    use crate::worker::UiEvent;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use std::ptr::NonNull;

    /// How far a trackpad scrolls for one wheel step, in points.
    const POINTS_PER_STEP: f32 = 40.0;

    let handler = block2::RcBlock::new(move |event: NonNull<NSEvent>| {
        // The system hands the block a valid event for the length of the call.
        let event = unsafe { event.as_ref() };
        let delta = event.scrollingDeltaY() as f32;
        if delta == 0.0 {
            return;
        }
        // A wheel's notch is one event, whatever speed the system makes of it.
        let steps = if event.hasPreciseScrollingDeltas() { delta / POINTS_PER_STEP } else { delta.signum() };
        // In screen points from the main display's top-left corner, as windows are placed.
        let at = objc2_core_graphics::CGEvent::location(event.CGEvent().as_deref());
        ui.send(UiEvent::Scrolled { at: (at.x as f32, at.y as f32), steps });
    });
    // The monitor lasts as long as the app does.
    NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &handler).map(std::mem::forget).is_some()
}

#[cfg(not(target_os = "macos"))]
pub fn watch(_ui: UiSender) -> bool {
    false
}
