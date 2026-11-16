//! The Android test build (docs/ANDROID.md; Patrick, 2026-11-15): the real engine on the phone,
//! straight into the phone's Test Range (`host::phone_range`: one invincible target, obstacles, a
//! few guns), drawn by wgpu on Vulkan in the activity's native window, with touch controls.
//!
//! - **The game files** are the player's own, copied to the phone ([`find_game`]): the folder
//!   holding `FalloutNV.exe` and `Data`, in the app's own folder
//!   (`Android/data/io.github.lhomer.nvrs.test/files/FalloutNV`) or in `/sdcard/NVRS/FalloutNV`.
//!   Nothing is written there but this run's log (`nvrs-log.txt` beside it); the game's own writes
//!   stay in memory, and the range never saves.
//! - **The game** runs on a thread of its own, as the viewer's main loop does (`viewer/src/main.rs`):
//!   the host started on the Data folder, main's frames one after another. The activity's thread
//!   only hands over the window and the touches.
//! - **The touch controls** ([`Controls`]): the left half a virtual stick (W, A, S, D), the right
//!   half a look swipe (the mouse's motion), and buttons: fire (the left mouse button), aim (the
//!   right one), reload (R), jump (Space), the next weapon, the target put back, the range menu
//!   (Home). Drawn over the game's frame (`d3d9_wgpu::touch_overlay`), colour-coded (no text).

#![cfg(target_os = "android")]

mod notice;

use android_activity::input::{InputEvent, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use d3d9_wgpu::touch_overlay::{self, Shape};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// DirectInput key codes the controls press.
const DIK_W: u8 = 0x11;
const DIK_A: u8 = 0x1e;
const DIK_S: u8 = 0x1f;
const DIK_D: u8 = 0x20;
const DIK_R: u8 = 0x13;
const DIK_SPACE: u8 = 0x39;
const DIK_HOME: u8 = 0xc7;

/// The look swipe's mouse counts per pixel moved.
const LOOK_GAIN: f32 = 1.4;
/// How far (a share of the stick's radius) the stick must lean before a key goes down.
const STICK_DEAD: f32 = 0.35;

/// A button's action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Fire,
    Aim,
    Reload,
    Jump,
    Weapon,
    Reset,
    Menu,
}

/// A button: its action, centre (the window's fractions), radius (a share of its height) and
/// colour.
struct Button {
    action: Action,
    centre: [f32; 2],
    radius: f32,
    colour: [f32; 3],
}

const BUTTONS: [Button; 7] = [
    Button {
        action: Action::Fire,
        centre: [0.88, 0.72],
        radius: 0.10,
        colour: [0.9, 0.15, 0.1],
    },
    Button {
        action: Action::Aim,
        centre: [0.75, 0.83],
        radius: 0.075,
        colour: [0.15, 0.4, 0.95],
    },
    Button {
        action: Action::Reload,
        centre: [0.75, 0.60],
        radius: 0.065,
        colour: [0.95, 0.8, 0.1],
    },
    Button {
        action: Action::Jump,
        centre: [0.63, 0.88],
        radius: 0.055,
        colour: [0.9, 0.9, 0.9],
    },
    Button {
        action: Action::Weapon,
        centre: [0.90, 0.45],
        radius: 0.065,
        colour: [0.15, 0.8, 0.3],
    },
    Button {
        action: Action::Reset,
        centre: [0.95, 0.12],
        radius: 0.05,
        colour: [0.7, 0.2, 0.85],
    },
    Button {
        action: Action::Menu,
        centre: [0.84, 0.12],
        radius: 0.05,
        colour: [0.5, 0.5, 0.5],
    },
];

/// The stick's centre (the window's fractions) and radius (a share of its height).
const STICK_CENTRE: [f32; 2] = [0.15, 0.70];
const STICK_RADIUS: f32 = 0.17;

/// What a finger is doing.
#[derive(Clone, Copy, Debug)]
enum Finger {
    Stick,
    Look { last: [f32; 2] },
    Button(usize),
}

/// The touch controls' state.
#[derive(Default)]
struct Controls {
    /// Each finger down: its pointer id and what it does.
    fingers: Vec<(i32, Finger)>,
    /// The stick's lean (-1 to 1 each way, y down) and the keys it holds.
    stick: [f32; 2],
    keys: [bool; 4],
    /// The window's size in pixels.
    size: (f32, f32),
}

