//! The drawing side's own copy of the render layer's shared guest state (step 2 of the world's drawing
//! thread: the per-side render state the cut needs before the draw can leave main).
//!
//! The device model keeps one Direct3D state per producer (`engine_platform` d3d9draw.rs, piece 5), but the
//! game keeps its own record of that state in guest memory, one copy, which every draw reads and writes:
//! `NiDX9RenderState`'s cached render, stage and sampler states, textures, shaders and declaration (a setter
//! sends a call only when the cached value differs, then stores it), Bethesda's render state lock counters,
//! the texture stages' shadowed textures, the current pass, and the shader state a pass begin leaves (its
//! stages' textures, its constant entries and switches, the shaders' constant rows, which the device reads
//! by pointer). Run beside main's tail, the drawing side and main would each skip calls the other's entries
//! claim, on a device state of their own that never got them, and read each other's constant rows.
//!
//! So at the cut main opens a [`Side`] ([`open`]) over the ranges the draw reads before it writes (each
//! range's bytes are the snapshot and the drawing side's copy) and hands it to whoever draws ([`install`] on
//! that thread). On the drawing side the accessors ([`load_u32`] and its kin) read and write the copy and leave
//! guest memory alone; its device calls go to its own device state, which starts from main's at the cut, as
//! the copy does. The shader state a pass begin writes (the draw's own accessors, `engine_typed::draw::own`)
//! is copied word by word as the drawing side first writes it ([`store_own_u32`]): those words are shared
//! shader objects and constant rows, not a list known at the cut. A pointer argument the device model reads
//! (constant rows, a matrix) gets the drawing side's bytes laid over it ([`overlay_words`]).
//!
//! Main goes on in guest memory; while a side is open its routed writes note the bytes they change (a write
//! of the value already there changes nothing, as the setters then send nothing). At the join ([`join`], on
//! main, the side given back by [`uninstall`]) every byte the side holds is merged with the device model's
//! own rule (`d3d9draw::draw_join`): what main changed since the cut stays main's, everything else takes the
//! drawing side's value. A cache entry changes exactly when its setter sends the call, so the merged cache
//! describes the merged device state, and main's next setters skip only what the device holds.
//!
//! - A byte main changed without the routed accessors is found at the join as a guest byte that differs from
//!   the snapshot: it stays main's and is counted ([`JoinReport::unrouted`]): a routing still missing.
//! - [`Kind::Balanced`] ranges (the lock counters) are counters both sides move by adding: the join adds the
//!   drawing side's move to the live value, as the serial order would have it; a counter the drawing side left
//!   moved is counted ([`JoinReport::unbalanced`]: the original's accounting can leave one raised, and once the
//!   draw runs beside main, main's own setters read the count without that move until the join).
//! - [`Kind::Private`] ranges are the drawing side's scratch and never go back.
//! - A word copied at its first write takes its snapshot then, not at the cut: main's routed writes before it
//!   are noted and win; an unrouted one before it is not seen.
//! - The check ([`poison`], `NV_DRAW_THREAD=poison`): with the draw on main, the guest bytes of the fixed
//!   ranges are scribbled while the drawing side runs, so a read that misses the side sees the scribble, and
//!   a write that misses it is counted when they are put back.
//!
//! Off until something opens a side: then every accessor is the guest access it replaces behind one relaxed
//! load.

use crate::Engine;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// How a fixed range merges at the join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The device model's rule: main's changes since the cut stay, the rest takes the drawing side's bytes.
    Device,
    /// Counters both sides move by adding: the join adds the drawing side's move to the live value (words); a
    /// counter the drawing side left moved is counted.
    Balanced,
    /// The drawing side's own: never merged back.
    Private,
}

/// One fixed range of a side: its start, kind, the snapshot taken at the cut and the drawing side's copy.
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

    #[inline(always)]
    fn holds(&self, addr: u32) -> Option<usize> {
        self.offset(addr, 1)
    }
}

/// A word the drawing side copied as it first wrote it ([`store_own_u32`]): its bytes then and now.
#[derive(Clone, Copy)]
struct Word {
    snapshot: [u8; 4],
    copy: [u8; 4],
}

/// The drawing side's state: what [`open`] makes and [`install`] puts on the drawing thread. `Send`: it is made
/// on main and used on the drawing thread.
pub struct Side {
    /// The run that opened it (the kernel table's address, as the value packet's).
    run: usize,
    ranges: Vec<Range>,
    /// The words copied at their first write, by their 4-aligned address.
    words: HashMap<u32, Word>,
}

