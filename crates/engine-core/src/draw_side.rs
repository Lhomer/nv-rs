//! The drawing side's own copy of the render layer's shared guest state (step 2 of the world's drawing
//! thread: the per-side render state the cut needs before the draw can leave main).
//!
//! The device model keeps one Direct3D state per producer (`engine_platform` d3d9draw.rs, piece 5), but the
//! game keeps its own record of that state in guest memory, one copy, which every draw reads and writes:
//! `NiDX9RenderState`'s cached render, stage and sampler states, textures, shaders and declaration (a setter
//! sends a call only when the cached value differs, then stores it), Bethesda's render state lock counters,
//! the texture stages' shadowed textures. Run beside main's tail, the drawing side and main would each skip
//! calls the other's entries claim, on a device state of their own that never got them.
//!
//! So at the cut main opens a [`Side`] over those ranges ([`open`]): each range's bytes are copied (the
//! snapshot) and handed, as the side's own copy, to whoever draws ([`install`] on that thread). On the drawing
//! side the accessors ([`load_u32`] and its kin) read and write the copy and leave guest memory alone; its
//! device calls go to its own device state as before, which starts from main's at the cut, as the copy does.
//! Main goes on in guest memory; while a side is open its accessors note the bytes a write changes (a write
//! of the value already there changes nothing, as the setters that send nothing then). At the join
//! ([`join`], on main, the drawing side's copy given back by [`uninstall`]) each range is merged by its
//! [`Kind`] with the device model's own rule (`d3d9draw::draw_join`): what main changed since the cut stays
//! main's, everything else takes the drawing side's value. A cache entry changes exactly when its setter
//! sends the call, so the merged cache describes the merged device state, and main's next setters skip
//! only what the device holds.
//!
//! - A byte main changed without the accessors (a translated path not routed here) is found at the join as a
//!   guest byte that differs from the snapshot: it stays main's, and it is counted ([`JoinReport::unrouted`]):
//!   a routing still missing.
//! - [`Kind::Balanced`] ranges (the lock counters) merge the same way; a byte the drawing side left changed is
//!   counted ([`JoinReport::unbalanced`]): in the serial order main would have read it, and here it read its
//!   own.
//! - [`Kind::Private`] ranges are the drawing side's scratch and never go back.
//!
//! Off until something opens a side: then every accessor is the guest access it replaces behind one relaxed
//! load. Nothing in play opens one yet (the drawing thread does not post the draw); the checks do.

use crate::Engine;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// How a range merges at the join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The device model's rule: main's changes since the cut stay, the rest takes the drawing side's bytes.
    Device,
    /// As [`Kind::Device`], and a byte the drawing side left changed is counted (a counter it did not bring
    /// back).
    Balanced,
    /// The drawing side's own: never merged back.
    Private,
}

/// One range of a side: its start, kind, the snapshot taken at the cut and the drawing side's copy.
struct Range {
    start: u32,
    kind: Kind,
    snapshot: Vec<u8>,
    copy: Vec<u8>,
}

impl Range {
    #[inline(always)]
    fn offset(&self, addr: u32, len: u32) -> Option<usize> {
        let at = addr.wrapping_sub(self.start);
        let end = u64::from(at) + u64::from(len);
        (end <= self.copy.len() as u64).then_some(at as usize)
    }
}

/// The drawing side's state: what [`open`] makes and [`install`] puts on the drawing thread. `Send`: it is made
/// on main and used on the drawing thread.
pub struct Side {
    /// The run that opened it (the kernel table's address, as the value packet's).
    run: usize,
    ranges: Vec<Range>,
}

impl Side {
    #[inline(always)]
    fn find(&mut self, addr: u32, len: u32) -> Option<(&mut Range, usize)> {
        self.ranges
            .iter_mut()
            .find_map(|r| r.offset(addr, len).map(|at| (r, at)))
    }
}

/// Main's record while a side is open: the ranges, and the bytes main's writes changed.
struct MainLog {
    run: usize,
    ranges: Vec<(u32, Vec<bool>)>,
}

thread_local! {
    /// On the drawing thread: its side.
    static DRAWING: RefCell<Option<Side>> = const { RefCell::new(None) };
    /// On main: its record of the open side.
    static MAIN: RefCell<Option<MainLog>> = const { RefCell::new(None) };
}

/// How many threads hold a side or main's record: the accessors' one check while none does.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn run(e: &Engine) -> usize {
    e.kernel.id()
}

