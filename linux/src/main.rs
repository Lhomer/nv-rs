//! The Linux test room (docs/LINUX.md): the real engine natively on Linux x86_64, straight into
//! the same Test Range room as the Android test build (`host::phone_range`, shared code: one
//! invincible target, a railing, a half-wall, a table and a low obstacle, five weapons with ammo
//! kept topped up), drawn by wgpu on Vulkan (a graphics card, or Mesa's lavapipe in software).
//!
//! ```text
//! nvrs-room <game folder> [options]
//!   --headless           no window: frames drawn off screen (needs no display)
//!   --test               the built-in scripted run (host::room_script::DEFAULT_SCRIPT)
//!   --script FILE        a scripted run from FILE (host::room_script's steps)
//!   --out FILE           the scripted run's JSON lines (else the standard output, `NVRS-ROOM `)
//!   --shots DIR          pictures of the frame: each `shot` step, and with --frames-every MS one
//!                        every MS milliseconds (BMP files)
//!   --frames-every MS    see --shots
//!   --size WxH           the window's (or the off-screen frame's) size (1280x720)
//!   --software           draw with the software adapter (lavapipe) even with a graphics card
//!   --check-shaders      convert every shader of Data/Shaders and build each on the GPU (the
//!                        `sdp-convert` beside this program), then end
//!   --print-script       print the built-in script and end
//! ```
//!
//! The game folder holds `FalloutNV.exe` and `Data` (the player's own Fallout: New Vegas
//! 1.4.0.525 files; none are in the repository or the release). Nothing is written there, and
//! nothing is saved: the range refuses every save.
//!
//! - **The game** runs on a thread of its own (`host::phone_range::run`, the Android app's too); the
//!   window's thread hands over the window and the keyboard and mouse.
//! - **The keyboard and mouse** reach the game as the Windows build's window gives them
//!   (`engine_platform::d3d9real::inject_*`): a click in the window takes the mouse for the look,
//!   F10 (or the window losing focus) gives it back. Extra keys: F6 the next weapon, F7 the target
//!   put back; Home is the range menu.

use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// The command line.
#[derive(Debug, Default)]
struct Options {
    game: Option<PathBuf>,
    headless: bool,
    script: Option<String>,
    out: Option<PathBuf>,
    shots: Option<PathBuf>,
    frames_every: Option<u64>,
    size: (u32, u32),
    software: bool,
    check_shaders: bool,
}

const USAGE: &str = "usage: nvrs-room <game folder (FalloutNV.exe and Data)> [--headless] [--test | --script FILE] \
[--out FILE] [--shots DIR] [--frames-every MS] [--size WxH] [--software] [--check-shaders] [--print-script]";

fn options() -> Result<Options, String> {
    let mut o = Options {
        size: (1280, 720),
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--headless" => o.headless = true,
            "--test" => o.script = Some(host::room_script::DEFAULT_SCRIPT.to_string()),
            "--script" => {
                let path = value("--script")?;
                o.script = Some(
                    std::fs::read_to_string(&path)
                        .map_err(|e| format!("the script {path}: {e}"))?,
                );
            }
            "--out" => o.out = Some(absolute(value("--out")?)),
            "--shots" => o.shots = Some(absolute(value("--shots")?)),
            "--frames-every" => {
                o.frames_every = Some(
                    value("--frames-every")?
                        .parse()
                        .map_err(|_| "--frames-every takes milliseconds".to_string())?,
                )
            }
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v
                    .split_once(['x', 'X'])
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h): &(u32, u32)| {
                        (320..=7680).contains(&w) && (200..=4320).contains(&h)
                    })
                    .ok_or_else(|| format!("--size {v}: WIDTHxHEIGHT, e.g. 1280x720"))?;
                o.size = (w, h);
            }
            "--software" => o.software = true,
            "--check-shaders" => o.check_shaders = true,
            "--print-script" => {
                print!("{}", host::room_script::DEFAULT_SCRIPT);
                std::process::exit(0);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ if a.starts_with("--") => return Err(format!("unknown option {a}\n{USAGE}")),
            _ if o.game.is_none() => o.game = Some(absolute(a)),
            _ => return Err(format!("one game folder only: {a}\n{USAGE}")),
        }
    }
    if o.game.is_none() {
        o.game = std::env::var_os("NV_GAME_DIR").map(|p| absolute(PathBuf::from(p)));
    }
    Ok(o)
}

