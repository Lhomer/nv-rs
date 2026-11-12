//! A field of view slider in the game's own Settings menu (`NV_FOV_SLIDER`), a row of the `fixes`
//! preset ([`crate::fixes`], docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the
//! player asked for (Patrick, 2026-11-12); in the `vanilla` preset, or with `bFovSlider=0`, the
//! menu is the original's and nothing here runs.
//!
//! **What it sets.** One number for two of the game's Fallout.ini settings (the player asked for
//! the weapon and arms to follow the world, 2026-11-12): the world's field of view,
//! `[Display] fDefaultWorldFOV` (the float setting at `0120315c`), and the first-person view's, the
//! arms and the weapon, `[Display] fDefault1stPersonFOV` (`01203168`). Both have their value at
//! `+4`, both are in the INI collection `011f96a0` (the game reads them from Fallout.ini), and both
//! start at the constant `0102f0f8`, 75 (the names come from the gaps between their name strings,
//! `010c28b0` and `010c28cc`). Every frame of play outside the vanity camera the player's character
//! steers its two fields toward them, `fWorldFOV` and `f1stPersonFOV` (`PlayerCharacter`'s
//! `0095de30`, when the Fallout.ini bool at `011e097c`, default 1, is on; zoomed, the first-person
//! target is scaled from the same two), and the first-person pass hands `f1stPersonFOV` to its
//! camera's `SetCameraFOV` each time it draws (read through `00874900`), so a new value shows on
//! both as soon as play resumes. The Pip-Boy's own first-person view (`fPipboy1stPersonFOV`,
//! `01203174`) is left alone. Settings.cmd sets the same two keys, from 70 to 120
//! (`distribution/Settings.ps1`), and so does this slider: each shows what the other saved.
//!
//! **The row.** The Settings menu's options are made once by `StartMenu::CreateMenuElements`
//! (`007cc6e0`): plain options in `pOptionsA`, user options (toggles and sliders) in
//! `pUserOptionsA`, each with the pages it shows on (`iModes`: 0x10 the Display page). Here
//! `CreateMenuElements` runs as the translation has it, and then one more user option is made the
//! way it makes its own (`StartMenu::UserOption::UserOption`, `007d6350`): Display (0x10), a meter
//! (type 2, the slider the Display page's other sliders use), 51 settings (70 to 120 degrees), no
//! arrow spacing. It is the last row of the Display page. Its label is a 32-byte text of the game's
//! heap, `Field of View: <degrees>`, made again whenever the value changes.
//!
//! **Its change function** ([`CHANGE_FN`]): the menu calls every user option's change function
//! with the save flag (`0x20`) clear to read the current values (`007d3aa0`), and the dirty
//! option's with it set when the player moves it (`StartMenu::Update`, after `DisplayOption`).
//! Reading, the row's value is the world setting's (rounded, kept inside the slider) and its
//! default 75 (the menu's Defaults puts both back to 75). Moved, both settings get the row's
//! degrees, the row's label shows them, and in a launcher's play run the run's Fallout.ini gets
//! `fDefaultWorldFOV=<degrees>.0000` and `fDefault1stPersonFOV=<degrees>.0000` (the game's own
//! `%.4f`, as Settings.cmd writes them; `crate::game_ini::set_key`, Play.exe's rule), so the next
//! start has them.
//! The game itself never writes Fallout.ini, only FalloutPrefs.ini.
//!
//! **After a kill cam** (Patrick, 2026-11-12: "Whenever I get a cinematic kill cam, my viewmodel
//! returns to the original viewmodel and undoes the changes that the FOV slider did. This persists
//! until I enter the pip boy"). `0095de30` has one branch that keeps the world's field of view on
//! its setting (`00950610`) but leaves the arms' `f1stPersonFOV` where it is: while the free (vanity)
//! camera byte `011e07b8` and the "settled" byte `011a3b31` are both set it does that and returns.
//! The kill cam's playback (VATS mode 4) sets `011a3b31`; a cinematic that leaves `011e07b8` set
//! keeps the arms at whatever the kill cam left until the Pip-Boy (`0095de30`'s first branch writes
//! `f1stPersonFOV` itself, and the steering takes over from there). So `0095de30` is wrapped here:
//! in that branch, and only while the arms are on the screen (VATS mode 0, first person, no Pip-Boy,
//! no iron sights: [`arms_left_behind`]), the arms get `fDefault1stPersonFOV` at once, the value the
//! steering would end at. Every other branch, the kill cam itself and `fPipboy1stPersonFOV` are
//! untouched.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_ai::playercharacter_p3::fn_0095de30;
use engine::units::fallout_interface::startmenu::start_menu_create_menu_elements;

