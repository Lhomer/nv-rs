//! The test room's scripted runs (the Linux room, docs/LINUX.md): set shots at the room's target
//! from set places, with no one at the keyboard, and what the active ragdoll did, as JSON lines.
//! On only when the room's app hands a script over ([`start`]); run on the game's thread, a step
//! at a time, from the phone's range's frame (`crate::phone_range::frame`). Nothing is saved.
//!
//! **The script**, one step a line (`#` starts a comment):
//!
//! | Step | Does |
//! | --- | --- |
//! | `test NAME` | Starts a test: the lines after it carry its name, and its summary is written at the next `test` or the end |
//! | `weapon N` | The Nth weapon of the loadout in hand (`phone_range::GUNS`: 0 the 9mm pistol, 1 the .44 Magnum revolver, 2 the 10mm pistol, 3 the anti-materiel rifle, 4 the 10mm SMG, 5 the assault carbine, 6 the light machine gun, 7 the minigun, 8 frag grenades, 9 the grenade launcher, 10 the missile launcher), or by editor ID |
//! | `stand D [A] [H]` | The player D metres from where the target first stood, A degrees around it from where the player started (0: the obstacles behind the target), aiming at H units above the target's feet (85, the chest, when not given) |
//! | `fire [N] [GAP] [HOLD]` | N shots (1), GAP frames apart (60), the trigger held HOLD frames (4) |
//! | `wait F` | F frames |
//! | `reset` | The target put back where it first stood (a fresh one) |
//! | `state` | The target's state now: where it stands, knocked or not, health |
//! | `shot [NAME]` | A picture of the frame (`--shots`), named in the log |
//! | `look DX DY` | The view turned by mouse counts |
//! | `quit` | The run ends (the end of the script does too) |
//!
//! **The log** ([`set_out`]): one JSON object a line: `{"event":"step",..}` for each step,
//! the active ragdoll's events (`crate::room_events`: `hit`, `steps`, `trip`, `brace`, `fall`,
//! `note`, `knockdown_note`), `state`, `shot`, a `summary` per test and `done` at the end. Each
//! carries `"test"` (the test's name) and `"frame"` (frames since the script started).

use crate::room_events::{self, num, text};
use engine::prelude::*;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// One step of a script.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Test(String),
    Weapon(String),
    Stand { metres: f32, around: f32, aim: f32 },
    Fire { count: u32, gap: u32, hold: u32 },
    Wait(u32),
    Reset,
    State,
    Shot(String),
    Look(i32, i32),
    Quit,
}

/// The script run when none is given: the pistols and the .44 from the front at the target's chest
/// (the railing behind it), the anti-materiel rifle into the railing, the half-wall and the table
/// (the player 68 degrees around, so the push points at them: `phone_range::Piece::spot`), a 10mm
/// SMG burst toward the low obstacle (the trigger held half a second), a burst of pistol shots (the
/// pain's escalation), and a minigun burst (held two seconds: it spins up first).
pub const DEFAULT_SCRIPT: &str = "\
# The room's default run (crates/host/src/room_script.rs).
test pistol-front
weapon 0
stand 6 0
wait 60
fire 3 90
wait 240
state
shot
reset
wait 120

test magnum-front
weapon 1
stand 6 0
wait 60
fire 1
wait 300
state
shot
reset
wait 120

test 10mm-front
weapon 2
stand 8 0
wait 60
fire 2 60
wait 300
state
shot
reset
wait 120

test amr-railing
weapon 3
stand 10 0
wait 60
fire 1
wait 360
state
shot
reset
wait 120

test amr-half-wall
weapon 3
stand 10 68
wait 60
fire 1
wait 360
state
shot
reset
wait 120

test amr-table
weapon 3
stand 10 -68
wait 60
fire 1
wait 360
state
shot
reset
wait 120

test smg-low-obstacle
weapon 4
stand 5 65
wait 60
fire 1 60 30
wait 300
state
shot
reset
wait 120

