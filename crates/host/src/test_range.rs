//! The Test Range (weighty combat's slice 2, docs/GUNPLAY.md): a main-menu entry that starts a
//! throwaway game in a quiet test cell with targets, every weapon and ammo, to try and tune the
//! weighty combat (recoil, sway, impact punch, reach, hit reactions, dismemberment). Part of the
//! weighty combat row ([`crate::gunplay::FIX`]); **not 1:1 with FalloutNV.exe**.
//!
//! **Only the game's own content and machinery.** The range is the game's own `QASmoke` test cell
//! (FalloutNV.esm, the developers' item hall; `[nv-rs Gunplay] sTestRangeCell` names another).
//! Everything is done the way the console does it: the console's own compile-and-run of a line
//! ([`run_line`], as `MenuConsole::Idle` runs a typed line) for `coc`, `MoveTo`, `AddItem`,
//! `SetRestrained` and the rest, the game's `PlaceAtMe` worker (`005c4b30`) for the references
//! it needs back, its message box (`00703e80`) for the range menu, and its return to the main
//! menu (`StartMenu::ChooseMainMenu`, `007d0a70`). Targets are base forms the loaded plugins
//! hold, found by their names; weapons are every playable weapon of every loaded plugin and DLC.
//! No asset is added.
//!
//! **Flow.** The main menu's "Test Range" (made with the game's own options, just above Quit)
//! runs `coc <cell>`: from the main menu the game starts a fresh game there, with a default
//! character, no save needed. The `CenterOnCell` new game's seam (`0093db69`) marks the range
//! active. Some frames into play the range is set up ([`setup`]): a container with one of every
//! weapon, 500 of every ammo, and a lane of targets in front of the player at 5 to 100 m, passive
//! (`SetRestrained 1`: they stand and take hits). Home opens the range menu: Weapons (the
//! container), Fill Ammo, Make Hostile / Make Passive, Respawn Targets (a fresh lane down the line
//! the player now faces), Exit to Main Menu, Close.
//!
//! **The player's saves are never touched.** While the range is active every save is refused:
//! `SaveGame` (`008503b0`: the menus' saves and the `SaveGame` command), the quick save
//! (`008509a0`) and the queued autosave, forced and system saves (`00851d90`, whose queue is
//! emptied). Nothing reaches the save folder, so the copy-out (`NV_SAVE_OUT`) has nothing to copy
//! or delete. The range ends with a save loaded (seam `00847fd9`), a new game from the menu
//! (`007d33ce`) or the return to the main menu (`007d0b17`).

use engine::extend::{Owner, SiteSlot};
use engine::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

/// The weighty combat row as the owner of its seams (`crate::gunplay::FIX`'s name).
pub const ROW: Owner = Owner::Row("weighty combat");
/// The seams: a new game through `CenterOnCell` from the main menu, a new game from the menu, a
/// save about to load, the return to the main menu.
pub const COC_NEW_GAME: SiteSlot = SiteSlot::new(ROW, 0x0093_db69);
pub const MENU_NEW_GAME: SiteSlot = SiteSlot::new(ROW, 0x007d_33ce);
pub const PRE_LOAD: SiteSlot = SiteSlot::new(ROW, 0x0084_7fd9);
pub const TO_MAIN_MENU: SiteSlot = SiteSlot::new(ROW, 0x007d_0b17);

/// `BGSSaveLoadManager::SaveGame` (`thiscall`: name, device, statistics), the quick save
/// (`thiscall`) and `UpdateQueuedSaves` (`thiscall`).
pub const SAVE_GAME: u32 = 0x0085_03b0;
pub const QUICK_SAVE: u32 = 0x0085_09a0;
pub const QUEUED_SAVES: u32 = 0x0085_1d90;
/// The save manager's queue bytes: autosave, forced save, system save.
const QUEUE_BYTES: [u32; 3] = [0x0f, 0x10, 0x11];

/// The main menu option's choose function and the range menu's answer callback: code addresses
/// after weighty combat's Settings rows' (`crate::gunplay::CHANGE_FNS`), in the platform's area.
pub const CHOOSE_FN: u32 = 0x7e06_00c0;
pub const MENU_CALLBACK: u32 = 0x7e06_00b0;

/// The cell the range starts in when the file names none.
pub const DEFAULT_CELL: &str = "QASmoke";

static CELL: Mutex<String> = Mutex::new(String::new());

/// The range's cell (`sTestRangeCell`, `crate::fixes::install`): an editor id of letters, digits
/// and `_`, up to 64; anything else keeps [`DEFAULT_CELL`].
pub fn set_cell(name: &str) {
    let name = if valid_cell(name) { name } else { DEFAULT_CELL };
    *CELL.lock().unwrap_or_else(|p| p.into_inner()) = name.to_string();
}

fn cell() -> String {
    let cell = CELL.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if cell.is_empty() {
        DEFAULT_CELL.to_string()
    } else {
        cell
    }
}

