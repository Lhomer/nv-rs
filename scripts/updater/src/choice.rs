//! What the launcher starts once the update check is done (Patrick, 2026-11-14: always ask):
//! **Play** (Play.exe as `Play.cmd` starts it) or **Play with mods** (`Play.exe --mods`, as
//! `Play with mods.cmd` does, with the `mods` folder). The last choice is remembered in
//! `userdata\launcher-choice.txt` and offered as the default button the next time; the question is
//! asked every time.

use std::path::{Path, PathBuf};

/// What to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Play.exe as it is.
    Play,
    /// Play.exe with the mods in the `mods` folder.
    Mods,
}

impl Choice {
    /// Play.exe's arguments for it.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Choice::Play => &[],
            Choice::Mods => &["--mods"],
        }
    }

    /// The word kept in the file.
    pub fn word(self) -> &'static str {
        match self {
            Choice::Play => "play",
            Choice::Mods => "mods",
        }
    }

    /// The choice a file's text holds; [`Choice::Play`] for anything else (a missing or damaged
    /// file).
    pub fn parse(text: &str) -> Choice {
        match text.trim().to_ascii_lowercase().as_str() {
            "mods" => Choice::Mods,
            _ => Choice::Play,
        }
    }
}

/// Where the last choice is kept: `userdata\launcher-choice.txt` beside Play.exe (the update never
/// touches `userdata\`).
pub fn file(install: &Path) -> PathBuf {
    install.join("userdata").join("launcher-choice.txt")
}

/// The last choice, [`Choice::Play`] when there is none.
pub fn remembered(install: &Path) -> Choice {
    std::fs::read_to_string(file(install)).map_or(Choice::Play, |text| Choice::parse(&text))
}

/// Keeps `choice` for the next start; a file that can't be written is not an error (the choice
/// is still made, only not remembered).
pub fn remember(install: &Path, choice: Choice) {
    let path = file(install);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{}\n", choice.word()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play is Play.exe as it is, Play with mods is `--mods`, as the two .cmd files start it.
    #[test]
    fn the_choices_start_play_exe_as_the_cmd_files_do() {
        assert!(Choice::Play.args().is_empty());
        assert_eq!(Choice::Mods.args(), ["--mods"]);
        let cmd = include_str!("../../../distribution/Play with mods.cmd");
        assert!(cmd.contains("Play.exe\" --mods"), "{cmd}");
    }

    /// The last choice comes back; none, or a damaged file, is Play.
    #[test]
    fn the_last_choice_is_remembered() {
        let dir = std::env::temp_dir().join(format!("nv-rs-choice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(remembered(&dir), Choice::Play);
        remember(&dir, Choice::Mods);
        assert_eq!(remembered(&dir), Choice::Mods);
        assert!(file(&dir).starts_with(dir.join("userdata")));
        remember(&dir, Choice::Play);
        assert_eq!(remembered(&dir), Choice::Play);
        std::fs::write(file(&dir), "garbage").unwrap();
        assert_eq!(remembered(&dir), Choice::Play);
        assert_eq!(Choice::parse(" MODS\r\n"), Choice::Mods);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
