//! The original launcher's graphics settings in the game's own Settings menu
//! (`NV_GRAPHICS_SETTINGS`), a row of the `fixes` preset ([`crate::fixes`], docs/FIXES.md): **not
//! 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for (Patrick, 2026-11-12: "can you
//! add the graphics settings that are natively in the game's launcher, in to the game's pause
//! menu? This way I can change graphics settings without restarting my game"); in the `vanilla`
//! preset, or with `bGraphicsSettings=0`, the menu is the original's and nothing here runs.
//!
//! **The rows.** Five user options on the Display page (`iModes` 0x10), after the Field of View
//! slider ([`crate::fov_slider`]), made the way `StartMenu::CreateMenuElements` (`007cc6e0`) makes
//! its own list options (`StartMenu::UserOption::UserOption`, `007d6350`, type 0: a value between
//! two arrows, the value strings in the option's array): Resolution, Anti-Aliasing, Anisotropic
//! Filtering, Window Mode and V-Sync, with the values Settings.cmd offers (`distribution/Settings.ps1`).
//! A value a file holds that the list lacks is a value of its own in it, as Settings.cmd keeps
//! one. The launcher's adapter choice is not offered: wgpu, the default renderer, picks the card
//! itself, and a new card needs a new device, which the game never makes in play. Labels and
//! value texts are short strings of the game's heap, made with the rows each time the menu is
//! made (a few hundred bytes, never freed, as the Field of View row's label).
//!
//! **Its change functions** ([`CHANGE_FNS`], one a row): as the Field of View row's, the menu calls
//! them with the save flag clear to read the current values, and the moved row's with it set. A
//! row's default is its current value, so the menu's Defaults leaves them as they are. Moved:
//! - **Window Mode** (Borderless / Window, Fallout.ini `[nv-rs Display] bBorderless`): applied at
//!   once. The platform's window thread gives the game's window the borderless window's style and
//!   the monitor's size, or back the game's own frame, place and size
//!   (`engine_platform::d3d9real::switch_window_mode`); the game draws on into it as before
//!   (Direct3D 9 stretches its back buffer over the window, wgpu fits its swap chain to it each
//!   frame). There is no exclusive full screen to choose: this build has none.
//! - **V-Sync** (On / Off, Fallout.ini `[Display] iPresentInterval` 1 / 0): applied at once with the
//!   Vulkan renderer (wgpu's swap chain takes the present mode at its next frame,
//!   `d3d9_wgpu::want_vsync`); the Direct3D 9 renderer's device has its interval from its creation,
//!   so there it applies at the next start.
//! - **Resolution**, **Anti-Aliasing** and **Anisotropic Filtering** (FalloutPrefs.ini `[Display]`
//!   `iSize W` / `iSize H`, `iMultiSample` 0, 2, 4, 8, `iMaxAnisotropy` 0, 2, 4, 8, 15): saved, and
//!   applied at the next start; the row's label then says "(next start)". The game's renderer
//!   takes them when it is made: its size and its multisampled targets, and the screen-sized
//!   targets, the save screenshots and the interface built on them. The game's own way to change
//!   the size of a running renderer (`004dc360`, which shuts the interface down, rebuilds the
//!   swap chain through `NiDX9Renderer::Recreate` and builds the interface again) is never reached
//!   in the PC game, and the device's `Reset` it needs is not provided with every resource it
//!   would meet (`engine_platform` d3d9device.rs): it is not used, so the picture never goes
//!   black or the device lost mid-game. The game's own anisotropy level (`011a9608`, which its
//!   console's `Maxaniso` sets) is not touched either: nothing here shows it is the one
//!   `iMaxAnisotropy` feeds.
//!
//! **Saved** in a launcher's play run only (`crate::fixes::is_play_run`), to the keys Settings.cmd
//! writes, in the profile the run uses: Fallout.ini (the game never writes it) is written beside
//! itself and renamed over ([`crate::game_ini::write_keys`]); FalloutPrefs.ini through the run's
//! copy-out (`crate::save_out::prefer`), which also sets the lines in every later copy of the
//! game's own write of the file (its settings in memory keep the values the run started with).
//! Settings.cmd reads the same keys, so each shows what the other saved; with the mods profile,
//! Settings.cmd shows the game profile's files, as for the Field of View. A file that can't be
//! written is said once in the log; the live change stays.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::startmenu::start_menu_create_menu_elements;
use std::cell::RefCell;
use std::collections::HashMap;

