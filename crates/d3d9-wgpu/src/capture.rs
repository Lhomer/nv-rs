//! The game's frames for a recording (the Windows test room's clips, docs/WINDOWS-ROOM.md): while
//! a capture runs ([`start`]), every present at most [`Capture`]'s rate copies the back buffer (the
//! game's own frame: no frame-rate counter, no touch controls) into a buffer of a small pool, maps
//! it once the copy is submitted, and a later present hands the pixels on ([`Frame`], BGRA) without
//! ever waiting for the GPU: a frame whose buffer is not ready yet waits for the next present; one
//! with no free buffer is skipped. Nothing at all while no capture runs.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A captured frame: its size, its pixels (B, G, R, A rows, top first, `width * 4` bytes a row)
/// and when it was presented.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub at: Instant,
}

/// The capture running: how often a frame is taken, the last taken, and where frames go.
struct Capture {
    every: Duration,
    last: Option<Instant>,
    sender: SyncSender<Frame>,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

/// Buffers in flight at most (copied, waiting for their map): more frames than this behind are
/// skipped.
pub(crate) const IN_FLIGHT: usize = 4;
/// Frames waiting for the receiver at most (it is behind: newer frames are dropped).
const QUEUED: usize = 8;

/// A capture started at `fps` frames a second (1 to 120): the frames come on the answer. A
/// capture already running is replaced.
pub fn start(fps: u32) -> Receiver<Frame> {
    let (sender, receiver) = sync_channel(QUEUED);
    let fps = fps.clamp(1, 120);
    *CAPTURE.lock().unwrap_or_else(|p| p.into_inner()) = Some(Capture {
        every: Duration::from_secs_f64(1.0 / f64::from(fps)),
        last: None,
        sender,
    });
    receiver
}

/// The capture stopped (its receiver gets no more frames).
pub fn stop() {
    *CAPTURE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Whether a capture runs.
pub fn running() -> bool {
    CAPTURE.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// Whether the present at `now` takes a frame (a capture runs and its interval has passed since
/// the last frame taken); taken, it counts as the last.
pub(crate) fn due(now: Instant) -> bool {
    let mut guard = CAPTURE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(c) = guard.as_mut() else {
        return false;
    };
    match c.last {
        // Kept on the rate's grid (not drifting later with each late present), never more than
        // a frame behind.
        Some(last) if now.duration_since(last) < c.every => false,
        Some(last) => {
            let next = last + c.every;
            c.last = Some(if now.duration_since(next) > c.every {
                now
            } else {
                next
            });
            true
        }
        None => {
            c.last = Some(now);
            true
        }
    }
}

/// A frame handed to the capture's receiver (dropped when it is behind or gone).
pub(crate) fn send(frame: Frame) {
    let mut guard = CAPTURE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(c) = guard.as_ref() {
        if let Err(TrySendError::Disconnected(_)) = c.sender.try_send(frame) {
            *guard = None;
        }
    }
}

/// `pixels` (rows of `row` bytes, `width * 4` used) as tight B, G, R, A rows; R and B swapped for
/// an RGBA frame.
pub(crate) fn tight_bgra(pixels: &[u8], row: u32, width: u32, height: u32, rgba: bool) -> Vec<u8> {
    let used = (width * 4) as usize;
    let mut out = Vec::with_capacity(used * height as usize);
    for y in 0..height as usize {
        let at = y * row as usize;
        if let Some(r) = pixels.get(at..at + used) {
            out.extend_from_slice(r);
        }
    }
    if rgba {
        for p in out.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_taken_at_the_rate_and_sent() {
        let rx = start(30);
        let t0 = Instant::now();
        assert!(due(t0));
        assert!(!due(t0 + Duration::from_millis(10)));
        assert!(due(t0 + Duration::from_millis(34)));
        // Far behind: back on the present's time, not a burst of catch-up frames.
        assert!(due(t0 + Duration::from_millis(500)));
        assert!(!due(t0 + Duration::from_millis(510)));
        send(Frame {
            width: 1,
            height: 1,
            bgra: vec![1, 2, 3, 4],
            at: t0,
        });
        assert_eq!(rx.try_recv().unwrap().bgra, vec![1, 2, 3, 4]);
        stop();
        assert!(!running());
        assert!(!due(t0 + Duration::from_secs(5)));
    }

    #[test]
    fn rows_are_made_tight_and_rgba_turned_bgra() {
        // Two pixels a row in rows of 12 bytes.
        let pixels = [
            1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
        ];
        assert_eq!(
            tight_bgra(&pixels, 12, 2, 2, false),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(&tight_bgra(&pixels, 12, 2, 2, true)[..4], &[3, 2, 1, 4]);
    }
}
