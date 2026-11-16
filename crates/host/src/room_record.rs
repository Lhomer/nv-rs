//! The Windows test room's clips (docs/WINDOWS-ROOM.md; Patrick, 2026-11-16): **F9**, or the
//! Record button drawn in the window's top right corner (clickable while the cursor shows: in the
//! range menu or the pause menu), starts and stops a video of the game's frames, saved as
//! `%USERPROFILE%\Videos\nv-rs-test-clips\<date>_<time>_<commit>_<weapon>_<scenario>.mp4` (H.264,
//! 30 frames a second, up to 1920 by 1080; [`PREROLL`] seconds from before the press included)
//! with a JSON sidecar of the same name: the clip's room events (each hit's reaction, the catch
//! steps, trips, braces and their hands, falls: `crate::room_events`) and the target's state ten
//! times a second, each at its time in the clip.
//!
//! **In the regular game too** (Patrick, 2026-11-17: to record bugs): in a launcher's play run
//! ([`want_play`], from the host's start) F9 and a small Record button do the same, the clip named
//! `..._PLAY-<weapon>_...` and its sidecar holding the place (the player's cell and position), the
//! log's last lines (`NV_REPORT_LOG`, the file the launcher gives the game's output) and every
//! reacting actor's state while it reacts. `[nv-rs Display] bRecordButton=0` hides the button (F9
//! still records). F9 is then the recorder's, not the game's Quickload. Nothing is written but the
//! clips: never a save.
//!
//! **How:** on in the room (`NV_ROOM_RECORD=1`, which Play.exe's room sets). Once the room is set up
//! the wgpu backend hands over the frames (`d3d9_wgpu::capture`, without waiting for the GPU); a
//! thread of its own sizes them, turns them into NV12, keeps the last [`PREROLL`] seconds and,
//! while recording, writes them with Media Foundation (`engine_platform::record::Mp4`). The game's
//! frame ([`frame`]) watches F9 and the button, keeps the events, names and ends the clips. A clip
//! stops by itself after [`MOST_SECONDS`], and at the game's end (its exit hook). Windows only: on
//! other systems nothing happens.

use engine::prelude::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Seconds of frames (and events) from before the press a clip starts with.
pub const PREROLL: f32 = 1.5;
/// The longest clip (s).
pub const MOST_SECONDS: f32 = 120.0;
/// Frames a second, and the largest frame (a larger game window is scaled down).
pub const FPS: u32 = 30;
pub const MOST_SIZE: (u32, u32) = (1920, 1080);
/// The clip's bitrate (bits a second).
pub const BITRATE: u32 = 12_000_000;
/// F9's DirectInput key code: the clips' key (kept from the game, whose F9 is Quickload:
/// `crate::player_move`'s keyboard leaves it out in the room).
pub const RECORD_KEY: usize = 0x43;
/// F6 and F7: the next of the room's weapons, the target put back (as in the Linux room; kept from
/// the game too).
pub const NEXT_WEAPON_KEY: usize = 0x40;
pub const RESET_KEY: usize = 0x41;
/// The keys the room takes from the game.
pub const ROOM_KEYS: [usize; 3] = [RECORD_KEY, NEXT_WEAPON_KEY, RESET_KEY];
/// The Record button: its centre (the window's fractions) and radius (a share of its height):
/// small, in the top-right corner under the frame-rate counter (Patrick: the first one was huge).
/// The clips never show it: the recorder copies the game's frame before the overlay is drawn.
const BUTTON: ([f32; 2], f32) = ([0.985, 0.075], 0.011);
/// How often the target's state is kept (s).
const SAMPLE_EVERY: f32 = 0.1;

/// The source commit this build was made from (CI sets it), else `local`.
pub fn commit() -> &'static str {
    option_env!("NV_RS_COMMIT").unwrap_or("local")
}

/// Whether clips can be made in this run: the room with `NV_ROOM_RECORD=1`, or a play run
/// ([`want_play`]).
pub fn on() -> bool {
    room_on() || PLAY.load(Ordering::Relaxed)
}

/// The test room's clips (`NV_ROOM_RECORD=1`).
fn room_on() -> bool {
    crate::phone_range::on() && std::env::var("NV_ROOM_RECORD").is_ok_and(|v| v == "1")
}