/// `StartMenu::CreateMenuElements` (Ghidra name), shared with [`crate::fov_slider`].
pub const CREATE_MENU_ELEMENTS: u32 = crate::fov_slider::CREATE_MENU_ELEMENTS;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "graphics in the game's settings",
    env: "NV_GRAPHICS_SETTINGS",
    addresses: &[CREATE_MENU_ELEMENTS],
    summary: "the game's Settings menu has the launcher's graphics settings on its Display page (resolution, anti-aliasing, anisotropic filtering, window mode, V-sync)",
    install,
};

/// The rows' change functions, in [`ROWS`]' order: code addresses beside the Field of View row's
/// (`crate::fov_slider::CHANGE_FN`, `0x7e06_0000`), in the platform's area that no exe or platform
/// code uses.
pub const CHANGE_FNS: [u32; 5] = [
    0x7e06_0010,
    0x7e06_0020,
    0x7e06_0030,
    0x7e06_0040,
    0x7e06_0050,
];

/// The rows, in the order they are made.
pub const ROWS: [Row; 5] = [
    Row::Resolution,
    Row::AntiAliasing,
    Row::Anisotropy,
    Row::WindowMode,
    Row::VSync,
];

/// A row.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Row {
    Resolution,
    AntiAliasing,
    Anisotropy,
    WindowMode,
    VSync,
}

impl Row {
    pub fn name(self) -> &'static str {
        match self {
            Row::Resolution => "Resolution",
            Row::AntiAliasing => "Anti-Aliasing",
            Row::Anisotropy => "Anisotropic Filtering",
            Row::WindowMode => "Window Mode",
            Row::VSync => "V-Sync",
        }
    }

    /// The menu's arrow spacing (`iArrowSpacing`): as the game's own rows of long values (85),
    /// short ones (55) and its on / off rows (0).
    fn arrow_spacing(self) -> u32 {
        match self {
            Row::Resolution | Row::WindowMode => 85,
            Row::AntiAliasing | Row::Anisotropy => 55,
            Row::VSync => 0,
        }
    }
}

/// A row's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Value {
    /// A resolution, width by height.
    Size(u32, u32),
    /// Samples or anisotropy, 0 for off.
    Times(u32),
    /// The file has no number for it: the game's own choice.
    Unset,
    /// The borderless window (true) or a window.
    Borderless(bool),
    /// V-sync on or off.
    VSync(bool),
}

impl Value {
    /// The text the menu shows between the arrows.
    pub fn text(self) -> String {
        match self {
            Value::Size(w, h) => format!("{w}x{h}"),
            Value::Times(0) => "Off".into(),
            Value::Times(n) => format!("{n}x"),
            Value::Unset => "Game's".into(),
            Value::Borderless(true) => "Borderless".into(),
            Value::Borderless(false) => "Window".into(),
            Value::VSync(true) => "On".into(),
            Value::VSync(false) => "Off".into(),
        }
    }
}

/// Settings.cmd's resolutions after the monitor's own (`distribution/Settings.ps1`).
pub const RESOLUTIONS: [(u32, u32); 5] = [
    (3840, 2160),
    (2560, 1440),
    (1920, 1080),
    (1600, 900),
    (1280, 720),
];
/// Settings.cmd's anti-aliasing and anisotropic filtering numbers.
pub const SAMPLES: [u32; 4] = [0, 2, 4, 8];
pub const ANISOTROPY: [u32; 5] = [0, 2, 4, 8, 15];