/// Opens a side over `ranges` (start, length, kind) for `e`'s run, on main at the cut: each range's bytes are
/// the snapshot and the drawing side's copy, and main's writes through the accessors are noted from now on.
/// `None` (nothing done) when this thread holds a side or a record already, or a range is not mapped or
/// overlaps another.
pub fn open(e: &Engine, ranges: &[(u32, u32, Kind)]) -> Option<Side> {
    if MAIN.with(|m| m.borrow().is_some()) || DRAWING.with(|d| d.borrow().is_some()) {
        return None;
    }
    let mut taken: Vec<Range> = Vec::with_capacity(ranges.len());
    for &(start, len, kind) in ranges {
        let end = u64::from(start) + u64::from(len);
        if len == 0
            || end > 1 << 32
            || taken.iter().any(|r| {
                u64::from(start) < u64::from(r.start) + r.copy.len() as u64
                    && u64::from(r.start) < end
            })
        {
            return None;
        }
        let mut snapshot = vec![0u8; len as usize];
        e.mem.try_read(start, &mut snapshot).ok()?;
        taken.push(Range {
            start,
            kind,
            copy: snapshot.clone(),
            snapshot,
        });
    }
    let log = MainLog {
        run: run(e),
        ranges: taken
            .iter()
            .map(|r| (r.start, vec![false; r.copy.len()]))
            .collect(),
    };
    MAIN.with(|m| *m.borrow_mut() = Some(log));
    ACTIVE.fetch_add(1, Relaxed);
    Some(Side {
        run: run(e),
        ranges: taken,
    })
}

/// Puts `side` on this thread (the drawing thread, as its job starts): its accessors use the copy from now on.
/// Gives it back when this thread holds one already.
pub fn install(side: Side) -> Result<(), Side> {
    DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        if d.is_some() {
            return Err(side);
        }
        *d = Some(side);
        ACTIVE.fetch_add(1, Relaxed);
        Ok(())
    })
}

/// Takes this thread's side off it (the drawing job's end), for main's [`join`].
pub fn uninstall() -> Option<Side> {
    let side = DRAWING.with(|d| d.borrow_mut().take());
    if side.is_some() {
        ACTIVE.fetch_sub(1, Relaxed);
    }
    side
}

/// Whether this thread holds a side (the drawing side) or main's record of one, for `e`'s run.
pub fn is_open(e: &Engine) -> bool {
    ACTIVE.load(Relaxed) != 0
        && (DRAWING.with(|d| d.borrow().as_ref().is_some_and(|s| s.run == run(e)))
            || MAIN.with(|m| m.borrow().as_ref().is_some_and(|l| l.run == run(e))))
}

/// Whether this thread is the drawing side of an open side (for checks and diagnostics).
pub fn is_open_on_drawing_side() -> bool {
    ACTIVE.load(Relaxed) != 0 && DRAWING.with(|d| d.borrow().is_some())
}

/// What the join found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JoinReport {
    /// Bytes that took the drawing side's value.
    pub taken: u64,
    /// Bytes main changed through the accessors (kept main's).
    pub main_changed: u64,
    /// Bytes main changed without them (kept main's; a routing still missing).
    pub unrouted: u64,
    /// Bytes of balanced ranges the drawing side left changed.
    pub unbalanced: u64,
}

/// Merges `side` (given back by [`uninstall`]) into guest memory on main and ends main's record ([`Kind`]):
/// a byte main changed since the cut (through the accessors, or found different from the snapshot) stays
/// main's; any other takes the drawing side's value; private ranges stay main's. The default report when main
/// holds no record for `e`'s run (the side is dropped).
pub fn join(e: &mut Engine, side: Side) -> JoinReport {
    let log = MAIN.with(|m| {
        let mut m = m.borrow_mut();
        if m.as_ref().is_some_and(|l| l.run == run(e)) {
            m.take()
        } else {
            None
        }
    });
    let Some(log) = log else {
        return JoinReport::default();
    };
    ACTIVE.fetch_sub(1, Relaxed);
    let mut report = JoinReport::default();
    for (range, (start, changed)) in side.ranges.iter().zip(&log.ranges) {
        debug_assert_eq!(range.start, *start);
        if range.kind == Kind::Private {
            continue;
        }
        let mut live = vec![0u8; range.copy.len()];
        if e.mem.try_read(range.start, &mut live).is_err() {
            continue;
        }
        for (i, byte) in live.iter_mut().enumerate() {
            if range.kind == Kind::Balanced && range.copy[i] != range.snapshot[i] {
                report.unbalanced += 1;
            }
            if changed[i] {
                report.main_changed += 1;
            } else if *byte != range.snapshot[i] {
                report.unrouted += 1;
            } else if *byte != range.copy[i] {
                *byte = range.copy[i];
                report.taken += 1;
            }
        }
        e.mem.write(range.start, &live);
    }
    if report.unrouted != 0 || report.unbalanced != 0 {
        noted(report);
    }
    report
}