impl Controls {
    fn frac(&self, x: f32, y: f32) -> [f32; 2] {
        [x / self.size.0.max(1.0), y / self.size.1.max(1.0)]
    }

    /// Which button a touch at `p` (fractions) is on.
    fn button_at(&self, p: [f32; 2]) -> Option<usize> {
        let aspect = self.size.0 / self.size.1.max(1.0);
        BUTTONS.iter().position(|b| {
            let dx = (p[0] - b.centre[0]) * aspect;
            let dy = p[1] - b.centre[1];
            (dx * dx + dy * dy).sqrt() <= b.radius * 1.15
        })
    }

    fn down(&mut self, id: i32, x: f32, y: f32) {
        let p = self.frac(x, y);
        let finger = if let Some(b) = self.button_at(p) {
            press(BUTTONS[b].action, true);
            Finger::Button(b)
        } else if p[0] < 0.5 {
            self.lean(p);
            Finger::Stick
        } else {
            Finger::Look { last: [x, y] }
        };
        self.fingers.retain(|f| f.0 != id);
        self.fingers.push((id, finger));
    }

    fn moved(&mut self, id: i32, x: f32, y: f32) {
        let Some(i) = self.fingers.iter().position(|f| f.0 == id) else {
            return;
        };
        match self.fingers[i].1 {
            Finger::Stick => {
                let p = self.frac(x, y);
                self.lean(p);
            }
            Finger::Look { last } => {
                let (dx, dy) = (x - last[0], y - last[1]);
                engine_platform::d3d9real::inject_motion(
                    (dx * LOOK_GAIN).round() as i32,
                    (dy * LOOK_GAIN).round() as i32,
                );
                self.fingers[i].1 = Finger::Look { last: [x, y] };
            }
            Finger::Button(_) => {}
        }
    }

    fn up(&mut self, id: i32) {
        let Some(i) = self.fingers.iter().position(|f| f.0 == id) else {
            return;
        };
        match self.fingers.remove(i).1 {
            Finger::Stick => {
                self.stick = [0.0; 2];
                self.set_keys([false; 4]);
            }
            Finger::Button(b) => press(BUTTONS[b].action, false),
            Finger::Look { .. } => {}
        }
    }

    fn release_all(&mut self) {
        self.fingers.clear();
        self.stick = [0.0; 2];
        self.keys = [false; 4];
        engine_platform::d3d9real::inject_release_all();
    }

    /// The stick leant toward `p` (fractions): W, A, S, D down past the dead zone.
    fn lean(&mut self, p: [f32; 2]) {
        let aspect = self.size.0 / self.size.1.max(1.0);
        let mut v = [
            (p[0] - STICK_CENTRE[0]) * aspect / STICK_RADIUS,
            (p[1] - STICK_CENTRE[1]) / STICK_RADIUS,
        ];
        let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
        if l > 1.0 {
            v = [v[0] / l, v[1] / l];
        }
        self.stick = v;
        self.set_keys([
            v[1] < -STICK_DEAD,
            v[0] < -STICK_DEAD,
            v[1] > STICK_DEAD,
            v[0] > STICK_DEAD,
        ]);
    }

    fn set_keys(&mut self, keys: [bool; 4]) {
        for (i, &dik) in [DIK_W, DIK_A, DIK_S, DIK_D].iter().enumerate() {
            if keys[i] != self.keys[i] {
                engine_platform::d3d9real::inject_key(dik, keys[i]);
            }
        }
        self.keys = keys;
    }