/// A row's values: Settings.cmd's, `current` among them (a value of its own when they lack it),
/// and the monitor's own size `native` among the resolutions; largest first for the sizes,
/// smallest first for the rest.
pub fn values(row: Row, current: Value, native: Option<(u32, u32)>) -> Vec<Value> {
    let mut values: Vec<Value> = match row {
        Row::Resolution => {
            let mut sizes: Vec<(u32, u32)> = native.into_iter().chain(RESOLUTIONS).collect();
            if let Value::Size(w, h) = current {
                if w > 0 && h > 0 {
                    sizes.push((w, h));
                }
            }
            sizes.sort_by_key(|&(w, h)| std::cmp::Reverse((u64::from(w) * u64::from(h), w)));
            sizes.dedup();
            return sizes.into_iter().map(|(w, h)| Value::Size(w, h)).collect();
        }
        Row::AntiAliasing => SAMPLES.iter().map(|&n| Value::Times(n)).collect(),
        Row::Anisotropy => ANISOTROPY.iter().map(|&n| Value::Times(n)).collect(),
        Row::WindowMode => vec![Value::Borderless(true), Value::Borderless(false)],
        Row::VSync => vec![Value::VSync(false), Value::VSync(true)],
    };
    if !values.contains(&current) {
        match current {
            Value::Times(_) => {
                values.push(current);
                values.sort_by_key(|v| match v {
                    Value::Times(n) => *n,
                    _ => 0,
                });
            }
            Value::Unset => values.insert(0, current),
            _ => {}
        }
    }
    values
}

/// What a moved row did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applied {
    /// It shows now.
    Now,
    /// It is saved and shows at the next start.
    NextStart,
}

/// The row's label: its name, and "(next start)" while the value chosen differs from the one the
/// run shows.
pub fn label(row: Row, waiting: bool) -> String {
    if waiting {
        format!("{} (next start)", row.name())
    } else {
        row.name().to_string()
    }
}

/// The keys a value is saved to: `(file, section, key, value)`, the file `true` for
/// FalloutPrefs.ini, `false` for Fallout.ini; Settings.cmd's keys and numbers.
pub fn keys(value: Value) -> Vec<(bool, &'static str, &'static str, String)> {
    match value {
        Value::Size(w, h) => vec![
            (true, "Display", "iSize W", w.to_string()),
            (true, "Display", "iSize H", h.to_string()),
        ],
        Value::Times(_) | Value::Unset => vec![],
        Value::Borderless(on) => vec![(
            false,
            "nv-rs Display",
            "bBorderless",
            u32::from(on).to_string(),
        )],
        Value::VSync(on) => vec![(
            false,
            "Display",
            "iPresentInterval",
            u32::from(on).to_string(),
        )],
    }
}

/// [`keys`] for a row: the numbers' rows name their key.
pub fn row_keys(row: Row, value: Value) -> Vec<(bool, &'static str, &'static str, String)> {
    match (row, value) {
        (Row::AntiAliasing, Value::Times(n)) => {
            vec![(true, "Display", "iMultiSample", n.to_string())]
        }
        (Row::Anisotropy, Value::Times(n)) => {
            vec![(true, "Display", "iMaxAnisotropy", n.to_string())]
        }
        _ => keys(value),
    }
}

// ---- The game's words ------------------------------------------------------------------------

/// The settings the run started with (values at `+4`): `iSize W:Display`, `iSize H:Display`,
/// `iMultiSample:Display`, `iPresentInterval:Display` (crates/engine-fallout-cell renderer.rs).
const I_SIZE_W: u32 = 0x011c_73dc;
const I_SIZE_H: u32 = 0x011c_718c;
const I_MULTI_SAMPLE: u32 = 0x011c_73f4;
const I_PRESENT_INTERVAL: u32 = 0x011c_7628;
const VALUE: u32 = 4;