/// The first join a run finds unrouted or unbalanced bytes says so once.
#[cold]
#[inline(never)]
fn noted(report: JoinReport) {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        eprintln!(
            "nv-draw side: the join found {} byte(s) main changed outside the routed accessors and {} the \
             drawing side left changed in a balanced range (said once)",
            report.unrouted, report.unbalanced
        )
    });
}

// ---- The accessors --------------------------------------------------------------------------------------

/// The slow path of a read: the drawing side's copy when this thread is the drawing side and holds the bytes.
#[cold]
#[inline(never)]
fn side_read(e: &Engine, addr: u32, out: &mut [u8]) -> bool {
    DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        let Some(side) = d.as_mut().filter(|s| s.run == run(e)) else {
            return false;
        };
        let len = out.len() as u32;
        match side.find(addr, len) {
            Some((range, at)) => {
                out.copy_from_slice(&range.copy[at..at + out.len()]);
                true
            }
            None => false,
        }
    })
}

/// The slow path of a write: on the drawing side, into its copy (true); on main with a record, the changed
/// bytes noted (the write itself is the caller's, false).
#[cold]
#[inline(never)]
fn side_write(e: &Engine, addr: u32, bytes: &[u8]) -> bool {
    let drawing = DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        let Some(side) = d.as_mut().filter(|s| s.run == run(e)) else {
            return false;
        };
        match side.find(addr, bytes.len() as u32) {
            Some((range, at)) => {
                range.copy[at..at + bytes.len()].copy_from_slice(bytes);
                true
            }
            None => false,
        }
    });
    if drawing {
        return true;
    }
    MAIN.with(|m| {
        let mut m = m.borrow_mut();
        let Some(log) = m.as_mut().filter(|l| l.run == run(e)) else {
            return;
        };
        let len = bytes.len() as u32;
        for (start, changed) in &mut log.ranges {
            let at = addr.wrapping_sub(*start);
            if u64::from(at) + u64::from(len) > changed.len() as u64 {
                continue;
            }
            let mut old = vec![0u8; bytes.len()];
            if e.mem.try_read(addr, &mut old).is_ok() && old != bytes {
                // A whole value: the device model's key is the value, not its bytes.
                changed[at as usize..at as usize + bytes.len()].fill(true);
            }
            return;
        }
    });
    false
}

/// A word: the drawing side's copy where it holds it, else guest memory.
#[inline(always)]
pub fn load_u32(e: &Engine, addr: u32) -> u32 {
    if ACTIVE.load(Relaxed) != 0 {
        let mut b = [0u8; 4];
        if side_read(e, addr, &mut b) {
            return u32::from_le_bytes(b);
        }
    }
    e.mem.u32(addr)
}

/// A byte: as [`load_u32`].
#[inline(always)]
pub fn load_u8(e: &Engine, addr: u32) -> u8 {
    if ACTIVE.load(Relaxed) != 0 {
        let mut b = [0u8; 1];
        if side_read(e, addr, &mut b) {
            return b[0];
        }
    }
    e.mem.u8(addr)
}

/// A float: as [`load_u32`].
#[inline(always)]
pub fn load_f32(e: &Engine, addr: u32) -> f32 {
    f32::from_bits(load_u32(e, addr))
}

/// Writes a word: into the drawing side's copy where it holds it, else guest memory (main's change noted while a
/// side is open).
#[inline(always)]
pub fn store_u32(e: &Engine, addr: u32, value: u32) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &value.to_le_bytes()) {
        return;
    }
    e.mem.set_u32(addr, value);
}

/// Writes a byte: as [`store_u32`].
#[inline(always)]
pub fn store_u8(e: &Engine, addr: u32, value: u8) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &[value]) {
        return;
    }
    e.mem.set_u8(addr, value);
}

/// Writes a float: as [`store_u32`].
#[inline(always)]
pub fn store_f32(e: &Engine, addr: u32, value: f32) {
    store_u32(e, addr, value.to_bits());
}

