//! Linux (the test room, docs/LINUX.md): the app's window as wgpu needs it, its raw window and
//! display handles (X11 or Wayland, from winit) and its size in pixels, handed over by the app
//! ([`set_handles`], [`set_size`]); the device's surface is made on them (`device::surface_for`).

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::sync::Mutex;

/// The handles, kept while the app's window lives.
struct Handles(RawWindowHandle, RawDisplayHandle);

// SAFETY: raw handles are plain identifiers of the app's window and display; the app keeps both
// alive while the game draws in them, and wgpu makes its surface from them on any thread.
unsafe impl Send for Handles {}

static HANDLES: Mutex<Option<Handles>> = Mutex::new(None);
static SIZE: Mutex<(u32, u32)> = Mutex::new((0, 0));

/// The app's window's raw handles (`None`: the window is gone).
pub fn set_handles(handles: Option<(RawWindowHandle, RawDisplayHandle)>) {
    *HANDLES.lock().unwrap_or_else(|p| p.into_inner()) = handles.map(|(w, d)| Handles(w, d));
}

/// The app's window's size in pixels.
pub fn set_size(width: u32, height: u32) {
    *SIZE.lock().unwrap_or_else(|p| p.into_inner()) = (width, height);
}

pub(crate) fn handles() -> Option<(RawWindowHandle, RawDisplayHandle)> {
    HANDLES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|h| (h.0, h.1))
}

pub(crate) fn size() -> Option<(u32, u32)> {
    let s = *SIZE.lock().unwrap_or_else(|p| p.into_inner());
    (s.0 > 0 && s.1 > 0).then_some(s)
}