/// Clips in the regular game (a launcher's play run on Windows), and whether the Record button
/// shows (`[nv-rs Display] bRecordButton`, `NV_RECORD_BUTTON`): from the host's start.
static PLAY: AtomicBool = AtomicBool::new(false);
static BUTTON_HIDDEN: AtomicBool = AtomicBool::new(false);

/// The regular game's clips on (`on`) or off, the button shown or hidden (F9 records either way).
pub fn want_play(on: bool, button: bool) {
    PLAY.store(on, Ordering::Relaxed);
    BUTTON_HIDDEN.store(!button, Ordering::Relaxed);
}

/// The keys the clips take from the game: the room's F9, F6 and F7; in play F9 alone.
pub fn taken_keys() -> &'static [usize] {
    if room_on() {
        &ROOM_KEYS
    } else if PLAY.load(Ordering::Relaxed) {
        &[RECORD_KEY]
    } else {
        &[]
    }
}

/// The name mark of a play run's clip (in place of the room's weapon alone).
pub fn play_weapon(weapon: &str) -> String {
    format!("PLAY-{weapon}")
}

/// A clip being made: when, its stamp and weapon, and (in play) where it began (JSON fields).
struct Clip {
    started: Instant,
    stem: String,
    place: String,
}

/// The game thread's side.
#[derive(Default)]
struct Control {
    ready: bool,
    f9_was: bool,
    f6_was: bool,
    f7_was: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    click_was: bool,
    clip: Option<Clip>,
    /// The room's events and the target's samples of the last moments (for a clip's pre-roll) and
    /// of the clip: each at its time.
    events: VecDeque<(Instant, String)>,
    samples: VecDeque<(Instant, String)>,
    last_sample: Option<Instant>,
    #[cfg(windows)]
    recorder: Option<windows_side::Recorder>,
}

static CONTROL: Mutex<Option<Control>> = Mutex::new(None);

/// The name a clip's files take (without the extension): its date and time, the build's commit,
/// the weapon in hand and the scenario, each cleaned to letters, digits, `-` and `.`.
pub fn clip_stem(stamp: &str, commit: &str, weapon: &str, scenario: &str) -> String {
    let clean = |s: &str, most: usize| -> String {
        let t: String = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' || (most == 32 && c == '_') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let t = t.trim_matches('-').to_string();
        let t: String = t.chars().take(most).collect();
        if t.is_empty() {
            "none".into()
        } else {
            t
        }
    };
    format!(
        "{}_{}_{}_{}",
        clean(stamp, 32),
        clean(&commit.chars().take(7).collect::<String>(), 7),
        clean(weapon, 40),
        clean(scenario, 31)
    )
}

/// The scenario a clip shows, from its events (the room's `event` kinds and fields): the most
/// telling one: folded over an obstacle, tripped and fell, fell, braced (on what), tripped,
/// caught itself with steps, staggered, flinched; `room` with none.
pub fn scenario_of<'a>(events: impl IntoIterator<Item = &'a str>) -> String {
    let mut best = (0u8, "room".to_string());
    let mut take = |rank: u8, name: String| {
        if rank > best.0 {
            best = (rank, name);
        }
    };
    for e in events {
        let is = |k: &str| e.contains(&format!("\"event\":\"{k}\""));
        if (is("brace") || is("fall")) && e.contains("\"fold\":true") {
            take(9, "fold".into());
        } else if is("trip") && e.contains("\"falls\":true") {
            take(8, "trip-fall".into());
        } else if is("fall") {
            take(7, "fall".into());
        } else if is("brace") {
            let on = if e.contains("waist-high") {
                "waist"
            } else if e.contains("low obstacle") {
                "low"
            } else {
                "wall"
            };
            take(6, format!("brace-{on}"));
        } else if is("trip") {
            take(5, "trip".into());
        } else if is("steps") {
            take(4, "steps".into());
        } else if is("hit") && e.contains("\"decision\":\"Knockdown\"") {
            take(3, "knockdown".into());
        } else if is("hit") && e.contains("\"decision\":\"Stagger\"") {
            take(2, "stagger".into());
        } else if is("hit") {
            take(1, "flinch".into());
        }
    }
    best.1
}

