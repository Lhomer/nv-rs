//! Quick start (`NV_QUICK_START`, `bQuickStart` in `[nv-rs Fixes]`), a row of the `fixes` preset
//! ([`crate::fixes`], docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the player
//! asked for (Patrick, 2026-11-15: start straight at the main menu with its buttons, to test
//! faster). In the `vanilla` preset, or with `bQuickStart=0`, the start is the original's. Nothing
//! here reads or writes a save.
//!
//! **What the original does at start.** Main plays the start-up movies, the comma-separated list
//! of the INI setting `0x011de72c` (`sIntroSequence`; empty in the shipped INI), with
//! `MoviePlayer::StartSequence` (`00ec16d0`, main's call after its renderer set-up). Then the
//! main menu (`StartMenu`) comes up holding back its buttons (flag `0x80000`) while the loading
//! menu shows its first screens one after another (`LoadingMenu::MakeLoadScreenList` on the
//! initial load: the welcome screens, the logos, then `Main`, each with its slide animation on the
//! model tile). When the last one has been reached the loading menu sets its state bit 0, and the
//! start menu's idle (`StartMenu::DoIdle`, `007cf9a0`) swaps the flag for `0x10`: the buttons. A
//! key or button pressed during the screens goes to the loading menu's handler (`0078aa90`),
//! which jumps to the last screen at once: the original's own skip.
//!
//! **What the row does.**
//! - The start-up movies: `StartSequence` called with the text of `sIntroSequence` while the game
//!   has not left its first load (`LoadingMenu::bInitialLoad`, `011a0294`) starts nothing. Any
//!   other sequence (the loading menu's own movie with the title music, `0x011de740`, which the
//!   original starts by the same call) plays as before, so the main menu's music and background
//!   are the original's.
//! - The screens: each frame of the start menu's idle while its buttons are held back, the
//!   loading menu's own skip (`0078aa90`) is pressed for the player whenever the next swap of
//!   screens is still ahead (the skip sets it to now, so once a swap). The rest is the original's: the slide to the last screen, its music start,
//!   the state bit, the swap to the buttons.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::startmenu::{start_menu_do_idle, StartMenu};
use std::sync::atomic::{AtomicBool, Ordering};

/// `MoviePlayer::StartSequence` (`00ec16d0`) and `StartMenu::DoIdle` (`007cf9a0`).
pub const START_SEQUENCE: u32 = 0x00ec_16d0;
pub const DO_IDLE: u32 = 0x007c_f9a0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "quick start",
    env: "NV_QUICK_START",
    addresses: &[START_SEQUENCE, DO_IDLE],
    summary: "the game starts straight at the main menu with its buttons: no start-up movies or logo screens",
    install,
};

// The game's words.
/// The `sIntroSequence` setting object; its text (`00403df0`, `thiscall`).
const INTRO_SEQUENCE: u32 = 0x011d_e72c;
const SETTING_TEXT: u32 = 0x0040_3df0;
/// `LoadingMenu::bInitialLoad` and `LoadingMenu::pMe`.
const INITIAL_LOAD: u32 = 0x011a_0294;
const LOADING_MENU: u32 = 0x011d_a0c0;
/// `LoadingMenu::uiNextBackgroundSwap` (a tick count) and the game's tick count (`00457fe0`).
const NEXT_BACKGROUND_SWAP: u32 = 0x1e0;
const TICK_COUNT: u32 = 0x0045_7fe0;
/// The loading menu's skip (`0078aa90`, `thiscall`), as `StartMenu::DoKey` calls it.
const LOADING_SKIP: u32 = 0x0078_aa90;
/// `StartMenu::uiFlags` (`+0x1a8`); the flag that holds the buttons back.
const MENU_FLAGS: u32 = 0x1a8;
const HELD_BACK: u32 = 0x8_0000;

static SAID_MOVIES: AtomicBool = AtomicBool::new(false);
static SAID_SCREENS: AtomicBool = AtomicBool::new(false);

/// Registers `StartSequence` and `StartMenu::DoIdle`.
pub fn install(e: &mut Engine) {
    e.register(START_SEQUENCE, |e, a| {
        if e.mem.u8(INITIAL_LOAD) != 0 && is_intro_sequence(e, a[1]) {
            if !SAID_MOVIES.swap(true, Ordering::Relaxed) {
                let text = String::from_utf8_lossy(&e.mem.cstr(a[1])).into_owned();
                eprintln!("Quick start: the start-up movies are skipped (sIntroSequence={text})");
            }
            return Ret::default();
        }
        let (_, start_sequence) = engine::entry!(
            0x00ec16d0,
            engine::units::bsmovie::movieplayer_xenon::movie_player_start_sequence(
                Ptr, u32, u8, u32, u8, u8, f32
            ) -> u32
        );
        start_sequence(e, a)
    });
    e.register(DO_IDLE, |e, a| {
        skip_screens(e, a[0]);
        start_menu_do_idle(e, Ptr::<StartMenu>::new(a[0]));
        Ret::default()
    });
}

/// Whether `text` is the text of `sIntroSequence`, and not empty.
fn is_intro_sequence(e: &mut Engine, text: u32) -> bool {
    if text == 0 {
        return false;
    }
    let intro = e.call(SETTING_TEXT, &args![INTRO_SEQUENCE]).u32();
    let wanted = e.mem.cstr(text);
    !wanted.is_empty() && (intro == text || (intro != 0 && e.mem.cstr(intro) == wanted))
}

/// The loading menu's own skip pressed while the start menu `menu` holds its buttons back for the
/// first screens and the next swap of screens is still ahead.
fn skip_screens(e: &mut Engine, menu: u32) {
    if menu == 0 || e.mem.u32(menu + MENU_FLAGS) & HELD_BACK == 0 || e.mem.u8(INITIAL_LOAD) == 0 {
        return;
    }
    let loading = e.mem.u32(LOADING_MENU);
    if loading == 0 {
        return;
    }
    // Pressed once for each swap the loading menu has set ahead: the skip sets it to now (the
    // swap then comes at once), and each swap sets the next one ahead again.
    let swap = e.mem.u32(loading + NEXT_BACKGROUND_SWAP);
    let now = e.call(TICK_COUNT, &args![]).u32();
    if !is_ahead(swap, now) {
        return;
    }
    if e.call(LOADING_SKIP, &args![loading]).bool() && !SAID_SCREENS.swap(true, Ordering::Relaxed) {
        eprintln!("Quick start: the start-up screens are skipped to the main menu");
    }
}

/// Whether the tick count `swap` is after `now` (within half the counter's range, so a wrap of
/// the count is no trouble).
fn is_ahead(swap: u32, now: u32) -> bool {
    let d = swap.wrapping_sub(now);
    d != 0 && d < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swap_is_ahead_until_its_time() {
        assert!(is_ahead(1500, 1000));
        assert!(!is_ahead(1000, 1000));
        assert!(!is_ahead(900, 1000));
        assert!(is_ahead(10, u32::MAX - 10));
        assert!(!is_ahead(u32::MAX - 10, 10));
    }
}
