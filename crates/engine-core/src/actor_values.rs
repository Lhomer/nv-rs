//! The actor values as nv-rs's own code (not a translation) asks for them: one guarded way in.
//!
//! The game keeps one `ActorValueInfo` per actor value in the table at `011d61c8`, `0x4d` of them
//! (0 to `0x4c`). An actor's actor-value owner (`+0xa4`, its slot `+0xC` the current value) looks
//! an index up there; a creature's (`008d4870` < `008d4a80` < `00406d70`) reads
//! `[011d61c8 + 4 * index]` and that entry's flag word at `+0x44` for any index it is given. Asked
//! for `0x4d` (`HitData::ClearData`'s "no skill", kept by a creature's bite) it read the word past
//! the table and faulted (Patrick's 16:17 crash: `crate::hit_force`'s melee momentum asked for
//! the hit's skill). So nv-rs's helpers ask through [`current`]: an index outside the table is no
//! value, and the owner is never asked.
//!
//! Each call is marked with its Rust caller ([`asking`]) while it runs, so a read past the table
//! that still happens says who asked (the host's `av_table` log).

use crate::args;
use crate::Engine;
use std::cell::Cell;
use std::panic::Location;

/// The actor values: indices 0 to `COUNT - 1`.
pub const COUNT: u32 = 0x4d;
/// The actor-value owner's place in an actor, and its current-value slot.
pub const OWNER: u32 = 0xa4;
pub const CURRENT: u32 = 0xc;

thread_local! {
    static ASKING: Cell<Option<&'static Location<'static>>> = const { Cell::new(None) };
}

/// The Rust code asking for an actor value through [`current`] on this thread now, if any.
pub fn asking() -> Option<&'static Location<'static>> {
    ASKING.with(Cell::get)
}

/// Whether `index` names an actor value (0 to `0x4c`).
pub fn valid(index: u32) -> bool {
    index < COUNT
}

/// `actor`'s current actor value `index` (its owner's slot `+0xC`), as a finite float; none for
/// no actor, an index outside the table (never asked), or a value that is not finite.
#[track_caller]
pub fn current(e: &mut Engine, actor: u32, index: u32) -> Option<f32> {
    if actor == 0 || !valid(index) {
        return None;
    }
    let was = ASKING.with(|a| a.replace(Some(Location::caller())));
    let v = e
        .vcall(actor.wrapping_add(OWNER), CURRENT, &args![index])
        .f64() as f32;
    ASKING.with(|a| a.set(was));
    v.is_finite().then_some(v)
}
