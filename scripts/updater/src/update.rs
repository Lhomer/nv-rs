//! What Launcher.exe does to the installed package, with no Windows in it (tested everywhere).
//!
//! **What it never touches.** `userdata\` (the player's saves, `Fallout.ini`, `FalloutPrefs.ini`,
//! `settings.ini`, logs, reports and the mods' play data): no file in it is written, replaced,
//! renamed or deleted, whatever the new package holds ([`protected`]). Files the new package does
//! not have are left as they are (a mod in `mods\`, for one).
//!
//! **How the programs are replaced.** All or nothing ([`apply`]): every new file is first copied
//! beside its place (`<name>.nv-rs-new`); only when all are there does each old file step aside
//! (`<name>.nv-rs-old`) for its new one; if any step fails, every file put in place goes and every
//! old one comes back, so the installed build still plays. The old files go afterwards; one that is
//! still running (Launcher.exe itself: Windows lets a running program be renamed, not deleted) is
//! cleared at the next start ([`clear_leftovers`]).

use std::io;
use std::path::{Component, Path, PathBuf};

/// The player's folder, never touched.
pub const USERDATA: &str = "userdata";
/// Where an update is downloaded and unpacked, inside the install (not in `userdata`).
pub const STAGING: &str = "update.nv-rs";
/// The marks of a new file waiting for its place and of an old one that stepped aside.
pub const NEW: &str = ".nv-rs-new";
pub const OLD: &str = ".nv-rs-old";

/// The installed build's source commit: `BUILD.txt`'s `Source commit: <sha>` line.
pub fn installed_commit(build_txt: &str) -> Option<String> {
    build_txt.lines().find_map(|line| {
        let commit = line.trim().strip_prefix("Source commit:")?.trim();
        is_commit(commit).then(|| commit.to_ascii_lowercase())
    })
}

fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The newest build on the release (`latest.txt`, written by the Play package workflow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub commit: String,
    pub version: String,
    /// The zip's asset name and size in bytes (for the progress line).
    pub zip: String,
    pub bytes: u64,
}

/// `latest.txt`'s `key=value` lines; `None` without a commit or a zip name.
pub fn parse_latest(text: &str) -> Option<Latest> {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let commit = value("commit")?.to_ascii_lowercase();
    let zip = value("zip")?;
    if !is_commit(&commit) || !safe_asset_name(&zip) {
        return None;
    }
    Some(Latest {
        commit,
        version: value("version").unwrap_or_default(),
        zip,
        bytes: value("bytes").and_then(|b| b.parse().ok()).unwrap_or(0),
    })
}

/// An asset name the launcher fetches: letters, digits, `.`, `-` and `_` only.
fn safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The hash a `.sha256` file gives (`<hex>  <name>`, or the hex alone), lowercase.
pub fn parse_sha256(text: &str) -> Option<String> {
    let hex = text.split_whitespace().next()?.to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// Whether the downloaded file is the published one.
pub fn verify(file: &Path, expected: &str) -> io::Result<bool> {
    Ok(crate::sha256::file_hex(file)? == expected.to_ascii_lowercase())
}

/// Whether a path inside the package is the player's (`userdata\...`), or one the launcher keeps
/// for itself (its staging folder, a file marked new or old): never written by an update.
pub fn protected(relative: &Path) -> bool {
    let mut parts = relative.components();
    let first = match parts.next() {
        Some(Component::Normal(first)) => first.to_string_lossy(),
        _ => return true,
    };
    if first.eq_ignore_ascii_case(USERDATA) || first.eq_ignore_ascii_case(STAGING) {
        return true;
    }
    let name = relative
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(NEW) || name.ends_with(OLD)
}

/// A relative path with only plain parts (no root, no `..`).
fn plain(relative: &Path) -> bool {
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        && relative.components().next().is_some()
}

/// The unpacked package's own folder: the zip holds one folder (`nv-rs-preview-<version>-...`)
/// with the package in it; a zip of loose files is its own folder.
pub fn package_root(unpacked: &Path) -> io::Result<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(unpacked)?.collect::<Result<_, _>>()?;
    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        return Ok(entries[0].path());
    }
    Ok(unpacked.to_path_buf())
}

