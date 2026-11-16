//! Launcher.exe's Windows side: its windows (messages and a small window saying what it is
//! doing, as Play.exe's own), the downloads (Windows' own `curl.exe`, HTTPS), the unpacking
//! (Windows' own `tar.exe`, which reads zip files, else PowerShell's `Expand-Archive`), and
//! asking what to start (Play or Play with mods) and starting Play.exe. Both tools come with
//! Windows 10 (1803) and 11, so nothing is installed.

use crate::choice::{self, Choice};
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
    fn DestroyWindow(window: *mut c_void) -> i32;
    fn IsDialogMessageW(window: *mut c_void, message: *mut [usize; 8]) -> i32;
    fn SetFocus(window: *mut c_void) -> *mut c_void;
    fn SetForegroundWindow(window: *mut c_void) -> i32;
    fn SendMessageW(window: *mut c_void, message: u32, w: usize, l: isize) -> isize;
}
#[link(name = "gdi32")]
extern "system" {
    fn GetStockObject(index: i32) -> *mut c_void;
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

/// The choice window's buttons and what it answered (0: closed without a choice).
const ID_PLAY: usize = 101;
const ID_MODS: usize = 102;
const ID_TESTER: usize = 103;
const IDCANCEL: usize = 2;
const WM_COMMAND: u32 = 0x111;
const WM_SETFONT: u32 = 0x30;
const DM_GETDEFID: u32 = 0x400;
const DC_HASDEFID: isize = 0x534b;
static CHOSEN: AtomicIsize = AtomicIsize::new(0);
static DEFAULT_ID: AtomicIsize = AtomicIsize::new(ID_PLAY as isize);

unsafe extern "system" fn choice_procedure(
    window: *mut c_void,
    message: u32,
    w: usize,
    l: isize,
) -> isize {
    match message {
        WM_COMMAND => {
            let id = w & 0xffff;
            if id == ID_PLAY || id == ID_MODS || id == ID_TESTER {
                CHOSEN.store(id as isize, Ordering::Relaxed);
                DestroyWindow(window);
            } else if id == IDCANCEL {
                DestroyWindow(window);
            }
            0
        }
        // Enter presses the default button (the last choice).
        DM_GETDEFID => (DC_HASDEFID << 16) | DEFAULT_ID.load(Ordering::Relaxed),
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, w, l),
    }
}