/// `StartMenu::CreateMenuElements` (Ghidra name).
pub const CREATE_MENU_ELEMENTS: u32 = 0x007c_c6e0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "field of view in the game's settings",
    env: "NV_FOV_SLIDER",
    addresses: &[CREATE_MENU_ELEMENTS, FOV_UPDATE],
    summary: "the game's Settings menu has a Field of View slider on its Display page (fDefaultWorldFOV and fDefault1stPersonFOV, 70 to 120)",
    install,
};

/// The row's change function: a code address no exe code and no platform code uses, in the
/// platform's area (`engine_core::mem::PLATFORM_AREA`), between nvapi's code (`0x7e04_0000`) and
/// the platform's objects (`0x7e12_0000` on).
pub const CHANGE_FN: u32 = 0x7e06_0000;

/// `fDefaultWorldFOV:Display`, `fDefault1stPersonFOV:Display` and where a setting's value is
/// (`+4`, as `0043d4d0` gives it).
const WORLD_FOV: u32 = 0x0120_315c;
const FIRST_PERSON_FOV: u32 = 0x0120_3168;
/// Their Fallout.ini keys, in `[Display]`, in the order they are written.
pub const KEYS: [&str; 2] = ["fDefaultWorldFOV", "fDefault1stPersonFOV"];
const VALUE: u32 = 4;
/// The slider's ends and the original's default, in degrees (Settings.cmd's too).
pub const LOWEST: i32 = 70;
pub const HIGHEST: i32 = 120;
pub const ORIGINAL: i32 = 75;

/// `pUserOptionsA` (`BSSimpleArray<StartMenu::UserOption *, 1024>`, Xbox PDB) and its `Add(&item)`
/// (`007cb2e0`, `RET 4`).
const USER_OPTIONS: u32 = 0x011d_ab50;
const ARRAY_ADD: u32 = 0x007c_b2e0;
/// `operator new` (`00401000`) and `StartMenu::UserOption::UserOption` (`007d6350`, cdecl: `this`,
/// text, change function, modes, type, number of settings, arrow spacing).
const OPERATOR_NEW: u32 = 0x0040_1000;
const USER_OPTION_NEW: u32 = 0x007d_6350;
/// A user option's size, and its words: the text (`pDisplayText`, `+4`), the value and default
/// (`iVal`, `iDefaultVal`, `+0x14`, `+0x18`).
const USER_OPTION_SIZE: u32 = 0x30;
const TEXT: u32 = 4;
const VAL: u32 = 0x14;
const DEFAULT_VAL: u32 = 0x18;
/// The Display page's mode bit, the meter type, and the label's buffer.
const DISPLAY: u32 = 0x10;
const METER: u32 = 2;
const LABEL_SIZE: u32 = 32;

/// `StartMenu::pMe`, the `uiFlags` test (`004a4080`, `thiscall(bits)`), the save flag, the options
/// list (`lbOptions`, `+0x114`) and its `GetTileFor(&option)` (`007a22d0`).
const START_MENU: u32 = 0x011d_aac0;
const MENU_FLAGS_TEST: u32 = 0x004a_4080;
const FLAG_SAVE_OPTIONS: u32 = 0x20;
const OPTIONS_LIST: u32 = 0x114;
const GET_TILE_FOR: u32 = 0x007a_22d0;
/// `Tile::SetString(id, text, flag)` (`00a01350`) and the string trait.
const TILE_SET_STRING: u32 = 0x00a0_1350;
const TRAIT_STRING: u32 = 4036;

/// Registers `CreateMenuElements` and the row's change function.
pub fn install(e: &mut Engine) {
    e.register(FOV_UPDATE, |e, a| {
        let player = a[0];
        let left_behind = player != 0 && arms_left_behind(arms_state(e, player));
        fn_0095de30(e, Ptr::new(player), f32::from_bits(a[1]));
        if left_behind {
            restore_arms(e, player);
        }
        Ret::default()
    });
    e.register(CREATE_MENU_ELEMENTS, |e, _| {
        start_menu_create_menu_elements(e);
        add_row(e);
        Ret::default()
    });
    e.register(CHANGE_FN, |e, a| {
        change(e, Ptr::new(a[0]));
        Ret::default()
    });
}