/// A clip's sidecar: what it is, its video's outcome, and its events and samples (each object
/// with `t`, seconds from the clip's first frame, and the room event's own fields).
#[allow(clippy::too_many_arguments)]
pub fn sidecar(
    stem: &str,
    weapon: &str,
    scenario: &str,
    video: &str,
    start: Instant,
    events: &[(Instant, String)],
    samples: &[(Instant, String)],
    extra: &str,
) -> String {
    use crate::room_events::{num, text};
    let at = |t: Instant| num(t.saturating_duration_since(start).as_secs_f32());
    let with_t = |(t, json): &(Instant, String)| {
        let body = json.strip_prefix('{').unwrap_or(json);
        format!("{{\"t\":{},{body}", at(*t))
    };
    format!(
        "{{\n\"clip\":{},\n\"commit\":{},\n\"weapon\":{},\n\"scenario\":{},\n\"fps\":{FPS},\n\"preroll_s\":{},\n\"video\":{video},\n{extra}\"events\":[\n{}\n],\n\"samples\":[\n{}\n]\n}}\n",
        text(stem),
        text(commit()),
        text(weapon),
        text(scenario),
        num(PREROLL),
        events.iter().map(with_t).collect::<Vec<_>>().join(",\n"),
        samples.iter().map(with_t).collect::<Vec<_>>().join(",\n"),
    )
}

/// The folder clips go to: `%USERPROFILE%\Videos\nv-rs-test-clips` (`NV_ROOM_CLIPS` when set).
pub fn clips_folder() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("NV_ROOM_CLIPS") {
        return dir.into();
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_else(|| ".".into());
    std::path::Path::new(&home)
        .join("Videos")
        .join("nv-rs-test-clips")
}

/// The weapon in the player's hand, by editor ID (`none` without one).
fn weapon_in_hand(e: &mut Engine) -> String {
    let player = e.global::<u32>(PLAYER);
    let item = crate::weapon_mods::held_item(e, player);
    if item == 0 {
        return "none".into();
    }
    let form = e.mem.u32(item + ITEM_FORM);
    if form == 0 {
        return "none".into();
    }
    crate::gun_velocity::editor_id(e.mem.u32(form + FORM_ID)).unwrap_or_else(|| "weapon".into())
}

const PLAYER: u32 = 0x011d_ea3c;
const ITEM_FORM: u32 = 8;
const FORM_ID: u32 = 0x0c;
/// A reference's position (`+0x30`) and parent cell (`+0x40`); `TESForm::GetEditorID` (vtable
/// `+0x130`).
const REFR_POSITION: u32 = 0x30;
const PARENT_CELL: u32 = 0x40;
const EDITOR_ID_SLOT: u32 = 0x130;
/// The log lines a play run's clip keeps.
const LOG_LINES: usize = 200;
/// `LookupFormByID`, for the target's reference.
const LOOKUP_FORM: u32 = 0x0048_39c0;