impl Side {
    /// The copy's byte at `addr`, if the side holds it.
    #[inline(always)]
    fn byte(&self, addr: u32) -> Option<u8> {
        for r in &self.ranges {
            if let Some(at) = r.holds(addr) {
                return Some(r.copy[at]);
            }
        }
        self.words
            .get(&(addr & !3))
            .map(|w| w.copy[(addr & 3) as usize])
    }

    /// Reads `out` from the copy where the side holds each byte, `guest` elsewhere; false when it holds none.
    fn read(&self, addr: u32, out: &mut [u8], guest: impl Fn(u32) -> u8) -> bool {
        let len = out.len() as u32;
        if let Some((r, at)) = self
            .ranges
            .iter()
            .find_map(|r| r.offset(addr, len).map(|at| (r, at)))
        {
            out.copy_from_slice(&r.copy[at..at + out.len()]);
            return true;
        }
        let mut any = false;
        for (i, slot) in (0u32..).zip(out.iter_mut()) {
            let a = addr.wrapping_add(i);
            *slot = match self.byte(a) {
                Some(b) => {
                    any = true;
                    b
                }
                None => guest(a),
            };
        }
        any
    }

    /// Writes `bytes` into the copy where the side holds each byte; with `own`, a word it does not hold yet is
    /// copied from guest memory first (`guest` answers its bytes, `None` when unmapped). False when a byte is
    /// neither held nor taken (the caller writes guest memory).
    fn write(
        &mut self,
        addr: u32,
        bytes: &[u8],
        own: bool,
        guest: impl Fn(u32) -> Option<[u8; 4]>,
    ) -> bool {
        let len = bytes.len() as u32;
        if let Some(r) = self
            .ranges
            .iter_mut()
            .find(|r| r.offset(addr, len).is_some())
        {
            let at = r.offset(addr, len).unwrap_or(0);
            r.copy[at..at + bytes.len()].copy_from_slice(bytes);
            return true;
        }
        // Every byte must land in the side: held already, or (own) its word taken now.
        for i in 0..len {
            let a = addr.wrapping_add(i);
            if self.ranges.iter().any(|r| r.holds(a).is_some()) {
                continue;
            }
            let key = a & !3;
            if let std::collections::hash_map::Entry::Vacant(slot) = self.words.entry(key) {
                if !own {
                    return false;
                }
                let Some(now) = guest(key) else {
                    return false;
                };
                // A word taken with nothing written yet is its snapshot: harmless at the join if the write
                // then lands in guest memory instead.
                slot.insert(Word {
                    snapshot: now,
                    copy: now,
                });
            }
        }
        for (i, &b) in (0u32..).zip(bytes) {
            let a = addr.wrapping_add(i);
            if let Some(r) = self.ranges.iter_mut().find(|r| r.holds(a).is_some()) {
                let at = r.holds(a).unwrap_or(0);
                r.copy[at] = b;
            } else if let Some(w) = self.words.get_mut(&(a & !3)) {
                w.copy[(a & 3) as usize] = b;
            }
        }
        true
    }
}