/// `PlayerCharacter`'s per-frame field of view update (`0095de30`).
pub const FOV_UPDATE: u32 = 0x0095_de30;

/// What [`arms_left_behind`] reads before `0095de30` runs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArmsState {
    /// The Fallout.ini bool `0095de30` needs (`011e097c`).
    pub enabled: bool,
    /// The free (vanity) camera byte `011e07b8` and the settled byte `011a3b31`.
    pub free_camera: bool,
    pub settled: bool,
    /// The VATS mode (the word at `011f2250 + 8`): 0 none, 2 menu, 3 scan, 4 the kill cam.
    pub vats_mode: i32,
    pub third_person: bool,
    pub pipboy: bool,
    pub iron_sights: bool,
}

/// Whether `0095de30` is about to take its world-only branch while the arms are shown: enabled,
/// the free camera and settled bytes set, no VATS, first person, no Pip-Boy, no iron sights (the
/// zoomed arms' own target is left to the game).
pub fn arms_left_behind(state: ArmsState) -> bool {
    state.enabled
        && state.free_camera
        && state.settled
        && state.vats_mode == 0
        && !state.third_person
        && !state.pipboy
        && !state.iron_sights
}

/// The game's words [`arms_state`] reads.
const FOV_ENABLED_SETTING: u32 = 0x011e_097c;
const SETTING_BOOL_POINTER: u32 = 0x0040_8d60;
const FREE_CAMERA: u32 = 0x011e_07b8;
const FOV_SETTLED: u32 = 0x011a_3b31;
const VATS_MODE: u32 = 0x011f_2250 + 8;
const THIRD_PERSON: u32 = 0x64a;
const PLAYER_IS_PIPBOY_ACTIVE: u32 = 0x0096_7ae0;
const ACTOR_GET_IRON_SIGHTS: u32 = 0x008b_bc10;
/// `fDefault1stPersonFOV`'s value and the player's `f1stPersonFOV`.
const FIRST_PERSON_DEFAULT: u32 = 0x0120_3168 + 4;
const ARMS_FOV: u32 = 0x674;

/// The state before `0095de30` runs (the calls only when the cheap bytes already say so).
fn arms_state(e: &mut Engine, player: u32) -> ArmsState {
    let free_camera = e.mem.u8(FREE_CAMERA) != 0;
    let settled = e.mem.u8(FOV_SETTLED) != 0;
    let mut state = ArmsState {
        free_camera,
        settled,
        vats_mode: e.mem.i32(VATS_MODE),
        third_person: e.mem.u8(player + THIRD_PERSON) != 0,
        ..ArmsState::default()
    };
    if free_camera && settled && state.vats_mode == 0 && !state.third_person {
        let enabled = e
            .call(SETTING_BOOL_POINTER, &args![FOV_ENABLED_SETTING])
            .u32();
        state.enabled = enabled != 0 && e.mem.u8(enabled) != 0;
        state.pipboy = e.call(PLAYER_IS_PIPBOY_ACTIVE, &args![player]).bool();
        state.iron_sights = e.call(ACTOR_GET_IRON_SIGHTS, &args![player]).bool();
    }
    state
}

/// The arms back on `fDefault1stPersonFOV`, the value `0095de30`'s steering ends at.
fn restore_arms(e: &mut Engine, player: u32) {
    let default = e.mem.f32(FIRST_PERSON_DEFAULT);
    e.mem.set_f32(player + ARMS_FOV, default);
}

/// The slider's step for `degrees` (kept inside the slider) and the degrees of a step.
pub fn step_of(degrees: f32) -> u32 {
    let whole = if degrees.is_nan() {
        ORIGINAL
    } else {
        (degrees.round() as i32).clamp(LOWEST, HIGHEST)
    };
    (whole - LOWEST) as u32
}

pub fn degrees_of(step: u32) -> i32 {
    LOWEST + step.min((HIGHEST - LOWEST) as u32) as i32
}

