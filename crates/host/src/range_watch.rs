//! The Test Range's start, watched ([`crate::test_range`]): a log line for each step, a trace of
//! the calls the `coc`'s load makes, and a watchdog thread that says where a start stands while
//! nothing moves on and ends the run when it never will.
//!
//! **Why.** Patrick (312dda26): the log stopped at "attaching the cell 00157A92 (TES 00453dc0)",
//! the loading screen stayed and nothing more was written. The main thread was inside the game's
//! own load, where no frame of the range runs. So:
//! - while the `coc` loads, an engine call hook ([`Engine::set_call_hook`], a diagnostic the engine
//!   already has) writes a line for each call the load makes at its own level (`CenterOnCell`'s
//!   and, inside the cell's attach, `00453dc0`'s), with how long the slow ones took, and keeps the
//!   chain of calls running now;
//! - the watchdog, a host thread, writes every 10 s without a step the last step and that chain,
//!   so a hang names the function it is in;
//! - after [`STUCK_LOADING`] s without a step inside the load, or [`STUCK_WAITING`] s after the
//!   `coc` returned (the range's own 90 s timeout should have gone back to the main menu by then),
//!   the watchdog says so in the log and in a message box, and ends the run: the main thread cannot
//!   be got back from inside the game's load, and a frozen game the player has to close is worse.
//!
//! Not 1:1 with FalloutNV.exe: the range is an addition (docs/GUNPLAY.md, slice 2).

use engine::prelude::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Seconds without a step while the `coc` loads before the run is ended.
pub const STUCK_LOADING: u64 = 90;
/// Seconds without a step after the `coc` returned before the run is ended.
pub const STUCK_WAITING: u64 = 150;
/// Seconds between the watchdog's lines while nothing moves on.
const SAY_EVERY: u64 = 10;
/// Calls of the chain kept (the innermost beyond are not shown).
const CHAIN_KEEP: usize = 32;
/// A call at the load's level slower than this has its return written too.
const SLOW_MS: u128 = 20;

/// The step the start is at, and when it began.
static STEP: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// A step of the start or the set-up: one log line, and the watchdog's last known place.
pub fn step(what: &str) {
    eprintln!("Test Range: {what}");
    *STEP.lock().unwrap_or_else(|p| p.into_inner()) = Some((what.to_string(), Instant::now()));
}

/// The last step and its age in seconds.
fn last_step() -> Option<(String, u64)> {
    STEP.lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|(what, at)| (what.clone(), at.elapsed().as_secs()))
}

type Hook = fn(&mut Engine, u32, bool);

/// The trace of the load's calls.
struct Trace {
    /// The hook that was on before (a diagnostic run's), called on.
    previous: Option<Hook>,
    /// The engine's call depth at `CenterOnCell`'s level: its calls are entered there.
    load: usize,
    /// The depth of the cell attach's calls, while it runs.
    attach: Option<usize>,
    /// The calls running now from the load's level in.
    chain: Vec<u32>,
    /// When the calls running at the load's and the attach's level were entered.
    entered_load: Option<Instant>,
    entered_attach: Option<Instant>,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

/// Starts the trace at `CenterOnCell`'s level (its seam).
pub fn begin_trace(e: &mut Engine) {
    let previous = e.take_call_hook();
    *TRACE.lock().unwrap_or_else(|p| p.into_inner()) = Some(Trace {
        previous,
        load: e.call_stack.len(),
        attach: None,
        chain: Vec::new(),
        entered_load: None,
        entered_attach: None,
    });
    e.set_call_hook(Some(hook));
}

/// Ends the trace (the `coc` returned, or the range ended); the hook before it is back.
pub fn end_trace(e: &mut Engine) {
    let trace = TRACE.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(trace) = trace {
        e.set_call_hook(trace.previous);
    }
}

/// The cell's attach (`00453dc0`) begins or ends: its calls get lines of their own.
pub fn attach(e: &Engine, running: bool) {
    if let Some(t) = TRACE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        t.attach = running.then_some(e.call_stack.len());
    }
}

/// The calls running now, outermost first.
fn chain() -> String {
    TRACE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|t| chain_text(&t.chain))
        .unwrap_or_default()
}

fn chain_text(chain: &[u32]) -> String {
    chain
        .iter()
        .map(|a| format!("{a:08x}"))
        .collect::<Vec<_>>()
        .join(" > ")
}

/// The call hook: the chain kept, and a line for each call at the load's and the attach's level.
fn hook(e: &mut Engine, addr: u32, returning: bool) {
    let depth = e.call_stack.len();
    let (previous, line) = {
        let mut guard = TRACE.lock().unwrap_or_else(|p| p.into_inner());
        let Some(t) = guard.as_mut() else {
            return;
        };
        let line = trace_call(t, depth, addr, returning, Instant::now());
        (t.previous, line)
    };
    if let Some(line) = line {
        step(&line);
    }
    if let Some(previous) = previous {
        previous(e, addr, returning);
    }
}

/// One call entered or returned at `depth` (the engine's depth outside the call): the chain kept,
/// and the line to write, if any.
fn trace_call(
    t: &mut Trace,
    depth: usize,
    addr: u32,
    returning: bool,
    now: Instant,
) -> Option<String> {
    let level = depth.saturating_sub(t.load);
    t.chain.truncate(level);
    if !returning && depth >= t.load && t.chain.len() == level && level < CHAIN_KEEP {
        t.chain.push(addr);
    }
    let (label, entered) = if t.attach == Some(depth) {
        ("attach", &mut t.entered_attach)
    } else if depth == t.load {
        ("load", &mut t.entered_load)
    } else {
        return None;
    };
    if returning {
        let ms = entered
            .take()
            .map_or(0, |at| now.duration_since(at).as_millis());
        (ms >= SLOW_MS).then(|| format!("{label}: {addr:08x} returned after {ms} ms"))
    } else {
        *entered = Some(now);
        Some(format!("{label}: calls {addr:08x}"))
    }
}