/// Whether `name` can be a cell's editor id on a console line: 1 to 64 letters, digits or `_`.
pub fn valid_cell(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// --- The state. --------------------------------------------------------------------------------

/// The range under way.
#[derive(Debug, Default)]
struct Range {
    /// The main menu's entry was chosen and its `coc` runs.
    starting: bool,
    /// A range game is the one running.
    active: bool,
    /// Frames of play since the range game started, until it is set up.
    frames: u32,
    set_up: bool,
    /// The weapons container and the targets (reference ids), whether the targets fight.
    container: u32,
    targets: Vec<u32>,
    hostile: bool,
    /// Every ammo the loaded weapons use (form ids).
    ammo: Vec<u32>,
}

static RANGE: Mutex<Option<Range>> = Mutex::new(None);
/// The range menu's answer, until the next frame of play acts on it (`0xff`: none).
static ANSWER: AtomicU8 = AtomicU8::new(NO_ANSWER);
const NO_ANSWER: u8 = 0xff;
/// Home was down at the last look.
static KEY_DOWN: AtomicBool = AtomicBool::new(false);

fn with_range<R>(f: impl FnOnce(&mut Range) -> R) -> R {
    let mut guard = RANGE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(Range::default))
}

/// Whether a range game is running.
pub fn active() -> bool {
    with_range(|r| r.active)
}

/// The range ends (a save loads, a new game, the main menu): nothing of it is kept.
fn end(_: &mut Engine, _: &[u32]) -> Option<u32> {
    with_range(|r| *r = Range::default());
    ANSWER.store(NO_ANSWER, Ordering::Relaxed);
    None
}

// --- The hooks. --------------------------------------------------------------------------------

/// Registers the range's hooks, seams and functions (from `crate::gunplay::install`).
pub fn install(e: &mut Engine) {
    use engine::units::fallout_misc::bgssaveloadmanager as manager;
    e.register(CHOOSE_FN, |e, _| {
        start(e);
        Ret::default()
    });
    e.register(MENU_CALLBACK, |e, _| {
        let answer = e.call(GET_MESSAGE_MENU_RESULT, &[]).u8();
        ANSWER.store(answer, Ordering::Relaxed);
        Ret::default()
    });
    e.register(SAVE_GAME, |e, a| {
        if active() {
            refused(e, a[1] != 0 && is_manual(e, a[1]));
            return Ret::default();
        }
        let saved = manager::bgssaveloadmanager_save_game(
            e,
            Ptr::new(a[0]),
            Ptr::new(a[1]),
            a[2],
            a[3] & 0xff != 0,
        );
        u32::from(saved).into_ret()
    });
    e.register(QUICK_SAVE, |e, a| {
        if active() {
            refused(e, true);
            return Ret::default();
        }
        manager::fn_008509a0(e, Ptr::new(a[0]));
        Ret::default()
    });
    e.register(QUEUED_SAVES, |e, a| {
        if active() {
            for byte in QUEUE_BYTES {
                e.mem.set_u8(a[0] + byte, 0);
            }
            return Ret::default();
        }
        manager::bgssaveloadmanager_update_queued_saves(e, Ptr::new(a[0]));
        Ret::default()
    });
    e.on_site(COC_NEW_GAME, |_, _| {
        with_range(|r| {
            if r.starting {
                *r = Range {
                    active: true,
                    ..Range::default()
                };
            } else {
                *r = Range::default();
            }
        });
        None
    });
    e.on_site(MENU_NEW_GAME, end);
    e.on_site(PRE_LOAD, end);
    e.on_site(TO_MAIN_MENU, end);
}

/// Whether a save's name is the player's own (not the autosave or quick save names, which the
/// game saves quietly).
fn is_manual(e: &Engine, name: u32) -> bool {
    let name = e.mem.cstr(name).to_ascii_lowercase();
    !(name.starts_with(b"autosave") || name.starts_with(b"quicksave"))
}

/// A refused save: one line in the log, and for a save the player asked for a note on screen.
fn refused(e: &mut Engine, asked: bool) {
    eprintln!("Test Range: a save was refused (the range never saves)");
    if asked {
        note(e, "Saving is off in the Test Range.");
    }
}

/// A note on the screen (`QueueUIMessage`, two seconds).
fn note(e: &mut Engine, text: &str) {
    e.with_stack(0x100, |e, at| {
        let bytes = &text.as_bytes()[..text.len().min(0xff)];
        e.mem.set_cstr(at.addr(), bytes);
        e.call(
            QUEUE_UI_MESSAGE,
            &args![at.addr(), 0u32, 0u32, 0u32, 3.0f32, 0u32],
        );
    });
}

/// The main menu's entry: the range's cell, from the main menu, as the console's `coc` starts
/// a game there.
///
/// Until 2026-11-14 it ran the line on the console only when the console already existed: the
/// game makes it on first use (`MenuConsole::Instance(1)`, `0071b160`), so on a start where the
/// console had not been opened the click did nothing (Patrick, build 68956e3). Now the console is
/// made the game's way ([`run_line`]), the cell is looked up first, and anything that stops the
/// start is said in the log and on the screen, with the menu's list enabled again.
fn start(e: &mut Engine) {
    let cell = cell();
    eprintln!("Test Range: starting in {cell}");
    let outcome = start_outcome(
        cell_exists(e, &cell),
        || {
            with_range(|r| {
                *r = Range {
                    starting: true,
                    ..Range::default()
                }
            });
            run_line(e, &format!("coc {cell}"))
        },
        active,
    );
    if let Err(why) = outcome {
        let text = why.text(&cell);
        eprintln!("{text}");
        with_range(|r| *r = Range::default());
        fail_on_menu(e, &text);
    }
}