/// The row's label for `degrees`.
pub fn label(degrees: i32) -> String {
    format!("Field of View: {degrees}")
}

/// The setting's value now.
fn world_fov(e: &Engine) -> f32 {
    e.mem.f32(WORLD_FOV + VALUE)
}

/// Makes the row and appends it to `pUserOptionsA`, as `CreateMenuElements` appends its own (a
/// failed allocation appends nothing here: the original appends a null).
fn add_row(e: &mut Engine) {
    let text = e.call(OPERATOR_NEW, &args![LABEL_SIZE]).u32();
    let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
    if text == 0 || block == 0 {
        return;
    }
    let degrees = degrees_of(step_of(world_fov(e)));
    write_label(e, text, degrees);
    let option = e
        .call(
            USER_OPTION_NEW,
            &args![
                block,
                text,
                CHANGE_FN,
                DISPLAY,
                METER,
                (HIGHEST - LOWEST + 1) as u32,
                0u32
            ],
        )
        .u32();
    e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option);
        e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
    });
}

/// The label's text into the row's buffer.
fn write_label(e: &mut Engine, text: u32, degrees: i32) {
    let label = label(degrees);
    let bytes = &label.as_bytes()[..label.len().min(LABEL_SIZE as usize - 1)];
    e.mem.set_cstr(text, bytes);
}

/// The row's change function (module documentation).
fn change(e: &mut Engine, option: Ptr) {
    if option.is_null() {
        return;
    }
    let menu = e.global::<u32>(START_MENU);
    let saving = menu != 0
        && e.call(MENU_FLAGS_TEST, &args![menu, FLAG_SAVE_OPTIONS])
            .u32()
            & 0xff
            != 0;
    let text = e.mem.u32(option.addr() + TEXT);
    if !saving {
        let step = step_of(world_fov(e));
        e.mem.set_u32(option.addr() + VAL, step);
        e.mem
            .set_u32(option.addr() + DEFAULT_VAL, step_of(ORIGINAL as f32));
        if text != 0 {
            write_label(e, text, degrees_of(step));
        }
        return;
    }
    let degrees = degrees_of(e.mem.u32(option.addr() + VAL));
    for setting in [WORLD_FOV, FIRST_PERSON_FOV] {
        e.mem.set_f32(setting + VALUE, degrees as f32);
    }
    if text != 0 {
        write_label(e, text, degrees);
        let tile = e.with_stack(4, |e, cell| {
            e.mem.set_u32(cell.addr(), option.addr());
            e.call(GET_TILE_FOR, &args![menu + OPTIONS_LIST, cell])
                .u32()
        });
        if tile != 0 {
            e.call(TILE_SET_STRING, &args![tile, TRAIT_STRING, text, 1u32]);
        }
    }
    save(degrees);
}

/// `fDefaultWorldFOV=<degrees>.0000` and `fDefault1stPersonFOV=<degrees>.0000` ([`KEYS`]) into the
/// run's Fallout.ini, in a launcher's play run only
/// (`crate::fixes::is_play_run`): written beside it and renamed over it, so a reader never sees half
/// of it. A file that can't be read or written is named once in the log; the game goes on with
/// the value it has.
fn save(degrees: i32) {
    if !crate::fixes::is_play_run(|name| std::env::var(name).ok()) {
        return;
    }
    let Some(path) = crate::game_ini::path() else {
        return;
    };
    if let Err(why) = write_fov(&path, degrees) {
        static NAMED: std::sync::Once = std::sync::Once::new();
        NAMED.call_once(|| {
            eprintln!(
                "Field of View: {} not saved ({why}); it applies until the game closes.",
                path.display()
            )
        });
    }
}