/// The room's frame for its clips (from the viewer's loop, each of main's frames): nothing unless
/// [`on`].
pub fn frame(e: &mut Engine) {
    if !on() {
        return;
    }
    let mut guard = CONTROL.lock().unwrap_or_else(|p| p.into_inner());
    let c = guard.get_or_insert_with(Control::default);
    let now = Instant::now();
    let room = room_on();
    let target = if room {
        crate::test_range::phone_target()
    } else {
        None
    };
    if !c.ready {
        // The room once it is set up; the regular game once there is a player.
        let ready = if room {
            target.is_some()
        } else {
            e.global::<u32>(PLAYER) != 0
        };
        if !ready {
            return;
        }
        c.ready = true;
        crate::room_events::set_on(true);
        #[cfg(windows)]
        {
            c.recorder = windows_side::Recorder::start();
            engine_platform::on_exit(at_exit);
        }
        println!(
            "{}: F9{}; saved in {}",
            who(),
            if BUTTON_HIDDEN.load(Ordering::Relaxed) {
                " (the Record button hidden: bRecordButton=0)"
            } else {
                " or the Record button"
            },
            clips_folder().display()
        );
    }
    // The room's events, and the target's state ten times a second, every frame while it
    // reacts (its pelvis offset, the body's spread and the base drift: a divergence shows).
    for line in crate::room_events::take() {
        c.events.push_back((now, line));
    }
    let due = c.last_sample.map_or(true, |t| {
        now.duration_since(t).as_secs_f32() >= SAMPLE_EVERY
    });
    if let Some((form, _, _)) = target {
        let actor = e.call(LOOKUP_FORM, &args![form]).u32();
        if actor != 0 && (due || crate::active_ragdoll::reacting(actor)) {
            c.last_sample = Some(now);
            let fields = crate::knockdown::state_fields(e, actor);
            let body = crate::active_ragdoll::sample_fields(actor);
            c.samples.push_back((
                now,
                format!("{{\"target\":\"{form:08X}\",{fields},{body}}}"),
            ));
        }
    } else if !room {
        // In play: every actor reacting now, each frame while it reacts.
        for actor in crate::active_ragdoll::reacting_actors() {
            let form = e.mem.u32(actor + FORM_ID);
            let fields = crate::knockdown::state_fields(e, actor);
            let body = crate::active_ragdoll::sample_fields(actor);
            c.samples
                .push_back((now, format!("{{\"actor\":\"{form:08X}\",{fields},{body}}}")));
        }
    }
    // Only the last moments kept while no clip is made.
    if c.clip.is_none() {
        let keep = Duration::from_secs_f32(PREROLL + 0.5);
        while c
            .events
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > keep)
        {
            c.events.pop_front();
        }
        while c
            .samples
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > keep)
        {
            c.samples.pop_front();
        }
    }
    // F9, or a click on the button.
    let keys = engine_platform::d3d9real::held_keys();
    let f9 = keys[RECORD_KEY];
    let pressed = f9 && !c.f9_was;
    c.f9_was = f9;
    // F6 the next weapon, F7 the target put back (the phone's buttons): the room's only.
    let (f6, f7) = (keys[NEXT_WEAPON_KEY], keys[RESET_KEY]);
    if room && f6 && !c.f6_was {
        crate::phone_range::ask_next_weapon();
    }
    if room && f7 && !c.f7_was {
        crate::phone_range::ask_reset();
    }
    (c.f6_was, c.f7_was) = (f6, f7);
    let clicked = !BUTTON_HIDDEN.load(Ordering::Relaxed) && button_clicked(c);
    let too_long = c
        .clip
        .as_ref()
        .is_some_and(|k| now.duration_since(k.started).as_secs_f32() >= MOST_SECONDS);
    if pressed || clicked || too_long {
        if c.clip.is_some() {
            let place = place_fields(e, "place_end");
            stop(c, &place);
        } else {
            let weapon = weapon_in_hand(e);
            let weapon = if room { weapon } else { play_weapon(&weapon) };
            let place = if room {
                String::new()
            } else {
                place_fields(e, "place")
            };
            begin(c, &weapon, place);
        }
    }
    show_button(c, now);
}

/// The log's name in this run's lines.
fn who() -> &'static str {
    if room_on() {
        "Test room clips"
    } else {
        "Clips"
    }
}

/// `"<key>":{...},` with where the player is: its cell (form id, editor ID, name) and position.
fn place_fields(e: &mut Engine, key: &str) -> String {
    use crate::room_events::{num, text};
    let player = e.global::<u32>(PLAYER);
    if player == 0 {
        return String::new();
    }
    let at = [0u32, 4, 8].map(|o| e.mem.f32(player + REFR_POSITION + o));
    let cell = e.mem.u32(player + PARENT_CELL);
    let (id, editor, name) = if cell == 0 {
        (0, String::new(), String::new())
    } else {
        let editor_text = e.vcall(cell, EDITOR_ID_SLOT, &args![]).u32();
        let editor = if editor_text == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(&e.mem.cstr(editor_text)).into_owned()
        };
        let name = String::from_utf8_lossy(&jip::commands::the_name(e, cell)).into_owned();
        (e.mem.u32(cell + FORM_ID), editor, name)
    };
    format!(
        "\"{key}\":{{\"cell\":\"{id:08X}\",\"editor_id\":{},\"name\":{},\"position\":[{},{},{}]}},\n",
        text(&editor),
        text(&name),
        num(at[0]),
        num(at[1]),
        num(at[2])
    )
}

