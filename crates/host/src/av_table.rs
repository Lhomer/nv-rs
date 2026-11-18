//! The game's actor-value table watched (Patrick's 14:05 and 14:49 crashes, both with New
//! Physics on: `HitMe` (`0089a760`) < `008d4870` (a creature's current actor value, slot `+0xC` of
//! its actor-value owner) < `008d4a80` < `00406d70` read `[011d61c8 + 4 * index]` = `0x3fc90fdb`
//! (pi/2 as a float) and faulted reading its flag word at `+0x44`).
//!
//! The table (`011d61c8`, one `ActorValueInfo` pointer for each of the 0x4d actor values, 0x134
//! bytes) is filled at start and never written again by the game. Two things could give pi/2:
//! - **An index past the table.** `Actor::HitMe`'s limb is the hit body part's `cActorValue`
//!   (`BGSBodyPart` `+0x63`, a signed byte); the limb-damage section returns with it set when it
//!   is outside 0..0x4c, and the death section (`0089bfaf`..) checks only for -1 before asking
//!   the actor-value owner for it (the exe does the same: fnv_decomp's `Actor::HitMe` reads
//!   `limbIndex = part->GetByte63()` and later `if (limbIndex != -1) avo.vf0c(limbIndex)`). Any
//!   other byte reads a word within 512 bytes of the table.
//! - **A writer.** Something (a queued Havok record pointing at a dead frame, a stray store)
//!   overwrote a word of the table.
//!
//! So:
//! - [`has_flags`] stands for `00406d70` (the game's own read): an index outside the table, or an
//!   entry that is no mapped pointer, is said with the index, the calls that led there and the
//!   word read (`[av table] 00406d70 ...`), before the read, so a crash's log names them.
//! - [`check`] compares the table with its baseline (said once, `[av table] baseline`, when first
//!   seen whole) before and after every hit (`knockdown::hit_me_hooked`), every write of the hit
//!   reactions (`active_ragdoll::apply`) and every New Physics pass (`new_physics::apply`,
//!   `main_thread`); a change is said with where it was seen and the words before and after
//!   (`[av table] changed ...`).
//!
//! The fix is the index's: `actor_p3::hit_me_death` treats a limb outside 0..0x4c as none.

use engine::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// The table and its length in words (the actor values, `0x4d`).
pub const TABLE: u32 = 0x011d_61c8;
pub const COUNT: u32 = 0x4d;
/// `00406d70`: `cdecl(index, mask)`, whether entry `index`'s flag word (`+0x44`) has a bit of
/// `mask`.
pub const HAS_FLAGS: u32 = 0x0040_6d70;

/// The table as first seen whole (every word non-zero).
static BASELINE: Mutex<Option<Vec<u32>>> = Mutex::new(None);
/// Reads by `00406d70` past the table (or of an entry that is no pointer), and changes seen.
static PAST: AtomicU32 = AtomicU32::new(0);
static CHANGED: AtomicU32 = AtomicU32::new(0);
static CHECKS: AtomicU32 = AtomicU32::new(0);
static READS: AtomicU32 = AtomicU32::new(0);

/// `00406d70`'s reads since [`clear`] (inside the table or not).
pub fn reads() -> u32 {
    READS.load(Ordering::Relaxed)
}

/// Reads by `00406d70` outside the table (or of an entry that is no mapped pointer) since
/// [`clear`].
pub fn reads_past() -> u32 {
    PAST.load(Ordering::Relaxed)
}

/// Changes of the table seen by [`check`] since [`clear`].
pub fn changes() -> u32 {
    CHANGED.load(Ordering::Relaxed)
}

/// The checks made since [`clear`] (that the watch ran at all).
pub fn checks() -> u32 {
    CHECKS.load(Ordering::Relaxed)
}

/// Forgets the baseline and the counts.
pub fn clear() {
    *BASELINE.lock().unwrap_or_else(|p| p.into_inner()) = None;
    PAST.store(0, Ordering::Relaxed);
    CHANGED.store(0, Ordering::Relaxed);
    CHECKS.store(0, Ordering::Relaxed);
    READS.store(0, Ordering::Relaxed);
}

