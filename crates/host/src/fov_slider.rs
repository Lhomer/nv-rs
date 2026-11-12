//! A field of view slider in the game's own Settings menu (`NV_FOV_SLIDER`), a row of the `fixes`
//! preset ([`crate::fixes`], docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the
//! player asked for (Patrick, 2026-11-12); in the `vanilla` preset, or with `bFovSlider=0`, the
//! menu is the original's and nothing here runs.
//!
//! **What it sets.** The world's field of view is Fallout.ini's `[Display] fDefaultWorldFOV`, the
//! float setting at `0120315c` (its value at `+4`; in the INI collection `011f96a0`, so the game
//! reads it from Fallout.ini; the original's default 75). The player's character steers its view to
//! it every frame while not zoomed (`PlayerCharacter`'s `0095de30`, when the Fallout.ini bool at
//! `011e097c`, default 1, is on), so a new value shows as soon as play resumes. The weapon and arms
//! (`fDefault1stPersonFOV`) are left alone. Settings.cmd sets the same key, from 70 to 120
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
//! Reading, the row's value is the setting's (rounded, kept inside the slider) and its default 75.
//! Moved, the setting gets the row's degrees, the row's label shows them, and in a launcher's play
//! run the run's Fallout.ini gets `fDefaultWorldFOV=<degrees>.0000` (the game's own `%.4f`, as
//! Settings.cmd writes it; `crate::game_ini::set_key`, Play.exe's rule), so the next start has it.
//! The game itself never writes Fallout.ini, only FalloutPrefs.ini.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::startmenu::start_menu_create_menu_elements;

/// `StartMenu::CreateMenuElements` (Ghidra name).
pub const CREATE_MENU_ELEMENTS: u32 = 0x007c_c6e0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "field of view in the game's settings",
    env: "NV_FOV_SLIDER",
    addresses: &[CREATE_MENU_ELEMENTS],
    summary: "the game's Settings menu has a Field of View slider on its Display page (fDefaultWorldFOV, 70 to 120)",
    install,
};

/// The row's change function: a code address no exe code and no platform code uses, in the
/// platform's area (`engine_core::mem::PLATFORM_AREA`), between nvapi's code (`0x7e04_0000`) and
/// the platform's objects (`0x7e12_0000` on).
pub const CHANGE_FN: u32 = 0x7e06_0000;

/// `fDefaultWorldFOV:Display` and where its value is (`+4`, as `0043d4d0` gives a setting's).
const WORLD_FOV: u32 = 0x0120_315c;
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
    e.mem.set_f32(WORLD_FOV + VALUE, degrees as f32);
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

/// `fDefaultWorldFOV=<degrees>.0000` into the run's Fallout.ini, in a launcher's play run only
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
    if let Err(why) = write_world_fov(&path, degrees) {
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
pub fn write_world_fov(path: &std::path::Path, degrees: i32) -> std::io::Result<()> {
    let bytes = std::fs::read(path)?;
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    let value = format!("{degrees}.0000");
    let text = crate::game_ini::set_key(&text, "Display", "fDefaultWorldFOV", &value);
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
        for page in [WORLD_FOV, START_MENU, 0x0001_0000] {
            e.mem.map(page & !0xfff, 0x1000);
        }
        e.mem.set_f32(WORLD_FOV + VALUE, 75.0);
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
        assert!(strings.borrow().is_empty());
    }

    #[test]
    fn moving_it_sets_the_world_fov_and_its_label() {
        let (mut e, strings) = engine(true);
        let option = row(&mut e);
        e.mem.set_u32(option.addr() + VAL, 35);
        change(&mut e, option);
        assert_eq!(e.mem.f32(WORLD_FOV + VALUE), 105.0);
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
        write_world_fov(&path, 100).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"[General]\r\nsA=\xe9\r\n[Display]\r\nfDefaultWorldFOV=100.0000\r\nfDefault1stPersonFOV=55.0000\r\n"
        );
        // A file without the key gets it under [Display].
        std::fs::write(&path, b"[Display]\r\nfDefaultFOV=75.0000\r\n").unwrap();
        write_world_fov(&path, 90).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"[Display]\r\nfDefaultWorldFOV=90.0000\r\nfDefaultFOV=75.0000\r\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
