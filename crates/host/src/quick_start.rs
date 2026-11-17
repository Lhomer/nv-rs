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
//! **What the row does** (Patrick, 2026-11-17: click Play and land on the main menu, nothing
//! before it). Every screen the game shows between the launch and the main menu's first frame
//! with its buttons is in [`STARTUP`], with what quick start does with it:
//! - The start-up movies: `StartSequence` called with the text of `sIntroSequence` while the game
//!   has not left its first load (`LoadingMenu::bInitialLoad`, `011a0294`) starts nothing.
//! - The first load's screens: the loading menu builds them from the five settings at `011da1b0`
//!   (`Interface\Loading\<name>.dds`, the last from `Main`: the publisher's and developer's
//!   logos, the legal screen, the title card, then the menu's own backdrop). Its
//!   `MakeLoadScreenList` (`0078a5a0`) is run, then on the first load every screen but the last is
//!   dropped before anything is drawn ([`keep_last_screen`]): no logo texture is ever loaded.
//! - The screens' timers: the start menu holds its buttons back (flag `0x80000`) until the
//!   loading menu has reached its last screen (its state bit 0). Once the title music has started
//!   (or none is set), that bit is set for it ([`release_buttons`]): the start menu's own idle
//!   swaps the flag for its buttons the same frame.
//! - Kept, as the original has them: the last screen (the main menu's backdrop) and the loading
//!   menu's own movie with the title music (`0x011de740`, started by the loading menu's update
//!   in its first 3 s), so the main menu's music and background are the original's.
//!
//! One line at the main menu says what was skipped and the time since nv-rs started
//! ([`said_at_menu`]). The fixes preset (and with it this row) is on in Play, the Test Range and
//! the physics tester alike.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::loadingmenu::{
    fn_0078b3f0, loading_menu_make_load_screen_list, LoadingMenu,
};
use engine::units::fallout_interface::startmenu::{start_menu_do_idle, StartMenu};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// `MoviePlayer::StartSequence` (`00ec16d0`), `StartMenu::DoIdle` (`007cf9a0`) and
/// `LoadingMenu::MakeLoadScreenList` (`0078a5a0`).
pub const START_SEQUENCE: u32 = 0x00ec_16d0;
pub const DO_IDLE: u32 = 0x007c_f9a0;
pub const MAKE_LIST: u32 = 0x0078_a5a0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "quick start",
    env: "NV_QUICK_START",
    addresses: &[START_SEQUENCE, DO_IDLE, MAKE_LIST],
    summary: "the game starts straight at the main menu with its buttons: no start-up movies, logo, legal or title screens",
    install,
};

/// What the game shows between the launch and the main menu, and whether quick start skips it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `sIntroSequence` (`011de72c`): main's start-up movies.
    IntroMovies,
    /// The first load's screen at this place of the five settings at `011da1b0`.
    FirstLoadScreen(u8),
    /// The loading menu's movie with the title music (`011de740`).
    TitleMovie,
    /// The start menu's buttons held back (flag `0x80000`) for the screens' timers.
    ButtonsHeldBack,
}

/// How many screens the first load shows (the settings at `011da1b0`, 12 bytes apart: the array
/// its `atexit` destructor walks).
pub const FIRST_LOAD_SCREENS: u8 = 5;

/// Every start-up screen ([`Source`]), what it is, and whether quick start skips it.
pub const STARTUP: [(Source, &str, bool); 8] = [
    (
        Source::IntroMovies,
        "the start-up movies (sIntroSequence: the publisher's and developer's logo movies, the intro)",
        true,
    ),
    (
        Source::FirstLoadScreen(0),
        "first-load screen 1 (Interface\\Loading: a logo)",
        true,
    ),
    (
        Source::FirstLoadScreen(1),
        "first-load screen 2 (Interface\\Loading: a logo)",
        true,
    ),
    (
        Source::FirstLoadScreen(2),
        "first-load screen 3 (Interface\\Loading: the legal and warning screen)",
        true,
    ),
    (
        Source::FirstLoadScreen(3),
        "first-load screen 4 (Interface\\Loading: the title card)",
        true,
    ),
    (
        Source::FirstLoadScreen(4),
        "first-load screen 5 (Main: the main menu's own backdrop)",
        false,
    ),
    (
        Source::TitleMovie,
        "the title movie and music (the main menu's own)",
        false,
    ),
    (
        Source::ButtonsHeldBack,
        "the screens' timers holding the main menu's buttons back",
        true,
    ),
];