/// Why the range did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// No cell of the name in the loaded game (`sTestRangeCell`).
    NoCell,
    /// The game's console, whose compiler runs the `coc`, could not be had.
    NoConsole,
    /// The `coc` ran, but no new game began in the cell.
    NotStarted,
}

impl StartError {
    /// The line for the log and the screen.
    pub fn text(self, cell: &str) -> String {
        match self {
            StartError::NoCell => format!(
                "Test Range: no cell named {cell} in the loaded game. Set sTestRangeCell in Fallout.ini's [nv-rs Gunplay] to a cell's editor id."
            ),
            StartError::NoConsole => {
                "Test Range: the game's console could not be made, so the range could not start."
                    .to_string()
            }
            StartError::NotStarted => format!(
                "Test Range: the game did not start in {cell}. The game's log says more."
            ),
        }
    }
}

/// The start's steps in order: the cell must exist, then the `coc` must run (`run`, which marks
/// the start), then the new game must have begun (`started`, the `CenterOnCell` new game's seam).
pub fn start_outcome(
    cell_found: bool,
    run: impl FnOnce() -> bool,
    started: impl FnOnce() -> bool,
) -> Result<(), StartError> {
    if !cell_found {
        return Err(StartError::NoCell);
    }
    if !run() {
        return Err(StartError::NoConsole);
    }
    if !started() {
        return Err(StartError::NotStarted);
    }
    Ok(())
}

/// Whether the loaded game has a cell named `name`: an interior by its editor id
/// (`TESDataHandler` `00461ae0`) or an exterior one (`00461cf0`, its worldspace), as
/// `CenterOnCell` looks it up.
fn cell_exists(e: &mut Engine, name: &str) -> bool {
    let handler = e.global::<u32>(DATA_HANDLER);
    if handler == 0 {
        return false;
    }
    e.with_stack(0x60, |e, frame| {
        let text = frame.addr();
        let coordinates = text + 0x48;
        e.mem.set_cstr(text, name.as_bytes());
        if e.call(CELL_BY_EDITOR_ID, &args![handler, text]).u32() != 0 {
            return true;
        }
        e.mem.set_u32(coordinates, 0);
        e.mem.set_u32(coordinates + 4, 0);
        e.call(
            EXTERIOR_BY_EDITOR_ID,
            &args![handler, text, coordinates, coordinates + 4],
        )
        .u32()
            != 0
    })
}

/// A failed start on the main menu: the menu's main list enabled again (the menu disables it
/// before it calls an option's choose function) and the reason in the game's message box.
fn fail_on_menu(e: &mut Engine, text: &str) {
    let menu = e.global::<u32>(START_MENU);
    if menu != 0 {
        e.call(LIST_BOX_SET_ENABLED, &args![menu + MAIN_LIST, 1u32]);
    }
    e.with_stack(0x300, |e, frame| {
        let base = frame.addr();
        let bytes = &text.as_bytes()[..text.len().min(0x27f)];
        e.mem.set_cstr(base, bytes);
        let ok = base + 0x280;
        e.mem.set_cstr(ok, b"OK");
        let mut words = vec![base, 0, 0, GAME_BOX_CALLBACK, 0, 0x17, 0, 0, ok];
        words.extend_from_slice(&[0u32; 9]);
        words.push(0);
        e.call(SHOW_MESSAGE_BOX, &words);
    });
}

/// Once a frame of play (after weighty combat's frame): the set-up when it is due, the Home key,
/// the range menu's answer.
pub fn frame(e: &mut Engine, player: u32) {
    let (active, due) = with_range(|r| {
        if r.active && !r.set_up {
            r.frames += 1;
        }
        (r.active, r.active && !r.set_up && r.frames >= SETUP_FRAMES)
    });
    if !active {
        return;
    }
    if due && e.mem.u32(player + PARENT_CELL) != 0 {
        setup(e, player);
    }
    if home_pressed() {
        open_menu(e);
    }
    let answer = ANSWER.swap(NO_ANSWER, Ordering::Relaxed);
    if answer != NO_ANSWER {
        act(e, player, answer);
    }
}

/// Frames of play before the set-up (the cell's references settle in).
const SETUP_FRAMES: u32 = 30;

/// Home's press (its going down), on the real window's keyboard (Windows).
fn home_pressed() -> bool {
    #[cfg(windows)]
    let down = engine_platform::d3d9real::held_keys()[DIK_HOME];
    #[cfg(not(windows))]
    let down = false;
    let was = KEY_DOWN.swap(down, Ordering::Relaxed);
    down && !was
}

/// `DIK_HOME`.
#[cfg_attr(not(windows), allow(dead_code))]
const DIK_HOME: usize = 0xc7;

// --- The game's words. --------------------------------------------------------------------------