fn absolute(p: impl Into<PathBuf>) -> PathBuf {
    let p = p.into();
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
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

/// The game's Data folder from the folder given: the folder holding `FalloutNV.exe` and
/// `Data/FalloutNV.esm`, or its Data folder itself.
fn find_data(game: &Path) -> Result<PathBuf, String> {
    let data = if child(game, "FalloutNV.esm").is_some() {
        game.to_path_buf()
    } else {
        child(game, "Data").ok_or_else(|| format!("{}: no Data folder", game.display()))?
    };
    if child(&data, "FalloutNV.esm").is_none() {
        return Err(format!("{}: no FalloutNV.esm", data.display()));
    }
    let folder = data.parent().unwrap_or(&data);
    if child(folder, "FalloutNV.exe").is_none() {
        return Err(format!(
            "{}: no FalloutNV.exe (the game's code is read from it)",
            folder.display()
        ));
    }
    Ok(data)
}

/// `--check-shaders`: the `sdp-convert` beside this program on the game's shaders, `--gpu`.
fn check_shaders(data: &Path) -> i32 {
    let Some(shaders) = child(data, "Shaders") else {
        eprintln!("nvrs-room: {}: no Shaders folder", data.display());
        return 2;
    };
    let tool = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("sdp-convert")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("sdp-convert"));
    eprintln!(
        "nvrs-room: {} {} \"\" --gpu (NV_WGPU_BACKEND={})",
        tool.display(),
        shaders.display(),
        std::env::var("NV_WGPU_BACKEND").unwrap_or_default()
    );
    match std::process::Command::new(&tool)
        .arg(&shaders)
        .arg("")
        .arg("--gpu")
        .status()
    {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("nvrs-room: {}: {e}", tool.display());
            2
        }
    }
}

/// The game on its own thread (a large stack, as the Android app's).
fn start_game(data: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("nv-rs game".into())
        .stack_size(64 << 20)
        .spawn(move || host::phone_range::run(data, "nvrs-room"));
}

