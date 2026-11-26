//! Delirium Physics (`vendor/delirium-physics`, PVCPC's Rust port of the Euphoria active ragdoll
//! as GTA IV has it) behind New Physics.
//!
//! Delirium reads its numbers from the player's own copy of GTA IV: its program (`GTAIV.exe`,
//! the Steam PC one) unlocks them (`delirium_data::kv::unlock`), and its character file
//! (`pc/data/naturalmotion/Fred.art`) gives the body's muscles and reactions. Nothing of GTA IV
//! is in this repository, its CI or its packages: the player names their own copy in
//! `[nv-rs Gunplay] sDeliriumGame` (the exe or its folder) on their own PC. Without it, or when
//! anything about it is wrong, [`start`] says why and the current New Physics plays as before.
//!
//! The log lines never name the path: a report a player shares carries no folder of theirs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The setting's value that leaves Delirium off (and its default).
pub const OFF: &str = "none";

/// What [`start`] found: Delirium's numbers open and the character loaded, with the game's
/// hit tuning when it has one (`common/data/taskparams.txt`).
#[derive(Debug)]
pub struct Started {
    pub task_params: Option<delirium_data::taskparams::TaskParams>,
}

/// Why Delirium is not running: shown in the log as is, then the current New Physics plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// `sDeliriumGame=none` (the default).
    Off,
    /// The setting names nothing that is there.
    Missing(&'static str),
    /// The program is not one of the GTA IV versions Delirium reads its numbers from.
    NotAVersion,
    /// The program's folder has no character file (or it is not one Delirium reads).
    NoCharacter(&'static str),
}

impl Fallback {
    /// The log's line for it.
    pub fn line(&self) -> String {
        let tail = "New Physics plays as before, without Delirium";
        match self {
            Fallback::Off => format!("Delirium: off (sDeliriumGame={OFF}); {tail}"),
            Fallback::Missing(what) => format!(
                "Delirium: the GTA IV path in sDeliriumGame has no {what} (name GTAIV.exe or its folder); {tail}"
            ),
            Fallback::NotAVersion => format!(
                "Delirium: the GTAIV.exe sDeliriumGame names isn't a version Delirium reads its numbers from (the Steam PC one is); {tail}"
            ),
            Fallback::NoCharacter(why) => {
                format!("Delirium: GTA IV's character file (pc/data/naturalmotion/Fred.art) {why}; {tail}")
            }
        }
    }
}

/// The program and the game folder `setting` names: the exe itself, or a folder holding
/// `GTAIV.exe` (any letter case).
pub fn locate(setting: &str) -> Result<(PathBuf, PathBuf), Fallback> {
    let setting = setting.trim().trim_matches('"');
    if setting.is_empty() || setting.eq_ignore_ascii_case(OFF) {
        return Err(Fallback::Off);
    }
    let path = Path::new(setting);
    if path.is_file() {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        return Ok((path.to_path_buf(), dir));
    }
    if !path.is_dir() {
        return Err(Fallback::Missing("file or folder"));
    }
    let exe = child(path, "GTAIV.exe").ok_or(Fallback::Missing("GTAIV.exe"))?;
    Ok((exe, path.to_path_buf()))
}

/// `dir`'s entry named `name`, in any letter case.
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// `dir` joined with each of `parts`, each matched in any letter case.
fn under(dir: &Path, parts: &[&str]) -> Option<PathBuf> {
    parts
        .iter()
        .try_fold(dir.to_path_buf(), |at, p| child(&at, p))
}

/// Delirium started from `setting`: the program's numbers unlocked, the character loaded (as
/// GTA IV's own body, `nmrs::HOMER`), the hit tuning read when the game has it.
pub fn start(setting: &str) -> Result<Started, Fallback> {
    let (exe, dir) = locate(setting)?;
    let program = std::fs::read(&exe).map_err(|_| Fallback::Missing("readable GTAIV.exe"))?;
    if !delirium_data::kv::unlock(&program) {
        return Err(Fallback::NotAVersion);
    }
    drop(program);
    let character = under(&dir, &["pc", "data", "naturalmotion", "Fred.art"])
        .ok_or(Fallback::NoCharacter("isn't there"))?;
    let data = std::fs::read(character).map_err(|_| Fallback::NoCharacter("can't be read"))?;
    // Delirium's reader asserts on what it cannot read: a damaged file falls back, never stops
    // the game.
    std::panic::catch_unwind(|| delirium::nmrs::load(delirium::nmrs::HOMER, &data))
        .map_err(|_| Fallback::NoCharacter("isn't one Delirium reads"))?;
    let task_params = under(&dir, &["common", "data", "taskparams.txt"])
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| delirium_data::taskparams::read(&t));
    Ok(Started { task_params })
}