    /// The shapes drawn over the game: the stick's ring and knob, each button (brighter while
    /// held).
    fn shapes(&self) -> Vec<Shape> {
        let aspect = self.size.0 / self.size.1.max(1.0);
        let mut out = vec![
            Shape {
                centre: STICK_CENTRE,
                half: STICK_RADIUS,
                colour: [1.0, 1.0, 1.0, 0.35],
                round: true,
                ring: 0.06,
            },
            Shape {
                centre: [
                    STICK_CENTRE[0] + self.stick[0] * STICK_RADIUS / aspect.max(0.1),
                    STICK_CENTRE[1] + self.stick[1] * STICK_RADIUS,
                ],
                half: STICK_RADIUS * 0.38,
                colour: [1.0, 1.0, 1.0, 0.4],
                round: true,
                ring: 0.0,
            },
        ];
        for (i, b) in BUTTONS.iter().enumerate() {
            let held = self
                .fingers
                .iter()
                .any(|f| matches!(f.1, Finger::Button(x) if x == i));
            let [r, g, bl] = b.colour;
            out.push(Shape {
                centre: b.centre,
                half: b.radius,
                colour: [r, g, bl, if held { 0.7 } else { 0.38 }],
                round: true,
                ring: 0.0,
            });
        }
        out
    }
}

/// A button's action pressed (`down`) or let go.
fn press(action: Action, down: bool) {
    use engine_platform::d3d9real::{inject_button as touch_button, inject_key as touch_key};
    match action {
        Action::Fire => touch_button(0, down),
        Action::Aim => touch_button(1, down),
        Action::Reload => touch_key(DIK_R, down),
        Action::Jump => touch_key(DIK_SPACE, down),
        Action::Menu => touch_key(DIK_HOME, down),
        Action::Weapon if down => host::phone_range::ask_next_weapon(),
        Action::Reset if down => host::phone_range::ask_reset(),
        Action::Weapon | Action::Reset => {}
    }
}

/// The folders the game's files may be in: `/sdcard/NVRS` (needs "All files access") and the
/// app's own (no permission needed, but hidden from many file managers).
fn candidates(app: &AndroidApp) -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/storage/emulated/0/NVRS/FalloutNV"),
        PathBuf::from("/storage/emulated/0/NVRS"),
    ];
    if let Some(own) = app.external_data_path() {
        out.push(own.join("FalloutNV"));
        out.push(own);
    }
    out
}

/// A child of `dir` named `name` in any case.
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
}

/// What a candidate folder holds: the game (its Data folder), or why not.
#[derive(Debug)]
enum Looked {
    Game(PathBuf),
    NotThere,
    NotReadable,
    Missing(Vec<&'static str>),
}

/// What `dir` holds (the folder with `FalloutNV.exe` and `Data/FalloutNV.esm`).
fn look(dir: &Path) -> Looked {
    match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Looked::NotReadable,
        Err(_) => return Looked::NotThere,
        Ok(_) => {}
    }
    let exe = child(dir, "FalloutNV.exe");
    let data = child(dir, "Data");
    let esm = data.as_deref().and_then(|d| child(d, "FalloutNV.esm"));
    match (exe, data, esm) {
        (Some(_), Some(data), Some(_)) => Looked::Game(data),
        (exe, data, esm) => {
            let mut missing = Vec::new();
            if exe.is_none() {
                missing.push("FalloutNV.exe");
            }
            if data.is_none() {
                missing.push("Data");
            } else if esm.is_none() {
                missing.push("Data/FalloutNV.esm");
            }
            Looked::Missing(missing)
        }
    }
}

/// The game's Data folder (the first candidate holding the game), or what each candidate holds.
fn find_game(app: &AndroidApp) -> Result<PathBuf, Vec<(PathBuf, Looked)>> {
    let mut tried = Vec::new();
    for dir in candidates(app) {
        match look(&dir) {
            Looked::Game(data) => return Ok(data),
            other => tried.push((dir, other)),
        }
    }
    Err(tried)
}

/// The message shown when the game cannot start: where the app looked and what each place lacks,
/// the "All files access" state, what to copy where, and where the log is.
fn not_found_message(tried: &[(PathBuf, Looked)], access: Option<bool>, log: &str) -> Vec<String> {
    let mut out = vec![
        "NV-RS Test: the game files were not found.".to_string(),
        String::new(),
        "Looked in:".to_string(),
    ];
    for (dir, looked) in tried {
        let what = match looked {
            Looked::Game(_) => "found".to_string(),
            Looked::NotThere => "no such folder".to_string(),
            Looked::NotReadable => "not readable (needs All files access)".to_string(),
            Looked::Missing(m) => format!("missing {}", m.join(", ")),
        };
        out.push(format!("  {}: {what}", dir.display()));
    }
    out.push(String::new());
    out.push(match access {
        Some(true) => "All files access: allowed.".to_string(),
        Some(false) => "All files access: NOT allowed. Tap the screen to open the setting \
             (Settings > Apps > Special access > All files access > NV-RS Test), allow it, then \
             come back."
            .to_string(),
        None => "All files access: could not be checked.".to_string(),
    });
    out.push(String::new());
    out.push(
        "Copy your FalloutNV folder (FalloutNV.exe, Fallout_default.ini and Data with \
         FalloutNV.esm, the .bsa files and Shaders) to Internal storage/NVRS/FalloutNV."
            .to_string(),
    );
    out.push(String::new());
    out.push(format!("Log: {log}"));
    out
}

