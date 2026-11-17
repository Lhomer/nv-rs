//! The window layer on Android (`super`'s contract): no window thread. A real window asked for
//! stops the run with a line naming what is missing (a run without its window would go on drawing
//! nothing). The fork's test builds (Android, the Linux room) draw into the app's native window
//! instead, handed over to `super` (`set_native_window`, `window_created`): its title and position
//! follow there (`super::window_titled`, `super::window_moved`), not here.

use super::*;

/// The thread that would make the real windows: none, so asking for it stops the run.
pub(super) fn window_thread() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<Request>> {
    eprintln!(
        "NV_D3D9={}: the game's window is not provided on this system yet (no window layer): the run ends",
        tag()
    );
    crate::end_process(1)
}

/// No window layer's window to title (the activity's window is the game's, `super::window_titled`).
pub fn window_titled(_e: &mut Engine, _handle: u32, _title: &[u8]) {}

/// No window layer's window to move (`super::window_moved` keeps the game's rectangles).
pub fn window_moved(_e: &mut Engine, _handle: u32, _rect: [i32; 4], _flags: u32) {}