static STARTED: OnceLock<Result<Started, Fallback>> = OnceLock::new();

/// [`start`] once for the run (later calls answer the first's outcome): the log's line for it.
pub fn start_once(setting: &str) -> String {
    match STARTED.get_or_init(|| start(setting)) {
        Ok(s) => format!(
            "Delirium: on, from the player's own GTA IV (its numbers unlocked, its character loaded{})",
            if s.task_params.is_some() {
                ", its hit tuning read"
            } else {
                "; no taskparams.txt: Delirium's own tuning"
            }
        ),
        Err(f) => f.line(),
    }
}

/// Whether Delirium runs this time ([`start_once`] succeeded).
pub fn running() -> bool {
    STARTED.get().is_some_and(Result::is_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty folder for one test.
    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv-rs-delirium-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The default leaves Delirium off, and the line says New Physics plays as before.
    #[test]
    fn none_is_off() {
        for s in ["none", "NONE", "", "  ", "\"none\""] {
            assert_eq!(start(s).unwrap_err(), Fallback::Off, "{s:?}");
        }
        assert!(Fallback::Off.line().contains("New Physics plays as before"));
    }

    /// A path that isn't there, or a folder without GTAIV.exe, falls back with its reason; the
    /// lines never name the path.
    #[test]
    fn a_missing_game_falls_back() {
        let d = folder("missing");
        let gone = d.join("nothing here");
        let e = start(gone.to_str().unwrap()).unwrap_err();
        assert_eq!(e, Fallback::Missing("file or folder"));
        let e = start(d.to_str().unwrap()).unwrap_err();
        assert_eq!(e, Fallback::Missing("GTAIV.exe"));
        for f in [Fallback::Missing("GTAIV.exe"), Fallback::NotAVersion] {
            let l = f.line();
            assert!(!l.contains(d.to_str().unwrap()) && l.contains("New Physics plays as before"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A program that isn't GTA IV (any bytes, named GTAIV.exe in any case; the exe named itself
    /// or its folder) isn't a version: nothing unlocks, nothing stops.
    #[test]
    fn a_program_that_isnt_gta_iv_falls_back() {
        let d = folder("other");
        std::fs::write(d.join("gtaiv.EXE"), b"MZ not the game at all").unwrap();
        assert_eq!(
            start(d.to_str().unwrap()).unwrap_err(),
            Fallback::NotAVersion
        );
        let exe = d.join("gtaiv.EXE");
        assert_eq!(
            start(exe.to_str().unwrap()).unwrap_err(),
            Fallback::NotAVersion
        );
        assert!(!delirium_data::kv::open());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The folder found from the exe or from the folder, in any letter case.
    #[test]
    fn locate_takes_the_exe_or_its_folder() {
        let d = folder("locate");
        std::fs::write(d.join("GTAIV.exe"), b"MZ").unwrap();
        let (exe, dir) = locate(d.to_str().unwrap()).unwrap();
        assert_eq!(
            (exe.file_name().unwrap(), dir.as_path()),
            ("GTAIV.exe".as_ref(), d.as_path())
        );
        let (exe2, dir2) = locate(exe.to_str().unwrap()).unwrap();
        assert_eq!((exe2, dir2), (exe, d.clone()));
        std::fs::create_dir_all(d.join("PC/Data/NaturalMotion")).unwrap();
        std::fs::write(d.join("PC/Data/NaturalMotion/fred.ART"), b"x").unwrap();
        assert!(under(&d, &["pc", "data", "naturalmotion", "Fred.art"]).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