/// [`save`]'s write: the file's bytes held one character per byte, so every byte outside the line
/// comes back as it was.
pub fn write_fov(path: &std::path::Path, degrees: i32) -> std::io::Result<()> {
    let bytes = std::fs::read(path)?;
    let mut text: String = bytes.iter().map(|&b| char::from(b)).collect();
    let value = format!("{degrees}.0000");
    for key in KEYS {
        text = crate::game_ini::set_key(&text, "Display", key, &value);
    }
    let out: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
    if out == bytes {
        return Ok(());
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".nv-rs-fov");
    let temporary = std::path::PathBuf::from(temporary);
    std::fs::write(&temporary, &out)?;
    std::fs::rename(&temporary, path).map_err(|why| {
        let _ = std::fs::remove_file(&temporary);
        why
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn after_kill_cam() -> ArmsState {
        ArmsState {
            enabled: true,
            free_camera: true,
            settled: true,
            vats_mode: 0,
            ..ArmsState::default()
        }
    }

    #[test]
    fn the_arms_left_behind_by_a_kill_cam_get_the_slider_again() {
        // The world-only branch with the arms on the screen: restored.
        assert!(arms_left_behind(after_kill_cam()));
        // The kill cam itself and VATS: the game's own.
        for mode in [2, 3, 4] {
            assert!(!arms_left_behind(ArmsState {
                vats_mode: mode,
                ..after_kill_cam()
            }));
        }
        // No free camera or not settled: the steering already runs.
        assert!(!arms_left_behind(ArmsState {
            free_camera: false,
            ..after_kill_cam()
        }));
        assert!(!arms_left_behind(ArmsState {
            settled: false,
            ..after_kill_cam()
        }));
        // Third person, the Pip-Boy (fPipboy1stPersonFOV stays its own), iron sights, or the
        // update off: left as they are.
        for state in [
            ArmsState {
                third_person: true,
                ..after_kill_cam()
            },
            ArmsState {
                pipboy: true,
                ..after_kill_cam()
            },
            ArmsState {
                iron_sights: true,
                ..after_kill_cam()
            },
            ArmsState {
                enabled: false,
                ..after_kill_cam()
            },
        ] {
            assert!(!arms_left_behind(state), "{state:?}");
        }
    }

    /// The bytes as a kill cam leaves them: the arms go to fDefault1stPersonFOV, the Pip-Boy's
    /// setting is not read.
    #[test]
    fn the_arms_take_fdefault1stpersonfov() {
        let mut e = Engine::new();
        e.mem.map(0x011a_3000, 0x1000);
        e.mem.map(0x011e_0000, 0x1000);
        e.mem.map(0x011f_2000, 0x1000);
        e.mem.map(0x0120_3000, 0x1000);
        e.mem.map(0x1000, 0x1000);
        let player = 0x1000;
        e.mem.set_u8(FREE_CAMERA, 1);
        e.mem.set_u8(FOV_SETTLED, 1);
        e.mem.set_i32(VATS_MODE, 0);
        e.mem.set_f32(FIRST_PERSON_DEFAULT, 95.0);
        e.mem.set_f32(0x0120_3174 + 4, 47.0);
        e.mem.set_f32(player + ARMS_FOV, 55.0);
        let cell = 0x1800;
        e.mem.set_u8(cell, 1);
        e.register_double(SETTING_BOOL_POINTER, move |_, _| cell.into_ret());
        e.register(PLAYER_IS_PIPBOY_ACTIVE, |_, _| 0u32.into_ret());
        e.register(ACTOR_GET_IRON_SIGHTS, |_, _| 0u32.into_ret());
        let state = arms_state(&mut e, player);
        assert!(arms_left_behind(state), "{state:?}");
        restore_arms(&mut e, player);
        assert_eq!(e.mem.f32(player + ARMS_FOV), 95.0);
    }

    #[test]
    fn steps_cover_70_to_120_and_the_original_is_75() {
        assert_eq!(step_of(70.0), 0);
        assert_eq!(step_of(75.0), 5);
        assert_eq!(step_of(120.0), 50);
        assert_eq!(step_of(90.4), 20);
        assert_eq!(step_of(55.0), 0, "below the slider: its lowest");
        assert_eq!(step_of(150.0), 50, "above it: its highest");
        assert_eq!(step_of(f32::NAN), 5);
        assert_eq!(degrees_of(0), 70);
        assert_eq!(degrees_of(20), 90);
        assert_eq!(degrees_of(50), 120);
        assert_eq!(degrees_of(90), 120);
        assert_eq!(label(90), "Field of View: 90");
    }

    /// An engine with the game's words this module touches, and the calls it makes answered: the
    /// menu's flags (`saving`), the tile of the row (`0x5000`), and every `SetString` recorded.
    fn engine(saving: bool) -> (Engine, Rc<RefCell<Vec<Vec<u32>>>>) {
        let mut e = Engine::new();
        for page in [WORLD_FOV, FIRST_PERSON_FOV, START_MENU, 0x0001_0000] {
            e.mem.map(page & !0xfff, 0x1000);
        }
        e.mem.set_f32(WORLD_FOV + VALUE, 75.0);
        e.mem.set_f32(FIRST_PERSON_FOV + VALUE, 75.0);
        e.set_global(START_MENU, 0x0001_0100u32);
        e.register_double(MENU_FLAGS_TEST, move |_, a| Ret {
            eax: u32::from(saving && a[1] == FLAG_SAVE_OPTIONS),
            ..Ret::default()
        });
        e.register(GET_TILE_FOR, |e, a| Ret {
            eax: if e.mem.u32(a[1]) == 0x0001_0400 {
                0x5000
            } else {
                0
            },
            ..Ret::default()
        });
        let strings = Rc::new(RefCell::new(Vec::new()));
        let seen = strings.clone();
        e.register_double(TILE_SET_STRING, move |_, a| {
            seen.borrow_mut().push(a[..4].to_vec());
            Ret::default()
        });
        (e, strings)
    }

    /// The row: at 0x10400, its label at 0x10800.
    fn row(e: &mut Engine) -> Ptr {
        let option = Ptr::new(0x0001_0400);
        e.mem.set_u32(option.addr() + TEXT, 0x0001_0800);
        option
    }

    #[test]
    fn reading_shows_the_settings_value_and_the_default_is_75() {
        let (mut e, strings) = engine(false);
        e.mem.set_f32(WORLD_FOV + VALUE, 95.0);
        e.mem.set_f32(FIRST_PERSON_FOV + VALUE, 60.0);
        let option = row(&mut e);
        change(&mut e, option);
        assert_eq!(e.mem.u32(option.addr() + VAL), 25);
        assert_eq!(e.mem.u32(option.addr() + DEFAULT_VAL), 5);
        assert_eq!(e.mem.cstr(0x0001_0800), b"Field of View: 95");
        assert_eq!(
            e.mem.f32(WORLD_FOV + VALUE),
            95.0,
            "reading changes nothing"
        );
        assert_eq!(e.mem.f32(FIRST_PERSON_FOV + VALUE), 60.0);
        assert!(strings.borrow().is_empty());
    }

    #[test]
    fn moving_it_sets_both_fovs_and_its_label() {
        let (mut e, strings) = engine(true);
        let option = row(&mut e);
        e.mem.set_u32(option.addr() + VAL, 35);
        change(&mut e, option);
        assert_eq!(e.mem.f32(WORLD_FOV + VALUE), 105.0);
        assert_eq!(
            e.mem.f32(FIRST_PERSON_FOV + VALUE),
            105.0,
            "the arms and the weapon too"
        );
        assert_eq!(e.mem.cstr(0x0001_0800), b"Field of View: 105");
        assert_eq!(
            *strings.borrow(),
            [vec![0x5000, TRAIT_STRING, 0x0001_0800, 1]]
        );
    }

    #[test]
    fn the_value_goes_into_fallout_ini_as_settings_cmd_writes_it() {
        let dir = std::env::temp_dir().join(format!("nv-rs-fov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Fallout.ini");
        std::fs::write(
            &path,
            b"[General]\r\nsA=\xe9\r\n[Display]\r\nfDefaultWorldFOV=75.0000\r\nfDefault1stPersonFOV=55.0000\r\n",
        )
        .unwrap();
        write_fov(&path, 100).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"[General]\r\nsA=\xe9\r\n[Display]\r\nfDefaultWorldFOV=100.0000\r\nfDefault1stPersonFOV=100.0000\r\n",
            "both keys, where they stand"
        );
        // A file without the keys gets them under [Display], the world's first; fDefaultFOV and
        // the Pip-Boy's stay.
        std::fs::write(
            &path,
            b"[Display]\r\nfDefaultFOV=75.0000\r\nfPipboy1stPersonFOV=47.0000\r\n",
        )
        .unwrap();
        write_fov(&path, 90).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"[Display]\r\nfDefault1stPersonFOV=90.0000\r\nfDefaultWorldFOV=90.0000\r\nfDefaultFOV=75.0000\r\nfPipboy1stPersonFOV=47.0000\r\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
