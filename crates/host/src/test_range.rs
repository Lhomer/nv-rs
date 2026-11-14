//! The Test Range (weighty combat's slice 2, docs/GUNPLAY.md): a main-menu entry that starts a
//! throwaway game in a quiet test cell with targets, every weapon and ammo, to try and tune the
//! weighty combat (recoil, sway, impact punch, reach, hit reactions, dismemberment). Part of the
//! weighty combat row ([`crate::gunplay::FIX`]); **not 1:1 with FalloutNV.exe**.
//!
//! **Only the game's own content and machinery.** The range's place is chosen from the player's
//! own FalloutNV.esm ([`crate::range_cells`]: the widest, longest clear flat floor of a quiet, lit
//! interior's navmesh, or of the Mojave's open ground away from every placed actor, read with the
//! plugin reader), unless `[nv-rs Gunplay] sTestRangeCell` names a cell; the next ranked places,
//! then the Mojave at Goodsprings ([`last_resort`]), are the fallbacks ([`candidates`]).
//! Everything is done the way the console does it: the console's own compile-and-run of a line
//! ([`run_line`], as `MenuConsole::Idle` runs a typed line) for `coc`, `MoveTo`, `AddItem`,
//! `SetRestrained` and the rest, the game's `PlaceAtMe` worker (`005c4b30`) for the references
//! it needs back, its message box (`00703e80`) for the range menu, and its return to the main
//! menu (`StartMenu::ChooseMainMenu`, `007d0a70`). Targets are base forms the loaded plugins
//! hold, found by their names; weapons are every playable weapon of every loaded plugin and DLC.
//! No asset is added.
//!
//! **Flow.** The main menu's "Test Range" (made with the game's own options, just above Quit)
//! runs `coc <cell>` for the first place that starts: from the main menu the game starts a fresh game there, with a default
//! character, no save needed. The `CenterOnCell` new game's seam (`0093db69`) marks the range
//! active. Some frames into play the range is set up ([`setup`]): a container with one of every
//! weapon, 500 of every ammo, the player turned down the longest clear line of the room (the
//! game's own pick, as `PlaceAtMe` probes: [`clear_metres`]) and a lane of targets fitted to it
//! ([`fit_lane`]: 5 to 100 m where there is room), passive
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

use crate::range_cells::{Place, Spot};
use crate::range_watch::{self, step};
use engine::extend::{Owner, SiteSlot};
use engine::prelude::*;
use engine::units::fallout_shared::tes::fn_00453dc0;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// The weighty combat row as the owner of its seams (`crate::gunplay::FIX`'s name).
pub const ROW: Owner = Owner::Row("weighty combat");
/// The seams: a new game through `CenterOnCell` from the main menu, a new game from the menu, a
/// save about to load, the return to the main menu.
pub const COC_NEW_GAME: SiteSlot = SiteSlot::new(ROW, 0x0093_db69);
pub const MENU_NEW_GAME: SiteSlot = SiteSlot::new(ROW, 0x007d_33ce);
pub const PRE_LOAD: SiteSlot = SiteSlot::new(ROW, 0x0084_7fd9);
/// The main loop's pass (`main`, before `Main::OnIdle`; the seam xNVSE's tick runs at): the
/// range's start runs there.
pub const TICK: SiteSlot = SiteSlot::new(ROW, 0x0086_b386);
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

/// The range's cell when the file names none: `auto`, the best interior of the player's own
/// FalloutNV.esm ([`crate::range_cells`]).
pub const DEFAULT_CELL: &str = "auto";
/// The last resort when no place can be had: the Mojave's worldspace (`WastelandNV`, whose
/// editor id the base game has) at the cell of Goodsprings' streets, a place every game reaches.
/// Not quiet; only when nothing else starts.
pub fn last_resort() -> Target {
    Target::World {
        space: "WastelandNV".to_string(),
        x: -18,
        y: 0,
    }
}

static CELL: Mutex<String> = Mutex::new(String::new());

/// The range's cell (`sTestRangeCell`, `crate::fixes::install`): an editor id of letters, digits
/// and `_`, up to 64; anything else keeps [`DEFAULT_CELL`].
///
/// `QASmoke`, the first builds' default, is a Fallout 3 cell FalloutNV.esm does not have; Play.exe
/// wrote it into the `[nv-rs Gunplay]` block of players' Fallout.ini then, and it stayed there
/// (Patrick, 2c9c823: the configured cell was tried before `auto`'s). It counts as `auto`, and a
/// play run puts `auto` back in the file ([`forget_legacy_cell`]).
pub fn set_cell(name: &str) {
    let name = if is_legacy_cell(name) {
        eprintln!("Test Range: sTestRangeCell={name} is the first builds' default, which FalloutNV.esm does not have: auto instead");
        forget_legacy_cell();
        DEFAULT_CELL
    } else if valid_cell(name) {
        name
    } else {
        DEFAULT_CELL
    };
    *CELL.lock().unwrap_or_else(|p| p.into_inner()) = name.to_string();
}

/// Whether `name` is a default an earlier build wrote that no FalloutNV.esm has.
pub fn is_legacy_cell(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("QASmoke")
}

/// `sTestRangeCell=auto` into the run's Fallout.ini (a launcher's play run only).
fn forget_legacy_cell() {
    if !crate::fixes::is_play_run(|name| std::env::var(name).ok()) {
        return;
    }
    let Some(path) = crate::game_ini::path() else {
        return;
    };
    let written = crate::game_ini::write_keys(
        &path,
        &[(crate::gunplay::SECTION, "sTestRangeCell", DEFAULT_CELL)],
    );
    match written {
        Ok(()) => eprintln!(
            "Test Range: {} now says sTestRangeCell=auto",
            path.display()
        ),
        Err(why) => eprintln!("Test Range: {} not changed ({why})", path.display()),
    }
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

/// Where the range is.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No range.
    #[default]
    Off,
    /// The main menu's entry was chosen and its `coc` runs.
    Starting,
    /// The `coc` began a new game (the `CenterOnCell` seam) and has not returned yet.
    Loading,
    /// The `coc` returned; waiting for play to begin (no loading screen, no main menu, the
    /// player in a cell) before the set-up.
    WaitingForPlay,
    /// Set up; the range menu works.
    Ready,
}

