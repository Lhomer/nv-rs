//! Launcher.exe: keeps the preview package up to date from its GitHub Release, then starts
//! Play.exe. Double-clicked; no command prompt, no GitHub account, no git.
//!
//! It reads the installed build (`BUILD.txt`), asks the release for the newest one (`latest.txt`,
//! over HTTPS), and when that is another build offers to update: the zip is downloaded, checked
//! against the published SHA-256 (`<zip>.sha256`), unpacked, and its programs put in place of the
//! installed ones, all or nothing, never touching `userdata\` ([`update`]). Then, or when there is
//! nothing newer, or when the release can't be reached, it asks what to start: Play or Play with
//! mods ([`choice`]; the last choice is the default button). A failed update leaves the installed
//! build as it was and playable.
//!
//! The release is the Play package workflow's rolling prerelease of the branch the package was
//! built from: `NV_RS_UPDATE_REPO` and `NV_RS_UPDATE_TAG` at build time, else this fork's
//! `claude/mouse-look-like-original` one ([`DEFAULT_REPO`], [`DEFAULT_TAG`]). The test room's
//! package (`NV_RS_ROOM=1`) updates from `windows-test`'s `windows-latest.txt`
//! (`NV_RS_UPDATE_LATEST`) and starts the room without asking.
#![cfg_attr(windows, windows_subsystem = "windows")]
// Off Windows only the tests use the update logic.
#![cfg_attr(not(windows), allow(dead_code))]

mod choice;
mod sha256;
mod update;
#[cfg(windows)]
mod windows;

/// The release a package built without the workflow's settings looks at.
pub const DEFAULT_REPO: &str = "Lhomer/nv-rs";
pub const DEFAULT_TAG: &str = "play-claude-mouse-look-like-original";

/// The release's file naming the newest build (`latest.txt`; the test room's package reads
/// `windows-latest.txt`: `NV_RS_UPDATE_LATEST` at build time).
pub fn latest_name() -> &'static str {
    option_env!("NV_RS_UPDATE_LATEST").unwrap_or("latest.txt")
}

/// Whether this is the test room's package (`NV_RS_ROOM=1` at build time,
/// docs/WINDOWS-ROOM.md): Play.exe starts the room, so there is nothing to choose.
pub fn room() -> bool {
    option_env!("NV_RS_ROOM") == Some("1")
}

/// The release this build updates from: `(owner/repo, tag)`.
pub fn channel() -> (&'static str, &'static str) {
    (
        option_env!("NV_RS_UPDATE_REPO").unwrap_or(DEFAULT_REPO),
        option_env!("NV_RS_UPDATE_TAG").unwrap_or(DEFAULT_TAG),
    )
}

/// The download address of a release asset (`https://github.com/<repo>/releases/download/...`):
/// public, no account needed.
pub fn asset_url(repo: &str, tag: &str, asset: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{asset}")
}

#[cfg(windows)]
fn main() {
    windows::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Launcher.exe runs on Windows only.");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_release_is_this_forks_branch() {
        assert_eq!(
            asset_url(DEFAULT_REPO, DEFAULT_TAG, "latest.txt"),
            "https://github.com/Lhomer/nv-rs/releases/download/play-claude-mouse-look-like-original/latest.txt"
        );
    }
}
