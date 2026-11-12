//! Launcher.exe's Windows side: its windows (messages and a small window saying what it is
//! doing, as Play.exe's own), the downloads (Windows' own `curl.exe`, HTTPS), the unpacking
//! (Windows' own `tar.exe`, which reads zip files, else PowerShell's `Expand-Archive`), and
//! starting Play.exe. Both tools come with Windows 10 (1803) and 11, so nothing is installed.

use crate::update::{self, Latest};
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(owner: *mut c_void, text: *const u16, title: *const u16, kind: u32) -> i32;
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn DefWindowProcW(window: *mut c_void, message: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(message: *mut [usize; 8], window: *mut c_void, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const [usize; 8]) -> i32;
    fn DispatchMessageW(message: *const [usize; 8]) -> isize;
    fn PostMessageW(window: isize, message: u32, w: usize, l: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn SetWindowTextW(window: isize, text: *const u16) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn GetSysColorBrush(index: i32) -> *mut c_void;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

#[repr(C)]
struct WndClassW {
    style: u32,
    procedure: unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize,
    class_extra: i32,
    window_extra: i32,
    instance: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu_name: *const u16,
    class_name: *const u16,
}

const TITLE: &str = "nv-rs launcher";
const MB_OK: u32 = 0;
const MB_YESNO: u32 = 4;
const MB_ICONERROR: u32 = 0x10;
const MB_ICONWARNING: u32 = 0x30;
const MB_ICONINFORMATION: u32 = 0x40;
const IDYES: i32 = 6;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(text: &str, kind: u32) -> i32 {
    let (text, title) = (wide(text), wide(TITLE));
    // SAFETY: both strings live through the call; no owner window.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), kind) }
}

fn error(text: &str) {
    message_box(text, MB_OK | MB_ICONERROR);
}

fn warning(text: &str) {
    message_box(text, MB_OK | MB_ICONWARNING);
}

fn ask(text: &str) -> bool {
    message_box(text, MB_YESNO | MB_ICONINFORMATION) == IDYES
}

static STATUS_WINDOW: AtomicIsize = AtomicIsize::new(0);
static STATUS_TEXT: AtomicIsize = AtomicIsize::new(0);
const WM_DESTROY: u32 = 2;
const WM_CLOSE: u32 = 0x10;

unsafe extern "system" fn status_procedure(
    window: *mut c_void,
    message: u32,
    w: usize,
    l: isize,
) -> isize {
    if message == WM_DESTROY {
        STATUS_TEXT.store(0, Ordering::Relaxed);
        STATUS_WINDOW.store(0, Ordering::Relaxed);
        PostQuitMessage(0);
        return 0;
    }
    DefWindowProcW(window, message, w, l)
}

/// The small window saying what the launcher is doing, on a thread of its own.
struct Status;

impl Status {
    fn show(first: &str) -> Status {
        let text = first.to_owned();
        std::thread::spawn(move || status_thread(&text));
        for _ in 0..50 {
            if STATUS_TEXT.load(Ordering::Relaxed) != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Status
    }

    fn set(&self, text: &str) {
        let label = STATUS_TEXT.load(Ordering::Relaxed);
        if label != 0 {
            let text = wide(text);
            // SAFETY: the string lives through the call; a window that went only fails the call.
            unsafe { SetWindowTextW(label, text.as_ptr()) };
        }
    }

    fn close(&self) {
        let window = STATUS_WINDOW.load(Ordering::Relaxed);
        if window != 0 {
            // SAFETY: posting to a window that went only fails the call.
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
    }
}

fn status_thread(first: &str) {
    let class_name = wide("nv-rs-launcher-status");
    let (title, label, text) = (wide(TITLE), wide("STATIC"), wide(first));
    // SAFETY: the strings live for this whole function, and the loop keeps the window alive.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WndClassW {
            style: 0,
            procedure: status_procedure,
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(),
            // COLOR_BTNFACE.
            background: GetSysColorBrush(15),
            menu_name: std::ptr::null(),
            class_name: class_name.as_ptr(),
        };
        RegisterClassW(&class);
        let (width, height) = (480, 130);
        let (x, y) = (
            (GetSystemMetrics(0) - width) / 2,
            (GetSystemMetrics(1) - height) / 2,
        );
        // WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE.
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0x10c8_0000,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        // WS_CHILD | WS_VISIBLE.
        let label = CreateWindowExW(
            0,
            label.as_ptr(),
            text.as_ptr(),
            0x5000_0000,
            16,
            16,
            width - 48,
            height - 60,
            window,
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        STATUS_WINDOW.store(window as isize, Ordering::Relaxed);
        STATUS_TEXT.store(label as isize, Ordering::Relaxed);
        let mut message = [0usize; 8];
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Whether Play.exe runs (it holds this mutex while it and the game run).
fn play_is_running() -> bool {
    let name = wide("Local\\nv-rs-preview-launcher");
    // SAFETY: the name lives through the call; the handle, when there is one, is closed at once
    // (SYNCHRONIZE access only; holding it would make Play.exe think it runs twice).
    unsafe {
        let handle = OpenMutexW(0x0010_0000, 0, name.as_ptr());
        if handle.is_null() {
            false
        } else {
            CloseHandle(handle);
            true
        }
    }
}

/// A program of Windows' own (`%SystemRoot%\System32\<name>`): never one found on the PATH.
fn system_tool(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32").join(name)
}

/// `curl.exe`'s arguments for an HTTPS download: fail on an HTTP error, follow the release's
/// redirect, no progress meter, a few tries.
fn curl() -> Command {
    let mut command = Command::new(system_tool("curl.exe"));
    command
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "2",
            "--connect-timeout",
            "20",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null());
    command
}

/// A small text asset, `None` with the reason when it can't be had.
fn fetch_text(url: &str) -> Result<String, String> {
    let output = curl()
        .args(["--max-time", "60", url])
        .output()
        .map_err(|e| format!("Windows' curl.exe could not run ({e})"))?;
    if !output.status.success() {
        return Err(failure(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn failure(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_string();
    if text.is_empty() {
        "the download failed".into()
    } else {
        text
    }
}

/// The zip into `file`, its progress on the status window.
fn download(url: &str, file: &Path, bytes: u64, status: &Status) -> Result<(), String> {
    let mut child = curl()
        .arg("--output")
        .arg(file)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Windows' curl.exe could not run ({e})"))?;
    loop {
        if let Some(done) = child.try_wait().map_err(|e| e.to_string())? {
            let mut stderr = Vec::new();
            if let Some(mut pipe) = child.stderr.take() {
                use std::io::Read;
                let _ = pipe.read_to_end(&mut stderr);
            }
            return if done.success() {
                Ok(())
            } else {
                Err(failure(&stderr))
            };
        }
        let got = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let mb = |n: u64| n as f64 / 1_048_576.0;
        status.set(&if bytes > 0 {
            format!(
                "Downloading the new build: {:.1} of {:.1} MB ({}%)",
                mb(got),
                mb(bytes),
                (got * 100 / bytes).min(100)
            )
        } else {
            format!("Downloading the new build: {:.1} MB", mb(got))
        });
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Unpacks `zip` into `into`: Windows' `tar.exe`, else PowerShell's `Expand-Archive`.
fn unpack(zip: &Path, into: &Path) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let tar = Command::new(system_tool("tar.exe"))
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(into)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output();
    if matches!(&tar, Ok(output) if output.status.success()) {
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let quote = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        quote(zip),
        quote(into)
    );
    let shell = Command::new(system_tool("WindowsPowerShell\\v1.0\\powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("the zip could not be unpacked ({e})"))?;
    if shell.status.success() {
        Ok(())
    } else {
        Err(format!(
            "the zip could not be unpacked ({})",
            failure(&shell.stderr)
        ))
    }
}

/// Starts Play.exe from the install, as a double-click would; the launcher ends.
fn play(install: &Path) {
    let play = install.join("Play.exe");
    if let Err(why) = Command::new(&play).current_dir(install).spawn() {
        error(&format!(
            "Play.exe could not be started ({why}).\n\n{}",
            play.display()
        ));
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

/// Launcher.exe (the crate's documentation).
pub fn run() {
    let install = match std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        Some(dir) => dir,
        None => return error("The launcher could not find its own folder."),
    };
    if !install.join("Play.exe").exists() {
        return error(&format!(
            "Launcher.exe belongs in the nv-rs folder, beside Play.exe, and Play.exe is not here:\n\n{}",
            install.display()
        ));
    }
    update::clear_leftovers(&install);
    let installed = std::fs::read_to_string(install.join("BUILD.txt"))
        .ok()
        .and_then(|text| update::installed_commit(&text));
    let (repo, tag) = crate::channel();
    let status = Status::show("Looking for a newer build...");
    let latest = fetch_text(&crate::asset_url(repo, tag, "latest.txt")).and_then(|text| {
        update::parse_latest(&text).ok_or_else(|| "the release's latest.txt is not readable".into())
    });
    let latest = match latest {
        Ok(latest) => latest,
        Err(why) => {
            status.close();
            warning(&format!(
                "Could not look for a newer build ({why}).\n\nThe installed build starts."
            ));
            return play(&install);
        }
    };
    if installed.as_deref() == Some(latest.commit.as_str()) {
        status.set(&format!(
            "You have the newest build ({}). Starting the game...",
            short(&latest.commit)
        ));
        std::thread::sleep(Duration::from_millis(600));
        status.close();
        return play(&install);
    }
    status.close();
    let have = installed.as_deref().map_or("unknown", short);
    let question = format!(
        "A newer build is ready: {} ({}).\nInstalled: {}.\n\nUpdate now? Your saves and settings in the userdata folder stay as they are.\n\nNo starts the installed build.",
        short(&latest.commit),
        latest.version,
        have
    );
    if !ask(&question) {
        return play(&install);
    }
    if play_is_running() {
        warning("The game is running. Close it, then start Launcher.exe again to update.");
        return;
    }
    let status = Status::show("Downloading the new build...");
    let result = update_to(&install, repo, tag, &latest, &status);
    status.close();
    let _ = std::fs::remove_dir_all(install.join(update::STAGING));
    match result {
        Ok(()) => play(&install),
        Err(why) => {
            if ask(&format!(
                "The update did not happen: {why}\n\nThe installed build is as it was. Start it?"
            )) {
                play(&install);
            }
        }
    }
}

/// Downloads, checks, unpacks and puts `latest` in place (all or nothing).
fn update_to(
    install: &Path,
    repo: &str,
    tag: &str,
    latest: &Latest,
    status: &Status,
) -> Result<(), String> {
    let staging = install.join(update::STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let published = fetch_text(&crate::asset_url(
        repo,
        tag,
        &format!("{}.sha256", latest.zip),
    ))?;
    let expected = update::parse_sha256(&published)
        .ok_or_else(|| "the published SHA-256 is not readable".to_string())?;
    let zip = staging.join(&latest.zip);
    download(
        &crate::asset_url(repo, tag, &latest.zip),
        &zip,
        latest.bytes,
        status,
    )?;
    status.set("Checking the download...");
    if !update::verify(&zip, &expected).map_err(|e| e.to_string())? {
        return Err("the download's SHA-256 is not the published one (try again later)".into());
    }
    status.set("Unpacking...");
    let unpacked = staging.join("unpacked");
    unpack(&zip, &unpacked)?;
    let root = update::package_root(&unpacked).map_err(|e| e.to_string())?;
    let files = update::plan(&root).map_err(|e| e.to_string())?;
    status.set("Putting the new build in place...");
    update::apply(&root, install, &files).map_err(|e| {
        format!("{e}. If the game or a tool from this folder is open, close it and try again")
    })?;
    status.set(&format!(
        "Updated to {}. Starting the game...",
        short(&latest.commit)
    ));
    std::thread::sleep(Duration::from_millis(800));
    Ok(())
}