// The game's words.
/// The `sIntroSequence` setting object; its text (`00403df0`, `thiscall`).
const INTRO_SEQUENCE: u32 = 0x011d_e72c;
const SETTING_TEXT: u32 = 0x0040_3df0;
/// The loading menu's title movie (with the title music) setting.
const TITLE_MOVIE: u32 = 0x011d_e740;
/// `LoadingMenu::bInitialLoad`, `LoadingMenu::pMe`, and the title music's started byte.
const INITIAL_LOAD: u32 = 0x011a_0294;
const LOADING_MENU: u32 = 0x011d_a0c0;
const MUSIC_STARTED: u32 = 0x011d_a0c9;
/// The loading menu's state bits (`00458110` test, `00458080` set: `thiscall` (bit[, value])).
const FLAG_TEST: u32 = 0x0045_8110;
const FLAG_SET: u32 = 0x0045_8080;
/// `StartMenu::uiFlags` (`+0x1a8`); the flag that holds the buttons back.
const MENU_FLAGS: u32 = 0x1a8;
const HELD_BACK: u32 = 0x8_0000;
/// A load screen made for the first load is temporary (`004077c0`) and deleted by its virtual
/// slot `0x10` (argument 1); `BSSimpleList`'s empty (`00470470`) and append (`00905820`).
const IS_TEMPORARY: u32 = 0x0040_77c0;
const LIST_EMPTY: u32 = 0x0047_0470;
const LIST_ADD: u32 = 0x0090_5820;
/// `LoadingMenu::CalcFaces` (`0078bb40`).
const CALC_FACES: u32 = 0x0078_bb40;