/// `pUserOptionsA`, its `Add(&item)`, `operator new` and `StartMenu::UserOption::UserOption` (as
/// [`crate::fov_slider`]).
const USER_OPTIONS: u32 = 0x011d_ab50;
const ARRAY_ADD: u32 = 0x007c_b2e0;
const OPERATOR_NEW: u32 = 0x0040_1000;
const USER_OPTION_NEW: u32 = 0x007d_6350;
const USER_OPTION_SIZE: u32 = 0x30;
/// A user option's text, value and default.
const TEXT: u32 = 4;
const VAL: u32 = 0x14;
const DEFAULT_VAL: u32 = 0x18;
/// The Display page, the list type (a value between arrows), the label's buffer.
const DISPLAY: u32 = 0x10;
const LIST: u32 = 0;
const LABEL_SIZE: u32 = 48;
/// `StartMenu::pMe`, its flags test, the save flag, the options list and its `GetTileFor`;
/// `Tile::SetString` and the string trait.
const START_MENU: u32 = 0x011d_aac0;
const MENU_FLAGS_TEST: u32 = 0x004a_4080;
const FLAG_SAVE_OPTIONS: u32 = 0x20;
const OPTIONS_LIST: u32 = 0x114;
const GET_TILE_FOR: u32 = 0x007a_22d0;
const TILE_SET_STRING: u32 = 0x00a0_1350;
const TRAIT_STRING: u32 = 4036;