test pistol-sustained
weapon 0
stand 6 0
wait 60
fire 12 20
wait 300
state
shot
reset
wait 120

test minigun-burst
weapon 7
stand 8 0
wait 90
fire 1 60 120
wait 300
state
shot
reset
wait 120
";

/// Reads a script ([`Step`]s); an error names the line.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for (n, raw) in script.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("script line {}: `{line}`: {why}", n + 1);
        let mut words = line.split_whitespace();
        let verb = words.next().unwrap_or("").to_ascii_lowercase();
        let args: Vec<&str> = words.collect();
        let f = |i: usize, default: f32| -> Result<f32, String> {
            args.get(i).map_or(Ok(default), |a| {
                a.parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| bad(&format!("`{a}` is not a number")))
            })
        };
        let u = |i: usize, default: u32| -> Result<u32, String> {
            args.get(i).map_or(Ok(default), |a| {
                a.parse::<u32>()
                    .map_err(|_| bad(&format!("`{a}` is not a whole number")))
            })
        };
        let step = match verb.as_str() {
            "test" => Step::Test(args.join(" ")),
            "weapon" => match args.first() {
                Some(w) => Step::Weapon((*w).to_string()),
                None => return Err(bad("which weapon?")),
            },
            "stand" => {
                if args.is_empty() {
                    return Err(bad("how far?"));
                }
                let metres = f(0, 6.0)?;
                if !(0.5..=60.0).contains(&metres) {
                    return Err(bad("0.5 to 60 metres"));
                }
                Step::Stand {
                    metres,
                    around: f(1, 0.0)?,
                    aim: f(2, AIM_HEIGHT)?,
                }
            }
            "fire" => Step::Fire {
                count: u(0, 1)?.clamp(1, 1000),
                gap: u(1, 60)?.max(2),
                hold: u(2, 4)?.max(1),
            },
            "wait" => Step::Wait(u(0, 60)?),
            "reset" => Step::Reset,
            "state" => Step::State,
            "shot" => Step::Shot(args.join("_")),
            "look" => Step::Look(f(0, 0.0)? as i32, f(1, 0.0)? as i32),
            "quit" => Step::Quit,
            _ => return Err(bad("unknown step")),
        };
        out.push(step);
    }
    Ok(out)
}

/// Where the shots aim on the target when the script does not say (units above its feet: the
/// chest).
pub const AIM_HEIGHT: f32 = 85.0;
/// The player's eye above their feet (units), for the aim's pitch.
const EYE_HEIGHT: f32 = 120.0;
/// Frames the room is left to settle once set up before the first step (the obstacles placed,
/// the target standing).
const SETTLE_FRAMES: u32 = 150;

/// A step as the runner carries it out: the script's, cut into frame-sized pieces.
#[derive(Clone, Debug, PartialEq)]
enum Do {
    Step(usize, Step),
    Press,
    Release,
    Wait(u32),
}

struct Runner {
    todo: std::collections::VecDeque<Do>,
    wait: u32,
    frame: u64,
    settled: u32,
    test: String,
    counts: Counts,
    out: Option<std::fs::File>,
    done: bool,
}

/// The events of a test, counted for its summary.
#[derive(Default, Debug, Clone, Copy)]
struct Counts {
    shots: u32,
    hits: u32,
    flinch_only: u32,
    staggers: u32,
    knockdowns: u32,
    catch_steps: u32,
    braces: u32,
    folds: u32,
    trips: u32,
    trip_falls: u32,
    falls: u32,
}

static RUNNER: Mutex<Option<Runner>> = Mutex::new(None);
static FIRE: AtomicBool = AtomicBool::new(false);
static ENDED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
/// What to do when the script ends: end the process (`Some(status)`) or keep playing.
static END_WITH: Mutex<Option<i32>> = Mutex::new(None);