/// Main's record while a side is open: the fixed ranges with the bytes main's writes changed, and the bytes
/// main's routed writes changed outside them (for the words the drawing side copies as it writes).
struct MainLog {
    run: usize,
    ranges: Vec<(u32, Vec<bool>)>,
    others: HashSet<u32>,
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
/// the snapshot and the drawing side's copy, and main's routed writes are noted from now on. `None` (nothing
/// done) when this thread holds a side or a record already, or a range is not mapped or overlaps another.
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
        others: HashSet::new(),
    };
    MAIN.with(|m| *m.borrow_mut() = Some(log));
    ACTIVE.fetch_add(1, Relaxed);
    Some(Side {
        run: run(e),
        ranges: taken,
        words: HashMap::new(),
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
    /// Bytes main changed through the routed accessors (kept main's).
    pub main_changed: u64,
    /// Bytes main changed without them (kept main's; a routing still missing).
    pub unrouted: u64,
    /// Counters (words of balanced ranges) the drawing side left moved.
    pub unbalanced: u64,
    /// Words the drawing side copied as it first wrote them.
    pub own_words: u64,
}

/// Merges `side` (given back by [`uninstall`]) into guest memory on main and ends main's record ([`Kind`]):
/// a byte main changed since the cut (through the routed accessors, or found different from the snapshot)
/// stays main's; any other takes the drawing side's value; private ranges stay main's. The default report when
/// main holds no record for `e`'s run (the side is dropped).
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
    let mut report = JoinReport {
        own_words: side.words.len() as u64,
        ..JoinReport::default()
    };
    let merge = |live: &mut u8, snapshot: u8, copy: u8, main: bool, report: &mut JoinReport| {
        if main {
            report.main_changed += 1;
        } else if *live != snapshot {
            report.unrouted += 1;
        } else if *live != copy {
            *live = copy;
            report.taken += 1;
        }
    };
    for range in &side.ranges {
        if range.kind == Kind::Private {
            continue;
        }
        let changed = log
            .ranges
            .iter()
            .find(|(start, _)| *start == range.start)
            .map(|(_, c)| c.as_slice());
        let mut live = vec![0u8; range.copy.len()];
        if e.mem.try_read(range.start, &mut live).is_err() {
            continue;
        }
        if range.kind == Kind::Balanced {
            merge_counters(&mut live, range, changed, &mut report);
        } else {
            for (i, byte) in live.iter_mut().enumerate() {
                let main = changed.is_some_and(|c| c[i]);
                merge(byte, range.snapshot[i], range.copy[i], main, &mut report);
            }
        }
        e.mem.write(range.start, &live);
    }
    let mut words: Vec<(&u32, &Word)> = side.words.iter().collect();
    words.sort_by_key(|(a, _)| **a);
    for (&addr, word) in words {
        let mut live = [0u8; 4];
        if e.mem.try_read(addr, &mut live).is_err() {
            continue;
        }
        for (i, byte) in (0u32..).zip(live.iter_mut()) {
            let main = log.others.contains(&addr.wrapping_add(i));
            merge(
                byte,
                word.snapshot[i as usize],
                word.copy[i as usize],
                main,
                &mut report,
            );
        }
        e.mem.write(addr, &live);
    }
    if report.unrouted != 0 || report.unbalanced != 0 {
        noted(report);
    }
    report
}