fn main() {
    let o = match options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("nvrs-room: {e}");
            std::process::exit(2);
        }
    };
    let Some(game) = o.game.clone() else {
        eprintln!("nvrs-room: which game folder?\n{USAGE}");
        std::process::exit(2);
    };
    let data = match find_data(&game) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nvrs-room: {e}");
            std::process::exit(2);
        }
    };
    eprintln!(
        "nvrs-room {}: the game's Data folder is {}",
        env!("CARGO_PKG_VERSION"),
        data.display()
    );

    // The adapter: a graphics card when there is one, else the software one (lavapipe).
    if o.software {
        std::env::set_var("NV_WGPU_BACKEND", "software");
    } else if std::env::var_os("NV_WGPU_BACKEND").is_none()
        && d3d9_wgpu::hardware_adapter().is_none()
    {
        eprintln!("nvrs-room: no graphics card for Vulkan: the software adapter (lavapipe)");
        std::env::set_var("NV_WGPU_BACKEND", "software");
    }
    match d3d9_wgpu::hardware_adapter() {
        Some(a) => eprintln!("nvrs-room: drawing with {a}"),
        None => eprintln!(
            "nvrs-room: no Vulkan adapter found (install mesa-vulkan-drivers for lavapipe)"
        ),
    }
    if o.check_shaders {
        std::process::exit(check_shaders(&data));
    }
    // wgpu draws (never the real Direct3D 9, which Linux has not).
    std::env::set_var("NV_D3D9", "wgpu");
    // The fixes preset (the quick start, the Test Range's own rows among them), also for a
    // scripted or filmed run, which would otherwise run vanilla; NV_PRESET picks another.
    if std::env::var_os("NV_PRESET").is_none() {
        std::env::set_var("NV_PRESET", "fixes");
    }
    // The room's snapshot (its prepared save, for a fast start: made on the first run, loaded on
    // the next; made again for a new build or loadout) in ~/.local/share/nv-rs/testrange, never
    // the game's folder or its saves (NV_TEST_RANGE_CACHE names another).
    if std::env::var_os("NV_TEST_RANGE_CACHE").is_none() {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|d| d.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let Some(d) = data {
            std::env::set_var("NV_TEST_RANGE_CACHE", d.join("nv-rs").join("testrange"));
        }
    }
    // The machine's Direct3D 9 table (the game asks about the graphics card through it): looked
    // for beside this program and in ~/.local/share/nv-rs (NV_D3D9_ADAPTER names another).
    match host::adapter_table_path() {
        Some(p) if p.is_file() => eprintln!("nvrs-room: the adapter table is {}", p.display()),
        where_ => {
            eprintln!(
                "nvrs-room: no Direct3D 9 adapter table (d3d9_adapter.tsv{}): copy the one the \
                 Windows Play package made (its userdata\\machine\\d3d9_adapter.tsv, or \
                 %LOCALAPPDATA%\\nv-rs\\d3d9_adapter.tsv) beside nvrs-room or into \
                 ~/.local/share/nv-rs/ (docs/LINUX.md)",
                where_
                    .map(|p| format!(", looked for at {}", p.display()))
                    .unwrap_or_default()
            );
            std::process::exit(2);
        }
    }
    if let Some(dir) = &o.shots {
        let _ = std::fs::create_dir_all(dir);
        let every = o.frames_every.unwrap_or(0);
        std::env::set_var("NV_SHOT", format!("{},{}", dir.display(), every.max(1)));
        if o.frames_every.is_none() {
            // Only the script's `shot` steps (a present 0 never comes).
            std::env::set_var("NV_SHOT_LIST", "0");
        }
    }
    if let Some(script) = &o.script {
        let steps = match host::room_script::parse(script) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("nvrs-room: {e}");
                std::process::exit(2);
            }
        };
        let out = match &o.out {
            Some(path) => match std::fs::File::create(path) {
                Ok(f) => Some(f),
                Err(e) => {
                    eprintln!("nvrs-room: {}: {e}", path.display());
                    std::process::exit(2);
                }
            },
            None => None,
        };
        host::room_script::start(steps, out, true);
    }

    if o.headless {
        // Off screen: a token for the window, its size the frame's.
        std::env::set_var("NV_WGPU_OFFSCREEN", "1");
        engine_platform::d3d9real::set_native_window(1, o.size.0, o.size.1);
        host::phone_range::run(data, "nvrs-room");
    }

    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("nvrs-room: no display for a window ({e}); --headless runs without one");
            std::process::exit(2);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        data: Some(data),
        size: o.size,
        window: None,
        grabbed: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("nvrs-room: the window: {e}");
    }
    host::end_process(0);
}

struct App {
    data: Option<PathBuf>,
    size: (u32, u32),
    window: Option<Window>,
    grabbed: bool,
}