/// `MenuConsole::Instance(create)` (`cdecl`): the console, made when there is none; its compiler
/// is its first word (`pConsoleCompiler`).
const CONSOLE_GET: u32 = 0x0071_b160;
/// `TESDataHandler` (the pointer), its interior cell by editor id (`thiscall`: the id) and its
/// exterior cell's worldspace by editor id (`thiscall`: the id, out x, out y), as `CenterOnCell`
/// (`0093db60`) looks a `coc` cell up.
const DATA_HANDLER: u32 = 0x011c_3f2c;
const CELL_BY_EDITOR_ID: u32 = 0x0046_1ae0;
const EXTERIOR_BY_EDITOR_ID: u32 = 0x0046_1cf0;
/// `StartMenu::pMe`, its main list (`+0x84`) and `ListBox::SetEnabled` (`thiscall`: on).
const START_MENU: u32 = 0x011d_aac0;
const MAIN_LIST: u32 = 0x84;
const LIST_BOX_SET_ENABLED: u32 = 0x007d_74f0;
/// The game's own message box answer callback (`005b4a70`), for a box with only OK.
const GAME_BOX_CALLBACK: u32 = 0x005b_4a70;
/// `Script::Script`, `TESForm::SetTemporary`, `Script::SetText`, `Script::CompileAndRun` and the
/// destructor, as the console runs a line (`menuconsole.rs`, `run_console_script`).
const SCRIPT_NEW: u32 = 0x005a_a0f0;
const SET_TEMPORARY: u32 = 0x0048_4490;
const SCRIPT_SET_TEXT: u32 = 0x005a_be50;
const COMPILE_AND_RUN: u32 = 0x005a_c400;
const SCRIPT_DELETE: u32 = 0x005a_a1a0;
/// A `Script`'s bytes (0x54 on PC), with room.
const SCRIPT_SIZE: u32 = 0x80;
const LINE_SIZE: u32 = 0x200;
/// The game's `PlaceAtMe` worker (`cdecl`: reference, form, count, distance, direction, health
/// scale), answering the last reference placed.
const PLACE_AT_ME: u32 = 0x005c_4b30;
/// `ShowMessageBox` (`cdecl`: text, 0, 0, callback, 0, the menu type 0x17, 0, 0, ten buttons, 0)
/// and the pressed button (`Interface::GetMessageMenuResult`).
const SHOW_MESSAGE_BOX: u32 = 0x0070_3e80;
const GET_MESSAGE_MENU_RESULT: u32 = 0x0070_3fa0;
/// `QueueUIMessage` (`cdecl`: text, emotion, icon, sound, seconds, byte).
const QUEUE_UI_MESSAGE: u32 = 0x0070_52f0;
/// `StartMenu::ChooseMainMenu`.
const CHOOSE_MAIN_MENU: u32 = 0x007d_0a70;
/// The game's map of every form (`NiTPointerMap`: hash size `+4`, table `+8`; an item's next,
/// key and value at `+0`, `+4`, `+8`).
const ALL_FORMS: u32 = 0x011c_54c0;
/// A form's type (`+4`), flags (`+8`, deleted `0x20`) and id (`+0xC`); a reference's parent cell
/// (`+0x40`); the player's heading (virtual `+0x2bc`).
const FORM_TYPE: u32 = 4;
const FORM_FLAGS: u32 = 8;
const FORM_ID: u32 = 0xc;
const FLAG_DELETED: u32 = 0x20;
const PARENT_CELL: u32 = 0x40;
const HEADING_SLOT: u32 = 0x2bc;
/// Form types.
const CONT: u8 = 0x1b;
const WEAP: u8 = 0x28;
const AMMO: u8 = 0x29;
const NPC: u8 = 0x2a;
const CREA: u8 = 0x2b;
const FLST: u8 = 0x55;
/// `TESObjectWEAP`: the ammo (`BGSAmmoForm`, `+0xA8`: an ammo or a list of them) and `cFlags`
/// (`+0x100`: embedded `0x20`, not playable `0x80`). A form list's first entry (`+0x18`).
const WEAPON_AMMO: u32 = 0xa8;
const WEAPON_FLAGS: u32 = 0x100;
const WEAPON_EMBEDDED: u8 = 0x20;
const WEAPON_NOT_PLAYABLE: u8 = 0x80;
const LIST_FIRST: u32 = 0x18;
/// Game units in a metre (one unit is about 1.43 cm).
const UNITS_PER_METRE: f32 = 70.0;

// --- The console's way. ------------------------------------------------------------------------

/// Runs `line` as the console runs a typed line: a temporary script compiled and run by the
/// console's compiler. False when there is no console.
pub fn run_line(e: &mut Engine, line: &str) -> bool {
    // The console, made on first use as the game makes it when the console key is first pressed.
    let console = e.call(CONSOLE_GET, &args![1u32]).u32();
    if console == 0 {
        return false;
    }
    let compiler = e.mem.u32(console);
    if compiler == 0 {
        return false;
    }
    e.with_stack(SCRIPT_SIZE + LINE_SIZE, |e, frame| {
        let script = frame.addr();
        let text = script + SCRIPT_SIZE;
        let bytes = &line.as_bytes()[..line.len().min(LINE_SIZE as usize - 1)];
        e.mem.set_cstr(text, bytes);
        e.call(SCRIPT_NEW, &args![script]);
        e.call(SET_TEMPORARY, &args![script]);
        e.call(SCRIPT_SET_TEXT, &args![script, text]);
        e.call(COMPILE_AND_RUN, &args![script, compiler, 1u32, 0u32]);
        e.call(SCRIPT_DELETE, &args![script]);
    });
    true
}