/// A balanced range's words are counters, which both sides move by adding (Bethesda's render state locks): the
/// serial order's value is the cut's plus both sides' moves, so each word takes the drawing side's move on top
/// of the live value, whoever else moved it. A word the drawing side left moved is counted
/// ([`JoinReport::unbalanced`]: the original's own accounting can leave one raised, as the group draw's alpha
/// to coverage lock does on a card without it); main's routed and unrouted moves are counted as for the other
/// kinds. A tail that is not a whole word merges as a byte would.
fn merge_counters(
    live: &mut [u8],
    range: &Range,
    changed: Option<&[bool]>,
    report: &mut JoinReport,
) {
    let words = live.len() / 4;
    for k in 0..words {
        let at = 4 * k;
        let word = |b: &[u8]| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let (now, then, drawn) = (word(live), word(&range.snapshot), word(&range.copy));
        let moved = drawn.wrapping_sub(then);
        if moved != 0 {
            report.unbalanced += 1;
            report.taken += 1;
        }
        if changed.is_some_and(|c| c[at..at + 4].iter().any(|&b| b)) {
            report.main_changed += 1;
        } else if now != then {
            report.unrouted += 1;
        }
        live[at..at + 4].copy_from_slice(&now.wrapping_add(moved).to_le_bytes());
    }
    for i in 4 * words..live.len() {
        if !changed.is_some_and(|c| c[i]) && live[i] == range.snapshot[i] {
            live[i] = range.copy[i];
        }
    }
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

/// What [`poison`] scribbled: each fixed range's guest bytes as they were.
pub struct Poisoned {
    saved: Vec<(u32, Vec<u8>)>,
    scribble: u8,
}

/// The check (`NV_DRAW_THREAD=poison`, the draw still on main): every guest byte of `side`'s fixed ranges is
/// saved and set to `scribble`, so a read the side should answer and does not sees the scribble. Put back by
/// [`unpoison`].
pub fn poison(e: &mut Engine, side: &Side, scribble: u8) -> Poisoned {
    let mut saved = Vec::with_capacity(side.ranges.len());
    for r in &side.ranges {
        let mut bytes = vec![0u8; r.copy.len()];
        if e.mem.try_read(r.start, &mut bytes).is_ok() {
            e.mem.write(r.start, &vec![scribble; bytes.len()]);
            saved.push((r.start, bytes));
        }
    }
    Poisoned { saved, scribble }
}

/// Puts back what [`poison`] scribbled; a byte no longer the scribble (written while the draw ran, outside the
/// side) is left as written and counted: a write the side should have taken.
pub fn unpoison(e: &mut Engine, poisoned: Poisoned) -> u64 {
    let mut written = 0;
    for (start, saved) in poisoned.saved.into_iter().rev() {
        let mut live = vec![0u8; saved.len()];
        if e.mem.try_read(start, &mut live).is_err() {
            continue;
        }
        for (byte, old) in live.iter_mut().zip(&saved) {
            if *byte == poisoned.scribble {
                *byte = *old;
            } else {
                written += 1;
            }
        }
        e.mem.write(start, &live);
    }
    written
}

// ---- The accessors --------------------------------------------------------------------------------------

/// The slow path of a read: the drawing side's copy where it holds the bytes (the rest from guest memory).
#[cold]
#[inline(never)]
fn side_read(e: &Engine, addr: u32, out: &mut [u8]) -> bool {
    DRAWING.with(|d| {
        let d = d.borrow();
        let Some(side) = d.as_ref().filter(|s| s.run == run(e)) else {
            return false;
        };
        side.read(addr, out, |a| {
            let mut b = [0u8; 1];
            if e.mem.try_read(a, &mut b).is_ok() {
                b[0]
            } else {
                e.mem.u8(a)
            }
        })
    })
}

/// The slow path of a write: on the drawing side, into its copy (true; with `own`, a word not held yet is
/// copied first); on main with a record, the changed bytes noted (the write itself is the caller's, false).
#[cold]
#[inline(never)]
fn side_write(e: &Engine, addr: u32, bytes: &[u8], own: bool) -> bool {
    let drawing = DRAWING.with(|d| {
        let mut d = d.borrow_mut();
        let Some(side) = d.as_mut().filter(|s| s.run == run(e)) else {
            return false;
        };
        side.write(addr, bytes, own, |word| {
            let mut b = [0u8; 4];
            e.mem.try_read(word, &mut b).ok().map(|()| b)
        })
    });
    if drawing {
        return true;
    }
    MAIN.with(|m| {
        let mut m = m.borrow_mut();
        let Some(log) = m.as_mut().filter(|l| l.run == run(e)) else {
            return;
        };
        let mut old = vec![0u8; bytes.len()];
        if e.mem.try_read(addr, &mut old).is_err() || old == bytes {
            return;
        }
        // A whole value: the device model's key is the value, not its bytes.
        let len = bytes.len() as u32;
        for (start, changed) in &mut log.ranges {
            let at = addr.wrapping_sub(*start);
            if u64::from(at) + u64::from(len) <= changed.len() as u64 {
                changed[at as usize..at as usize + bytes.len()].fill(true);
                return;
            }
        }
        for i in 0..len {
            log.others.insert(addr.wrapping_add(i));
        }
    });
    false
}

/// Whether any thread holds a side or main's record of one (one relaxed load): callers that read guest memory
/// in a way the accessors cannot wrap (the device model reading what a pointer argument points to) test it and
/// then lay the drawing side's bytes over what they read ([`overlay_words`]).
#[inline(always)]
pub fn any_open() -> bool {
    ACTIVE.load(Relaxed) != 0
}

/// The run id [`overlay_words`] takes (the kernel table's address, as the accessors compare).
pub fn run_of(e: &Engine) -> usize {
    run(e)
}

/// On the drawing side of `run`'s open side: each byte of `words` (read from guest memory at `addr`, one word
/// after the other) that the side holds is replaced by the copy's. Nothing elsewhere. For a pointer argument's
/// data (a shader constant's registers, a transform's matrix) read by the device model, which reads it with the
/// device state already borrowed.
pub fn overlay_words(run: usize, addr: u32, words: &mut [u32]) {
    if !any_open() {
        return;
    }
    DRAWING.with(|d| {
        let d = d.borrow();
        let Some(side) = d.as_ref().filter(|s| s.run == run) else {
            return;
        };
        for (i, word) in (0u32..).zip(words.iter_mut()) {
            let mut b = word.to_le_bytes();
            for (k, byte) in (0u32..).zip(b.iter_mut()) {
                if let Some(copy) = side.byte(addr.wrapping_add(4 * i + k)) {
                    *byte = copy;
                }
            }
            *word = u32::from_le_bytes(b);
        }
    });
}

macro_rules! load {
    ($(#[$doc:meta])* $name:ident, $t:ty, $guest:ident) => {
        $(#[$doc])*
        #[inline(always)]
        pub fn $name(e: &Engine, addr: u32) -> $t {
            if ACTIVE.load(Relaxed) != 0 {
                let mut b = [0u8; std::mem::size_of::<$t>()];
                if side_read(e, addr, &mut b) {
                    return <$t>::from_le_bytes(b);
                }
            }
            e.mem.$guest(addr)
        }
    };
}

load!(
    /// A word: the drawing side's copy where it holds it, else guest memory.
    load_u32, u32, u32
);
load!(
    /// A half word: as [`load_u32`].
    load_u16, u16, u16
);
load!(
    /// A byte: as [`load_u32`].
    load_u8, u8, u8
);

/// A float: as [`load_u32`].
#[inline(always)]
pub fn load_f32(e: &Engine, addr: u32) -> f32 {
    f32::from_bits(load_u32(e, addr))
}

/// Writes a word: into the drawing side's copy where it holds it, else guest memory (main's change noted while
/// a side is open).
#[inline(always)]
pub fn store_u32(e: &Engine, addr: u32, value: u32) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &value.to_le_bytes(), false) {
        return;
    }
    e.mem.set_u32(addr, value);
}

/// Writes a byte: as [`store_u32`].
#[inline(always)]
pub fn store_u8(e: &Engine, addr: u32, value: u8) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &[value], false) {
        return;
    }
    e.mem.set_u8(addr, value);
}