/// The range under way.
#[derive(Debug, Default)]
struct Range {
    phase: Phase,
    /// When the phase began (for the timeout).
    since: Option<std::time::Instant>,
    /// The place started (for the log).
    place: String,
    /// Frames of play in a row with no loading screen, no main menu and the player in a cell.
    good_frames: u32,
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

/// Whether a range game is running (from its new game on: its saves are refused).
pub fn active() -> bool {
    with_range(|r| {
        matches!(
            r.phase,
            Phase::Loading | Phase::WaitingForPlay | Phase::Ready
        )
    })
}

/// The range's phase.
pub fn phase() -> Phase {
    with_range(|r| r.phase)
}

/// The range moves to `phase`, said in the log as a step ([`step`]).
fn enter(phase: Phase, what: &str) {
    with_range(|r| {
        r.phase = phase;
        r.since = Some(std::time::Instant::now());
        r.good_frames = 0;
    });
    step(what);
}

/// The watchdog's view of a start ([`range_watch::watch`]): the phase and the seconds without a
/// step that end the run, or `None` when no start is under way.
fn watch_state() -> Option<(&'static str, u64)> {
    match phase() {
        Phase::Starting => Some(("starting", range_watch::STUCK_LOADING)),
        Phase::Loading => Some(("loading", range_watch::STUCK_LOADING)),
        Phase::WaitingForPlay => Some(("waiting for play", range_watch::STUCK_WAITING)),
        Phase::Off | Phase::Ready => None,
    }
}

/// The main menu's entry was chosen: the start runs at the main loop's next pass ([`TICK`]).
static START_ASKED: AtomicBool = AtomicBool::new(false);

/// The range ends (a save loads, a new game, the main menu): nothing of it is kept.
fn end(e: &mut Engine, _: &[u32]) -> Option<u32> {
    range_watch::end_trace(e);
    if phase() != Phase::Off {
        step("the range ends (a load, a new game or the main menu)");
    }
    with_range(|r| *r = Range::default());
    ANSWER.store(NO_ANSWER, Ordering::Relaxed);
    None
}

// --- The hooks. --------------------------------------------------------------------------------

/// Registers the range's hooks, seams and functions (from `crate::gunplay::install`).
pub fn install(e: &mut Engine) {
    use engine::units::fallout_misc::bgssaveloadmanager as manager;
    // Chosen in the main menu's click handler (mouse, keys or pad): the start itself waits for
    // the main loop ([`TICK`]), out of the menu's handling, where xNVSE runs its own deferred
    // work. Until 312dda26 the `coc` ran inside the click handler and its load never finished
    // there (Patrick: stuck in the cell's attach).
    e.register(CHOOSE_FN, |_, _| {
        if phase() == Phase::Off && !START_ASKED.swap(true, Ordering::Relaxed) {
            step("chosen on the main menu: the start runs at the main loop's next pass");
        }
        Ret::default()
    });
    e.on_site(TICK, |e, _| {
        if START_ASKED.swap(false, Ordering::Relaxed) {
            step("the main loop's pass: starting");
            start(e);
        }
        None
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
    e.on_site(COC_NEW_GAME, |e, _| {
        if phase() == Phase::Starting {
            enter(
                Phase::Loading,
                "CenterOnCell began a new game (loading the cell)",
            );
            range_watch::begin_trace(e);
        } else {
            with_range(|r| *r = Range::default());
        }
        None
    });
    e.register(CELL_ATTACH, |e, a| {
        let watched = matches!(phase(), Phase::Starting | Phase::Loading);
        if watched {
            let id = if a[1] == 0 {
                0
            } else {
                e.mem.u32(a[1] + FORM_ID)
            };
            step(&format!("attaching the cell {id:08X} (TES 00453dc0)"));
            range_watch::attach(e, true);
        }
        fn_00453dc0(e, Ptr::new(a[0]), Ptr::new(a[1]), Ptr::new(a[2]));
        if watched {
            range_watch::attach(e, false);
            step("the cell is attached");
        }
        Ret::default()
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

/// Where the range can start: an interior cell by editor id (`coc`), or a worldspace's cell
/// (`cow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Interior(String),
    World { space: String, x: i32, y: i32 },
}

impl Target {
    /// The console line that starts a game there from the main menu.
    pub fn line(&self) -> String {
        match self {
            Target::Interior(cell) => format!("coc {cell}"),
            Target::World { space, x, y } => format!("cow {space} {x} {y}"),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Target::Interior(cell) => cell.clone(),
            Target::World { space, x, y } => format!("{space} {x},{y}"),
        }
    }
}

/// The places tried, in order: the configured cell (unless `auto`), the ranked places of the
/// player's FalloutNV.esm (up to eight), then [`last_resort`].
pub fn candidates(configured: &str, ranked: &[Target]) -> Vec<Target> {
    let mut list = Vec::new();
    if !configured.eq_ignore_ascii_case(DEFAULT_CELL) && valid_cell(configured) {
        list.push(Target::Interior(configured.to_string()));
    }
    for target in ranked.iter().take(8) {
        let fine = match target {
            Target::Interior(cell) => !cell.eq_ignore_ascii_case(configured) && valid_cell(cell),
            Target::World { space, .. } => valid_cell(space),
        };
        if fine && !list.contains(target) {
            list.push(target.clone());
        }
    }
    let last = last_resort();
    if !list.contains(&last) {
        list.push(last);
    }
    list
}

/// Where a ranked place starts.
pub fn target_of(spot: &Spot) -> Target {
    match &spot.place {
        Place::Interior(cell) => Target::Interior(cell.clone()),
        Place::Exterior { space, x, y } => Target::World {
            space: space.clone(),
            x: *x,
            y: *y,
        },
    }
}

/// The ranked place started as `name` ([`Target::name`]), with its lane and floor.
fn spot_named(name: &str) -> Option<Arc<Spot>> {
    RANKED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()?
        .iter()
        .find(|s| s.name() == name)
        .cloned()
}

/// Why the range did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// No place of the list is in the loaded game.
    NoPlace,
    /// The game's console, whose compiler runs the `coc`, could not be had.
    NoConsole,
    /// The places were there, but no new game began in any.
    NotStarted(String),
}

impl StartError {
    /// The line for the log and the screen.
    pub fn text(&self) -> String {
        match self {
            StartError::NoPlace => "Test Range: no place for the range in the loaded game. Set sTestRangeCell in Fallout.ini's [nv-rs Gunplay] to a cell's editor id.".to_string(),
            StartError::NoConsole => {
                "Test Range: the game's console could not be made, so the range could not start."
                    .to_string()
            }
            StartError::NotStarted(last) => format!(
                "Test Range: the game did not start in {last}. The game's log says more."
            ),
        }
    }
}

/// The start: the first place of `list` that is in the game (`there`) and where the `coc` or
/// `cow` (`run`, which marks the start) begins a new game (`started`); a place missing or not
/// started is passed for the next. `cx` is what the three steps work on (the engine).
pub fn first_start<C>(
    cx: &mut C,
    list: &[Target],
    there: impl Fn(&mut C, &Target) -> bool,
    run: impl Fn(&mut C, &Target) -> bool,
    started: impl Fn(&mut C) -> bool,
) -> Result<Target, StartError> {
    let mut last = None;
    for target in list {
        if !there(cx, target) {
            eprintln!("Test Range: {} is not in the loaded game", target.name());
            continue;
        }
        if !run(cx, target) {
            return Err(StartError::NoConsole);
        }
        if started(cx) {
            return Ok(target.clone());
        }
        eprintln!("Test Range: the game did not start in {}", target.name());
        last = Some(target.name());
    }
    Err(last.map_or(StartError::NoPlace, StartError::NotStarted))
}

/// The main menu's entry: the first place that starts ([`candidates`], [`first_start`]), as the
/// console's `coc` starts a game there from the main menu.
///
/// 2026-11-14: the console is made the game's way ([`run_line`]: the game makes it on first use,
/// so before that the click did nothing, Patrick on 68956e3); the default `QASmoke` was a Fallout
/// 3 cell FalloutNV.esm does not have (Patrick on d9b4aa8), so the place is now chosen from the
/// player's own FalloutNV.esm ([`crate::range_cells`]), with fallbacks. Anything that stops the
/// start is said in the log and on the screen, with the menu's list enabled again.
fn start(e: &mut Engine) {
    start_scan(e);
    let configured = cell();
    let ranked: Vec<Target> = ranked_places().iter().map(|s| target_of(s)).collect();
    let list = candidates(&configured, &ranked);
    eprintln!(
        "Test Range: starting (sTestRangeCell {configured}; tries {})",
        list.iter().map(Target::name).collect::<Vec<_>>().join(", ")
    );
    range_watch::watch(watch_state);
    let outcome = first_start(
        e,
        &list,
        |e, t| match t {
            Target::Interior(cell) => cell_exists(e, cell),
            Target::World { .. } => true,
        },
        |e, t| {
            with_range(|r| {
                *r = Range {
                    place: t.name(),
                    ..Range::default()
                }
            });
            FRAME_IN_LOAD.store(false, Ordering::Relaxed);
            enter(Phase::Starting, &format!("running `{}`", t.line()));
            let ran = run_line(e, &t.line());
            range_watch::end_trace(e);
            if ran {
                step(&format!("`{}` returned", t.line()));
            }
            ran
        },
        |_| active(),
    );
    match outcome {
        Ok(target) => {
            let player = e.global::<u32>(PLAYER);
            let (cell, at) = if player == 0 {
                (0, [0.0; 3])
            } else {
                let position = e.vcall(player, POSITION_SLOT, &args![]).u32();
                let at = if position == 0 {
                    [0.0; 3]
                } else {
                    [
                        e.mem.f32(position),
                        e.mem.f32(position + 4),
                        e.mem.f32(position + 8),
                    ]
                };
                let cell = e.mem.u32(player + PARENT_CELL);
                let id = if cell == 0 {
                    0
                } else {
                    e.mem.u32(cell + FORM_ID)
                };
                (id, at)
            };
            enter(
                Phase::WaitingForPlay,
                &format!(
                    "started in {} ({}): the player is in the cell {cell:08X} at {:.0}, {:.0}, {:.0}; waiting for play",
                    target.name(),
                    spot_named(&target.name())
                        .map_or_else(|| "the configured cell or the last resort".to_string(), |s| s.why.clone()),
                    at[0],
                    at[1],
                    at[2]
                ),
            );
        }
        Err(why) => {
            let text = why.text();
            eprintln!("{text}");
            with_range(|r| *r = Range::default());
            fail_on_menu(e, &text);
        }
    }
}

/// The ranked interiors of the player's FalloutNV.esm, read on a host thread from when the main
/// menu is made ([`start_scan`]); waited for (up to a minute) when the range starts first.
static RANKED: Mutex<Option<Vec<Arc<Spot>>>> = Mutex::new(None);
static RANKED_READY: std::sync::Condvar = std::sync::Condvar::new();
static SCAN_STARTED: AtomicBool = AtomicBool::new(false);

/// Starts reading the base game's master for the range's places, once per run.
fn start_scan(e: &Engine) {
    if SCAN_STARTED.load(Ordering::Relaxed) {
        return;
    }
    let paths = crate::cell_offsets::loaded_plugin_paths(e);
    let Some(path) = crate::range_cells::base_master(&paths).cloned() else {
        return;
    };
    if SCAN_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("test range cells".into())
        .spawn(move || {
            let ranked = crate::range_cells::ranked_spots(&path)
                .into_iter()
                .map(Arc::new)
                .collect();
            *RANKED.lock().unwrap_or_else(|p| p.into_inner()) = Some(ranked);
            RANKED_READY.notify_all();
        });
    if spawned.is_err() {
        *RANKED.lock().unwrap_or_else(|p| p.into_inner()) = Some(Vec::new());
    }
}

/// The ranked places, once the reading is done; none when it never started.
fn ranked_places() -> Vec<Arc<Spot>> {
    if !SCAN_STARTED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let guard = RANKED.lock().unwrap_or_else(|p| p.into_inner());
    let (guard, _) = RANKED_READY
        .wait_timeout_while(guard, std::time::Duration::from_secs(60), |r| r.is_none())
        .unwrap_or_else(|p| p.into_inner());
    guard.clone().unwrap_or_default()
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
///
/// The set-up waits until the `coc` has returned and play has begun: no loading screen
/// (`LoadingMenu`, `011da0c0`), no main menu (`0070edf0`), the player in a cell, for
/// [`SETUP_FRAMES`] frames in a row, and never while a console line runs. Through 2c9c823 it only
/// counted frames from the new game's seam, which fires inside the `coc`: frames the game ran
/// during the load could start it there, nested inside the `coc`'s own script (Patrick: the
/// loading screen, then a freeze). When play has not begun after [`PLAY_TIMEOUT`], the range goes
/// back to the main menu and says so.
pub fn frame(e: &mut Engine, player: u32) {
    match phase() {
        Phase::WaitingForPlay => {
            let playing = !RUNNING.load(Ordering::Relaxed)
                && e.global::<u32>(LOADING_MENU) == 0
                && !e.call(IS_IN_START_MENU, &[]).bool()
                && e.mem.u32(player + PARENT_CELL) != 0;
            let next = with_range(|r| {
                let waited = r.since.map_or(0, |s| s.elapsed().as_secs());
                let next = wait(r.good_frames, playing, waited);
                if let Wait::Keep(frames) = next {
                    r.good_frames = frames;
                }
                next
            });
            if next == Wait::SetUp {
                let cell = e.mem.u32(player + PARENT_CELL);
                enter(
                    Phase::Ready,
                    &format!(
                        "play has begun in the cell {:08X}: setting up",
                        e.mem.u32(cell + FORM_ID)
                    ),
                );
                setup(e, player);
            } else if next == Wait::TimeOut {
                time_out(e);
            }
            return;
        }
        Phase::Ready => {}
        Phase::Loading => {
            // Said once a start: the game runs frames inside the `coc` (the set-up waits).
            if !FRAME_IN_LOAD.swap(true, Ordering::Relaxed) {
                step("a frame ran while the cell loads (the set-up waits for the coc to return)");
            }
            return;
        }
        _ => return,
    }
    if home_pressed() {
        open_menu(e);
    }
    let answer = ANSWER.swap(NO_ANSWER, Ordering::Relaxed);
    if answer != NO_ANSWER {
        act(e, player, answer);
    }
}

/// A frame ran during the start's load ([`frame`]).
static FRAME_IN_LOAD: AtomicBool = AtomicBool::new(false);

/// What a frame of the wait for play does.
#[derive(Debug, PartialEq, Eq)]
enum Wait {
    /// Keep waiting, with this many frames of play in a row.
    Keep(u32),
    SetUp,
    TimeOut,
}

/// The wait for play: `frames` of play in a row so far, whether this frame is one, the seconds
/// since the `coc` returned.
fn wait(frames: u32, playing: bool, waited: u64) -> Wait {
    let frames = if playing { frames + 1 } else { 0 };
    if frames >= SETUP_FRAMES {
        Wait::SetUp
    } else if waited >= PLAY_TIMEOUT {
        Wait::TimeOut
    } else {
        Wait::Keep(frames)
    }
}

/// Frames of play in a row before the set-up (the cell's references settle in).
const SETUP_FRAMES: u32 = 30;
/// Seconds after the `coc` returned within which play must begin.
const PLAY_TIMEOUT: u64 = 90;
// The watchdog gives the range's own timeout time to go back to the main menu first.
const _: () = assert!(range_watch::STUCK_WAITING > PLAY_TIMEOUT);

/// Play never began: back to the main menu, the reason shown there ([`add_option`]).
fn time_out(e: &mut Engine) {
    let place = with_range(|r| r.place.clone());
    let text = format!(
        "Test Range: {place} did not finish loading within {PLAY_TIMEOUT} s, so the range went back to the main menu. The game's log says where it stopped."
    );
    step(&text);
    with_range(|r| *r = Range::default());
    *PENDING_ERROR.lock().unwrap_or_else(|p| p.into_inner()) = Some(text);
    e.call(CHOOSE_MAIN_MENU, &[]);
}

/// A reason to show on the main menu when it is made again ([`time_out`]).
static PENDING_ERROR: Mutex<Option<String>> = Mutex::new(None);

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
/// The player, the loading menu (`LoadingMenu`, set while a loading screen is up) and whether the
/// main menu is up (`0070edf0`).
const PLAYER: u32 = 0x011d_ea3c;
const LOADING_MENU: u32 = 0x011d_a0c0;
const IS_IN_START_MENU: u32 = 0x0070_edf0;
/// `TES`'s switch of the loaded area to a cell (`thiscall`: the cell, the position), wrapped to
/// say when the range's cell is attached.
pub const CELL_ATTACH: u32 = 0x0045_3dc0;
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
///
/// One line at a time: a line asked for while another runs (the game running the range's frame
/// inside a `coc`) is refused and said in the log, never compiled inside the other.
pub fn run_line(e: &mut Engine, line: &str) -> bool {
    if RUNNING.swap(true, Ordering::Relaxed) {
        eprintln!("Test Range: `{line}` refused: another console line is running");
        return false;
    }
    let ran = run_line_now(e, line);
    RUNNING.store(false, Ordering::Relaxed);
    ran
}

/// A console line of the range's is running ([`run_line`]).
static RUNNING: AtomicBool = AtomicBool::new(false);

fn run_line_now(e: &mut Engine, line: &str) -> bool {
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

/// The standard distances of the lane, in metres.
pub const DISTANCES: [f32; 7] = [5.0, 10.0, 20.0, 35.0, 50.0, 75.0, 100.0];

/// The lane's distances for `clear_m` metres of clear line ahead (a target needs 2 m of room
/// before the wall): the standard ones that fit when four or more do, else four spread evenly
/// over the room there is (at least 2 m out), else one at 2 m.
pub fn fit_lane(clear_m: f32) -> Vec<f32> {
    let room = (clear_m - 2.0).max(0.0);
    let standard: Vec<f32> = DISTANCES.into_iter().filter(|&m| m <= room).collect();
    if standard.len() >= 4 {
        return standard;
    }
    if room < 3.0 {
        return vec![2.0];
    }
    (1..=4).map(|k| (room * k as f32 / 4.0).max(2.0)).collect()
}

/// The lane for `clear_m` metres of clear line ([`fit_lane`]): the distances and which kind of
/// target stands at each (humanoids and creatures alternate), with a small step to the side so
/// none hides another.
pub fn lane(clear_m: f32) -> Vec<(f32, bool, f32)> {
    fit_lane(clear_m)
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
    // The place's lane, when the range chose it from the plugin: the player at its start.
    let spot = spot_named(&with_range(|r| r.place.clone())).filter(|s| s.has_lane());
    if let Some(s) = &spot {
        place_player(e, s);
    }
    let forms = all_forms(e);
    let weapons = weapons(e, &forms);
    let mut ammo: Vec<u32> = weapons.iter().filter_map(|&w| ammo_of(e, w)).collect();
    ammo.sort_unstable_by_key(|&a| e.mem.u32(a + FORM_ID));
    ammo.dedup();
    let ammo: Vec<u32> = ammo.iter().map(|&a| e.mem.u32(a + FORM_ID)).collect();
    step(&format!(
        "set-up: {} forms, {} weapons, {} ammo types",
        forms.len(),
        weapons.len(),
        ammo.len()
    ));
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
    let heading = match &spot {
        Some(s) => s.lane.heading,
        None => e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32,
    };
    let mut container = 0;
    if let Some(&base) = container_base.first() {
        let placed = e
            .call(PLACE_AT_ME, &args![player, base, 1u32, 0u32, 0u32, 1.0f32])
            .u32();
        step(&format!(
            "set-up: the container {:08X} placed as {placed:08X}",
            e.mem.u32(base + FORM_ID)
        ));
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
            step(&format!("set-up: the weapons are in the container {c}"));
        }
    } else {
        step("set-up: no container in the loaded game");
    }
    with_range(|r| {
        r.container = container;
        r.ammo = ammo;
    });
    fill_ammo(e);
    step("set-up: the ammo is in the player's inventory");
    // Down the place's lane, or else the longest clear line of the room: the player turned to
    // face it.
    let best = match &spot {
        Some(_) => heading,
        None => {
            let (best, clear) = best_heading(e, player, heading);
            if best != heading {
                run_line(
                    e,
                    &format!(
                        "player.setangle z {:.0}",
                        best.to_degrees().rem_euclid(360.0)
                    ),
                );
            }
            step(&format!(
                "no lane from the plugin for this place: the longest line runs {clear:.0} m clear at {:.0} degrees",
                best.to_degrees().rem_euclid(360.0)
            ));
            best
        }
    };
    place_targets(e, player, best, spot.as_deref());
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

/// The player at the start of the place's lane, facing down it ([`crate::range_floor::Lane`]).
fn place_player(e: &mut Engine, spot: &Spot) {
    let lane = spot.lane;
    let [x, y, z] = lane.start;
    let degrees = lane.heading.to_degrees().rem_euclid(360.0);
    run_line(e, &format!("player.setpos x {x:.0}"));
    run_line(e, &format!("player.setpos y {y:.0}"));
    run_line(e, &format!("player.setpos z {:.0}", z + STAND_ABOVE));
    run_line(e, &format!("player.setangle z {degrees:.0}"));
    step(&format!(
        "the player stands at the lane's start ({x:.0}, {y:.0}, {z:.0}) facing {degrees:.0} degrees: {}",
        spot.why
    ));
}

/// How far above the navmesh's floor an actor is set down, in units (it settles onto the floor).
const STAND_ABOVE: f32 = 16.0;
/// The least clearance a target's spot needs, in metres ([`crate::range_floor::Floor::at`]: a
/// square at the navmesh's edge has its own size; inside, a metre more).
const TARGET_ROOM: f32 = 1.5;
/// The nudges tried for a target's spot, in order: metres ahead and to the right.
const NUDGES: [(f32, f32); 11] = [
    (0.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (1.5, 0.0),
    (-1.5, 0.0),
    (0.0, 2.0),
    (0.0, -2.0),
    (1.5, 1.5),
    (-1.5, -1.5),
    (3.0, 0.0),
    (-3.0, 0.0),
];

/// How many lanes side by side a floor `width_m` wide takes (5 m apart, 2 m from each side).
pub fn lane_count(width_m: f32) -> usize {
    if width_m >= 16.0 {
        3
    } else if width_m >= 10.0 {
        2
    } else {
        1
    }
}

/// The targets of `lanes` lanes side by side for `clear_m` metres of clear line: distance, a
/// humanoid or a creature, metres to the side. One lane alternates the kinds a step to each side
/// ([`lane`]); two are a humanoid lane and a creature lane; three add a second humanoid lane at
/// every other distance.
pub fn plan(clear_m: f32, lanes: usize) -> Vec<(f32, bool, f32)> {
    if lanes <= 1 {
        return lane(clear_m);
    }
    let distances = fit_lane(clear_m);
    let mut out = Vec::new();
    let sides: &[f32] = if lanes == 2 {
        &[-2.5, 2.5]
    } else {
        &[-5.0, 0.0, 5.0]
    };
    for (i, &metres) in distances.iter().enumerate() {
        out.push((metres, true, sides[0]));
        out.push((metres, false, sides[1]));
        if lanes >= 3 && i % 2 == 1 {
            out.push((metres, true, sides[2]));
        }
    }
    out
}

/// A target's spot near (x, y): the first of [`NUDGES`] (along `heading`) that is on the place's
/// floor with [`TARGET_ROOM`] to spare (when the place has one; its height then) and that `room`
/// finds free; with the nudge's index. `None`: no spot near.
pub fn settle(
    floor: Option<&crate::range_floor::Floor>,
    at: [f32; 3],
    heading: f32,
    mut room: impl FnMut([f32; 3]) -> bool,
) -> Option<([f32; 3], usize)> {
    for (k, &(ahead, right)) in NUDGES.iter().enumerate() {
        let (dx, dy) = offset(heading, ahead, right);
        let (x, y) = (at[0] + dx, at[1] + dy);
        let z = match floor {
            Some(f) => match f.at(x, y) {
                Some((z, clear)) if clear >= TARGET_ROOM => z,
                _ => continue,
            },
            None => at[2],
        };
        if room([x, y, z]) {
            return Some(([x, y, z], k));
        }
    }
    None
}

/// Whether an actor fits at the floor point `at`: nothing solid from 2 m above down to its knees,
/// nor across its chest each way (the game's pick, [`blocked`]).
fn has_room(e: &mut Engine, at: [f32; 3]) -> bool {
    let [x, y, z] = at;
    let half = 40.0;
    let chest = z + PICK_HEIGHT;
    !blocked(e, [x, y, z + 150.0], [x, y, z + 20.0])
        && !blocked(e, [x - half, y, chest], [x + half, y, chest])
        && !blocked(e, [x, y - half, chest], [x, y + half, chest])
}

/// A fresh lane of targets in front of the player facing `heading`, fitted to the clear line
/// there ([`clear_metres`], [`fit_lane`]) and, on a place the range chose, to its lane's length
/// and width (lanes side by side, [`plan`]); each target set down only where it fits
/// ([`settle`]); passive or hostile as the range is.
fn place_targets(e: &mut Engine, player: u32, heading: f32, spot: Option<&Spot>) {
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
    let seen = clear_metres(e, player, heading);
    let (clear, lanes) = match spot {
        Some(s) => (seen.min(s.lane.length_m), lane_count(s.lane.width_m)),
        None => (seen, 1),
    };
    step(&format!(
        "targets: {} humanoid and {} creature kinds, {clear:.0} m clear ({seen:.0} m by the game's pick), {lanes} lane(s)",
        humanoids.len(),
        creatures.len()
    ));
    let position = e.vcall(player, POSITION_SLOT, &args![]).u32();
    let here = if position == 0 {
        [0.0; 3]
    } else {
        [
            e.mem.f32(position),
            e.mem.f32(position + 4),
            e.mem.f32(position + 8),
        ]
    };
    let angle = facing_back(heading);
    let hostile = with_range(|r| r.hostile);
    let mut targets = Vec::new();
    let (mut h, mut c) = (0usize, 0usize);
    for (metres, humanoid, side) in plan(clear, lanes) {
        let (dx, dy) = offset(heading, metres, side);
        let wanted = [here[0] + dx, here[1] + dy, here[2]];
        let floor = spot.map(|s| &s.floor);
        let Some((at, nudge)) = settle(floor, wanted, heading, |p| has_room(e, p)) else {
            step(&format!(
                "targets: no room at {metres:.0} m, {side:+.1} m aside (off the floor or in geometry): skipped"
            ));
            continue;
        };
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
        let base_id = e.mem.u32(base + FORM_ID);
        if placed == 0 {
            step(&format!("targets: {base_id:08X} could not be placed"));
            continue;
        }
        let t = id(e.mem.u32(placed + FORM_ID));
        let (x, y) = (at[0] - here[0], at[1] - here[1]);
        step(&format!(
            "targets: {} ({base_id:08X}) placed as {t} at {metres:.0} m, {side:+.1} m aside{} ({:.0}, {:.0}, {:.0})",
            name(e, base),
            if nudge == 0 { String::new() } else { format!(", nudged ({:?})", NUDGES[nudge]) },
            at[0],
            at[1],
            at[2]
        ));
        run_line(e, &format!("{t}.moveto player {x:.0} {y:.0} 0"));
        if spot.is_some() {
            run_line(e, &format!("{t}.setpos z {:.0}", at[2] + STAND_ABOVE));
        }
        run_line(e, &format!("{t}.setangle z {angle:.0}"));
        for line in stance(&t, hostile) {
            run_line(e, &line);
        }
        targets.push(e.mem.u32(placed + FORM_ID));
    }
    step(&format!("targets: {} in the lane", targets.len()));
    with_range(|r| r.targets = targets);
}

/// The longest clear line of the room from the player: the current `heading` and the eight
/// compass headings, the first longest kept (the current one wins a tie).
fn best_heading(e: &mut Engine, player: u32, heading: f32) -> (f32, f32) {
    let mut best = (heading, clear_metres(e, player, heading));
    for k in 0..8 {
        let h = k as f32 * std::f32::consts::FRAC_PI_4;
        let clear = clear_metres(e, player, h);
        if clear > best.1 + 0.5 {
            best = (h, clear);
        }
    }
    best
}

/// How far, in metres, the lane facing `heading` is clear: the least of its middle line and its
/// two side lines ([`lane`]'s step to each side), each found by halving with the game's pick
/// ([`blocked`]) from 1 m ahead of the player, up to 105 m.
fn clear_metres(e: &mut Engine, player: u32, heading: f32) -> f32 {
    let position = e.vcall(player, POSITION_SLOT, &args![]).u32();
    if position == 0 {
        return 0.0;
    }
    let at = [
        e.mem.f32(position),
        e.mem.f32(position + 4),
        e.mem.f32(position + 8) + PICK_HEIGHT,
    ];
    let mut clear = MAX_CLEAR;
    for side in [-1.5f32, 0.0, 1.5] {
        let start = |m: f32| {
            let (x, y) = offset(heading, m, side);
            [at[0] + x, at[1] + y, at[2]]
        };
        let from = start(1.0);
        let free = |e: &mut Engine, m: f32| !blocked(e, from, start(1.0 + m));
        let line = if free(e, MAX_CLEAR - 1.0) {
            MAX_CLEAR
        } else {
            let (mut low, mut high) = (0.0f32, MAX_CLEAR - 1.0);
            for _ in 0..9 {
                let middle = (low + high) / 2.0;
                if free(e, middle) {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            low + 1.0
        };
        clear = clear.min(line);
    }
    clear
}

/// Whether something solid lies between `from` and `to`: the game's pick (`TES::Pick`) with the
/// mask its `PlaceAtMe` worker probes with (`005c4b30`: low bits `0x26`, high bits 3).
fn blocked(e: &mut Engine, from: [f32; 3], to: [f32; 3]) -> bool {
    let tes = e.global::<u32>(TES);
    if tes == 0 {
        return false;
    }
    e.with_stack(0x100, |e, frame| {
        let pick = frame.addr();
        let mask = pick + 0xc0;
        let start = pick + 0xd0;
        let end = pick + 0xe0;
        let hit = pick + 0xf0;
        e.call(PICK_CONSTRUCT, &args![pick]);
        e.call(STORE_VALUE, &args![mask, 0u32]);
        e.call(MASK_SET_LOW_BITS, &args![mask, 0x26u32]);
        e.call(MASK_SET_HIGH_BITS, &args![mask, 3u32]);
        let mask_value = e.mem.u32(mask);
        e.call(PICK_SET_MASK, &args![pick, mask_value]);
        for (k, (a, b)) in from.iter().zip(to).enumerate() {
            e.mem.set_f32(start + 4 * k as u32, *a);
            e.mem.set_f32(end + 4 * k as u32, b);
        }
        e.call(PICK_SET_START, &args![pick, start]);
        e.call(PICK_SET_END, &args![pick, end]);
        e.call(TES_PICK, &args![tes, pick]);
        let flag = e.call(PICK_HIT, &args![pick + 0x30, hit]).u32();
        e.call(BYTE_IS_SET, &args![flag]).bool()
    })
}

/// The pick's words, as `PlaceAtMe`'s worker uses them (`tesscriptfunctions_p3.rs`): `TES`, a
/// pick's constructor (`0xbc` bytes), a mask's store and its low and high bits, the pick's mask,
/// start and end, `TES::Pick`, the hit test of the pick's result (`+0x30`) and the byte test.
const TES: u32 = 0x011d_ea10;
const PICK_CONSTRUCT: u32 = 0x004a_3c20;
const STORE_VALUE: u32 = 0x008c_71b0;
const MASK_SET_LOW_BITS: u32 = 0x004a_39f0;
const MASK_SET_HIGH_BITS: u32 = 0x0059_ce80;
const PICK_SET_MASK: u32 = 0x004a_3f70;
const PICK_SET_START: u32 = 0x004a_3da0;
const PICK_SET_END: u32 = 0x004a_3eb0;
const TES_PICK: u32 = 0x0045_8420;
const PICK_HIT: u32 = 0x005c_53f0;
const BYTE_IS_SET: u32 = 0x0056_09f0;
/// A reference's position (virtual `+0x1f4`), the height above the feet the lines are cast at
/// (as `PlaceAtMe`'s, 64 units), and the longest line looked along, in metres.
const POSITION_SLOT: u32 = 0x1f4;
const PICK_HEIGHT: f32 = 64.0;
const MAX_CLEAR: f32 = 105.0;

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
            let heading = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
            let spot = spot_named(&with_range(|r| r.place.clone())).filter(|s| s.has_lane());
            place_targets(e, player, heading, spot.as_deref());
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
    // The base game's master is read for the range's places while the menu is up.
    start_scan(e);
    if let Some(text) = PENDING_ERROR
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        fail_on_menu(e, &text);
    }
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

    /// Patrick's clicks: the configured cell first (unless `auto`), the ranked interiors of the
    /// player's FalloutNV.esm, the last resort; a place not in the game or not started is passed
    /// for the next, a missing console stops; each failure has a line for the log and the screen.
    #[test]
    fn the_range_tries_its_places_in_order() {
        let ranked = vec![
            Target::Interior("BigHall".into()),
            Target::Interior("Hangar".into()),
        ];
        let list = candidates("auto", &ranked);
        assert_eq!(
            list,
            [
                Target::Interior("BigHall".into()),
                Target::Interior("Hangar".into()),
                last_resort()
            ]
        );
        // A ranked exterior is tried in its place; the last resort is not tried twice.
        let desert = Target::World {
            space: "WastelandNV".into(),
            x: 20,
            y: -5,
        };
        let with_desert = candidates("auto", &[desert.clone(), last_resort()]);
        assert_eq!(with_desert, [desert.clone(), last_resort()]);
        assert_eq!(desert.line(), "cow WastelandNV 20 -5");
        let list = candidates("MyCell", &ranked);
        assert_eq!(list[0], Target::Interior("MyCell".into()));
        assert_eq!(list.len(), 4);
        assert_eq!(candidates("hangar", &ranked).len(), 3, "no place twice");
        assert_eq!(last_resort().line(), "cow WastelandNV -18 0");
        assert_eq!(list[0].line(), "coc MyCell");

        // MyCell is missing, BigHall does not start, Hangar does.
        struct Game {
            missing: Vec<&'static str>,
            dead: Vec<&'static str>,
            last: String,
            ran: Vec<String>,
            console: bool,
        }
        let there = |g: &mut Game, t: &Target| !g.missing.contains(&t.name().as_str());
        let run = |g: &mut Game, t: &Target| {
            g.ran.push(t.line());
            g.last = t.name();
            g.console
        };
        let started = |g: &mut Game| !g.dead.contains(&g.last.as_str());
        let mut g = Game {
            missing: vec!["MyCell"],
            dead: vec!["BigHall"],
            last: String::new(),
            ran: Vec::new(),
            console: true,
        };
        assert_eq!(
            first_start(&mut g, &list, there, run, started),
            Ok(Target::Interior("Hangar".into()))
        );
        assert_eq!(g.ran, ["coc BigHall", "coc Hangar"]);
        // Nothing there: the last resort starts.
        let mut g = Game {
            missing: vec!["MyCell", "BigHall", "Hangar"],
            dead: vec![],
            last: String::new(),
            ran: Vec::new(),
            console: true,
        };
        assert_eq!(
            first_start(&mut g, &list, there, run, started),
            Ok(last_resort())
        );
        // No console: stops at once.
        let mut g = Game {
            missing: vec![],
            dead: vec![],
            last: String::new(),
            ran: Vec::new(),
            console: false,
        };
        assert_eq!(
            first_start(&mut g, &list, there, run, started),
            Err(StartError::NoConsole)
        );
        assert_eq!(g.ran.len(), 1);
        // Nothing starts anywhere.
        let mut g = Game {
            missing: vec![],
            dead: vec!["MyCell", "BigHall", "Hangar", "WastelandNV -18,0"],
            last: String::new(),
            ran: Vec::new(),
            console: true,
        };
        let err = first_start(&mut g, &list, there, run, started).unwrap_err();
        assert_eq!(err, StartError::NotStarted("WastelandNV -18,0".into()));
        for why in [StartError::NoPlace, StartError::NoConsole, err] {
            let text = why.text();
            assert!(
                text.starts_with("Test Range: ") && text.len() < 0x27f,
                "{text}"
            );
        }
        assert!(StartError::NoPlace.text().contains("sTestRangeCell"));
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
        static NESTED: AtomicU32 = AtomicU32::new(u32::MAX);
        e.register(COMPILE_AND_RUN, |e, a| {
            RAN.store(a[1], Ordering::Relaxed);
            // A line asked for while this one runs (the range's frame inside a `coc`): refused.
            NESTED.store(
                u32::from(run_line(e, "player.additem f 1")),
                Ordering::Relaxed,
            );
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
        assert_eq!(
            NESTED.load(Ordering::Relaxed),
            0,
            "a nested line is refused"
        );
        assert!(!RUNNING.load(Ordering::Relaxed), "the next line may run");
        // No console and none to be made: false, nothing run.
        CONSOLE.store(0, Ordering::Relaxed);
        RAN.store(0, Ordering::Relaxed);
        assert!(!run_line(&mut e, "coc QASmoke"));
        assert_eq!(RAN.load(Ordering::Relaxed), 0);
    }

    /// The set-up waits for 30 frames of play in a row (a loading screen, the main menu or a
    /// running console line starts the count again) and gives up 90 s after the `coc` returned.
    #[test]
    fn the_set_up_waits_for_play() {
        assert_eq!(wait(0, true, 0), Wait::Keep(1));
        assert_eq!(wait(28, true, 5), Wait::Keep(29));
        assert_eq!(wait(29, true, 5), Wait::SetUp);
        assert_eq!(wait(29, false, 5), Wait::Keep(0));
        assert_eq!(wait(10, false, PLAY_TIMEOUT), Wait::TimeOut);
        assert_eq!(wait(29, true, PLAY_TIMEOUT), Wait::SetUp);
    }

    /// The watchdog watches a start from its `coc` until play begins, with more time once the
    /// `coc` returned (where the range's own timeout goes back to the main menu first).
    #[test]
    fn the_watchdog_watches_the_start_only() {
        let at = |phase| {
            with_range(|r| r.phase = phase);
            watch_state()
        };
        assert_eq!(at(Phase::Off), None);
        assert_eq!(
            at(Phase::Loading).map(|s| s.1),
            Some(range_watch::STUCK_LOADING)
        );
        assert_eq!(
            at(Phase::WaitingForPlay).map(|s| s.1),
            Some(range_watch::STUCK_WAITING)
        );
        assert_eq!(at(Phase::Ready), None);
        with_range(|r| *r = Range::default());
    }

    /// A wide floor takes lanes side by side: a humanoid and a creature lane, then a second
    /// humanoid lane at every other distance; a narrow one alternates as before.
    #[test]
    fn wide_floors_take_lanes_side_by_side() {
        assert_eq!(lane_count(8.0), 1);
        assert_eq!(lane_count(12.0), 2);
        assert_eq!(lane_count(25.0), 3);
        assert_eq!(plan(105.0, 1), lane(105.0));
        let two = plan(105.0, 2);
        assert_eq!(two.len(), 2 * DISTANCES.len());
        assert!(two
            .iter()
            .all(|&(_, humanoid, side)| humanoid == (side < 0.0)));
        let three = plan(105.0, 3);
        assert_eq!(three.len(), 2 * DISTANCES.len() + DISTANCES.len() / 2);
        assert!(three.iter().any(|&(_, _, side)| side == 5.0));
    }

    /// A target's spot: on the floor with a metre to spare and free by the game's pick, nudged
    /// sideways or along when not, skipped when nothing near fits.
    #[test]
    fn targets_stand_only_where_they_fit() {
        use crate::range_floor::{Floor, UNITS_PER_METRE as M};
        // Floor 10 m wide (x 0..10 m), 100 m long, at height 5.
        let slab = [
            [
                [0.0, 0.0, 5.0],
                [10.0 * M, 0.0, 5.0],
                [10.0 * M, 100.0 * M, 5.0],
            ],
            [
                [0.0, 0.0, 5.0],
                [10.0 * M, 100.0 * M, 5.0],
                [0.0, 100.0 * M, 5.0],
            ],
        ];
        let floor = Floor::of(&slab, 0.0, M).unwrap();
        let north = 0.0;
        // In the middle: as asked, the floor's height.
        let (at, nudge) = settle(Some(&floor), [5.0 * M, 50.0 * M, 0.0], north, |_| true).unwrap();
        assert_eq!((at, nudge), ([5.0 * M, 50.0 * M, 5.0], 0));
        // Half a metre from the wall: nudged a metre in.
        let (at, nudge) = settle(Some(&floor), [9.6 * M, 50.0 * M, 0.0], north, |_| true).unwrap();
        assert_eq!(nudge, 2);
        assert!((at[0] / M - 8.6).abs() < 1e-3, "{at:?}");
        // Something solid where asked (the pick): the next nudge that is free.
        let (_, nudge) = settle(Some(&floor), [5.0 * M, 50.0 * M, 0.0], north, |p| {
            p[0] != 5.0 * M
        })
        .unwrap();
        assert_eq!(nudge, 1);
        // Off the floor: none.
        assert_eq!(
            settle(Some(&floor), [40.0 * M, 50.0 * M, 0.0], north, |_| true),
            None
        );
        // No floor known: the pick alone, the height as given.
        let (at, _) = settle(None, [1.0, 2.0, 3.0], north, |_| true).unwrap();
        assert_eq!(at, [1.0, 2.0, 3.0]);
        assert_eq!(settle(None, [1.0, 2.0, 3.0], north, |_| false), None);
    }

    /// The first builds' `QASmoke` counts as `auto`; other names stay as given.
    #[test]
    fn the_old_default_cell_is_auto() {
        assert!(is_legacy_cell("QASmoke"));
        assert!(is_legacy_cell(" qasmoke "));
        assert!(!is_legacy_cell("NorthernPassageINT"));
        assert!(!is_legacy_cell(DEFAULT_CELL));
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
        let lane = lane(105.0);
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

    /// The lane fits the room: the standard distances that fit, else four spread over the room,
    /// never past the wall less 2 m.
    #[test]
    fn the_lane_fits_the_room() {
        assert_eq!(fit_lane(105.0), DISTANCES.to_vec());
        assert_eq!(fit_lane(60.0), [5.0, 10.0, 20.0, 35.0, 50.0]);
        assert_eq!(fit_lane(38.0), [5.0, 10.0, 20.0, 35.0]);
        let short = fit_lane(22.0);
        assert_eq!(short, [5.0, 10.0, 15.0, 20.0]);
        let tiny = fit_lane(6.0);
        assert_eq!(tiny.len(), 4);
        assert!(tiny.iter().all(|&m| (2.0..=4.0).contains(&m)), "{tiny:?}");
        assert_eq!(fit_lane(1.0), [2.0]);
        for clear in [3.0, 9.0, 30.0, 80.0, 105.0] {
            let lane = lane(clear);
            assert!(
                lane.iter().all(|l| l.0 <= (clear - 2.0).max(2.0)),
                "{clear}: {lane:?}"
            );
            assert!(lane.windows(2).all(|w| w[0].1 != w[1].1));
        }
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