/// A form id as a console line writes it.
fn id(form_id: u32) -> String {
    format!("{form_id:08X}")
}

// --- The forms. --------------------------------------------------------------------------------

/// Every form the game holds, in id order.
fn all_forms(e: &Engine) -> Vec<u32> {
    let map = e.mem.u32(ALL_FORMS);
    if map == 0 {
        return Vec::new();
    }
    let size = e.mem.u32(map + 4).min(1 << 22);
    let table = e.mem.u32(map + 8);
    let mut forms = Vec::new();
    for bucket in 0..size {
        let mut item = e.mem.u32(table + 4 * bucket);
        let mut guard = 0;
        while item != 0 && guard < 1 << 16 {
            let form = e.mem.u32(item + 8);
            if form != 0 && e.mem.u32(form + FORM_FLAGS) & FLAG_DELETED == 0 {
                forms.push(form);
            }
            item = e.mem.u32(item);
            guard += 1;
        }
    }
    forms.sort_by_key(|&f| e.mem.u32(f + FORM_ID));
    forms
}

fn kind(e: &Engine, form: u32) -> u8 {
    e.mem.u8(form + FORM_TYPE)
}

fn name(e: &Engine, form: u32) -> String {
    String::from_utf8_lossy(&jip::commands::the_name(e, form)).into_owned()
}

/// The weapons the player can carry, every loaded plugin's: playable, not a creature's own
/// (embedded), named.
fn weapons(e: &Engine, forms: &[u32]) -> Vec<u32> {
    forms
        .iter()
        .copied()
        .filter(|&f| kind(e, f) == WEAP)
        .filter(|&f| {
            let flags = e.mem.u8(f + WEAPON_FLAGS);
            flags & (WEAPON_EMBEDDED | WEAPON_NOT_PLAYABLE) == 0
        })
        .filter(|&f| !name(e, f).is_empty())
        .collect()
}

/// A weapon's ammo: its ammo form, or the first of its ammo list.
fn ammo_of(e: &Engine, weapon: u32) -> Option<u32> {
    let ammo = e.mem.u32(weapon + WEAPON_AMMO);
    if ammo == 0 {
        return None;
    }
    match kind(e, ammo) {
        AMMO => Some(ammo),
        FLST => {
            let first = e.mem.u32(ammo + LIST_FIRST);
            (first != 0 && kind(e, first) == AMMO).then_some(first)
        }
        _ => None,
    }
}

/// The humanoid and creature targets, by the names the base game gives them (the first form of
/// each name found, lowest id first); the player's own base form (`0x7`) when no humanoid is found.
const HUMANOIDS: [&str; 6] = [
    "Powder Ganger",
    "Fiend",
    "NCR Trooper",
    "Legionary",
    "Great Khan",
    "Raider",
];
const CREATURES: [&str; 8] = [
    "Gecko",
    "Giant Rat",
    "Coyote",
    "Radscorpion",
    "Giant Mantis",
    "Feral Ghoul",
    "Mr. Handy",
    "Protectron",
];
/// The containers the weapons go into, by name, else the first named container.
const CONTAINERS: [&str; 4] = ["Footlocker", "Ammo Box", "Toolbox", "Metal Box"];
const PLAYER_BASE: u32 = 7;

/// The first forms of `kind` named as in `names`, in the order of `names`, up to `most`.
fn by_names(e: &Engine, forms: &[u32], of: u8, names: &[&str], most: usize) -> Vec<u32> {
    let mut found = Vec::new();
    for wanted in names {
        if found.len() >= most {
            break;
        }
        if let Some(&form) = forms
            .iter()
            .find(|&&f| kind(e, f) == of && name(e, f).eq_ignore_ascii_case(wanted))
        {
            found.push(form);
        }
    }
    found
}

/// The lane: the distances in metres and which kind of target stands at each (humanoids and
/// creatures alternate), with a small step to the side so none hides another.
pub fn lane() -> Vec<(f32, bool, f32)> {
    [5.0, 10.0, 20.0, 35.0, 50.0, 75.0, 100.0]
        .into_iter()
        .enumerate()
        .map(|(i, metres)| {
            let side = if i % 2 == 0 { -1.5 } else { 1.5 };
            (metres, i % 2 == 0, side)
        })
        .collect()
}

/// Where a target stands relative to the player facing `heading` (around z, 0 north,
/// clockwise): `metres` ahead and `side` metres to the right, in game units (x, y).
pub fn offset(heading: f32, metres: f32, side: f32) -> (f32, f32) {
    let (s, c) = heading.sin_cos();
    let ahead = metres * UNITS_PER_METRE;
    let right = side * UNITS_PER_METRE;
    (s * ahead + c * right, c * ahead - s * right)
}

/// The heading in degrees that faces back at the player (`heading` the player's, radians).
pub fn facing_back(heading: f32) -> f32 {
    (heading.to_degrees() + 180.0).rem_euclid(360.0)
}

// --- The set-up and the menu. ------------------------------------------------------------------