/// The last `most` lines of the game's log (`NV_REPORT_LOG`, the file the launcher gives the game's
/// output), as a JSON array; none without it.
pub fn recent_log(most: usize) -> String {
    use crate::room_events::text;
    let Some(path) = std::env::var_os("NV_REPORT_LOG") else {
        return "[]".into();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return "[]".into();
    };
    let tail = &bytes[bytes.len().saturating_sub(256 * 1024)..];
    let lines: Vec<&[u8]> = tail.split(|&b| b == b'\n').collect();
    let from = lines.len().saturating_sub(most + 1);
    let shown: Vec<String> = lines[from..]
        .iter()
        .map(|l| String::from_utf8_lossy(l).trim_end().to_string())
        .filter(|l| !l.is_empty())
        .map(|l| text(&l))
        .collect();
    format!("[\n{}\n]", shown.join(",\n"))
}

/// Whether the Record button was clicked this frame (the cursor shown over it, the left button
/// going down).
fn button_clicked(c: &mut Control) -> bool {
    #[cfg(windows)]
    {
        let down = engine_platform::record::left_button_down();
        let was = c.click_was;
        c.click_was = down;
        if !down || was {
            return false;
        }
        let Some((at, aspect)) =
            engine_platform::record::cursor_on_window(engine_platform::d3d9real::game_window())
        else {
            return false;
        };
        let (centre, radius) = BUTTON;
        let dx = (at[0] - centre[0]) * aspect;
        let dy = at[1] - centre[1];
        dx * dx + dy * dy <= radius * radius * 1.3
    }
    #[cfg(not(windows))]
    {
        let _ = c;
        false
    }
}

/// The Record button drawn over the game (dim when idle, blinking bright while recording).
fn show_button(c: &Control, now: Instant) {
    #[cfg(any(windows, target_os = "android", target_os = "linux"))]
    {
        use engine_platform::d3d9real::touch_overlay::{set, Shape};
        if BUTTON_HIDDEN.load(Ordering::Relaxed) {
            set(Vec::new());
            return;
        }
        let (centre, radius) = BUTTON;
        let alpha = match &c.clip {
            Some(k) => {
                if (now.duration_since(k.started).as_secs_f32() * 2.0) as u32 % 2 == 0 {
                    0.95
                } else {
                    0.55
                }
            }
            None => 0.4,
        };
        set(vec![
            Shape {
                centre,
                half: radius * 1.3,
                colour: [1.0, 1.0, 1.0, 0.55],
                round: true,
                ring: 0.18,
            },
            Shape {
                centre,
                half: radius,
                colour: [0.9, 0.1, 0.1, alpha],
                round: true,
                ring: 0.0,
            },
        ]);
    }
    #[cfg(not(any(windows, target_os = "android", target_os = "linux")))]
    let _ = (c, now);
}

/// A clip begun: named, the recorder told (it starts with the pre-roll's frames).
fn begin(c: &mut Control, weapon: &str, place: String) {
    let stamp = engine_platform::record::local_stamp();
    // The scenario is known at the end; the clip's name is given then (the file renamed).
    let stem = clip_stem(&stamp, commit(), weapon, "rec");
    let folder = clips_folder();
    if let Err(e) = std::fs::create_dir_all(&folder) {
        eprintln!("{}: {}: {e}", who(), folder.display());
        return;
    }
    #[cfg(windows)]
    if let Some(r) = &c.recorder {
        r.begin(folder.join(format!("{stem}.mp4")));
    }
    println!("{}: recording {stem}", who());
    c.clip = Some(Clip {
        started: Instant::now(),
        stem: format!("{stamp}|{weapon}"),
        place,
    });
}