/// Every file of the new package that an update puts in place, relative to its root: all but the
/// [`protected`] ones. A package without `Play.exe` is not one (an error).
pub fn plan(new_root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk(new_root, Path::new(""), &mut files)?;
    files.retain(|f| plain(f) && !protected(f));
    files.sort();
    if !files
        .iter()
        .any(|f| f.as_os_str().eq_ignore_ascii_case("Play.exe"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the download is not an nv-rs package (it has no Play.exe)",
        ));
    }
    Ok(files)
}

fn walk(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !protected(&path) {
                walk(root, &path, files)?;
            }
        } else if kind.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn marked(path: &Path, mark: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(mark);
    PathBuf::from(name)
}

/// Puts `files` (from [`plan`]) of `new_root` in place in `install`, all or nothing (module
/// documentation). On an error the install is as it was, and the error says which file.
pub fn apply(new_root: &Path, install: &Path, files: &[PathBuf]) -> io::Result<()> {
    for file in files {
        assert!(plain(file) && !protected(file), "{}", file.display());
    }
    // 1. Every new file beside its place.
    let mut copied: Vec<PathBuf> = Vec::new();
    for file in files {
        let target = install.join(file);
        let step = || -> io::Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(new_root.join(file), marked(&target, NEW))?;
            Ok(())
        };
        if let Err(why) = step() {
            for done in &copied {
                let _ = std::fs::remove_file(marked(done, NEW));
            }
            let _ = std::fs::remove_file(marked(&target, NEW));
            return Err(with_file(why, file));
        }
        copied.push(target);
    }
    // 2. Each old file aside, its new one in place.
    let mut placed: Vec<(PathBuf, bool)> = Vec::new();
    for target in &copied {
        let had = target.exists();
        let step = || -> io::Result<()> {
            if had {
                std::fs::rename(target, marked(target, OLD))?;
            }
            if let Err(why) = std::fs::rename(marked(target, NEW), target) {
                if had {
                    let _ = std::fs::rename(marked(target, OLD), target);
                }
                return Err(why);
            }
            Ok(())
        };
        if let Err(why) = step() {
            // Back as it was: the placed files go, the old ones return, the waiting ones go.
            for (done, had) in placed.iter().rev() {
                let _ = std::fs::remove_file(done);
                if *had {
                    let _ = std::fs::rename(marked(done, OLD), done);
                }
            }
            for waiting in &copied {
                let _ = std::fs::remove_file(marked(waiting, NEW));
            }
            let relative = target.strip_prefix(install).unwrap_or(target);
            return Err(with_file(why, relative));
        }
        placed.push((target.clone(), had));
    }
    // 3. The old files go (a running one stays until the next start).
    for (target, had) in &placed {
        if *had {
            let _ = std::fs::remove_file(marked(target, OLD));
        }
    }
    Ok(())
}

fn with_file(why: io::Error, file: &Path) -> io::Error {
    io::Error::new(why.kind(), format!("{}: {why}", file.display()))
}

/// What a past update left: files marked new or old (outside `userdata`) and the staging folder.
pub fn clear_leftovers(install: &Path) {
    let _ = std::fs::remove_dir_all(install.join(STAGING));
    let mut files = Vec::new();
    if walk_marked(install, Path::new(""), &mut files).is_ok() {
        for file in files {
            let _ = std::fs::remove_file(install.join(file));
        }
    }
}