/// Writes a float: as [`store_u32`].
#[inline(always)]
pub fn store_f32(e: &Engine, addr: u32, value: f32) {
    store_u32(e, addr, value.to_bits());
}

/// Writes a word of the shader state the draw's pass begins leave (shared shader objects, constant rows): on
/// the drawing side into its copy, the word copied from guest memory at its first write; elsewhere as
/// [`store_u32`].
#[inline(always)]
pub fn store_own_u32(e: &Engine, addr: u32, value: u32) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &value.to_le_bytes(), true) {
        return;
    }
    e.mem.set_u32(addr, value);
}

/// A float of that state: as [`store_own_u32`].
#[inline(always)]
pub fn store_own_f32(e: &Engine, addr: u32, value: f32) {
    store_own_u32(e, addr, value.to_bits());
}

/// A byte of that state: as [`store_own_u32`].
#[inline(always)]
pub fn store_own_u8(e: &Engine, addr: u32, value: u8) {
    if ACTIVE.load(Relaxed) != 0 && side_write(e, addr, &[value], true) {
        return;
    }
    e.mem.set_u8(addr, value);
}

/// Runs `f` as the drawing side with `side` installed on this thread, and gives the side back: for the checks
/// and for the draw while it still runs on main, both sides on one thread (main's record off the thread
/// meanwhile). A stop unwinding out of `f` takes the side off and puts main's record back first, so the next
/// cut finds the thread clean; the side is then dropped and main's record ended (nothing merged).
pub fn as_drawing_side<R>(side: Side, f: impl FnOnce() -> R) -> (R, Side) {
    let held = MAIN.with(|m| m.borrow_mut().take());
    if install(side).is_err() {
        unreachable!("a side was already installed on this thread");
    }
    if held.is_some() {
        ACTIVE.fetch_sub(1, Relaxed);
    }
    let put_back = |held: Option<MainLog>| {
        if let Some(log) = held {
            MAIN.with(|m| *m.borrow_mut() = Some(log));
            ACTIVE.fetch_add(1, Relaxed);
        }
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => {
            let side = uninstall().expect("the side installed above");
            put_back(held);
            (r, side)
        }
        Err(stop) => {
            drop(uninstall());
            if held.is_some() {
                // Main's record ends with the stop: nothing is merged.
                drop(held);
            }
            std::panic::resume_unwind(stop)
        }
    }
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

    /// Counters: both sides' moves add up, as in the serial order (main's +2 and the drawing side's +1 make +3),
    /// whoever moved last.
    #[test]
    fn counters_add_both_sides_moves() {
        let mut e = engine();
        e.mem.set_u32(A, 4);
        e.mem.set_u32(A + 4, 7);
        let side = open(&e, &[(A, 8, Kind::Balanced)]).unwrap();
        store_u32(&e, A, 6);
        let ((), side) = as_drawing_side(side, || {
            store_u32(&e, A, load_u32(&e, A) + 1);
            store_u32(&e, A + 4, load_u32(&e, A + 4) + 1);
            store_u32(&e, A + 4, load_u32(&e, A + 4) - 1);
        });
        let report = join(&mut e, side);
        assert_eq!(e.mem.u32(A), 7, "4 + main's 2 + the drawing side's 1");
        assert_eq!(e.mem.u32(A + 4), 7, "taken and given back");
        assert_eq!(
            (report.unbalanced, report.main_changed, report.unrouted),
            (1, 1, 0)
        );
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

    /// The words a pointer argument points to: the drawing side's copy laid over guest memory where it holds
    /// them; main reads guest memory.
    #[test]
    fn overlay_words_lays_the_copy_over_a_read() {
        let mut e = engine();
        e.mem.set_u32(A, 1);
        e.mem.set_u32(A + 4, 2);
        let side = open(&e, &[(A + 4, 4, Kind::Device)]).unwrap();
        let run = run_of(&e);
        let mut main = [e.mem.u32(A), e.mem.u32(A + 4)];
        overlay_words(run, A, &mut main);
        assert_eq!(main, [1, 2]);
        let (seen, side) = as_drawing_side(side, || {
            store_u32(&e, A + 4, 9);
            let mut words = [e.mem.u32(A), e.mem.u32(A + 4)];
            overlay_words(run, A, &mut words);
            (words, load_u16(&e, A + 4))
        });
        assert_eq!(seen, ([1, 9], 9));
        join(&mut e, side);
    }

    /// A word the drawing side first writes through `store_own_*` is copied then; main's routed change of it
    /// (before or after) stays main's at the join, the rest takes the drawing side's; the device model's read of
    /// it by pointer sees the copy on the drawing side.
    #[test]
    fn own_words_are_copied_at_their_first_write() {
        let mut e = engine();
        for k in 0..3 {
            e.mem.set_u32(A + 0x100 + 4 * k, k);
        }
        let side = open(&e, &[(A, 4, Kind::Device)]).unwrap();
        store_own_u32(&e, A + 0x104, 50);
        let run = run_of(&e);
        let ((), side) = as_drawing_side(side, || {
            store_own_u32(&e, A + 0x100, 7);
            store_own_u32(&e, A + 0x104, 8);
            store_own_u8(&e, A + 0x109, 9);
            assert_eq!(load_u32(&e, A + 0x100), 7);
            let mut words = [e.mem.u32(A + 0x100), e.mem.u32(A + 0x104)];
            overlay_words(run, A + 0x100, &mut words);
            assert_eq!(words, [7, 8]);
        });
        assert_eq!(e.mem.u32(A + 0x100), 0, "guest memory is main's");
        let report = join(&mut e, side);
        assert_eq!(e.mem.u32(A + 0x100), 7);
        assert_eq!(e.mem.u32(A + 0x104), 50, "main's change stays");
        assert_eq!(e.mem.u32(A + 0x108), 2 | (9 << 8));
        assert_eq!(report.own_words, 3);
        assert_eq!(report.unrouted, 0);
    }

    /// POISON: the fixed ranges' guest bytes are scribbled while the drawing side runs and put back after; a
    /// write that missed the side is counted.
    #[test]
    fn poison_scribbles_the_fixed_ranges() {
        let mut e = engine();
        e.mem.set_u32(A, 0x1234_5678);
        e.mem.set_u32(A + 8, 5);
        let side = open(&e, &[(A, 8, Kind::Device)]).unwrap();
        let poisoned = poison(&mut e, &side, 0x6e);
        assert_eq!(e.mem.u32(A), 0x6e6e_6e6e);
        let ((), side) = as_drawing_side(side, || {
            assert_eq!(load_u32(&e, A), 0x1234_5678, "the side answers");
            e.mem.set_u8(A + 4, 1);
        });
        assert_eq!(
            unpoison(&mut e, poisoned),
            1,
            "the write that missed the side"
        );
        assert_eq!(e.mem.u32(A), 0x1234_5678);
        join(&mut e, side);
    }

    /// A stop in the drawing side's work leaves the thread clean for the next cut.
    #[test]
    fn a_stop_leaves_the_thread_clean() {
        let e = engine();
        let side = open(&e, &[(A, 4, Kind::Device)]).unwrap();
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            as_drawing_side(side, || panic!("a stop"))
        }));
        assert!(stopped.is_err());
        assert!(!is_open(&e));
        let side = open(&e, &[(A, 4, Kind::Device)]).expect("the thread is clean");
        drop(side);
    }

    /// The side moves to another thread and back (its job's thread).
    #[test]
    fn the_side_is_send() {
        fn send<T: Send>() {}
        send::<Side>();
    }
}