/// The clip stopped: the video finished, named for its scenario, and its sidecar written.
fn stop(c: &mut Control, place_end: &str) {
    let Some(clip) = c.clip.take() else {
        return;
    };
    let (stamp, weapon) = clip.stem.split_once('|').unwrap_or((&clip.stem, "none"));
    let first = clip.started - Duration::from_secs_f32(PREROLL);
    let events: Vec<(Instant, String)> = c
        .events
        .iter()
        .filter(|(t, _)| *t >= first)
        .cloned()
        .collect();
    let samples: Vec<(Instant, String)> = c
        .samples
        .iter()
        .filter(|(t, _)| *t >= first)
        .cloned()
        .collect();
    let scenario = scenario_of(events.iter().map(|e| e.1.as_str()));
    let stem = clip_stem(stamp, commit(), weapon, &scenario);
    let folder = clips_folder();
    let video = finish_video(c, &folder, stamp, weapon, &stem);
    // In play: where it was, and the log's last lines.
    let extra = if room_on() {
        String::new()
    } else {
        format!(
            "\"mode\":\"play\",\n{}{}\"log\":{},\n",
            clip.place,
            place_end,
            recent_log(LOG_LINES)
        )
    };
    let json = sidecar(
        &stem, weapon, &scenario, &video, first, &events, &samples, &extra,
    );
    let path = folder.join(format!("{stem}.json"));
    match std::fs::write(&path, json) {
        Ok(()) => println!("{}: saved {}", who(), path.display()),
        Err(e) => eprintln!("{}: {}: {e}", who(), path.display()),
    }
}

/// The recorder's clip finished and renamed to `stem`; its outcome as JSON.
#[allow(unused_variables)]
fn finish_video(
    c: &mut Control,
    folder: &std::path::Path,
    stamp: &str,
    weapon: &str,
    stem: &str,
) -> String {
    #[cfg(windows)]
    {
        use crate::room_events::text;
        let Some(r) = &c.recorder else {
            return "{\"error\":\"no recorder\"}".into();
        };
        let was = folder.join(format!("{}.mp4", clip_stem(stamp, commit(), weapon, "rec")));
        let to = folder.join(format!("{stem}.mp4"));
        match r.end() {
            Ok((frames, (w, h))) => {
                let named = std::fs::rename(&was, &to).is_ok();
                let file = if named { &to } else { &was };
                println!(
                    "Test room clips: saved {} ({frames} frames, {w}x{h})",
                    file.display()
                );
                format!(
                    "{{\"file\":{},\"frames\":{frames},\"width\":{w},\"height\":{h}}}",
                    text(&file.display().to_string())
                )
            }
            Err(e) => {
                eprintln!("Test room clips: the video: {e}");
                format!("{{\"error\":{}}}", text(&e))
            }
        }
    }
    #[cfg(not(windows))]
    {
        "{\"error\":\"clips are made on Windows only\"}".into()
    }
}

/// The game's end: a clip being made is finished first (its file would be unreadable otherwise).
#[cfg(windows)]
fn at_exit() {
    let mut guard = CONTROL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(c) = guard.as_mut() {
        if c.clip.is_some() {
            stop(c, "");
        }
    }
}

#[cfg(windows)]
mod windows_side {
    //! The recorder's thread: the frames sized, turned NV12, kept for the pre-roll, written.
    use super::{BITRATE, FPS, MOST_SIZE, PREROLL};
    use engine_platform::d3d9real::capture::{self, Frame};
    use engine_platform::record::{bgra_to_nv12, fit, scale_bgra, Mp4};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    /// A finished clip: its frame count and size, or why it failed.
    type Finished = Result<(u32, (u32, u32)), String>;

    enum Ask {
        Begin(PathBuf),
        End(Sender<Finished>),
    }

    pub struct Recorder {
        asks: Sender<Ask>,
    }

    impl Recorder {
        /// The capture started and the recorder's thread with it.
        pub fn start() -> Option<Recorder> {
            let frames = capture::start(FPS);
            let (asks, rx) = channel();
            std::thread::Builder::new()
                .name("nv-rs room recorder".into())
                .spawn(move || run(frames, rx))
                .ok()?;
            Some(Recorder { asks })
        }

        pub fn begin(&self, path: PathBuf) {
            let _ = self.asks.send(Ask::Begin(path));
        }