/// What the rows know in this run (the menu runs on main): each row made (by its option's
/// address) with its values; the value chosen for each row in this run; the value each row
/// showed the first time it was read (what the run shows, for the rows that apply at the next
/// start).
#[derive(Default)]
struct State {
    made: HashMap<u32, (Row, Vec<Value>)>,
    chosen: HashMap<Row, Value>,
    shown: HashMap<Row, Value>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Registers `CreateMenuElements` and the rows' change functions.
pub fn install(e: &mut Engine) {
    e.register(CREATE_MENU_ELEMENTS, |e, _| {
        start_menu_create_menu_elements(e);
        add_rows(e);
        Ret::default()
    });
    install_change_fns(e);
}

/// The rows' change functions alone (`crate::fixes::install` joins the two rows' `CreateMenuElements`).
pub fn install_change_fns(e: &mut Engine) {
    /// One function a row (a registered function is a plain function).
    fn row_fn<const N: usize>(e: &mut Engine, a: &[u32]) -> Ret {
        change(e, ROWS[N], Ptr::new(a[0]));
        Ret::default()
    }
    let fns: [AbiFn; 5] = [
        row_fn::<0>,
        row_fn::<1>,
        row_fn::<2>,
        row_fn::<3>,
        row_fn::<4>,
    ];
    for (address, f) in CHANGE_FNS.into_iter().zip(fns) {
        e.register(address, f);
    }
}

/// An `i32` setting the run started with.
fn setting(e: &Engine, address: u32) -> i32 {
    e.mem.i32(address + VALUE)
}

/// The value the run shows for `row`, as far as it is known here.
fn running(e: &Engine, row: Row) -> Value {
    match row {
        Row::Resolution => Value::Size(
            setting(e, I_SIZE_W).max(0) as u32,
            setting(e, I_SIZE_H).max(0) as u32,
        ),
        Row::AntiAliasing => Value::Times(setting(e, I_MULTI_SAMPLE).max(0) as u32),
        Row::Anisotropy => prefs_number("iMaxAnisotropy").map_or(Value::Unset, Value::Times),
        Row::WindowMode => Value::Borderless(window_is_borderless()),
        Row::VSync => Value::VSync(setting(e, I_PRESENT_INTERVAL) != 0),
    }
}

/// Whether the game's window is the borderless one now.
fn window_is_borderless() -> bool {
    #[cfg(windows)]
    {
        engine_platform::d3d9real::is_borderless()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// The monitor's own resolution.
fn native() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        engine_platform::d3d9real::native_size()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// A number of the run's FalloutPrefs.ini `[Display]` (the profile's, beside its Fallout.ini).
fn prefs_number(key: &str) -> Option<u32> {
    let path = crate::game_ini::path()?.with_file_name("FalloutPrefs.ini");
    let bytes = std::fs::read(path).ok()?;
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    crate::game_ini::value(&text, "Display", key)?
        .trim()
        .parse()
        .ok()
}

/// The value a row shows now: the one chosen in this run, else the run's (the window's as it is
/// now).
fn current(e: &Engine, row: Row) -> Value {
    let chosen = STATE.with(|s| s.borrow().chosen.get(&row).copied());
    match row {
        Row::WindowMode => chosen.unwrap_or_else(|| running(e, row)),
        _ => chosen.unwrap_or_else(|| shown(e, row)),
    }
}

/// The value the run showed `row` with the first time it was read in this run.
fn shown(e: &Engine, row: Row) -> Value {
    if let Some(value) = STATE.with(|s| s.borrow().shown.get(&row).copied()) {
        return value;
    }
    let value = running(e, row);
    STATE.with(|s| s.borrow_mut().shown.insert(row, value));
    value
}

/// Whether `row`'s label says the value waits for the next start.
fn waiting(e: &Engine, row: Row) -> bool {
    match row {
        Row::WindowMode => current(e, row) != running(e, row),
        Row::VSync if vsync_is_live() => false,
        _ => current(e, row) != shown(e, row),
    }
}

/// Whether V-sync applies at once (the Vulkan renderer draws).
fn vsync_is_live() -> bool {
    #[cfg(windows)]
    {
        engine_platform::d3d9real::wgpu_draws()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// A string of the game's heap holding `text`, 0 when none could be had.
fn heap_text(e: &mut Engine, text: &str, size: u32) -> u32 {
    let buffer = e.call(OPERATOR_NEW, &args![size]).u32();
    if buffer != 0 {
        let bytes = &text.as_bytes()[..text.len().min(size as usize - 1)];
        e.mem.set_cstr(buffer, bytes);
    }
    buffer
}

/// Makes the rows and appends them to `pUserOptionsA`, after the game's own and the Field of View
/// row (a failed allocation appends nothing).
pub fn add_rows(e: &mut Engine) {
    let native = native();
    for (row, change_fn) in ROWS.into_iter().zip(CHANGE_FNS) {
        let values = values(row, current(e, row), native);
        let text = heap_text(e, &label(row, waiting(e, row)), LABEL_SIZE);
        let strings: Vec<u32> = values.iter().map(|v| heap_text(e, &v.text(), 16)).collect();
        let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
        if text == 0 || block == 0 || strings.contains(&0) {
            continue;
        }
        let mut call = args![
            block,
            text,
            change_fn,
            DISPLAY,
            LIST,
            values.len() as u32,
            row.arrow_spacing()
        ];
        call.extend(strings.iter().copied());
        let option = e.call(USER_OPTION_NEW, &call).u32();
        STATE.with(|s| s.borrow_mut().made.insert(option, (row, values)));
        e.with_stack(4, |e, cell| {
            e.mem.set_u32(cell.addr(), option);
            e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
        });
    }
}

/// A row's change function (module documentation).
fn change(e: &mut Engine, row: Row, option: Ptr) {
    if option.is_null() {
        return;
    }
    let Some(values) = STATE.with(|s| s.borrow().made.get(&option.addr()).map(|m| m.1.clone()))
    else {
        return;
    };
    let menu = e.global::<u32>(START_MENU);
    let saving = menu != 0
        && e.call(MENU_FLAGS_TEST, &args![menu, FLAG_SAVE_OPTIONS])
            .u32()
            & 0xff
            != 0;
    let now = current(e, row);
    if !saving {
        let index = values.iter().position(|v| *v == now).unwrap_or(0) as u32;
        e.mem.set_u32(option.addr() + VAL, index);
        e.mem.set_u32(option.addr() + DEFAULT_VAL, index);
        write_label(e, menu, option, row);
        return;
    }
    let index = (e.mem.u32(option.addr() + VAL) as usize).min(values.len().saturating_sub(1));
    let Some(&value) = values.get(index) else {
        return;
    };
    if value == now {
        return;
    }
    STATE.with(|s| s.borrow_mut().chosen.insert(row, value));
    let applied = apply(row, value);
    let saved = save(e, row, value);
    write_label(e, menu, option, row);
    eprintln!(
        "Graphics settings: {} {}: {}{}",
        row.name(),
        value.text(),
        match applied {
            Applied::Now => "applied now",
            Applied::NextStart => "applies at the next start",
        },
        if saved { ", saved" } else { ", not saved" }
    );
}

/// Applies `value` now where it can be ([`Applied`]).
fn apply(row: Row, value: Value) -> Applied {
    #[cfg(windows)]
    {
        use engine_platform::d3d9real;
        match (row, value) {
            (Row::WindowMode, Value::Borderless(on)) if d3d9real::switch_window_mode(on) => {
                return Applied::Now
            }
            (Row::VSync, Value::VSync(on)) if d3d9real::want_vsync(on) => return Applied::Now,
            _ => {}
        }
    }
    let _ = (row, value);
    Applied::NextStart
}

/// The row's label into its text and its tile, when the row is shown.
fn write_label(e: &mut Engine, menu: u32, option: Ptr, row: Row) {
    let text = e.mem.u32(option.addr() + TEXT);
    if text == 0 {
        return;
    }
    let label = label(row, waiting(e, row));
    let bytes = &label.as_bytes()[..label.len().min(LABEL_SIZE as usize - 1)];
    if e.mem.cstr(text) == bytes {
        return;
    }
    e.mem.set_cstr(text, bytes);
    if menu == 0 {
        return;
    }
    let tile = e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option.addr());
        e.call(GET_TILE_FOR, &args![menu + OPTIONS_LIST, cell])
            .u32()
    });
    if tile != 0 {
        e.call(TILE_SET_STRING, &args![tile, TRAIT_STRING, text, 1u32]);
    }
}

/// `value`'s keys into the run's files ([`row_keys`]), in a launcher's play run only; false when
/// not saved (a file that can't be written is said once in the log).
fn save(e: &Engine, row: Row, value: Value) -> bool {
    if !crate::fixes::is_play_run(|name| std::env::var(name).ok()) {
        return false;
    }
    let keys = row_keys(row, value);
    let prefs: Vec<(&str, &str, String)> = keys
        .iter()
        .filter(|k| k.0)
        .map(|k| (k.1, k.2, k.3.clone()))
        .collect();
    let ini: Vec<(&str, &str, &str)> = keys
        .iter()
        .filter(|k| !k.0)
        .map(|k| (k.1, k.2, k.3.as_str()))
        .collect();
    let mut saved = true;
    if !prefs.is_empty() && !crate::save_out::prefer(e, &prefs) {
        saved = false;
    }
    if !ini.is_empty() {
        let written = crate::game_ini::path()
            .ok_or_else(|| "no Fallout.ini for this run".to_string())
            .and_then(|path| {
                crate::game_ini::write_keys(&path, &ini)
                    .map_err(|why| format!("{} ({why})", path.display()))
            });
        if let Err(why) = written {
            saved = false;
            static NAMED: std::sync::Once = std::sync::Once::new();
            NAMED.call_once(|| eprintln!("Graphics settings: {why} not saved"));
        }
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn the_values_are_settings_cmds_with_the_files_own() {
        let sizes = |current, native| -> Vec<String> {
            values(Row::Resolution, current, native)
                .iter()
                .map(|v| v.text())
                .collect()
        };
        assert_eq!(
            sizes(Value::Size(1920, 1080), Some((2560, 1440))),
            [
                "3840x2160",
                "2560x1440",
                "1920x1080",
                "1600x900",
                "1280x720"
            ]
        );
        assert_eq!(
            sizes(Value::Size(1366, 768), Some((3440, 1440))),
            [
                "3840x2160",
                "3440x1440",
                "2560x1440",
                "1920x1080",
                "1600x900",
                "1366x768",
                "1280x720"
            ],
            "the monitor's and the file's own sizes among Settings.cmd's"
        );
        let texts = |row, current| -> Vec<String> {
            values(row, current, None)
                .iter()
                .map(|v| v.text())
                .collect()
        };
        assert_eq!(
            texts(Row::AntiAliasing, Value::Times(4)),
            ["Off", "2x", "4x", "8x"]
        );
        assert_eq!(
            texts(Row::AntiAliasing, Value::Times(6)),
            ["Off", "2x", "4x", "6x", "8x"]
        );
        assert_eq!(
            texts(Row::Anisotropy, Value::Times(15)),
            ["Off", "2x", "4x", "8x", "15x"]
        );
        assert_eq!(
            texts(Row::Anisotropy, Value::Unset),
            ["Game's", "Off", "2x", "4x", "8x", "15x"]
        );
        assert_eq!(
            texts(Row::WindowMode, Value::Borderless(false)),
            ["Borderless", "Window"]
        );
        assert_eq!(texts(Row::VSync, Value::VSync(true)), ["Off", "On"]);
    }

    /// The keys and numbers Settings.cmd writes for the same choices.
    #[test]
    fn each_value_goes_to_settings_cmds_key() {
        assert_eq!(
            row_keys(Row::Resolution, Value::Size(1920, 1080)),
            [
                (true, "Display", "iSize W", "1920".to_string()),
                (true, "Display", "iSize H", "1080".to_string())
            ]
        );
        assert_eq!(
            row_keys(Row::AntiAliasing, Value::Times(0)),
            [(true, "Display", "iMultiSample", "0".to_string())]
        );
        assert_eq!(
            row_keys(Row::Anisotropy, Value::Times(15)),
            [(true, "Display", "iMaxAnisotropy", "15".to_string())]
        );
        assert!(row_keys(Row::Anisotropy, Value::Unset).is_empty());
        assert_eq!(
            row_keys(Row::WindowMode, Value::Borderless(false)),
            [(false, "nv-rs Display", "bBorderless", "0".to_string())]
        );
        assert_eq!(
            row_keys(Row::VSync, Value::VSync(true)),
            [(false, "Display", "iPresentInterval", "1".to_string())]
        );
    }

    #[test]
    fn a_label_says_when_its_value_waits_for_the_next_start() {
        assert_eq!(label(Row::Resolution, false), "Resolution");
        assert_eq!(
            label(Row::Anisotropy, true),
            "Anisotropic Filtering (next start)"
        );
        assert!(label(Row::Anisotropy, true).len() < LABEL_SIZE as usize);
    }

    /// An engine with the run's settings (1280x720, no multisampling, V-sync on), the menu's flags
    /// (`saving`), the tile of each row (`0x5000`) and every `SetString` recorded; the rows made
    /// with their values as `add_rows` makes them.
    fn engine(saving: bool) -> (Engine, Rc<RefCell<Vec<Vec<u32>>>>) {
        STATE.with(|s| *s.borrow_mut() = State::default());
        let mut e = Engine::new();
        for page in [I_SIZE_H, I_SIZE_W, START_MENU, 0x0001_0000] {
            e.mem.map(page & !0xfff, 0x1000);
        }
        e.mem.set_i32(I_SIZE_W + VALUE, 1280);
        e.mem.set_i32(I_SIZE_H + VALUE, 720);
        e.mem.set_i32(I_MULTI_SAMPLE + VALUE, 0);
        e.mem.set_i32(I_PRESENT_INTERVAL + VALUE, 1);
        e.set_global(START_MENU, 0x0001_0100u32);
        e.register_double(MENU_FLAGS_TEST, move |_, a| Ret {
            eax: u32::from(saving && a[1] == FLAG_SAVE_OPTIONS),
            ..Ret::default()
        });
        e.register(GET_TILE_FOR, |_, _| 0x5000u32.into_ret());
        let strings = Rc::new(RefCell::new(Vec::new()));
        let seen = strings.clone();
        e.register_double(TILE_SET_STRING, move |_, a| {
            seen.borrow_mut().push(a[..4].to_vec());
            Ret::default()
        });
        (e, strings)
    }

    /// `row` made at `option` with its label at `option + 0x40`, as `add_rows` would.
    fn made(e: &mut Engine, row: Row, option: u32) -> Ptr {
        let values = values(row, current(e, row), None);
        let text = option + 0x40;
        e.mem.set_cstr(text, label(row, false).as_bytes());
        e.mem.set_u32(option + TEXT, text);
        STATE.with(|s| s.borrow_mut().made.insert(option, (row, values)));
        Ptr::new(option)
    }

    #[test]
    fn reading_shows_the_runs_values_and_the_default_is_the_current_one() {
        let (mut e, strings) = engine(false);
        let option = made(&mut e, Row::Resolution, 0x0001_0400);
        change(&mut e, Row::Resolution, option);
        // 3840x2160, 2560x1440, 1920x1080, 1600x900, 1280x720.
        assert_eq!(e.mem.u32(option.addr() + VAL), 4);
        assert_eq!(e.mem.u32(option.addr() + DEFAULT_VAL), 4);
        let option = made(&mut e, Row::VSync, 0x0001_0600);
        change(&mut e, Row::VSync, option);
        assert_eq!(e.mem.u32(option.addr() + VAL), 1, "on");
        assert!(strings.borrow().is_empty(), "the labels stay");
    }

    /// Moved off the run's resolution, the row says it waits for the next start, and the run's
    /// own setting is left as it is (the renderer, the save screenshots and the interface keep
    /// the size they were made at); moved back, the label is plain again.
    #[test]
    fn a_resolution_chosen_waits_for_the_next_start() {
        let (mut e, strings) = engine(true);
        let option = made(&mut e, Row::Resolution, 0x0001_0400);
        e.mem.set_u32(option.addr() + VAL, 2);
        change(&mut e, Row::Resolution, option);
        assert_eq!(
            current(&e, Row::Resolution),
            Value::Size(1920, 1080),
            "the menu shows the choice"
        );
        assert_eq!(
            e.mem.i32(I_SIZE_W + VALUE),
            1280,
            "the run's own setting stays"
        );
        assert_eq!(e.mem.i32(I_SIZE_H + VALUE), 720);
        assert_eq!(e.mem.cstr(0x0001_0440), b"Resolution (next start)");
        assert_eq!(
            *strings.borrow(),
            [vec![0x5000, TRAIT_STRING, 0x0001_0440, 1]]
        );
        e.mem.set_u32(option.addr() + VAL, 4);
        change(&mut e, Row::Resolution, option);
        assert_eq!(e.mem.cstr(0x0001_0440), b"Resolution");
        assert_eq!(strings.borrow().len(), 2);
    }

    /// The same value again changes nothing.
    #[test]
    fn the_same_value_again_changes_nothing() {
        let (mut e, strings) = engine(true);
        let option = made(&mut e, Row::AntiAliasing, 0x0001_0400);
        e.mem.set_u32(option.addr() + VAL, 0);
        change(&mut e, Row::AntiAliasing, option);
        assert!(STATE.with(|s| s.borrow().chosen.is_empty()));
        assert!(strings.borrow().is_empty());
    }
}