/// The range's set-up: the weapons container, the ammo, the targets.
fn setup(e: &mut Engine, player: u32) {
    with_range(|r| r.set_up = true);
    let forms = all_forms(e);
    let weapons = weapons(e, &forms);
    let mut ammo: Vec<u32> = weapons.iter().filter_map(|&w| ammo_of(e, w)).collect();
    ammo.sort_unstable_by_key(|&a| e.mem.u32(a + FORM_ID));
    ammo.dedup();
    let ammo: Vec<u32> = ammo.iter().map(|&a| e.mem.u32(a + FORM_ID)).collect();
    // The container, behind the player (opened from the range menu).
    let mut container_base = by_names(e, &forms, CONT, &CONTAINERS, 1);
    if container_base.is_empty() {
        container_base = forms
            .iter()
            .copied()
            .filter(|&f| kind(e, f) == CONT && !name(e, f).is_empty())
            .take(1)
            .collect();
    }
    let heading = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
    let mut container = 0;
    if let Some(&base) = container_base.first() {
        let placed = e
            .call(PLACE_AT_ME, &args![player, base, 1u32, 0u32, 0u32, 1.0f32])
            .u32();
        if placed != 0 {
            container = e.mem.u32(placed + FORM_ID);
            let (x, y) = offset(heading, -1.8, 0.0);
            let c = id(container);
            run_line(e, &format!("{c}.moveto player {x:.0} {y:.0} 0"));
            run_line(e, &format!("{c}.removeallitems"));
            for &weapon in &weapons {
                let w = id(e.mem.u32(weapon + FORM_ID));
                run_line(e, &format!("{c}.additem {w} 1 1"));
            }
        }
    }
    with_range(|r| {
        r.container = container;
        r.ammo = ammo;
    });
    fill_ammo(e);
    place_targets(e, player);
    eprintln!(
        "Test Range: {} weapons, a lane of targets; Home opens the range menu",
        weapons.len()
    );
    note(e, "Test Range: press Home for the range menu.");
}

/// 500 of every ammo the loaded weapons use, in the player's inventory (topped up: what the
/// player holds is taken first).
fn fill_ammo(e: &mut Engine) {
    let ammo = with_range(|r| r.ammo.clone());
    for a in ammo {
        let a = id(a);
        run_line(e, &format!("player.removeitem {a} 99999 1"));
        run_line(e, &format!("player.additem {a} 500 1"));
    }
}

/// A fresh lane of targets in front of the player, passive or hostile as the range is.
fn place_targets(e: &mut Engine, player: u32) {
    let forms = all_forms(e);
    let mut humanoids = by_names(e, &forms, NPC, &HUMANOIDS, 4);
    if humanoids.is_empty() {
        if let Some(&base) = forms
            .iter()
            .find(|&&f| e.mem.u32(f + FORM_ID) == PLAYER_BASE)
        {
            humanoids.push(base);
        }
    }
    let creatures = by_names(e, &forms, CREA, &CREATURES, 4);
    let heading = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
    let angle = facing_back(heading);
    let hostile = with_range(|r| r.hostile);
    let mut targets = Vec::new();
    let (mut h, mut c) = (0usize, 0usize);
    for (metres, humanoid, side) in lane() {
        let base = if humanoid || creatures.is_empty() {
            h += 1;
            humanoids.get((h - 1) % humanoids.len().max(1)).copied()
        } else {
            c += 1;
            creatures.get((c - 1) % creatures.len()).copied()
        };
        let Some(base) = base else {
            continue;
        };
        let placed = e
            .call(PLACE_AT_ME, &args![player, base, 1u32, 0u32, 0u32, 1.0f32])
            .u32();
        if placed == 0 {
            continue;
        }
        let t = id(e.mem.u32(placed + FORM_ID));
        let (x, y) = offset(heading, metres, side);
        run_line(e, &format!("{t}.moveto player {x:.0} {y:.0} 0"));
        run_line(e, &format!("{t}.setangle z {angle:.0}"));
        for line in stance(&t, hostile) {
            run_line(e, &line);
        }
        targets.push(e.mem.u32(placed + FORM_ID));
    }
    with_range(|r| r.targets = targets);
}

/// The console lines that make a target passive (it stands and takes hits) or hostile.
pub fn stance(target: &str, hostile: bool) -> Vec<String> {
    if hostile {
        vec![
            format!("{target}.setrestrained 0"),
            format!("{target}.setav aggression 2"),
            format!("{target}.startcombat player"),
        ]
    } else {
        vec![
            format!("{target}.stopcombat"),
            format!("{target}.setav aggression 0"),
            format!("{target}.setrestrained 1"),
        ]
    }
}

/// The range menu's buttons, in order.
pub fn buttons(hostile: bool) -> [&'static str; 6] {
    [
        "Weapons",
        "Fill Ammo",
        if hostile {
            "Make Targets Passive"
        } else {
            "Make Targets Hostile"
        },
        "Respawn Targets",
        "Exit to Main Menu",
        "Close",
    ]
}