fn walk_marked(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let first_is_userdata = path
            .components()
            .next()
            .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(USERDATA));
        if first_is_userdata {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !name.eq_ignore_ascii_case(STAGING) {
                walk_marked(root, &path, files)?;
            }
        } else if kind.is_file() && (name.ends_with(NEW) || name.ends_with(OLD)) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A scratch folder of its own, gone when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "nv-rs-updater-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(root: &Path, files: &[(&str, &str)]) {
        for (name, text) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    /// Every file under `root`, by its relative path with `/`, and its text.
    fn tree(root: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        fn go(root: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
            for entry in std::fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    go(root, &path, out);
                } else {
                    let name = path.strip_prefix(root).unwrap().to_string_lossy();
                    out.insert(
                        name.replace('\\', "/"),
                        std::fs::read_to_string(&path).unwrap(),
                    );
                }
            }
        }
        go(root, root, &mut out);
        out
    }

    /// An install with the player's files, and a new package that (wrongly) holds files under
    /// `userdata` too.
    fn install_and_package(scratch: &Scratch) -> (PathBuf, PathBuf) {
        let install = scratch.0.join("nv-rs");
        write(
            &install,
            &[
                ("Play.exe", "old play"),
                ("Launcher.exe", "old launcher"),
                ("BUILD.txt", "Source commit: old"),
                ("app/FalloutNV.exe", "old game"),
                ("app/tools/old-only.exe", "kept"),
                ("mods/My Mod.esp", "the player's mod"),
                (
                    "userdata/game/Documents/My Games/FalloutNV/Fallout.ini",
                    "fov 100",
                ),
                (
                    "userdata/game/Documents/My Games/FalloutNV/Saves/Save 1.fos",
                    "save",
                ),
                ("userdata/logs/launcher.log", "log"),
            ],
        );
        let package = scratch
            .0
            .join("unpacked/nv-rs-preview-ci-x-windows-x64-test");
        write(
            &package,
            &[
                ("Play.exe", "new play"),
                ("Launcher.exe", "new launcher"),
                ("BUILD.txt", "Source commit: new"),
                ("app/FalloutNV.exe", "new game"),
                ("app/tools/d3d9-capture.exe", "new tool"),
                ("mods/READ ME.txt", "mods readme"),
                (
                    "userdata/game/Documents/My Games/FalloutNV/Fallout.ini",
                    "OVERWRITTEN",
                ),
                ("userdata/new-in-userdata.txt", "OVERWRITTEN"),
                ("Settings.ps1.nv-rs-new", "a mark"),
            ],
        );
        (install, package)
    }

    #[test]
    fn an_update_replaces_the_programs_and_never_touches_userdata() {
        let scratch = Scratch::new("apply");
        let (install, package) = install_and_package(&scratch);
        let before = tree(&install);
        let root = package_root(&scratch.0.join("unpacked")).unwrap();
        assert_eq!(root, package);
        let files = plan(&root).unwrap();
        assert!(files.iter().all(|f| !protected(f)));
        assert!(!files
            .iter()
            .any(|f| f.to_string_lossy().contains("userdata")));
        apply(&root, &install, &files).unwrap();
        let after = tree(&install);
        for (name, text) in &before {
            if name.starts_with("userdata/") || name.starts_with("mods/My Mod") {
                assert_eq!(after.get(name), Some(text), "{name} stays as it was");
            }
        }
        assert!(!after.contains_key("userdata/new-in-userdata.txt"));
        assert_eq!(after["Play.exe"], "new play");
        assert_eq!(after["Launcher.exe"], "new launcher");
        assert_eq!(after["app/FalloutNV.exe"], "new game");
        assert_eq!(after["app/tools/d3d9-capture.exe"], "new tool");
        assert_eq!(after["app/tools/old-only.exe"], "kept");
        assert_eq!(after["mods/READ ME.txt"], "mods readme");
        assert!(!after.keys().any(|k| k.ends_with(NEW) || k.ends_with(OLD)));
    }

    #[test]
    fn a_failed_step_leaves_the_install_as_it_was() {
        let scratch = Scratch::new("rollback");
        let (install, package) = install_and_package(&scratch);
        // Settings.cmd's old file can't step aside (a folder that is not empty is where it would
        // go), after BUILD.txt, Launcher.exe and Play.exe (first in the plan's order) are in place.
        write(&package, &[("Settings.cmd", "new settings")]);
        write(
            &install,
            &[
                ("Settings.cmd", "old settings"),
                ("Settings.cmd.nv-rs-old/inside.txt", "in the way"),
            ],
        );
        let before = tree(&install);
        let files = plan(&package).unwrap();
        let error = apply(&package, &install, &files).unwrap_err();
        assert!(error.to_string().contains("Settings.cmd"), "{error}");
        assert_eq!(
            tree(&install),
            before,
            "every file as it was, nothing marked left"
        );
    }

    #[test]
    fn a_download_without_play_exe_is_refused() {
        let scratch = Scratch::new("not-a-package");
        write(&scratch.0, &[("readme.txt", "hello"), ("app/x.exe", "x")]);
        assert!(plan(&scratch.0).is_err());
    }

    #[test]
    fn only_the_programs_are_ever_written() {
        for path in [
            "userdata/game/Saves/a.fos",
            "USERDATA/x",
            "UserData",
            "update.nv-rs/x.zip",
            "Play.exe.nv-rs-new",
            "app/FalloutNV.exe.NV-RS-OLD",
        ] {
            assert!(protected(Path::new(path)), "{path}");
        }
        for path in [
            "Play.exe",
            "app/FalloutNV.exe",
            "mods/READ ME.txt",
            "userdata2/x",
        ] {
            assert!(!protected(Path::new(path)), "{path}");
        }
        assert!(!plain(Path::new("../outside")));
        assert!(!plain(Path::new("/abs")));
    }

    #[test]
    fn leftovers_go_but_never_from_userdata() {
        let scratch = Scratch::new("leftovers");
        let install = &scratch.0;
        write(
            install,
            &[
                ("Launcher.exe.nv-rs-old", "old launcher"),
                ("app/FalloutNV.exe.nv-rs-new", "half"),
                ("update.nv-rs/nv-rs-play.zip", "zip"),
                ("userdata/keep.nv-rs-old", "the player's"),
                ("Play.exe", "play"),
            ],
        );
        clear_leftovers(install);
        let left = tree(install);
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            ["Play.exe", "userdata/keep.nv-rs-old"]
        );
    }

    #[test]
    fn the_hash_check_takes_only_the_published_file() {
        let scratch = Scratch::new("hash");
        let zip = scratch.0.join("nv-rs-play.zip");
        std::fs::write(&zip, b"abc").unwrap();
        let published =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  nv-rs-play.zip\r\n";
        let hex = parse_sha256(published).unwrap();
        assert!(verify(&zip, &hex).unwrap());
        assert!(verify(&zip, &hex.to_ascii_uppercase()).unwrap());
        std::fs::write(&zip, b"abd").unwrap();
        assert!(!verify(&zip, &hex).unwrap(), "a changed byte is refused");
        assert_eq!(parse_sha256("not a hash"), None);
        assert_eq!(parse_sha256(""), None);
    }

    #[test]
    fn the_builds_are_told_apart_by_their_commit() {
        let commit = "0aad366ba0b036d9001373ad9f51e9d538edadda";
        let build = format!("nv-rs Windows preview ci-x\r\nSource commit: {commit}\r\napp\\FalloutNV.exe SHA256: AB\r\n");
        assert_eq!(installed_commit(&build).as_deref(), Some(commit));
        assert_eq!(installed_commit("Source commit: abc"), None);
        let latest = parse_latest(&format!(
            "commit={}\nversion=ci-claude-x-0aad366\nzip=nv-rs-play.zip\nbytes=32989888\n",
            commit.to_ascii_uppercase()
        ))
        .unwrap();
        assert_eq!(latest.commit, commit);
        assert_eq!(latest.zip, "nv-rs-play.zip");
        assert_eq!(latest.bytes, 32_989_888);
        assert_eq!(latest.version, "ci-claude-x-0aad366");
        assert_eq!(parse_latest("commit=abc\nzip=a.zip\n"), None);
        assert_eq!(
            parse_latest(&format!("commit={commit}\nzip=../../evil.zip\n")),
            None,
            "only a plain asset name is fetched"
        );
    }
}