        /// The clip finished: its frame count and size, or why not (waits for it, at most 10 s).
        pub fn end(&self) -> Finished {
            let (tx, rx) = channel();
            self.asks
                .send(Ask::End(tx))
                .map_err(|_| "the recorder is gone".to_string())?;
            rx.recv_timeout(Duration::from_secs(10))
                .map_err(|_| "the recorder did not finish in time".to_string())?
        }
    }

    /// A frame ready for the clip: when it was presented, its size, NV12.
    struct Ready {
        at: Instant,
        size: (u32, u32),
        nv12: Vec<u8>,
    }

    struct Writing {
        mp4: Mp4,
        first: Instant,
        frames: u32,
        error: Option<String>,
    }

    fn ready(f: &Frame, size: (u32, u32)) -> Ready {
        let scaled = scale_bgra(&f.bgra, f.width, f.height, size.0, size.1);
        Ready {
            at: f.at,
            size,
            nv12: bgra_to_nv12(&scaled, size.0, size.1),
        }
    }

    fn write(w: &mut Writing, r: &Ready) {
        if w.error.is_some() {
            return;
        }
        let time = r.at.saturating_duration_since(w.first).as_nanos() as i64 / 100;
        let duration = 10_000_000 / i64::from(FPS);
        if let Err(e) = w.mp4.write(&r.nv12, time, duration) {
            w.error = Some(e);
        } else {
            w.frames += 1;
        }
    }