/// This run's log (the process's standard output and error: the engine's lines) in
/// `nvrs-log.txt`: in Download (which any file manager shows; Android lets an app make its own
/// files there without a permission), else in `fallback` (the app's own folder). Where it went.
fn log_to(fallback: Option<&Path>) -> Option<PathBuf> {
    use std::os::fd::AsRawFd;
    let mut places = vec![PathBuf::from("/storage/emulated/0/Download")];
    places.extend(fallback.map(Path::to_path_buf));
    for dir in places {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("nvrs-log.txt");
        let Ok(file) = std::fs::File::create(&path) else {
            continue;
        };
        extern "C" {
            fn dup2(old: i32, new: i32) -> i32;
        }
        // SAFETY: the file stays open for the whole run (leaked below); 1 and 2 are the
        // process's standard output and error.
        unsafe {
            dup2(file.as_raw_fd(), 1);
            dup2(file.as_raw_fd(), 2);
        }
        std::mem::forget(file);
        return Some(path);
    }
    None
}

/// The game on its own thread: the room's game, shared with the Linux room
/// (`host::phone_range::run`).
fn run_game(data: PathBuf) {
    host::phone_range::run(data, "nv-rs Android");
}

/// The window's handle and size handed to the platform layer (0: gone).
fn hand_over(app: &AndroidApp, controls: &mut Controls) {
    match app.native_window() {
        Some(w) => {
            let (width, height) = (w.width().max(0) as u32, w.height().max(0) as u32);
            controls.size = (width as f32, height as f32);
            engine_platform::d3d9real::set_native_window(w.ptr().as_ptr() as isize, width, height);
            // The window stays alive while the activity holds it (its `TerminateWindow` hands 0).
            std::mem::forget(w);
        }
        None => engine_platform::d3d9real::set_native_window(0, 0, 0),
    }
}

/// The app's package (its "All files access" page).
const PACKAGE: &str = "io.github.lhomer.nvrs.test";

