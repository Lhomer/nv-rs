//! The drawing side's own render state at the world draw (`engine_core::draw_side`): what the cut needs before
//! the draw can leave main, wired into the world draw's two halves behind `NV_DRAW_THREAD` (a check, not for
//! play, as the value packet [`super::capture`]).
//!
//! With `NV_DRAW_THREAD` set, each half's render mode draw or finish ([`run`]) opens a side over the render
//! layer's shared state ([`ranges`]: the state cache, the lock counters, the stages' shadowed textures, the
//! current pass and the pass being rendered with its type), runs as the drawing side on this thread, and
//! joins it at the half's end, so what runs between the halves (the overlay's draw) is main's in guest memory
//! as in the serial order. The draw is still on main: this is the drawing thread's state, not the thread.
//! With `poison` the side's guest bytes are scribbled while the half runs, so a read of that state that does
//! not go through the side sees the scribble, and a write that misses it is counted; the totals are printed
//! with the value packet's.
//!
//! Unset, [`run`] is the call it wraps.

use super::capture::{self, Mode};
use crate::render::stage;
use crate::shader::state;
use engine_core::draw_side::{self, JoinReport, Kind};
use engine_core::Engine;
use std::sync::Mutex;

/// The pass being rendered and its type (`011f91e0`, `011f91e4`) and the current shader pass (`0126f74c`).
const RENDERING_PASS: (u32, u32) = (0x011f_91e0, 8);
const CURRENT_PASS: (u32, u32) = (0x0126_f74c, 4);
/// The group-zero flag the first half sets around render group 0, which is the anisotropy level the stages
/// read (`011a9608`).
const GROUP_ZERO_FLAG: (u32, u32) = (0x011a_9608, 4);

/// The fixed ranges a side holds, the unmapped ones left out: the state cache and lock counters
/// ([`state::side_ranges`]), the shadowed textures ([`stage::side_ranges`]), the passes.
pub fn ranges(e: &Engine) -> Vec<(u32, u32, Kind)> {
    let mut all = state::side_ranges(e);
    all.extend(stage::side_ranges());
    all.push((RENDERING_PASS.0, RENDERING_PASS.1, Kind::Device));
    all.push((CURRENT_PASS.0, CURRENT_PASS.1, Kind::Device));
    all.push((GROUP_ZERO_FLAG.0, GROUP_ZERO_FLAG.1, Kind::Device));
    all.retain(|&(start, len, _)| {
        let mut probe = vec![0u8; len as usize];
        e.mem.try_read(start, &mut probe).is_ok()
    });
    all
}

/// What the sides saw so far: joins, and their reports and POISON's missed writes added up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub sides: u64,
    pub refused: u64,
    pub join: JoinReport,
    pub poison_written: u64,
}

static TOTALS: Mutex<Totals> = Mutex::new(Totals {
    sides: 0,
    refused: 0,
    join: JoinReport {
        taken: 0,
        main_changed: 0,
        unrouted: 0,
        unbalanced: 0,
        own_words: 0,
    },
    poison_written: 0,
});

/// The totals so far.
pub fn totals() -> Totals {
    *TOTALS.lock().unwrap_or_else(|p| p.into_inner())
}

thread_local! {
    /// This thread's own totals (the checks, which run engines on threads of their own).
    static THREAD: std::cell::Cell<Totals> = const { std::cell::Cell::new(Totals {
        sides: 0,
        refused: 0,
        join: JoinReport {
            taken: 0,
            main_changed: 0,
            unrouted: 0,
            unbalanced: 0,
            own_words: 0,
        },
        poison_written: 0,
    }) };
}

/// This thread's totals so far.
pub fn thread_totals() -> Totals {
    THREAD.with(std::cell::Cell::get)
}

fn add(t: &mut Totals, refused: bool, report: JoinReport, written: u64) {
    if refused {
        t.refused += 1;
        return;
    }
    t.sides += 1;
    t.poison_written += written;
    t.join.taken += report.taken;
    t.join.main_changed += report.main_changed;
    t.join.unrouted += report.unrouted;
    t.join.unbalanced += report.unbalanced;
    t.join.own_words += report.own_words;
}

fn count(refused: bool, report: JoinReport, written: u64) {
    add(
        &mut TOTALS.lock().unwrap_or_else(|p| p.into_inner()),
        refused,
        report,
        written,
    );
    THREAD.with(|c| {
        let mut t = c.get();
        add(&mut t, refused, report, written);
        c.set(t);
    });
}

/// Runs `f` (a world draw half's render mode draw or finish) as the drawing side with its own render state, when
/// `NV_DRAW_THREAD` asks for the packet (module documentation); else just `f`.
pub fn run(e: &mut Engine, f: impl FnOnce(&mut Engine)) {
    let mode = capture::mode();
    if mode == Mode::Off {
        f(e);
        return;
    }
    let ranges = ranges(e);
    let Some(side) = draw_side::open(e, &ranges) else {
        count(true, JoinReport::default(), 0);
        f(e);
        return;
    };
    let poisoned = (mode == Mode::Poison).then(|| draw_side::poison(e, &side, capture::SCRIBBLE));
    let ((), side) = draw_side::as_drawing_side(side, || f(e));
    let written = poisoned.map_or(0, |p| draw_side::unpoison(e, p));
    let report = draw_side::join(e, side);
    count(false, report, written);
    if mode == Mode::Poison && written != 0 {
        missed_writes(written);
    }
}

/// POISON: a write of the side's state that went to guest memory: said once a run.
#[cold]
#[inline(never)]
fn missed_writes(written: u64) {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        eprintln!(
            "NV_DRAW_THREAD=poison: the draw wrote {written} byte(s) of the render state outside its own side (said once)"
        )
    });
}
