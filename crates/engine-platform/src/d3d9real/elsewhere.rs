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

/// No window layer to list the monitors (the activity's window is the game's): the machine table
/// goes on without its display lines (`crate::machine_table::this_display` names why).
pub(super) fn monitors() -> Result<Vec<crate::machine_table::Monitor>, String> {
    Err("no window layer on this system lists the monitors".to_string())
}

/// No window layer's loop to give the calling thread (macOS's need, upstream's #694): the game runs
/// on it, and the process ends when the game returns (status 0) or stops with a panic (101, its
/// message already printed).
pub(super) fn run_on_main_thread(game: Box<dyn FnOnce() + Send>) -> ! {
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(game));
    crate::end_process(if ended.is_ok() { 0 } else { 101 })
}