/// The watchdog of a start. `state` says the phase and how many seconds without a step end the
/// run, or `None` when the start is over (played, failed, or ended).
pub fn watch(state: fn() -> Option<(&'static str, u64)>) {
    let _ = std::thread::Builder::new()
        .name("test range watchdog".into())
        .spawn(move || {
            let mut said = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some((phase, limit)) = state() else {
                    return;
                };
                let Some((what, secs)) = last_step() else {
                    continue;
                };
                if secs < SAY_EVERY {
                    said = 0;
                    continue;
                }
                if secs >= limit {
                    give_up(phase, secs, &what);
                }
                if secs / SAY_EVERY > said {
                    said = secs / SAY_EVERY;
                    eprintln!(
                        "Test Range: nothing moved on for {secs} s ({phase}); last step: {what}; running: {}",
                        chain()
                    );
                }
            }
        });
}

/// The start will not finish: said in the log and a message box, then the run ends.
fn give_up(phase: &str, secs: u64, what: &str) -> ! {
    let running = chain();
    let line = format!(
        "Test Range: stuck for {secs} s ({phase}); last step: {what}; running: {running}. The game is closed: its main thread cannot be got back from inside the game's load."
    );
    eprintln!("{line}");
    let plain = format!(
        "The Test Range did not finish loading ({secs} s without progress), so the game will close.\n\nLast step: {what}\n\nThe game's log (userdata\\logs) has the details."
    );
    show_and_wait(&plain, Duration::from_secs(60));
    crate::end_process(3)
}

/// A message box on its own thread (so a box behind the game's window cannot keep the run
/// alive), waited for until it is answered or `limit` passes; nothing off Windows or with
/// `NV_RS_NO_WINDOWS` (scripted runs).
fn show_and_wait(text: &str, limit: Duration) {
    #[cfg(windows)]
    if std::env::var_os("NV_RS_NO_WINDOWS").is_none() {
        let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let (done, answered) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("test range message".into())
            .spawn(move || {
                #[link(name = "user32")]
                extern "system" {
                    fn MessageBoxW(
                        window: isize,
                        text: *const u16,
                        title: *const u16,
                        kind: u32,
                    ) -> i32;
                }
                let title: Vec<u16> = "nv-rs Test Range".encode_utf16().chain([0]).collect();
                // MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST
                // SAFETY: both strings end in a NUL and live through the call.
                unsafe {
                    MessageBoxW(0, text.as_ptr(), title.as_ptr(), 0x10 | 0x1_0000 | 0x4_0000)
                };
                let _ = done.send(());
            });
        let _ = answered.recv_timeout(limit);
    }
    #[cfg(not(windows))]
    let _ = (text, limit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace() -> Trace {
        Trace {
            previous: None,
            load: 10,
            attach: None,
            chain: Vec::new(),
            entered_load: None,
            entered_attach: None,
        }
    }

    /// The load's own calls get a line (and their return when slow); deeper calls only the
    /// chain, which always ends at the call running now.
    #[test]
    fn the_trace_names_the_load_calls_and_keeps_the_chain() {
        let mut t = trace();
        let at = Instant::now();
        let later = at + Duration::from_millis(5);
        assert_eq!(
            trace_call(&mut t, 10, 0x0045_3dc0, false, at).as_deref(),
            Some("load: calls 00453dc0")
        );
        t.attach = Some(11);
        assert_eq!(
            trace_call(&mut t, 11, 0x00c3_dfa0, false, at).as_deref(),
            Some("attach: calls 00c3dfa0")
        );
        assert_eq!(trace_call(&mut t, 12, 0x00c3_dbf0, false, at), None);
        assert_eq!(chain_text(&t.chain), "00453dc0 > 00c3dfa0 > 00c3dbf0");
        assert_eq!(trace_call(&mut t, 12, 0x00c3_dbf0, true, at), None);
        assert_eq!(chain_text(&t.chain), "00453dc0 > 00c3dfa0");
        // A quick return says nothing; a slow one says how long.
        assert_eq!(trace_call(&mut t, 11, 0x00c3_dfa0, true, later), None);
        trace_call(&mut t, 11, 0x0045_5400, false, at);
        assert_eq!(
            trace_call(
                &mut t,
                11,
                0x0045_5400,
                true,
                at + Duration::from_millis(250)
            )
            .as_deref(),
            Some("attach: 00455400 returned after 250 ms")
        );
        assert_eq!(chain_text(&t.chain), "00453dc0");
    }

    /// The chain stops growing at its cap but still follows the calls back out.
    #[test]
    fn the_chain_is_capped() {
        let mut t = trace();
        let at = Instant::now();
        for d in 0..(CHAIN_KEEP + 5) {
            trace_call(&mut t, 10 + d, d as u32, false, at);
        }
        assert_eq!(t.chain.len(), CHAIN_KEEP);
        trace_call(&mut t, 12, 2, true, at);
        assert_eq!(t.chain, vec![0, 1]);
    }
}