    fn run(frames: std::sync::mpsc::Receiver<Frame>, asks: Receiver<Ask>) {
        let mut pre: VecDeque<Ready> = VecDeque::new();
        let mut writing: Option<Writing> = None;
        let mut waiting: Option<PathBuf> = None;
        loop {
            // The asks first.
            while let Ok(ask) = asks.try_recv() {
                match ask {
                    Ask::Begin(path) => waiting = Some(path),
                    Ask::End(reply) => {
                        waiting = None;
                        let answer = match writing.take() {
                            Some(w) => {
                                let size = w.mp4.size();
                                let frames = w.frames;
                                match (w.error, w.mp4.finish()) {
                                    (Some(e), _) | (None, Err(e)) => Err(e),
                                    (None, Ok(())) => Ok((frames, size)),
                                }
                            }
                            None => Err("no frame came from the game".into()),
                        };
                        let _ = reply.send(answer);
                    }
                }
            }
            let frame = match frames.recv_timeout(Duration::from_millis(50)) {
                Ok(f) => f,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    if asks.recv().is_err() {
                        return;
                    }
                    continue;
                }
            };
            // A clip asked for: made at the pre-roll's size (or this frame's), its pre-roll first.
            if let Some(path) = waiting.take() {
                let size = pre
                    .back()
                    .map_or_else(|| fit(frame.width, frame.height, MOST_SIZE), |r| r.size);
                match Mp4::create(&path, size.0, size.1, FPS, BITRATE) {
                    Ok(mp4) => {
                        let first = pre.front().map_or(frame.at, |r| r.at);
                        let mut w = Writing {
                            mp4,
                            first,
                            frames: 0,
                            error: None,
                        };
                        for r in pre.drain(..).filter(|r| r.size == size) {
                            write(&mut w, &r);
                        }
                        writing = Some(w);
                    }
                    Err(e) => {
                        eprintln!("Test room clips: {}: {e}", path.display());
                        writing = None;
                    }
                }
            }
            match &mut writing {
                Some(w) => {
                    let r = ready(&frame, w.mp4.size());
                    write(w, &r);
                }
                None => {
                    let r = ready(&frame, fit(frame.width, frame.height, MOST_SIZE));
                    pre.push_back(r);
                    let keep = Duration::from_secs_f32(PREROLL);
                    while pre
                        .front()
                        .is_some_and(|f| frame.at.saturating_duration_since(f.at) > keep)
                    {
                        pre.pop_front();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_are_named_and_their_scenario_read() {
        assert_eq!(
            clip_stem(
                "2026-11-16_14-03-09",
                "992aab1e5b7d92a196",
                "WeapNVHuntingShotgun",
                "brace-waist"
            ),
            "2026-11-16_14-03-09_992aab1_WeapNVHuntingShotgun_brace-waist"
        );
        assert_eq!(
            clip_stem("x", "local", "Hunting Rifle (mod)", ""),
            "x_local_Hunting-Rifle--mod_none"
        );
        let events = [
            "{\"event\":\"hit\",\"decision\":\"Stagger\"}",
            "{\"event\":\"steps\",\"count\":1}",
            "{\"event\":\"brace\",\"on\":\"waist-high obstacle\",\"fold\":false}",
        ];
        assert_eq!(scenario_of(events), "brace-waist");
        assert_eq!(scenario_of(["{\"event\":\"fall\",\"fold\":true}"]), "fold");
        assert_eq!(
            scenario_of(["{\"event\":\"hit\",\"decision\":\"None\"}"]),
            "flinch"
        );
        assert_eq!(scenario_of(std::iter::empty::<&str>()), "room");
    }

    #[test]
    fn the_sidecar_carries_each_event_at_its_time() {
        let t0 = Instant::now();
        let later = t0 + Duration::from_millis(500);
        let json = sidecar(
            "stem",
            "WeapNV9mmPistol",
            "flinch",
            "{\"frames\":3}",
            t0,
            &[(later, "{\"event\":\"hit\",\"momentum\":21.000}".into())],
            &[(t0, "{\"target\":\"FF000001\",\"knock_state\":0}".into())],
            "",
        );
        assert!(
            json.contains("{\"t\":0.500,\"event\":\"hit\",\"momentum\":21.000}"),
            "{json}"
        );
        assert!(json.contains("{\"t\":0.000,\"target\":\"FF000001\""));
        assert!(json.contains("\"video\":{\"frames\":3}"));
        assert!(json.contains("\"weapon\":\"WeapNV9mmPistol\""));
        assert!(
            clips_folder().ends_with("Videos/nv-rs-test-clips")
                || std::env::var_os("NV_ROOM_CLIPS").is_some()
        );
    }

    /// A play run's clip: named as Play's, its sidecar with where it was and the log's last lines
    /// (the file the launcher gives the game's output), and still valid JSON.
    #[test]
    fn a_play_clip_says_where_and_what_the_log_said() {
        let stem = clip_stem(
            "2026-11-17_10-00-00",
            "c2a53c07",
            &play_weapon("WeapNV9mmPistol"),
            "flinch",
        );
        assert_eq!(
            stem,
            "2026-11-17_10-00-00_c2a53c0_PLAY-WeapNV9mmPistol_flinch"
        );
        let dir = std::env::temp_dir().join(format!("nv-rs-clip-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("game.log");
        let lines: String = (0..300).map(|i| format!("line {i} \"quoted\"\n")).collect();
        std::fs::write(&log, lines).unwrap();
        std::env::set_var("NV_REPORT_LOG", &log);
        let tail = recent_log(5);
        std::env::remove_var("NV_REPORT_LOG");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(tail.contains("\"line 299 \\\"quoted\\\"\""), "{tail}");
        assert!(
            tail.contains("line 295") && !tail.contains("line 294"),
            "{tail}"
        );
        let extra = format!(
            "\"mode\":\"play\",\n\"place\":{{\"cell\":\"0001A2B3\",\"editor_id\":\"REPCONNBasement\",\"name\":\"REPCONN Basement\",\"position\":[1.000,2.000,3.000]}},\n\"log\":{tail},\n"
        );
        let t0 = Instant::now();
        let json = sidecar(
            &stem,
            "PLAY-WeapNV9mmPistol",
            "flinch",
            "{}",
            t0,
            &[],
            &[],
            &extra,
        );
        assert!(json.contains("\"mode\":\"play\""), "{json}");
        assert!(json.contains("\"editor_id\":\"REPCONNBasement\""), "{json}");
        // Balanced (a JSON reader would take it): every brace and bracket closed.
        let depth = json.chars().fold(0i32, |d, c| match c {
            '{' | '[' => d + 1,
            '}' | ']' => d - 1,
            _ => d,
        });
        assert_eq!(depth, 0, "{json}");
        assert_eq!(recent_log(5), "[]", "no log without NV_REPORT_LOG");
    }
}
