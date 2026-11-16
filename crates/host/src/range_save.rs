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
/// configured place and [`GRANT_VERSION`], as 16 hex digits (the Test Range's: [`fingerprint_for`]
/// with no room).
#[cfg(test)]
pub fn fingerprint(plugins: &[(String, u64, u64)], cell: &str) -> String {
    fingerprint_for(plugins, cell, "")
}

/// [`fingerprint`] with `room` too: the physics room's build and contents ([`room_key`]); empty
/// for the Test Range, whose keys stay as they were.
pub fn fingerprint_for(plugins: &[(String, u64, u64)], cell: &str, room: &str) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    h = fnv(&GRANT_VERSION.to_le_bytes(), h);
    if !room.is_empty() {
        h = fnv(b"room ", h);
        h = fnv(room.as_bytes(), h);
    }
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

/// The physics room's set-up as this build makes it: raised when `crate::phone_range`'s room
/// changes in a way the build's commit would not say (a local build).
pub const ROOM_VERSION: u32 = 1;

/// The physics room's part of the key: [`ROOM_VERSION`], the build's commit (`NV_RS_COMMIT` at
/// build time, CI's; else `local`) and the loadout, so a new build or a new loadout makes the
/// snapshot again.
pub fn room_key(commit: &str, loadout: &[&str]) -> String {
    format!("v{ROOM_VERSION} {commit} {}", loadout.join(","))
}

/// The cache's root: `testrange` beside the launcher's `settings.ini` (a play run: the Play
/// package's or the physics tester's own `userdata`, never the player's saves), else the folder
/// `NV_TEST_RANGE_CACHE` names (the Linux room's, in `~/.local/share/nv-rs`); `None` otherwise.
pub fn cache_root() -> Option<PathBuf> {
    if let Some(settings) = std::env::var_os("NV_SETTINGS") {
        let parent = Path::new(&settings).parent()?.to_path_buf();
        return Some(parent.join(CACHE_FOLDER));
    }
    std::env::var_os("NV_TEST_RANGE_CACHE")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// What the range keeps of itself beside the save.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Kept {
    pub place: String,
    pub container: u32,
    pub targets: Vec<u32>,
    pub ammo: Vec<u32>,
    /// The physics room's own (`crate::phone_range`), when it made the save.
    pub room: Option<RoomKept>,
}

/// What the physics room keeps beside its snapshot: the target's base (form id), where it first
/// stood and its heading (degrees: the reset puts a fresh one there), the loadout's weapons, their
/// ammo and the thrown ones (form ids).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RoomKept {
    pub base: u32,
    pub at: [f32; 3],
    pub angle: f32,
    pub guns: Vec<u32>,
    pub ammo: Vec<u32>,
    pub thrown: Vec<u32>,
}

impl Kept {
    pub fn to_text(&self) -> String {
        let list = |v: &[u32]| {
            v.iter()
                .map(|x| format!("{x:08X}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut text = format!(
            "version={GRANT_VERSION}\nplace={}\ncontainer={:08X}\ntargets={}\nammo={}\n",
            self.place,
            self.container,
            list(&self.targets),
            list(&self.ammo)
        );
        if let Some(r) = &self.room {
            text.push_str(&format!(
                "room_spot={:08X} {} {} {} {}\nroom_guns={}\nroom_ammo={}\nroom_thrown={}\n",
                r.base,
                r.at[0],
                r.at[1],
                r.at[2],
                r.angle,
                list(&r.guns),
                list(&r.ammo),
                list(&r.thrown)
            ));
        }
        text
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
                "room_spot" => {
                    let w: Vec<&str> = value.split_whitespace().collect();
                    let [base, x, y, z, angle] = w.as_slice() else {
                        return None;
                    };
                    let f = |v: &str| v.parse::<f32>().ok().filter(|x| x.is_finite());
                    let room = kept.room.get_or_insert_with(RoomKept::default);
                    room.base = u32::from_str_radix(base, 16).ok()?;
                    room.at = [f(x)?, f(y)?, f(z)?];
                    room.angle = f(angle)?;
                }
                "room_guns" => kept.room.get_or_insert_with(RoomKept::default).guns = ids(value)?,
                "room_ammo" => kept.room.get_or_insert_with(RoomKept::default).ammo = ids(value)?,
                "room_thrown" => {
                    kept.room.get_or_insert_with(RoomKept::default).thrown = ids(value)?
                }
                _ => {}
            }
        }
        (version == Some(GRANT_VERSION)).then_some(kept)
    }
}

/// A prepared save: its bytes, its co-save's when there is one, and what the range kept.
#[derive(Debug, Clone, PartialEq)]
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
        // The physics room's snapshot: its own key, made again for a new build or loadout; the
        // Test Range's keys as they were.
        assert_eq!(fingerprint_for(&plugins(), "auto", ""), a);
        let room = fingerprint_for(
            &plugins(),
            "auto",
            &room_key("7e1b3d4", &["WeapNV9mmPistol"]),
        );
        assert_ne!(room, a);
        assert_ne!(
            room,
            fingerprint_for(
                &plugins(),
                "auto",
                &room_key("c22a2ed", &["WeapNV9mmPistol"])
            ),
            "a new build"
        );
        assert_ne!(
            room,
            fingerprint_for(
                &plugins(),
                "auto",
                &room_key("7e1b3d4", &["WeapNV9mmPistol", "WeapMinigun"])
            ),
            "a new loadout"
        );
    }

    /// What the range keeps reads back as written; another version's is not used.
    #[test]
    fn the_record_round_trips() {
        let kept = Kept {
            place: "Lucky38Int".into(),
            container: 0xff00_0801,
            targets: vec![0xff00_0802, 0xff00_0803],
            ammo: vec![0x0004_241e, 0x0010_0000],
            room: None,
        };
        assert_eq!(Kept::from_text(&kept.to_text()), Some(kept.clone()));
        let room = Kept {
            room: Some(RoomKept {
                base: 0x0010_c1d0,
                at: [1024.5, -88.25, 4.0],
                angle: 270.0,
                guns: vec![0x000e_3778, 0x0009_0b23],
                ammo: vec![0x0008_ed03],
                thrown: vec![0x0009_0b23],
            }),
            ..kept.clone()
        };
        assert_eq!(Kept::from_text(&room.to_text()), Some(room));
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