#[no_mangle]
fn android_main(app: AndroidApp) {
    let log = log_to(app.external_data_path().as_deref());
    let log_name = log
        .as_ref()
        .map_or("(no log could be written)".to_string(), |p| {
            p.display().to_string()
        });
    eprintln!("nv-rs Android test build {}", env!("CARGO_PKG_VERSION"));
    eprintln!("nv-rs Android: the log is {log_name}");
    let mut controls = Controls::default();
    let mut started = false;
    // The game's Data folder once found; else what was found where (looked again when the app
    // comes back, after the settings page).
    let mut data: Option<PathBuf> = None;
    let mut asked = false;
    // The current window was drawn by the CPU (the message): the game waits for the next one.
    let mut drawn = false;
    let look_again =
        |app: &AndroidApp, data: &mut Option<PathBuf>, asked: &mut bool| -> Option<Vec<String>> {
            if data.is_some() {
                return None;
            }
            match find_game(app) {
                Ok(found) => {
                    eprintln!(
                        "nv-rs Android: the game's Data folder is {}",
                        found.display()
                    );
                    *data = Some(found);
                    None
                }
                Err(tried) => {
                    let access = notice::all_files_access(app);
                    for (dir, looked) in &tried {
                        eprintln!("nv-rs Android: {}: {looked:?}", dir.display());
                    }
                    eprintln!("nv-rs Android: All files access: {access:?}");
                    // Not allowed: its settings page, once by itself (then on a tap).
                    if access == Some(false) && !*asked {
                        *asked = true;
                        let opened = notice::open_all_files_access(app, PACKAGE);
                        eprintln!("nv-rs Android: the All files access page opened: {opened}");
                    }
                    Some(not_found_message(&tried, access, &log_name))
                }
            }
        };
    let mut message = look_again(&app, &mut data, &mut asked);
    loop {
        let mut quit = false;
        app.poll_events(Some(Duration::from_millis(8)), |event| match event {
            PollEvent::Main(MainEvent::InitWindow { .. }) => {
                drawn = false;
                if !started {
                    message = look_again(&app, &mut data, &mut asked);
                }
                if let (false, Some(found)) = (started, data.clone()) {
                    hand_over(&app, &mut controls);
                    started = true;
                    let _ = std::thread::Builder::new()
                        .name("nv-rs game".into())
                        .stack_size(64 << 20)
                        .spawn(move || run_game(found));
                } else if started {
                    hand_over(&app, &mut controls);
                } else if let Some(lines) = &message {
                    drawn = notice::draw_message(&app, lines);
                }
            }
            // Back from the settings page (or the file manager): looked again.
            PollEvent::Main(MainEvent::Resume { .. }) | PollEvent::Main(MainEvent::GainedFocus)
                if !started =>
            {
                message = look_again(&app, &mut data, &mut asked);
                match (&data, &message) {
                    // Found, but this window was drawn by the CPU: the game starts in the next
                    // one (closing and opening the app makes it at once).
                    (Some(_), _) if drawn => {
                        let lines = vec![
                            "NV-RS Test: found the game files.".to_string(),
                            String::new(),
                            "Close NV-RS Test and open it again to start the game.".to_string(),
                            String::new(),
                            format!("Log: {log_name}"),
                        ];
                        notice::draw_message(&app, &lines);
                    }
                    (Some(found), _) if app.native_window().is_some() => {
                        hand_over(&app, &mut controls);
                        started = true;
                        let found = found.clone();
                        let _ = std::thread::Builder::new()
                            .name("nv-rs game".into())
                            .stack_size(64 << 20)
                            .spawn(move || run_game(found));
                    }
                    (None, Some(lines)) => {
                        drawn |= notice::draw_message(&app, lines);
                    }
                    _ => {}
                }
            }
            PollEvent::Main(MainEvent::WindowResized { .. }) if !started => {
                if let Some(lines) = &message {
                    drawn |= notice::draw_message(&app, lines);
                }
            }
            PollEvent::Main(MainEvent::WindowResized { .. }) => hand_over(&app, &mut controls),
            PollEvent::Main(MainEvent::TerminateWindow { .. }) => {
                engine_platform::d3d9real::set_native_window(0, 0, 0);
                controls.release_all();
            }
            PollEvent::Main(MainEvent::Pause) | PollEvent::Main(MainEvent::LostFocus) => {
                controls.release_all()
            }
            PollEvent::Main(MainEvent::Destroy) => quit = true,
            _ => {}
        });
        if quit {
            host::end_process(0);
        }
        if let Ok(mut events) = app.input_events_iter() {
            while events.next(|event| {
                // No game yet: a tap opens the All files access page again.
                if !started {
                    if let InputEvent::MotionEvent(m) = event {
                        if m.action() == MotionAction::Up
                            && notice::all_files_access(&app) == Some(false)
                        {
                            let opened = notice::open_all_files_access(&app, PACKAGE);
                            eprintln!(
                                "nv-rs Android: tap: the All files access page opened: {opened}"
                            );
                        }
                    }
                    return InputStatus::Handled;
                }
                if let InputEvent::MotionEvent(m) = event {
                    let index = m.pointer_index();
                    let pointer = m.pointer_at_index(index);
                    let (id, x, y) = (pointer.pointer_id(), pointer.x(), pointer.y());
                    match m.action() {
                        MotionAction::Down | MotionAction::PointerDown => controls.down(id, x, y),
                        MotionAction::Up | MotionAction::PointerUp => controls.up(id),
                        MotionAction::Cancel => controls.release_all(),
                        MotionAction::Move => {
                            for p in m.pointers() {
                                controls.moved(p.pointer_id(), p.x(), p.y());
                            }
                        }
                        _ => {}
                    }
                    InputStatus::Handled
                } else {
                    InputStatus::Unhandled
                }
            }) {}
        }
        touch_overlay::set(if started {
            controls.shapes()
        } else {
            Vec::new()
        });
    }
}
