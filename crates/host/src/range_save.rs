//! The Test Range's prepared save (Patrick, 2026-11-14: giving every item at the start froze the
//! game for about a minute and then brought slow level-ups). The first range game of a load order
//! is set up as before ([`crate::test_range`]: god mode, the whole inventory, the targets, no
//! tutorials or pop-ups, no XP), then saved once into a cache of its own; every later start with
//! the same load order loads that save instead, with nothing given again.
//!
//! **Where it is kept.** On the real disk: `<userdata>\testrange\<fingerprint>\` beside the
//! launcher's `settings.ini` (`NV_SETTINGS`' folder, the launcher's `userdata`, which git and the
//! play package leave out), never in a save profile's folder: `TestRange.fos`, its script
//! extender co-save `TestRange.nvse` when there is one, and `range.txt` (what the range keeps of
//! itself: the place, the container, the targets, the ammo). Without `NV_SETTINGS` (not a play
//! run) nothing is cached. In the game: the save is written and read under the virtual
//! `<Documents>\My Games\FalloutNV\nv-rs Test Range\` (the game's `sLocalSavePath` setting,
//! `011c3f74`, points there for the save and the load only), which stays in the run's memory:
//! the copy-out of saves (`crate::save_out`) leaves that folder out, the game's save list never
//! looks in it, and the player's save folders are neither read nor written.
//!
//! **When it is made again** ([`fingerprint`]): when the loaded plugins (names, order, sizes and
//! times), `sTestRangeCell` or the range's set-up ([`GRANT_VERSION`]) change; when the range
//! menu's "Rebuild Saved Range" was chosen; with `NV_TEST_RANGE_REBUILD=1`; or when the folder is
//! deleted. Up to [`KEEP`] load orders are kept (Play and Play with mods each keep their own).

use std::path::{Path, PathBuf};

/// The range's set-up as this build makes it: raised when [`crate::test_range`]'s set-up changes,
/// so an older save is made again.
pub const GRANT_VERSION: u32 = 1;
/// The save's name, its virtual folder (the game's `sLocalSavePath` while it is saved or loaded),
/// and the cache's folder under `userdata`.
pub const SAVE_NAME: &str = "TestRange";
pub const VIRTUAL_FOLDER: &str = "nv-rs Test Range\\";
pub const CACHE_FOLDER: &str = "testrange";
/// The load orders kept.
pub const KEEP: usize = 4;

/// The key the run's virtual files are kept under for the range's folder (lower case), which the
/// copy-out of saves leaves out.
pub const VIRTUAL_KEY: &[u8] = b"\\nv-rs test range\\";

/// FNV-1a, 64 bits.
fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The cache's key: the loaded plugins (each `(name, size, modified seconds)` in load order), the
/// configured place and [`GRANT_VERSION`], as 16 hex digits.
pub fn fingerprint(plugins: &[(String, u64, u64)], cell: &str) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = fnv(&GRANT_VERSION.to_le_bytes(), h);
    h = fnv(cell.to_ascii_lowercase().as_bytes(), h);
    for (name, size, modified) in plugins {
        h = fnv(name.to_ascii_lowercase().as_bytes(), h);
        h = fnv(&size.to_le_bytes(), h);
        h = fnv(&modified.to_le_bytes(), h);
    }
    format!("{h:016x}")
}

/// Each plugin's name, size and modified time (seconds), for [`fingerprint`].
pub fn plugin_facts(paths: &[PathBuf]) -> Vec<(String, u64, u64)> {
    paths
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (size, modified) = std::fs::metadata(p).map_or((0, 0), |m| {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                (m.len(), t)
            });
            (name, size, modified)
        })
        .collect()
}

/// The cache's root: `testrange` beside the launcher's `settings.ini`; `None` outside a play run.
pub fn cache_root() -> Option<PathBuf> {
    let settings = std::env::var_os("NV_SETTINGS")?;
    let parent = Path::new(&settings).parent()?.to_path_buf();
    Some(parent.join(CACHE_FOLDER))
}

/// What the range keeps of itself beside the save.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Kept {
    pub place: String,
    pub container: u32,
    pub targets: Vec<u32>,
    pub ammo: Vec<u32>,
}