/// The window that asks what to start: `note` (what the update check found) above the buttons
/// (Play, Play with mods, and Launch physics tester: the Windows test room, installed and updated
/// beside this install), `default` the one Enter presses. `None` when it is closed without a
/// choice.
fn ask_choice(note: &str, default: Choice) -> Option<Choice> {
    let class_name = wide("nv-rs-launcher-choice");
    let (title, label, button) = (wide(TITLE), wide("STATIC"), wide("BUTTON"));
    let text = wide(&format!("{note}\n\nWhat do you want to start?"));
    let (play, mods, tester) = (
        wide("Play"),
        wide("Play with mods"),
        wide("Launch physics tester"),
    );
    let default_id = match default {
        Choice::Play => ID_PLAY,
        Choice::Mods => ID_MODS,
        Choice::Tester => ID_TESTER,
    };
    CHOSEN.store(0, Ordering::Relaxed);
    DEFAULT_ID.store(default_id as isize, Ordering::Relaxed);
    // SAFETY: the strings live for this whole function, and the loop keeps the window alive
    // until it is destroyed.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WndClassW {
            style: 0,
            procedure: choice_procedure,
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
        let (width, height) = (600, 190);
        let (x, y) = (
            (GetSystemMetrics(0) - width) / 2,
            (GetSystemMetrics(1) - height) / 2,
        );
        // WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE; WS_EX_CONTROLPARENT for the keys.
        let window = CreateWindowExW(
            0x0001_0000,
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
        if window.is_null() {
            return Some(default);
        }
        let font = GetStockObject(17); // DEFAULT_GUI_FONT
                                       // WS_CHILD | WS_VISIBLE.
        let note = CreateWindowExW(
            0,
            label.as_ptr(),
            text.as_ptr(),
            0x5000_0000,
            16,
            14,
            width - 48,
            80,
            window,
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        let mut buttons = Vec::new();
        for (id, caption, left) in [
            (ID_PLAY, &play, 16),
            (ID_MODS, &mods, 200),
            (ID_TESTER, &tester, 384),
        ] {
            // WS_CHILD | WS_VISIBLE | WS_TABSTOP, BS_DEFPUSHBUTTON for the default.
            let style = 0x5001_0000 | u32::from(id == default_id);
            let handle = CreateWindowExW(
                0,
                button.as_ptr(),
                caption.as_ptr(),
                style,
                left,
                height - 84,
                170,
                32,
                window,
                id as *mut c_void,
                instance,
                std::ptr::null_mut(),
            );
            buttons.push((id, handle));
        }
        for handle in std::iter::once(note).chain(buttons.iter().map(|b| b.1)) {
            if !handle.is_null() && !font.is_null() {
                SendMessageW(handle, WM_SETFONT, font as usize, 1);
            }
        }
        SetForegroundWindow(window);
        if let Some(&(_, handle)) = buttons.iter().find(|b| b.0 == default_id) {
            SetFocus(handle);
        }
        let mut message = [0usize; 8];
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(window, &mut message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    match CHOSEN.load(Ordering::Relaxed) as usize {
        ID_PLAY => Some(Choice::Play),
        ID_MODS => Some(Choice::Mods),
        ID_TESTER => Some(Choice::Tester),
        _ => None,
    }
}

/// Asks what to start ([`ask_choice`], the last choice the default), remembers it, and starts
/// Play.exe for it; closing the window starts nothing.
fn choose_and_play(install: &Path, note: &str) {
    // The test room's package: Play.exe starts the room; nothing to choose.
    if crate::room() {
        return play(install, Choice::Play);
    }
    let Some(choice) = ask_choice(note, choice::remembered(install)) else {
        return;
    };
    choice::remember(install, choice);
    if choice == Choice::Tester {
        return tester(install);
    }
    play(install, choice);
}

/// The physics tester (the Windows test room, docs/WINDOWS-ROOM.md): the newest build on the
/// `windows-test` release (`windows-latest.txt`) installed or updated in its own folder beside this
/// install (`update::tester_folder`: its own programs, `userdata` and saves; never this install's),
/// then its Play.exe started with `--room`. Unreachable release: the installed tester starts, if
/// there is one. Play's own release and its `latest.txt` are not touched.
fn tester(play_install: &Path) {
    let dir = update::tester_folder(play_install);
    if let Err(why) = std::fs::create_dir_all(&dir) {
        return error(&format!(
            "The physics tester's folder could not be made ({why}):\n\n{}",
            dir.display()
        ));
    }
    update::clear_leftovers(&dir);
    let installed = std::fs::read_to_string(dir.join("BUILD.txt"))
        .ok()
        .and_then(|text| update::installed_commit(&text));
    let has_it = dir.join("Play.exe").is_file();
    let (repo, _) = crate::channel();
    let status = Status::show("Looking for the newest physics tester...");
    let latest = fetch_text(&crate::asset_url(
        repo,
        update::TESTER_TAG,
        update::TESTER_LATEST,
    ))
    .and_then(|text| {
        update::parse_latest(&text)
            .ok_or_else(|| format!("the release's {} is not readable", update::TESTER_LATEST))
    });
    let latest = match latest {
        Ok(latest) => latest,
        Err(why) => {
            status.close();
            if has_it {
                return start_tester(&dir);
            }
            return error(&format!(
                "The physics tester could not be downloaded ({why}).\n\nTry again later."
            ));
        }
    };
    if has_it && installed.as_deref() == Some(latest.commit.as_str()) {
        status.close();
        return start_tester(&dir);
    }
    if play_is_running() {
        status.close();
        warning("The game or the physics tester is running. Close it, then try again.");
        return;
    }
    status.set(&format!(
        "Downloading the physics tester ({})...",
        short(&latest.commit)
    ));
    let result = update_to(&dir, repo, update::TESTER_TAG, &latest, &status);
    status.close();
    let _ = std::fs::remove_dir_all(dir.join(update::STAGING));
    match result {
        Ok(()) => start_tester(&dir),
        Err(why) if has_it => {
            warning(&format!(
                "The physics tester was not updated: {why}\nThe installed one starts."
            ));
            start_tester(&dir);
        }
        Err(why) => error(&format!("The physics tester could not be installed: {why}")),
    }
}

/// The physics tester's Play.exe started (`--room`: straight into the room).
fn start_tester(dir: &Path) {
    let play = dir.join("Play.exe");
    if let Err(why) = Command::new(&play)
        .args(Choice::Tester.args())
        .current_dir(dir)
        .spawn()
    {
        error(&format!(
            "The physics tester could not be started ({why}).\n\n{}",
            play.display()
        ));
    }
}

/// Starts Play.exe from the install, as a double-click would (with `--mods` for Play with mods,
/// as `Play with mods.cmd` does); the launcher ends.
fn play(install: &Path, choice: Choice) {
    let play = install.join("Play.exe");
    if let Err(why) = Command::new(&play)
        .args(choice.args())
        .current_dir(install)
        .spawn()
    {
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
    let name = crate::latest_name();
    let latest = fetch_text(&crate::asset_url(repo, tag, name)).and_then(|text| {
        update::parse_latest(&text).ok_or_else(|| format!("the release's {name} is not readable"))
    });
    let latest = match latest {
        Ok(latest) => latest,
        Err(why) => {
            status.close();
            return choose_and_play(
                &install,
                &format!(
                    "Could not look for a newer build ({why}). You can play the installed build."
                ),
            );
        }
    };
    if installed.as_deref() == Some(latest.commit.as_str()) {
        status.close();
        return choose_and_play(
            &install,
            &format!("You have the newest build ({}).", short(&latest.commit)),
        );
    }
    status.close();
    let have = installed.as_deref().map_or("unknown", short);
    let question = format!(
        "A newer build is ready: {} ({}).\nInstalled: {}.\n\nUpdate now? Your saves and settings in the userdata folder stay as they are.\n\nNo keeps the installed build.",
        short(&latest.commit),
        latest.version,
        have
    );
    if !ask(&question) {
        return choose_and_play(&install, &format!("The installed build ({have}) is kept."));
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
        Ok(()) => choose_and_play(&install, &format!("Updated to {}.", short(&latest.commit))),
        Err(why) => choose_and_play(
            &install,
            &format!("The update did not happen: {why}\nThe installed build is as it was."),
        ),
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
    status.set(&format!("Updated to {}.", short(&latest.commit)));
    std::thread::sleep(Duration::from_millis(800));
    Ok(())
}