/// A script handed over: run once the room is set up, its log written to `out` (none: the
/// standard output), and the process ended when it ends (`end_process`) or the room kept.
pub fn start(steps: Vec<Step>, out: Option<std::fs::File>, end_process: bool) {
    room_events::set_on(true);
    let todo = steps
        .into_iter()
        .enumerate()
        .flat_map(|(i, s)| match s {
            Step::Fire { count, gap, hold } => {
                let mut v = vec![Do::Step(i, s.clone())];
                for _ in 0..count {
                    v.extend([Do::Press, Do::Wait(hold), Do::Release, Do::Wait(gap)]);
                }
                v
            }
            Step::Wait(n) => vec![Do::Step(i, s.clone()), Do::Wait(n)],
            Step::Stand { .. } => vec![Do::Step(i, s.clone()), Do::Wait(2)],
            Step::Reset => vec![Do::Step(i, s.clone()), Do::Wait(30)],
            Step::Shot(_) => vec![Do::Step(i, s.clone()), Do::Wait(3)],
            _ => vec![Do::Step(i, s.clone())],
        })
        .collect();
    *END_WITH.lock().unwrap_or_else(|p| p.into_inner()) = end_process.then_some(0);
    *RUNNER.lock().unwrap_or_else(|p| p.into_inner()) = Some(Runner {
        todo,
        wait: 0,
        frame: 0,
        settled: 0,
        test: String::new(),
        counts: Counts::default(),
        out,
        done: false,
    });
}