/// The range menu (the game's message box; its answer comes to [`MENU_CALLBACK`]).
fn open_menu(e: &mut Engine) {
    let hostile = with_range(|r| r.hostile);
    let text = format!(
        "nv-rs Test Range\nTargets are {}. Saving is off.\nTune: Esc > Settings > Display (Recoil, Weapon Sway, Impact Punch, Dismember Force); more in Fallout.ini [nv-rs Gunplay].",
        if hostile { "hostile" } else { "passive" }
    );
    let labels = buttons(hostile);
    e.with_stack(0x400, |e, frame| {
        let base = frame.addr();
        e.mem.set_cstr(base, text.as_bytes());
        let mut words = vec![base, 0, 0, MENU_CALLBACK, 0, 0x17, 0, 0];
        let mut at = base + 0x200;
        let mut slots = [0u32; 10];
        for (slot, label) in slots.iter_mut().zip(labels) {
            e.mem.set_cstr(at, label.as_bytes());
            *slot = at;
            at += 0x30;
        }
        words.extend_from_slice(&slots);
        words.push(0);
        ANSWER.store(NO_ANSWER, Ordering::Relaxed);
        e.call(SHOW_MESSAGE_BOX, &words);
    });
}

/// The range menu's answer, on the next frame of play.
fn act(e: &mut Engine, player: u32, answer: u8) {
    match answer {
        0 => {
            let container = with_range(|r| r.container);
            if container != 0 {
                run_line(e, &format!("{}.activate player", id(container)));
            }
        }
        1 => {
            fill_ammo(e);
            note(e, "Ammo filled.");
        }
        2 => {
            let (hostile, targets) = with_range(|r| {
                r.hostile = !r.hostile;
                (r.hostile, r.targets.clone())
            });
            for t in targets {
                for line in stance(&id(t), hostile) {
                    run_line(e, &line);
                }
            }
            note(
                e,
                if hostile {
                    "Targets are hostile."
                } else {
                    "Targets are passive."
                },
            );
        }
        3 => {
            let targets = with_range(|r| std::mem::take(&mut r.targets));
            for t in targets {
                let t = id(t);
                run_line(e, &format!("{t}.disable"));
                run_line(e, &format!("{t}.markfordelete"));
            }
            place_targets(e, player);
        }
        4 => {
            with_range(|r| *r = Range::default());
            e.call(CHOOSE_MAIN_MENU, &[]);
        }
        _ => {}
    }
}

/// The main menu's "Test Range" entry: a plain option of the title menu (modes 1), made as
/// `CreateMenuElements` makes its own and moved to just above Quit (`007d0a10`).
pub fn add_option(e: &mut Engine) {
    let text = e.call(OPERATOR_NEW, &args![16u32]).u32();
    let block = e.call(OPERATOR_NEW, &args![0x10u32]).u32();
    if text == 0 || block == 0 {
        return;
    }
    e.mem.set_cstr(text, b"Test Range");
    let option = e
        .call(OPTION_NEW, &args![block, text, CHOOSE_FN, TITLE_MENU])
        .u32();
    e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option);
        e.call(ARRAY_ADD, &args![OPTIONS, cell]);
    });
    let buffer = e.mem.u32(OPTIONS + 4);
    let count = e.mem.u32(OPTIONS + 8);
    if buffer == 0 || count < 2 {
        return;
    }
    let options: Vec<u32> = (0..count).map(|i| e.mem.u32(buffer + 4 * i)).collect();
    let quit = options
        .iter()
        .position(|&o| o != 0 && e.mem.u32(o + 8) == QUIT_CHOOSE);
    for (i, value) in above(&options, quit).into_iter().enumerate() {
        e.mem.set_u32(buffer + 4 * i as u32, value);
    }
}

/// `options` with its last entry moved to just before index `quit` (unchanged without one).
pub fn above(options: &[u32], quit: Option<usize>) -> Vec<u32> {
    let mut out = options.to_vec();
    if let Some(q) = quit.filter(|&q| q + 1 < out.len()) {
        if let Some(last) = out.pop() {
            out.insert(q, last);
        }
    }
    out
}

/// `operator new`, `StartMenu::Option::Option` (`007ce620`: text, choose, modes), `pOptionsA`
/// and its `Add`, the title menu's mode bit and Quit's choose function.
const OPERATOR_NEW: u32 = 0x0040_1000;
const OPTION_NEW: u32 = 0x007c_e620;
const OPTIONS: u32 = 0x011d_ab88;
const ARRAY_ADD: u32 = 0x007c_b2e0;
const TITLE_MENU: u32 = 1;
const QUIT_CHOOSE: u32 = 0x007d_0a10;

#[cfg(test)]
mod tests {
    use super::*;

    /// Patrick's click that did nothing: each step of the start is checked in order, and each
    /// failure has a line for the log and the screen.
    #[test]
    fn a_start_that_cannot_happen_says_why() {
        let mut ran = false;
        assert_eq!(
            start_outcome(
                false,
                || {
                    ran = true;
                    true
                },
                || true
            ),
            Err(StartError::NoCell)
        );
        assert!(!ran, "no coc for a cell that is not there");
        assert_eq!(
            start_outcome(true, || false, || true),
            Err(StartError::NoConsole)
        );
        assert_eq!(
            start_outcome(true, || true, || false),
            Err(StartError::NotStarted)
        );
        assert_eq!(start_outcome(true, || true, || true), Ok(()));
        for why in [
            StartError::NoCell,
            StartError::NoConsole,
            StartError::NotStarted,
        ] {
            let text = why.text("QASmoke");
            assert!(text.starts_with("Test Range: "), "{text}");
            assert!(text.len() < 0x27f);
        }
        assert!(StartError::NoCell
            .text("QASmoke")
            .contains("sTestRangeCell"));
    }