fn note(line: String) {
    crate::room_events::record(
        "av_table",
        &format!("\"text\":{}", crate::room_events::text(&line)),
    );
    static N: Mutex<(Option<Instant>, u32)> = Mutex::new((None, 0));
    let mut n = N.lock().unwrap_or_else(|p| p.into_inner());
    if n.0.map_or(true, |t| t.elapsed().as_secs() >= 1) {
        *n = (Some(Instant::now()), 0);
    }
    if n.1 < 20 {
        n.1 += 1;
        eprintln!("{line}");
    }
}

/// The last few calls on this engine's call stack, innermost first.
fn callers(e: &Engine) -> String {
    e.call_stack
        .iter()
        .rev()
        .take(8)
        .map(|a| format!("{a:08x}"))
        .collect::<Vec<_>>()
        .join(" < ")
}

/// Registers [`has_flags`] for `00406d70` (from `crate::knockdown::install`).
pub fn install(e: &mut Engine) {
    e.register(HAS_FLAGS, |e, a| has_flags(e, a[0] as i32, a[1]).into_ret());
}

/// `00406d70` as the game runs it, but an index outside the table, or an entry that is no mapped
/// pointer, said first (the index, the calls that led here, the word read).
pub fn has_flags(e: &mut Engine, index: i32, mask: u32) -> bool {
    READS.fetch_add(1, Ordering::Relaxed);
    let at = TABLE.wrapping_add((index as u32).wrapping_mul(4));
    let entry = if e.mem.is_mapped(at) {
        e.mem.u32(at)
    } else {
        0
    };
    let inside = (0..COUNT as i32).contains(&index);
    if !inside || !e.mem.is_mapped(entry.wrapping_add(0x44)) {
        PAST.fetch_add(1, Ordering::Relaxed);
        note(format!(
            "[av table] 00406d70 index {index} ({}) from {}: [{at:#010x}] = {entry:#010x}{}",
            if inside {
                "inside the table"
            } else {
                "past the table"
            },
            callers(e),
            if e.mem.is_mapped(entry.wrapping_add(0x44)) {
                ""
            } else {
                ", no mapped entry: its +0x44 faults"
            }
        ));
    }
    engine::units::unplaced::run_00406cc0::fn_00406d70(e, index, mask)
}

/// The table compared with its baseline, `at` naming the point (`"before HitMe on ..."`). The
/// baseline is taken, and said, when the table is first seen whole; a change is said (the words
/// that differ, before and after) and becomes the baseline. Answers whether the table is as it
/// was.
pub fn check(e: &Engine, at: &str) -> bool {
    if !e.mem.is_mapped(TABLE) || !e.mem.is_mapped(TABLE + 4 * (COUNT - 1)) {
        return true;
    }
    let now: Vec<u32> = (0..COUNT).map(|i| e.mem.u32(TABLE + 4 * i)).collect();
    let mut base = BASELINE.lock().unwrap_or_else(|p| p.into_inner());
    CHECKS.fetch_add(1, Ordering::Relaxed);
    match base.as_ref() {
        None => {
            if now.iter().all(|&w| w != 0) {
                note(format!(
                    "[av table] baseline at 011d61c8 ({at}), {COUNT:#x} words: {}",
                    now.iter()
                        .map(|w| format!("{w:08x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                *base = Some(now);
            }
            true
        }
        Some(was) if *was != now => {
            CHANGED.fetch_add(1, Ordering::Relaxed);
            let changed: Vec<String> = was
                .iter()
                .zip(&now)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(8)
                .map(|(i, (a, b))| format!("[{i:#x}] +{:#x} {a:#010x} -> {b:#010x}", 4 * i))
                .collect();
            note(format!(
                "[av table] changed, seen {at}: {}",
                changed.join(", ")
            ));
            *base = Some(now);
            false
        }
        _ => true,
    }
}

#[cfg(test)]
#[path = "av_table_tests.rs"]
mod tests;