/// Whether a script was handed over.
pub fn on() -> bool {
    RUNNER.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// Whether the script has begun (the room set up).
pub fn started() -> bool {
    STARTED.load(Ordering::Relaxed)
}

/// Whether the script has ended.
pub fn ended() -> bool {
    ENDED.load(Ordering::Relaxed)
}

/// The mouse buttons the script holds (the trigger: the left one).
pub fn buttons() -> [bool; 8] {
    let mut b = [false; 8];
    b[0] = FIRE.load(Ordering::Relaxed);
    b
}

/// A line of the log written, with the test and frame added.
fn write(r: &mut Runner, json: &str) {
    // `{"event":..}` → `{"test":..,"frame":..,"event":..}`.
    let body = json.strip_prefix('{').unwrap_or(json);
    let line = format!("{{\"test\":{},\"frame\":{},{body}", text(&r.test), r.frame);
    match &mut r.out {
        Some(f) => {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
        None => println!("NVRS-ROOM {line}"),
    }
}

fn count(c: &mut Counts, line: &str) {
    let is = |kind: &str| line.starts_with(&format!("{{\"event\":\"{kind}\""));
    if is("hit") {
        c.hits += 1;
        if line.contains("\"decision\":\"Knockdown\"") {
            c.knockdowns += 1;
        } else if line.contains("\"decision\":\"Stagger\"") {
            c.staggers += 1;
        } else {
            c.flinch_only += 1;
        }
    } else if is("steps") {
        c.catch_steps += 1;
    } else if is("brace") {
        c.braces += 1;
        if line.contains("\"fold\":true") {
            c.folds += 1;
        }
    } else if is("trip") {
        c.trips += 1;
        if line.contains("\"falls\":true") {
            c.trip_falls += 1;
        }
    } else if is("fall") {
        c.falls += 1;
        if line.contains("\"fold\":true") {
            c.folds += 1;
        }
    }
}

fn summary(r: &mut Runner) {
    if r.test.is_empty() && r.counts.shots == 0 && r.counts.hits == 0 {
        return;
    }
    let c = r.counts;
    let line = format!(
        "{{\"event\":\"summary\",\"shots\":{},\"hits\":{},\"flinch_only\":{},\"staggers\":{},\
         \"knockdowns\":{},\"catch_steps\":{},\"braces\":{},\"folds\":{},\"trips\":{},\
         \"trip_falls\":{},\"falls\":{}}}",
        c.shots,
        c.hits,
        c.flinch_only,
        c.staggers,
        c.knockdowns,
        c.catch_steps,
        c.braces,
        c.folds,
        c.trips,
        c.trip_falls,
        c.falls
    );
    write(r, &line);
    r.counts = Counts::default();
}

/// The script's frame, on the game's thread (from `crate::phone_range::frame`, the range set up):
/// the active ragdoll's events written, then the next step when its wait is over.
pub fn frame(e: &mut Engine, player: u32) {
    let mut guard = RUNNER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(r) = guard.as_mut() else {
        return;
    };
    if r.done {
        return;
    }
    let Some((target, at, angle)) = crate::test_range::phone_target() else {
        return;
    };
    // The room's snapshot first (`crate::test_range`'s prepared save).
    if crate::test_range::snapshot_pending() {
        return;
    }
    if r.settled < SETTLE_FRAMES {
        r.settled += 1;
        if r.settled == SETTLE_FRAMES {
            STARTED.store(true, Ordering::Relaxed);
            let guns = crate::test_range::phone_guns();
            let pieces = crate::phone_range::pieces();
            let line = format!(
                "{{\"event\":\"ready\",\"target\":\"{target:08X}\",\"spot\":[{},{},{}],\"facing\":{},\"guns\":[{}],\"obstacles\":[{}]}}",
                num(at[0]),
                num(at[1]),
                num(at[2]),
                num(angle),
                guns.iter()
                    .map(|g| text(&crate::gun_velocity::editor_id(*g).unwrap_or_else(|| "?".into())))
                    .collect::<Vec<_>>()
                    .join(","),
                pieces
                    .iter()
                    .map(|(p, id)| format!("{{\"kind\":{},\"form\":\"{id:08X}\"}}", text(&format!("{p:?}"))))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            write(r, &line);
        }
        let _ = room_events::take();
        return;
    }
    r.frame += 1;
    for line in room_events::take() {
        count(&mut r.counts, &line);
        write(r, &line);
    }
    if r.wait > 0 {
        r.wait -= 1;
        return;
    }
    // Steps run until one waits.
    while let Some(next) = r.todo.pop_front() {
        match next {
            Do::Wait(n) => {
                r.wait = n.saturating_sub(1);
                return;
            }
            Do::Press => {
                FIRE.store(true, Ordering::Relaxed);
                r.counts.shots += 1;
            }
            Do::Release => FIRE.store(false, Ordering::Relaxed),
            Do::Step(i, step) => {
                if let Step::Test(name) = &step {
                    summary(r);
                    r.test = name.clone();
                }
                let line = format!(
                    "{{\"event\":\"step\",\"index\":{i},\"step\":{}}}",
                    text(&format!("{step:?}"))
                );
                write(r, &line);
                if step == Step::Quit {
                    r.todo.clear();
                    break;
                }
                run_step(e, r, player, &step, target, at, angle);
            }
        }
    }
    // The end.
    FIRE.store(false, Ordering::Relaxed);
    summary(r);
    write(r, "{\"event\":\"done\"}");
    r.done = true;
    ENDED.store(true, Ordering::Relaxed);
    drop(guard);
    let end = *END_WITH.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(status) = end {
        crate::run_exit_hooks();
        crate::end_process(status);
    }
}

fn run_step(
    e: &mut Engine,
    r: &mut Runner,
    _player: u32,
    step: &Step,
    target: u32,
    at: [f32; 3],
    angle: f32,
) {
    use crate::test_range::{console_id, run_line};
    match step {
        Step::Weapon(w) => {
            let guns = crate::test_range::phone_guns();
            let chosen = match w.parse::<usize>() {
                Ok(i) => crate::phone_range::GUNS.get(i).and_then(|want| {
                    guns.iter().copied().find(|&g| {
                        crate::gun_velocity::editor_id(g)
                            .is_some_and(|e| e.eq_ignore_ascii_case(want))
                    })
                }),
                Err(_) => guns.iter().copied().find(|&g| {
                    crate::gun_velocity::editor_id(g).is_some_and(|e| e.eq_ignore_ascii_case(w))
                }),
            };
            match chosen {
                Some(g) => {
                    run_line(e, &format!("player.equipitem {} 0 1", console_id(g)));
                }
                None => write(
                    r,
                    &format!(
                        "{{\"event\":\"error\",\"text\":{}}}",
                        text(&format!("no such gun in the room: {w}"))
                    ),
                ),
            }
        }
        Step::Stand {
            metres,
            around,
            aim,
        } => {
            // From where the target first stood toward where the player started, turned `around`.
            let toward = (angle + around).to_radians();
            let units = metres * crate::test_range::UNITS_PER_METRE;
            let (x, y) = (at[0] + toward.sin() * units, at[1] + toward.cos() * units);
            run_line(e, &format!("player.setpos x {x:.0}"));
            run_line(e, &format!("player.setpos y {y:.0}"));
            run_line(e, &format!("player.setpos z {:.0}", at[2] + 4.0));
            let face = (angle + around + 180.0).rem_euclid(360.0);
            run_line(e, &format!("player.setangle z {face:.0}"));
            let pitch = (EYE_HEIGHT - aim).atan2(units).to_degrees();
            run_line(e, &format!("player.setangle x {pitch:.1}"));
        }
        Step::Reset => crate::test_range::phone_reset(e, e.global::<u32>(PLAYER)),
        Step::State => {
            let actor = target_actor(e, target);
            let fields = if actor == 0 {
                "\"missing\":true".to_string()
            } else {
                crate::knockdown::state_fields(e, actor)
            };
            write(
                r,
                &format!("{{\"event\":\"state\",\"target\":\"{target:08X}\",{fields}}}"),
            );
        }
        Step::Shot(name) => {
            d3d9_wgpu_shot(r, name);
        }
        Step::Look(dx, dy) => {
            #[cfg(not(windows))]
            engine_platform::d3d9real::inject_motion(*dx, *dy);
            #[cfg(windows)]
            let _ = (dx, dy);
        }
        Step::Test(_) | Step::Wait(_) | Step::Fire { .. } | Step::Quit => {}
    }
}

/// The game's `LookupFormByID`, for the target's reference.
const LOOKUP_FORM: u32 = 0x0048_39c0;
const PLAYER: u32 = 0x011d_ea3c;

fn target_actor(e: &mut Engine, target: u32) -> u32 {
    let current = crate::test_range::phone_target().map_or(target, |t| t.0);
    e.call(LOOKUP_FORM, &args![current]).u32()
}

/// A picture of the frame asked for (`--shots`: the wgpu backend's `NV_SHOT` folder), said in
/// the log by the present it will be.
fn d3d9_wgpu_shot(r: &mut Runner, name: &str) {
    #[cfg(any(windows, target_os = "android", target_os = "linux"))]
    {
        match engine_platform::d3d9real::ask_shot() {
            Some(dir) => write(
                r,
                &format!(
                    "{{\"event\":\"shot\",\"name\":{},\"dir\":{}}}",
                    text(name),
                    text(&dir.display().to_string())
                ),
            ),
            None => write(r, "{\"event\":\"shot\",\"skipped\":\"no --shots folder\"}"),
        }
    }
    #[cfg(not(any(windows, target_os = "android", target_os = "linux")))]
    let _ = (r, name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_script_reads() {
        let steps = parse(DEFAULT_SCRIPT).unwrap();
        assert!(steps.iter().any(|s| matches!(s, Step::Fire { .. })));
        assert_eq!(
            parse("stand 6 -20\nfire 3 30").unwrap(),
            vec![
                Step::Stand {
                    metres: 6.0,
                    around: -20.0,
                    aim: AIM_HEIGHT
                },
                Step::Fire {
                    count: 3,
                    gap: 30,
                    hold: 4
                }
            ]
        );
        assert!(parse("jump").unwrap_err().contains("line 1"));
        assert!(parse("stand x").is_err());
    }
}