/// Runs `f` as the drawing side with `side` installed on this thread, and gives the side back: for the checks,
/// which run both sides on one thread.
pub fn as_drawing_side<R>(side: Side, f: impl FnOnce() -> R) -> (R, Side) {
    let mut held = MAIN.with(|m| m.borrow_mut().take());
    if install(side).is_err() {
        unreachable!("a side was already installed on this thread");
    }
    if held.is_some() {
        // Main's record is off this thread while it plays the drawing side.
        ACTIVE.fetch_sub(1, Relaxed);
    }
    let r = f();
    let side = uninstall().expect("the side installed above");
    if let Some(log) = held.take() {
        MAIN.with(|m| *m.borrow_mut() = Some(log));
        ACTIVE.fetch_add(1, Relaxed);
    }
    (r, side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut e = Engine::new();
        e.mem.map(0x0001_0000, 0x1000);
        e
    }

    const A: u32 = 0x0001_0000;

    /// Off: the accessors are guest memory.
    #[test]
    fn without_a_side_the_accessors_are_guest_memory() {
        let e = engine();
        store_u32(&e, A, 7);
        store_u8(&e, A + 4, 9);
        assert_eq!(e.mem.u32(A), 7);
        assert_eq!(load_u32(&e, A), 7);
        assert_eq!(load_u8(&e, A + 4), 9);
    }

    /// The drawing side reads and writes its copy; guest memory keeps main's.
    #[test]
    fn the_drawing_side_writes_its_copy() {
        let mut e = engine();
        e.mem.set_u32(A, 1);
        let side = open(&e, &[(A, 0x40, Kind::Device)]).unwrap();
        let (seen, side) = as_drawing_side(side, || {
            let before = load_u32(&e, A);
            store_u32(&e, A, 2);
            (before, load_u32(&e, A))
        });
        assert_eq!(seen, (1, 2));
        assert_eq!(e.mem.u32(A), 1, "guest memory is main's");
        assert_eq!(load_u32(&e, A), 1, "main reads guest memory");
        let report = join(&mut e, side);
        assert_eq!(e.mem.u32(A), 2, "the join takes the drawing side's value");
        assert_eq!(report.taken, 1);
        assert!(!is_open(&e));
    }

    /// The device model's rule: what main changed stays main's (also a change and a change back: main sent
    /// both, and the device holds main's last), the rest takes the drawing side's; a write of the value already
    /// there is no change.
    #[test]
    fn the_join_keeps_what_main_changed() {
        let mut e = engine();
        for k in 0..4 {
            e.mem.set_u32(A + 4 * k, 10 + k);
        }
        let side = open(&e, &[(A, 0x10, Kind::Device)]).unwrap();
        // Main: word 0 changed, word 1 changed and back, word 2 written with its own value.
        store_u32(&e, A, 100);
        store_u32(&e, A + 4, 101);
        store_u32(&e, A + 4, 11);
        store_u32(&e, A + 8, 12);
        let ((), side) = as_drawing_side(side, || {
            for k in 0..4 {
                store_u32(&e, A + 4 * k, 200 + k);
            }
        });
        let report = join(&mut e, side);
        assert_eq!(e.mem.u32(A), 100);
        assert_eq!(
            e.mem.u32(A + 4),
            11,
            "main's change and change back stay main's"
        );
        assert_eq!(e.mem.u32(A + 8), 202, "an equal write is no change");
        assert_eq!(e.mem.u32(A + 12), 203);
        assert_eq!(report.main_changed, 8);
        assert_eq!(report.unrouted, 0);
    }

    /// A change main made outside the accessors stays main's and is counted; a balanced byte the drawing
    /// side left changed is counted; a private range never goes back.
    #[test]
    fn unrouted_unbalanced_and_private() {
        let mut e = engine();
        let side = open(
            &e,
            &[
                (A, 4, Kind::Device),
                (A + 4, 4, Kind::Balanced),
                (A + 8, 4, Kind::Private),
            ],
        )
        .unwrap();
        e.mem.set_u32(A, 5);
        let ((), side) = as_drawing_side(side, || {
            store_u32(&e, A, 6);
            store_u32(&e, A + 4, 1);
            store_u32(&e, A + 8, 9);
        });
        let report = join(&mut e, side);
        assert_eq!(e.mem.u32(A), 5, "main's unrouted change stays");
        assert_eq!(report.unrouted, 1);
        assert_eq!(e.mem.u32(A + 4), 1);
        assert_eq!(report.unbalanced, 1);
        assert_eq!(e.mem.u32(A + 8), 0, "private: the drawing side's own");
    }

    /// A second open on the same thread, overlapping ranges and unmapped ranges are refused.
    #[test]
    fn open_refuses_what_it_cannot_take() {
        let mut e = engine();
        assert!(open(&e, &[(A, 8, Kind::Device), (A + 4, 8, Kind::Device)]).is_none());
        assert!(open(&e, &[(0x0900_0000, 8, Kind::Device)]).is_none());
        let side = open(&e, &[(A, 8, Kind::Device)]).unwrap();
        assert!(open(&e, &[(A + 0x100, 8, Kind::Device)]).is_none());
        join(&mut e, side);
        assert!(!is_open(&e));
    }

    /// The side moves to another thread and back (its job's thread).
    #[test]
    fn the_side_is_send() {
        fn send<T: Send>() {}
        send::<Side>();
    }
}