impl Kept {
    pub fn to_text(&self) -> String {
        let list = |v: &[u32]| {
            v.iter()
                .map(|x| format!("{x:08X}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!(
            "version={GRANT_VERSION}\nplace={}\ncontainer={:08X}\ntargets={}\nammo={}\n",
            self.place,
            self.container,
            list(&self.targets),
            list(&self.ammo)
        )
    }

    /// The text [`Kept::to_text`] wrote; `None` when it is not one (another version included).
    pub fn from_text(text: &str) -> Option<Kept> {
        let mut kept = Kept::default();
        let mut version = None;
        let ids = |v: &str| -> Option<Vec<u32>> {
            v.split_whitespace()
                .map(|x| u32::from_str_radix(x, 16).ok())
                .collect()
        };
        for line in text.lines() {
            let (key, value) = line.split_once('=')?;
            match key {
                "version" => version = value.parse::<u32>().ok(),
                "place" => kept.place = value.to_string(),
                "container" => kept.container = u32::from_str_radix(value, 16).ok()?,
                "targets" => kept.targets = ids(value)?,
                "ammo" => kept.ammo = ids(value)?,
                _ => {}
            }
        }
        (version == Some(GRANT_VERSION)).then_some(kept)
    }
}

/// A prepared save: its bytes, its co-save's when there is one, and what the range kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub save: Vec<u8>,
    pub cosave: Option<Vec<u8>>,
    pub kept: Kept,
}

/// The prepared save in `dir`, when it is whole.
pub fn read(dir: &Path) -> Option<Prepared> {
    let save = std::fs::read(dir.join(format!("{SAVE_NAME}.fos"))).ok()?;
    if save.is_empty() {
        return None;
    }
    let kept = Kept::from_text(&std::fs::read_to_string(dir.join("range.txt")).ok()?)?;
    let cosave = std::fs::read(dir.join(format!("{SAVE_NAME}.nvse"))).ok();
    Some(Prepared { save, cosave, kept })
}

/// The prepared save written into `dir` (the record last, so a cut-short write is never whole),
/// and the oldest load orders beyond [`KEEP`] removed from `root`.
pub fn write(root: &Path, dir: &Path, prepared: &Prepared) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(dir.join("range.txt"));
    std::fs::write(dir.join(format!("{SAVE_NAME}.fos")), &prepared.save)?;
    match &prepared.cosave {
        Some(bytes) => std::fs::write(dir.join(format!("{SAVE_NAME}.nvse")), bytes)?,
        None => {
            let _ = std::fs::remove_file(dir.join(format!("{SAVE_NAME}.nvse")));
        }
    }
    std::fs::write(dir.join("range.txt"), prepared.kept.to_text())?;
    prune(root, dir);
    Ok(())
}

/// Removes the oldest load orders' folders beyond [`KEEP`], never `keep`.
fn prune(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|d| d.path())
        .filter(|p| p.is_dir() && p.as_path() != keep)
        .map(|p| {
            let t = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (t, p)
        })
        .collect();
    dirs.sort();
    while dirs.len() + 1 > KEEP {
        let (_, p) = dirs.remove(0);
        let _ = std::fs::remove_dir_all(&p);
    }
}

/// Whether the player asked for the save to be made again (`NV_TEST_RANGE_REBUILD=1`).
pub fn rebuild_asked() -> bool {
    std::env::var("NV_TEST_RANGE_REBUILD").is_ok_and(|v| v.trim() == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugins() -> Vec<(String, u64, u64)> {
        vec![("FalloutNV.esm".into(), 100, 5), ("WMX.esp".into(), 20, 7)]
    }

    /// The key follows the plugins, their order and files, the place and the set-up's version.
    #[test]
    fn the_fingerprint_follows_the_load_order() {
        let a = fingerprint(&plugins(), "auto");
        assert_eq!(a.len(), 16);
        assert_eq!(a, fingerprint(&plugins(), "AUTO"));
        let mut other = plugins();
        other.swap(0, 1);
        assert_ne!(a, fingerprint(&other, "auto"), "order");
        let mut other = plugins();
        other[1].1 = 21;
        assert_ne!(a, fingerprint(&other, "auto"), "a plugin changed");
        assert_ne!(a, fingerprint(&plugins()[..1], "auto"), "a plugin left out");
        assert_ne!(a, fingerprint(&plugins(), "SomeCell"), "the place");
    }

    /// What the range keeps reads back as written; another version's is not used.
    #[test]
    fn the_record_round_trips() {
        let kept = Kept {
            place: "Lucky38Int".into(),
            container: 0xff00_0801,
            targets: vec![0xff00_0802, 0xff00_0803],
            ammo: vec![0x0004_241e, 0x0010_0000],
        };
        assert_eq!(Kept::from_text(&kept.to_text()), Some(kept.clone()));
        let old = kept.to_text().replace("version=1", "version=0");
        assert_eq!(Kept::from_text(&old), None);
        assert_eq!(Kept::from_text("garbage"), None);
        let empty = Kept::default();
        assert_eq!(Kept::from_text(&empty.to_text()), Some(empty));
    }

    /// A prepared save is written whole and read back; the oldest load orders beyond the kept
    /// number go.
    #[test]
    fn prepared_saves_are_kept_per_load_order() {
        let root = std::env::temp_dir().join(format!("nv-rs-range-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = Prepared {
            save: vec![1, 2, 3],
            cosave: Some(vec![4]),
            kept: Kept {
                place: "x".into(),
                ..Kept::default()
            },
        };
        for i in 0..KEEP + 2 {
            let dir = root.join(format!("{i:016x}"));
            write(&root, &dir, &p).unwrap();
            assert_eq!(read(&dir), Some(p.clone()));
        }
        let left = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(left, KEEP);
        let dir = root.join("none");
        assert_eq!(read(&dir), None);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("TestRange.fos"), [1]).unwrap();
        assert_eq!(read(&dir), None, "no record: not whole");
        let _ = std::fs::remove_dir_all(&root);
    }
}