/// What was skipped, for the line at the main menu.
static MOVIES: Mutex<String> = Mutex::new(String::new());
static SCREENS_DROPPED: AtomicU32 = AtomicU32::new(0);
static RELEASED: AtomicBool = AtomicBool::new(false);
static SAID: AtomicBool = AtomicBool::new(false);
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Registers `StartSequence`, `MakeLoadScreenList` and `StartMenu::DoIdle`.
pub fn install(e: &mut Engine) {
    STARTED.get_or_init(Instant::now);
    e.register(START_SEQUENCE, |e, a| {
        if e.mem.u8(INITIAL_LOAD) != 0 && is_intro_sequence(e, a[1]) {
            let text = String::from_utf8_lossy(&e.mem.cstr(a[1])).into_owned();
            let mut said = MOVIES.lock().unwrap_or_else(|p| p.into_inner());
            if said.is_empty() {
                *said = text;
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
    e.register(MAKE_LIST, |e, a| {
        let menu = Ptr::<LoadingMenu>::new(a[0]);
        loading_menu_make_load_screen_list(e, menu);
        if e.mem.u8(INITIAL_LOAD) != 0 {
            let dropped = keep_last_screen(e, menu);
            SCREENS_DROPPED.fetch_max(dropped, Ordering::Relaxed);
        }
        Ret::default()
    });
    e.register(DO_IDLE, |e, a| {
        release_buttons(e, a[0]);
        start_menu_do_idle(e, Ptr::<StartMenu>::new(a[0]));
        said_at_menu(e, a[0]);
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

/// The first load's screen list of the loading menu `menu` cut to its last screen (the main
/// menu's backdrop), before anything is drawn: the dropped (temporary) screens deleted, the last
/// made current and shown. Answers how many were dropped.
pub fn keep_last_screen(e: &mut Engine, menu: Ptr<LoadingMenu>) -> u32 {
    let head = menu.at(LoadingMenu::LoadScreens).addr();
    let mut items = Vec::new();
    let mut node = head;
    while node != 0 && items.len() < 64 {
        let item = e.mem.u32(node);
        if item == 0 {
            break;
        }
        items.push(item);
        node = e.mem.u32(node + 4);
    }
    let Some((&last, dropped)) = items.split_last() else {
        return 0;
    };
    if dropped.is_empty() {
        return 0;
    }
    for &item in dropped {
        if e.call(IS_TEMPORARY, &args![item]).bool() {
            e.vcall(item, 0x10, &args![1u32]);
        }
    }
    e.call(LIST_EMPTY, &args![head]);
    e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), last);
        e.call(LIST_ADD, &args![head, cell]);
    });
    e.set(menu, LoadingMenu::pNextLoadScreen, Ptr::new(head));
    e.set(menu, LoadingMenu::pCurrentLoadScreen, Ptr::new(last));
    e.call(CALC_FACES, &args![menu]);
    fn_0078b3f0(e, menu, last, 1);
    dropped.len() as u32
}

/// While the start menu `menu` holds its buttons back on the first load and the title music has
/// started (or none is set), the loading menu is marked as at its last screen: the start menu's
/// idle swaps in its buttons. (Marked before the music starts, the music never would.)
fn release_buttons(e: &mut Engine, menu: u32) {
    if menu == 0 || e.mem.u32(menu + MENU_FLAGS) & HELD_BACK == 0 || e.mem.u8(INITIAL_LOAD) == 0 {
        return;
    }
    let loading = e.mem.u32(LOADING_MENU);
    if loading == 0 || e.call(FLAG_TEST, &args![loading, 0u32]).bool() {
        return;
    }
    let title = e.call(SETTING_TEXT, &args![TITLE_MOVIE]).u32();
    let no_music = title == 0 || e.mem.cstr(title).is_empty();
    if e.mem.u8(MUSIC_STARTED) != 0 || no_music {
        e.call(FLAG_SET, &args![loading, 0u32, 1u32]);
        RELEASED.store(true, Ordering::Relaxed);
    }
}

/// Once, the first frame the start menu `menu` shows its buttons: what was skipped and how long
/// since nv-rs started.
fn said_at_menu(e: &mut Engine, menu: u32) {
    if menu == 0 || e.mem.u32(menu + MENU_FLAGS) & HELD_BACK != 0 || SAID.load(Ordering::Relaxed) {
        return;
    }
    SAID.store(true, Ordering::Relaxed);
    let movies = MOVIES.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let seconds = STARTED.get().map_or(0.0, |t| t.elapsed().as_secs_f32());
    eprintln!(
        "{}",
        startup_line(
            seconds,
            &movies,
            SCREENS_DROPPED.load(Ordering::Relaxed),
            RELEASED.load(Ordering::Relaxed)
        )
    );
}

/// The line at the main menu.
pub fn startup_line(seconds: f32, movies: &str, screens: u32, released: bool) -> String {
    let mut skipped = Vec::new();
    if !movies.is_empty() {
        skipped.push(format!("the start-up movies ({movies})"));
    }
    if screens > 0 {
        skipped.push(format!(
            "{screens} first-load screens (logos, legal, title card)"
        ));
    }
    if released {
        skipped.push("the screens' timers".to_string());
    }
    format!(
        "Quick start: at the main menu {seconds:.2} s after nv-rs started; skipped: {}",
        if skipped.is_empty() {
            "nothing (none shown)".to_string()
        } else {
            skipped.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The skip list covers every start-up screen the game has (the intro movies, each of the
    /// first load's five screens, the title movie, the buttons held back), each once; only the
    /// main menu's own (its backdrop, its title movie and music) are kept.
    #[test]
    fn the_skip_list_covers_every_startup_screen() {
        let mut every = vec![
            Source::IntroMovies,
            Source::TitleMovie,
            Source::ButtonsHeldBack,
        ];
        every.extend((0..FIRST_LOAD_SCREENS).map(Source::FirstLoadScreen));
        for source in &every {
            let n = STARTUP.iter().filter(|(s, _, _)| s == source).count();
            assert_eq!(n, 1, "{source:?} listed {n} times");
        }
        assert_eq!(STARTUP.len(), every.len());
        for (source, what, skipped) in STARTUP {
            let menus_own = source == Source::TitleMovie
                || source == Source::FirstLoadScreen(FIRST_LOAD_SCREENS - 1);
            assert_eq!(skipped, !menus_own, "{what}");
        }
    }

    /// The first load's list cut to its last screen: the others (temporary) deleted, the last
    /// current and alone in the list.
    #[test]
    fn the_first_load_keeps_only_its_last_screen() {
        let mut e = Engine::new();
        e.mem.map(0x10000, 0x10000);
        let menu = 0x10000u32;
        let head = menu + 0x1d0;
        // Five screens in the list: the head node holds the first, then four chained nodes.
        let items = [0x12000u32, 0x12100, 0x12200, 0x12300, 0x12400];
        e.mem.set_u32(head, items[0]);
        let mut prev = head;
        for (k, &item) in items.iter().enumerate().skip(1) {
            let node = 0x13000 + k as u32 * 0x10;
            e.mem.set_u32(node, item);
            e.mem.set_u32(prev + 4, node);
            prev = node;
        }
        let deleted = std::sync::Arc::new(Mutex::new(Vec::new()));
        let vtable = 0x14000u32;
        let delete = 0x7000_0100u32;
        let d = deleted.clone();
        e.register_double(delete, move |_, a| {
            d.lock().unwrap().push(a[0]);
            0u32.into_ret()
        });
        e.mem.set_u32(vtable + 0x10, delete);
        for &item in &items {
            e.mem.set_u32(item, vtable);
        }
        e.register_double(IS_TEMPORARY, |_, _| 1u32.into_ret());
        e.register_double(LIST_EMPTY, |e, a| {
            e.mem.set_u32(a[0], 0);
            e.mem.set_u32(a[0] + 4, 0);
            0u32.into_ret()
        });
        e.register_double(LIST_ADD, |e, a| {
            let item = e.mem.u32(a[1]);
            e.mem.set_u32(a[0], item);
            0u32.into_ret()
        });
        e.register_double(CALC_FACES, |_, _| 0u32.into_ret());
        let dropped = keep_last_screen(&mut e, Ptr::new(menu));
        assert_eq!(dropped, 4);
        assert_eq!(*deleted.lock().unwrap(), items[..4].to_vec());
        assert_eq!(e.mem.u32(head), items[4]);
        assert_eq!(e.mem.u32(head + 4), 0);
        let m = Ptr::<LoadingMenu>::new(menu);
        assert_eq!(e.get(m, LoadingMenu::pCurrentLoadScreen).addr(), items[4]);
        assert_eq!(e.get(m, LoadingMenu::pNextLoadScreen).addr(), head);
        // A list of one is left as it is.
        assert_eq!(keep_last_screen(&mut e, Ptr::new(menu)), 0);
    }

    /// The line at the main menu says what was skipped and the time.
    #[test]
    fn the_line_at_the_menu() {
        let line = startup_line(2.5, "Bethesda.bik,Obsidian.bik", 4, true);
        assert!(line.contains("2.50 s"), "{line}");
        assert!(line.contains("Bethesda.bik,Obsidian.bik"), "{line}");
        assert!(line.contains("4 first-load screens"), "{line}");
        assert!(line.contains("timers"), "{line}");
        assert!(startup_line(1.0, "", 0, false).contains("nothing"));
    }
}