impl App {
    fn grab(&mut self, on: bool) {
        let Some(w) = &self.window else {
            return;
        };
        if on {
            let ok = w
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            w.set_cursor_visible(!ok);
            self.grabbed = ok;
            w.set_title(if ok {
                "NV-RS Test Room (F10 frees the mouse)"
            } else {
                "NV-RS Test Room"
            });
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
            self.grabbed = false;
            w.set_title("NV-RS Test Room (click to take the mouse)");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("NV-RS Test Room (click to take the mouse)")
            .with_inner_size(winit::dpi::PhysicalSize::new(self.size.0, self.size.1));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("nvrs-room: the window: {e}; --headless runs without one");
                host::end_process(2);
            }
        };
        use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
        let handles = window
            .window_handle()
            .ok()
            .zip(window.display_handle().ok())
            .map(|(w, d)| (w.as_raw(), d.as_raw()));
        if handles.is_none() {
            eprintln!("nvrs-room: the window has no handle wgpu can draw in");
            host::end_process(2);
        }
        d3d9_wgpu::native::set_handles(handles);
        let size = window.inner_size();
        engine_platform::d3d9real::set_native_window(1, size.width.max(1), size.height.max(1));
        self.window = Some(window);
        if let Some(data) = self.data.take() {
            start_game(data);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        use engine_platform::d3d9real::{inject_button, inject_key, inject_release_all};
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                host::end_process(0);
            }
            WindowEvent::Resized(size) => {
                engine_platform::d3d9real::set_native_window(
                    1,
                    size.width.max(1),
                    size.height.max(1),
                );
            }
            WindowEvent::Focused(false) => {
                inject_release_all();
                self.grab(false);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let down = event.state == ElementState::Pressed;
                match code {
                    KeyCode::F10 if down => self.grab(false),
                    KeyCode::F6 if down && !event.repeat => host::phone_range::ask_next_weapon(),
                    KeyCode::F7 if down && !event.repeat => host::phone_range::ask_reset(),
                    _ => {
                        if let Some(dik) = dik(code) {
                            inject_key(dik, down);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                if down && !self.grabbed {
                    self.grab(true);
                    return;
                }
                let index = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(_) => return,
                };
                inject_button(index, down);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // The wheel's notches as DirectInput's (120 each).
                let notches = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                engine_platform::d3d9real::inject_wheel((notches * 120.0) as i32);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.grabbed {
                engine_platform::d3d9real::inject_motion(
                    delta.0.round() as i32,
                    delta.1.round() as i32,
                );
            }
        }
    }
}

/// The DirectInput key code (the PC set-1 scan code) of a key.
fn dik(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Escape => 0x01,
        Digit1 => 0x02,
        Digit2 => 0x03,
        Digit3 => 0x04,
        Digit4 => 0x05,
        Digit5 => 0x06,
        Digit6 => 0x07,
        Digit7 => 0x08,
        Digit8 => 0x09,
        Digit9 => 0x0a,
        Digit0 => 0x0b,
        Minus => 0x0c,
        Equal => 0x0d,
        Backspace => 0x0e,
        Tab => 0x0f,
        KeyQ => 0x10,
        KeyW => 0x11,
        KeyE => 0x12,
        KeyR => 0x13,
        KeyT => 0x14,
        KeyY => 0x15,
        KeyU => 0x16,
        KeyI => 0x17,
        KeyO => 0x18,
        KeyP => 0x19,
        BracketLeft => 0x1a,
        BracketRight => 0x1b,
        Enter => 0x1c,
        ControlLeft => 0x1d,
        KeyA => 0x1e,
        KeyS => 0x1f,
        KeyD => 0x20,
        KeyF => 0x21,
        KeyG => 0x22,
        KeyH => 0x23,
        KeyJ => 0x24,
        KeyK => 0x25,
        KeyL => 0x26,
        Semicolon => 0x27,
        Quote => 0x28,
        Backquote => 0x29,
        ShiftLeft => 0x2a,
        Backslash => 0x2b,
        KeyZ => 0x2c,
        KeyX => 0x2d,
        KeyC => 0x2e,
        KeyV => 0x2f,
        KeyB => 0x30,
        KeyN => 0x31,
        KeyM => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        ShiftRight => 0x36,
        NumpadMultiply => 0x37,
        AltLeft => 0x38,
        Space => 0x39,
        CapsLock => 0x3a,
        F1 => 0x3b,
        F2 => 0x3c,
        F3 => 0x3d,
        F4 => 0x3e,
        F5 => 0x3f,
        F8 => 0x42,
        F9 => 0x43,
        Numpad7 => 0x47,
        Numpad8 => 0x48,
        Numpad9 => 0x49,
        NumpadSubtract => 0x4a,
        Numpad4 => 0x4b,
        Numpad5 => 0x4c,
        Numpad6 => 0x4d,
        NumpadAdd => 0x4e,
        Numpad1 => 0x4f,
        Numpad2 => 0x50,
        Numpad3 => 0x51,
        Numpad0 => 0x52,
        NumpadDecimal => 0x53,
        F11 => 0x57,
        F12 => 0x58,
        NumpadEnter => 0x9c,
        ControlRight => 0x9d,
        NumpadDivide => 0xb5,
        AltRight => 0xb8,
        Home => 0xc7,
        ArrowUp => 0xc8,
        PageUp => 0xc9,
        ArrowLeft => 0xcb,
        ArrowRight => 0xcd,
        End => 0xcf,
        ArrowDown => 0xd0,
        PageDown => 0xd1,
        Insert => 0xd2,
        Delete => 0xd3,
        _ => return None,
    })
}