    /// The console is made when there is none: `run_line` asks `MenuConsole::Instance` with
    /// create set, and runs the line with the console's compiler.
    #[test]
    fn the_console_is_made_for_the_line() {
        use std::sync::atomic::AtomicU32;
        static ASKED: AtomicU32 = AtomicU32::new(u32::MAX);
        static RAN: AtomicU32 = AtomicU32::new(0);
        let mut e = Engine::new();
        let console = e.mem.alloc(0x914);
        e.mem.set_u32(console, 0x5000);
        static CONSOLE: AtomicU32 = AtomicU32::new(0);
        CONSOLE.store(console, Ordering::Relaxed);
        e.register(CONSOLE_GET, |_, a| {
            ASKED.store(a[0], Ordering::Relaxed);
            CONSOLE.load(Ordering::Relaxed).into_ret()
        });
        for address in [SCRIPT_NEW, SET_TEMPORARY, SCRIPT_SET_TEXT, SCRIPT_DELETE] {
            e.register(address, |_, _| Ret::default());
        }
        e.register(COMPILE_AND_RUN, |_, a| {
            RAN.store(a[1], Ordering::Relaxed);
            Ret::default()
        });
        assert!(run_line(&mut e, "coc QASmoke"));
        assert_eq!(
            ASKED.load(Ordering::Relaxed),
            1,
            "asked to make the console"
        );
        assert_eq!(
            RAN.load(Ordering::Relaxed),
            0x5000,
            "run by the console's compiler"
        );
        // No console and none to be made: false, nothing run.
        CONSOLE.store(0, Ordering::Relaxed);
        RAN.store(0, Ordering::Relaxed);
        assert!(!run_line(&mut e, "coc QASmoke"));
        assert_eq!(RAN.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_option_goes_just_above_quit() {
        assert_eq!(above(&[1, 2, 3, 9], Some(2)), vec![1, 2, 9, 3]);
        assert_eq!(above(&[1, 2, 9], None), vec![1, 2, 9]);
        assert_eq!(above(&[9], Some(0)), vec![9]);
    }

    #[test]
    fn cells_are_editor_ids_only() {
        assert!(valid_cell("QASmoke"));
        assert!(valid_cell("Goodsprings_01"));
        assert!(!valid_cell(""));
        assert!(!valid_cell("QASmoke; player.kill"));
        assert!(!valid_cell("a b"));
        assert!(!valid_cell(&"x".repeat(65)));
    }

    /// The lane runs from 5 to 100 m straight ahead, alternating humanoids and creatures a step
    /// to each side, and the targets face back at the player.
    #[test]
    fn the_lane_is_ahead_of_the_player() {
        let lane = lane();
        assert_eq!(lane.first().unwrap().0, 5.0);
        assert_eq!(lane.last().unwrap().0, 100.0);
        assert!(lane.windows(2).all(|w| w[0].1 != w[1].1));
        // Facing north (heading 0): ahead is +y, right is +x.
        let (x, y) = offset(0.0, 10.0, 0.0);
        assert!(x.abs() < 1e-3 && (y - 700.0).abs() < 1e-3);
        let (x, y) = offset(0.0, 0.0, 1.0);
        assert!((x - 70.0).abs() < 1e-3 && y.abs() < 1e-3);
        // Facing east: ahead is +x.
        let (x, y) = offset(std::f32::consts::FRAC_PI_2, 10.0, 0.0);
        assert!((x - 700.0).abs() < 1e-2 && y.abs() < 1e-2);
        assert!((facing_back(0.0) - 180.0).abs() < 1e-3);
        assert!((facing_back(std::f32::consts::PI) - 0.0).abs() < 1e-3);
    }

    #[test]
    fn stances_and_buttons() {
        assert_eq!(
            stance("FF000801", false),
            [
                "FF000801.stopcombat",
                "FF000801.setav aggression 0",
                "FF000801.setrestrained 1"
            ]
        );
        assert!(stance("FF000801", true).contains(&"FF000801.startcombat player".to_string()));
        assert_eq!(buttons(false)[2], "Make Targets Hostile");
        assert_eq!(buttons(true)[2], "Make Targets Passive");
        assert_eq!(id(0x7), "00000007");
        assert_eq!(cell(), cell());
    }

    /// The range's saves: every one refused while a range game runs, the manual ones with a note.
    #[test]
    fn autosave_and_quicksave_names_are_not_the_players() {
        let mut e = Engine::new();
        let at = e.mem.alloc(0x40);
        for (name, manual) in [
            (&b"Autosave1\0"[..], false),
            (b"QuickSave\0", false),
            (b"Save 3 Courier\0", true),
        ] {
            e.mem.write(at, name);
            assert_eq!(is_manual(&e, at), manual, "{name:?}");
        }
    }
}
