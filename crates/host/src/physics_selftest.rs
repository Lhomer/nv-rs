//! The hit reactions' self-test in the game (`Play.exe --physics-test`, or `NV_PHYSICS_SELFTEST=1`
//! with the physics test room on): with no one at the keyboard, the room's Powder Ganger is struck
//! by a fixed list of hits ([`PLANS`]) on flat ground and on a slope near the room, each outcome
//! watched every frame and judged (`crate::physics_checks`), then the game ends by itself. Run on
//! the player's PC, with Delirium where it runs (the player's own GTA IV), so every check counts
//! there.
//!
//! The hits go into New Physics' own path as a bullet's do (`crate::new_physics::claim`, then
//! `hit_shot`: the body struck, its impulse, its direction), so nothing is aimed: a 9mm's 25.9 N s
//! (Patrick's logs) and a hit past the knockdown threshold (160 N s, an anti-materiel round's
//! order). Only the push: no damage (the kill is the console's `kill`, the target's essential flag
//! cleared first).
//!
//! It writes, beside the game's log (`NV_REPORT_LOG`'s folder), `physics-selftest-<UTC stamp>.txt`
//! (each check's `PASS|FAIL|SKIP <scenario>: <check> (<where>): <figures>`, then `ALL PASS` or
//! `FAILED: ...`) and `physics-selftest-<stamp>-replay.txt` (the inputs as
//! `crate::physics_replay` has them: nothing of GTA IV's, safe to paste), which the CI's replay runs
//! again over its stand-in body.

use crate::physics_checks::{self, Expect, Frame};
use crate::physics_replay::Scenario;
use engine::prelude::*;
use ragdoll_core::flinch::{length, sub, V3};
use std::sync::Mutex;

/// The self-test is asked for (`NV_PHYSICS_SELFTEST=1`).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NV_PHYSICS_SELFTEST").is_ok_and(|v| v.trim() == "1"))
}

/// Where a scenario stands the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Flat,
    Slope,
    /// Uneven ground, as a trench's dirt (bumps of 12 to 60 units within a 1.4 m square).
    Uneven,
}

/// Which way a hit comes: from the player (it falls away, face up) or from behind it (toward the
/// player, face down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum From {
    Front,
    Back,
}

/// One action of a scenario.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Hit(&'static str, f32, From),
    /// Seconds.
    Wait(f32),
    /// Until New Physics has it downed (at most the seconds given).
    UntilDown(f32),
    /// Until its rise is under way.
    UntilRising(f32),
    /// The console's kill (the essential flag cleared first).
    Kill,
}

/// A scenario: its name, place, actions, what it expects and how long it is watched after the
/// last action (s).
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    pub name: &'static str,
    pub place: Place,
    pub acts: &'static [Act],
    pub expect: &'static str,
    pub watch: f32,
}

const NINE: f32 = 25.9;
const HEAVY: f32 = 160.0;

/// Patrick's list (17:47): a light body shot standing, a head shot, a body shot while staggering,
/// hits downed face down and face up, a hit mid-rise, a killing shot mid-rise, a hit past the
/// knockdown threshold; the light hit and the heavy one again on the slope; and Patrick's 23:16
/// case, a light hit standing in a trench's uneven dirt, then hits while it is down there.
pub const PLANS: &[Plan] = &[
    Plan {
        name: "light-standing",
        place: Place::Flat,
        acts: &[Act::Hit("Bip01 Spine2", NINE, From::Front)],
        expect: "stays",
        watch: 5.0,
    },
    Plan {
        name: "head-shot",
        place: Place::Flat,
        acts: &[Act::Hit("Bip01 Head", NINE, From::Front)],
        expect: "stays",
        watch: 5.0,
    },
    Plan {
        name: "staggering-hit",
        place: Place::Flat,
        acts: &[
            Act::Hit("Bip01 Spine2", NINE, From::Front),
            Act::Wait(0.3),
            Act::Hit("Bip01 Spine2", NINE, From::Front),
        ],
        expect: "stays",
        watch: 5.0,
    },
    Plan {
        name: "downed-face-down",
        place: Place::Flat,
        acts: &[
            Act::Hit("Bip01 Spine2", HEAVY, From::Back),
            Act::UntilDown(4.0),
            Act::Wait(1.5),
            Act::Hit("Bip01 Spine2", NINE, From::Front),
        ],
        expect: "rises",
        watch: 14.0,
    },
    Plan {
        name: "downed-face-up",
        place: Place::Flat,
        acts: &[
            Act::Hit("Bip01 Spine2", HEAVY, From::Front),
            Act::UntilDown(4.0),
            Act::Wait(1.5),
            Act::Hit("Bip01 R Forearm", NINE, From::Front),
        ],
        expect: "rises",
        watch: 14.0,
    },
    Plan {
        name: "mid-rise-hit",
        place: Place::Flat,
        acts: &[
            Act::Hit("Bip01 Spine2", HEAVY, From::Front),
            Act::UntilRising(12.0),
            Act::Wait(0.5),
            Act::Hit("Bip01 Spine2", NINE, From::Front),
        ],
        expect: "rises",
        watch: 14.0,
    },
    Plan {
        name: "kill-mid-rise",
        place: Place::Flat,
        acts: &[
            Act::Hit("Bip01 Spine2", HEAVY, From::Front),
            Act::UntilRising(12.0),
            Act::Wait(0.5),
            Act::Kill,
        ],
        expect: "dies",
        watch: 5.0,
    },
    Plan {
        name: "past-threshold",
        place: Place::Flat,
        acts: &[Act::Hit("Bip01 Spine2", HEAVY, From::Front)],
        expect: "rises",
        watch: 14.0,
    },
    Plan {
        name: "light-standing-slope",
        place: Place::Slope,
        acts: &[Act::Hit("Bip01 Spine2", NINE, From::Front)],
        expect: "stays",
        watch: 5.0,
    },
    Plan {
        name: "past-threshold-slope",
        place: Place::Slope,
        acts: &[Act::Hit("Bip01 Spine2", HEAVY, From::Front)],
        expect: "rises",
        watch: 14.0,
    },
    // Patrick's 23:16 clip: one 9mm on a Powder Ganger standing in a trench's uneven dirt (it must
    // stay up: "a light hit never downs it"), then a heavy hit and more 9mm hits while it is down
    // (it must rise with its hands planted, never launched, stretched, sunk in the dirt, sliding
    // or down again of its own).
    Plan {
        name: "trench",
        place: Place::Uneven,
        acts: &[
            Act::Hit("Bip01 Spine2", NINE, From::Front),
            Act::Wait(4.0),
            Act::Hit("Bip01 Spine2", HEAVY, From::Front),
            Act::UntilDown(8.0),
            Act::Wait(1.0),
            Act::Hit("Bip01 R Forearm", NINE, From::Front),
            Act::Wait(0.5),
            Act::Hit("Bip01 Spine2", NINE, From::Front),
            Act::Wait(0.5),
            Act::Hit("Bip01 L Calf", NINE, From::Back),
        ],
        expect: "rises",
        watch: 14.0,
    },
];

/// Seconds the target is left to settle after it is placed, and the whole run's most (s).
const SETTLE: f32 = 2.5;
const RUN_MOST: f32 = 600.0;

/// Where in a scenario the run is.
enum Stage {
    /// Waiting for the room.
    Room,
    /// The target put back (and placed): settling until the seconds given.
    Placing(f32),
    /// Its actions: the next one's index, and when the current wait ends (s).
    Acting(usize, f32),
    /// Watching after the last action, until the seconds given.
    Watching(f32),
}

struct Run {
    plan: usize,
    stage: Stage,
    /// Seconds since the scenario's actions began (and since the run began).
    t: f32,
    total: f32,
    target: u32,
    frames: Vec<Frame>,
    replay: Scenario,
    havok_before: usize,
    standing: f32,
    lines: Vec<String>,
    failed: Vec<&'static str>,
    replays: Vec<Scenario>,
    slope: Option<Option<V3>>,
    /// The uneven spot (and its bumps' height), once looked for.
    uneven: Option<Option<(V3, f32)>>,
    /// The impulse of a hit this frame (N s), for the frame watched next.
    struck: f32,
}

static RUN: Mutex<Option<Run>> = Mutex::new(None);

/// The run's step this frame (the phone's range's frame, `crate::phone_range::frame`).
pub fn frame(e: &mut Engine, player: u32) {
    if !on() {
        return;
    }
    let mut guard = RUN.lock().unwrap_or_else(|p| p.into_inner());
    let run = guard.get_or_insert_with(|| Run {
        plan: 0,
        stage: Stage::Room,
        t: 0.0,
        total: 0.0,
        target: 0,
        frames: Vec::new(),
        replay: empty(""),
        havok_before: 0,
        standing: 0.0,
        lines: Vec::new(),
        failed: Vec::new(),
        replays: Vec::new(),
        slope: None,
        uneven: None,
        struck: 0.0,
    });
    let dt = {
        let s = e.mem.f32(0x011d_ea30);
        if s.is_finite() {
            s.clamp(0.0, 0.1)
        } else {
            0.0
        }
    };
    run.total += dt;
    if run.total > RUN_MOST {
        run.lines.push(format!(
            "FAIL run: the self-test ran out of time ({RUN_MOST:.0} s) at {}",
            PLANS.get(run.plan).map_or("the end", |p| p.name)
        ));
        run.failed.push("run");
        finish(run);
    }
    match run.stage {
        Stage::Room => {
            if crate::test_range::phone_target().is_some() && !crate::test_range::snapshot_pending()
            {
                begin(e, player, run);
            }
        }
        Stage::Placing(until) => {
            run.t += dt;
            if run.t >= until {
                let plan = &PLANS[run.plan];
                run.target = crate::test_range::phone_target().map_or(0, |t| t.0);
                run.havok_before = crate::new_physics::to_havok(run.target);
                run.frames.clear();
                run.replay = empty(plan.name);
                run.replay.expect = plan.expect.to_string();
                run.t = 0.0;
                run.stage = Stage::Acting(0, 0.0);
                run.standing = 0.0;
            }
        }
        Stage::Acting(i, wait_until) => {
            run.t += dt;
            observe(e, run, dt);
            let plan = &PLANS[run.plan];
            if run.t < wait_until {
                return;
            }
            let Some(act) = plan.acts.get(i).copied() else {
                run.stage = Stage::Watching(run.t + plan.watch);
                return;
            };
            let next = match act {
                Act::Hit(body, p, from) => {
                    hit(e, player, run, body, p, from);
                    Stage::Acting(i + 1, run.t)
                }
                Act::Wait(s) => Stage::Acting(i + 1, run.t + s),
                Act::UntilDown(most) | Act::UntilRising(most) => {
                    let w = crate::new_physics::watch(run.target);
                    let there = match act {
                        Act::UntilDown(_) => w.as_ref().is_some_and(|w| w.down),
                        _ => w.as_ref().is_some_and(|w| w.down && w.rise),
                    };
                    // At most `most` seconds into the scenario.
                    if there || run.t >= most {
                        Stage::Acting(i + 1, run.t)
                    } else {
                        Stage::Acting(i, run.t)
                    }
                }
                Act::Kill => {
                    let t = id(e, run.target);
                    let base = e.mem.u32(run.target + 0x20);
                    if base != 0 {
                        crate::test_range::run_line(
                            e,
                            &format!("setessential {:08X} 0", e.mem.u32(base + 0x0c)),
                        );
                    }
                    crate::test_range::run_line(e, &format!("{t}.kill"));
                    run.replay.kill = Some(run.replay.dt.len());
                    Stage::Acting(i + 1, run.t)
                }
            };
            run.stage = next;
        }
        Stage::Watching(until) => {
            run.t += dt;
            observe(e, run, dt);
            if run.t >= until {
                judge(e, run);
                run.plan += 1;
                if run.plan >= PLANS.len() {
                    finish(run);
                } else {
                    begin(e, player, run);
                }
            }
        }
    }
}

/// A scenario with nothing in it yet.
fn empty(name: &str) -> Scenario {
    Scenario {
        name: name.to_string(),
        expect: String::new(),
        standing: 0.0,
        slope: [0.0; 2],
        bumps: [0.0; 2],
        dt: Vec::new(),
        hits: Vec::new(),
        kill: None,
    }
}

/// A reference's form id as the console names it.
fn id(e: &Engine, r: u32) -> String {
    format!("{:08X}", e.mem.u32(r + 0x0c))
}

/// The next scenario begun: the target put back where it first stood (or on the slope).
fn begin(e: &mut Engine, player: u32, run: &mut Run) {
    let plan = &PLANS[run.plan];
    crate::test_range::phone_reset(e, player);
    let Some((target, at, _)) = crate::test_range::phone_target() else {
        return;
    };
    if plan.place == Place::Slope || plan.place == Place::Uneven {
        if plan.place == Place::Slope && run.slope.is_none() {
            run.slope = Some(find_slope(e, at));
        }
        if plan.place == Place::Uneven && run.uneven.is_none() {
            run.uneven = Some(find_uneven(e, at));
        }
        let spot = match plan.place {
            Place::Slope => run.slope.flatten(),
            _ => run.uneven.flatten().map(|u| u.0),
        };
        match spot {
            Some(p) => {
                let t = id(e, target);
                for (axis, v) in ["x", "y", "z"].iter().zip([p[0], p[1], p[2] + 5.0]) {
                    crate::test_range::run_line(e, &format!("{t}.setpos {axis} {v:.0}"));
                }
            }
            None => {
                run.lines.push(format!(
                    "SKIP {}: {} found within 30 m of the room",
                    plan.name,
                    if plan.place == Place::Slope {
                        "no slope between 1:8 and 1:2"
                    } else {
                        "no uneven ground (bumps of 12 to 60 units within 1.4 m)"
                    }
                ));
                run.plan += 1;
                if run.plan >= PLANS.len() {
                    finish(run);
                } else {
                    begin(e, player, run);
                }
                return;
            }
        }
    }
    run.t = 0.0;
    run.stage = Stage::Placing(SETTLE);
}

/// The ground's height under `at` (game units), cast from a metre over it.
fn ground(e: &mut Engine, at: [f32; 2], over: f32) -> Option<f32> {
    crate::active_ragdoll::cast_world(e, [at[0], at[1], over + 70.0], [0.0, 0.0, -1.0], 400.0)
        .map(|h| h[2])
}

/// A spot near `at` (within 20 m) whose ground slopes between 1:8 and 1:2, as found by casting
/// down around it; none if the room has none.
fn find_slope(e: &mut Engine, at: V3) -> Option<V3> {
    let u = ragdoll_core::np_body::UNITS;
    for r in [4.0f32, 6.0, 8.0, 11.0, 14.0, 17.0, 20.0] {
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let p = [at[0] + a.cos() * r * u, at[1] + a.sin() * r * u];
            let Some(h) = ground(e, p, at[2]) else {
                continue;
            };
            let half = 0.5 * u;
            let hx = ground(e, [p[0] + half, p[1]], h).zip(ground(e, [p[0] - half, p[1]], h));
            let hy = ground(e, [p[0], p[1] + half], h).zip(ground(e, [p[0], p[1] - half], h));
            let (Some((xa, xb)), Some((ya, yb))) = (hx, hy) else {
                continue;
            };
            let grade = ((xa - xb).powi(2) + (ya - yb).powi(2)).sqrt() / (2.0 * half);
            if (0.125..=0.5).contains(&grade) {
                return Some([p[0], p[1], h]);
            }
        }
    }
    None
}

/// A spot near `at` (within 30 m) whose ground is uneven as a trench's dirt: its heights over a
/// 1.4 m square (5 by 5 casts) 12 to 60 units apart, no step between neighbours over 1:1; and
/// the bumps' height (half the spread). None if the room has none.
fn find_uneven(e: &mut Engine, at: V3) -> Option<(V3, f32)> {
    let u = ragdoll_core::np_body::UNITS;
    let step = 0.35 * u;
    for r in [0.0f32, 3.0, 5.0, 8.0, 11.0, 14.0, 18.0, 22.0, 26.0, 30.0] {
        let n = if r == 0.0 { 1 } else { 16 };
        for k in 0..n {
            let a = k as f32 * std::f32::consts::TAU / n as f32;
            let p = [at[0] + a.cos() * r * u, at[1] + a.sin() * r * u];
            let Some(h) = ground(e, p, at[2]) else {
                continue;
            };
            let mut hs = [[0.0f32; 5]; 5];
            let mut ok = true;
            for (j, row) in hs.iter_mut().enumerate() {
                for (i, x) in row.iter_mut().enumerate() {
                    let q = [
                        p[0] + (i as f32 - 2.0) * step,
                        p[1] + (j as f32 - 2.0) * step,
                    ];
                    match ground(e, q, h) {
                        Some(z) => *x = z,
                        None => ok = false,
                    }
                }
            }
            if !ok {
                continue;
            }
            let all = hs.iter().flatten();
            let (lo, hi) = all.fold((f32::INFINITY, f32::NEG_INFINITY), |a, &z| {
                (a.0.min(z), a.1.max(z))
            });
            let steep = (0..5).any(|j| {
                (0..4).any(|i| {
                    (hs[j][i + 1] - hs[j][i]).abs() > step || (hs[i + 1][j] - hs[i][j]).abs() > step
                })
            });
            if (12.0..=60.0).contains(&(hi - lo)) && !steep {
                return Some(([p[0], p[1], h], (hi - lo) * 0.5));
            }
        }
    }
    None
}

/// A hit on `body` of the target with `p` N s.
fn hit(e: &mut Engine, player: u32, run: &mut Run, body: &'static str, p: f32, from: From) {
    let target = run.target;
    if !crate::new_physics::claim(e, target) {
        run.lines.push(format!(
            "NOTE {}: New Physics would not take the target for the hit on {body}",
            PLANS[run.plan].name
        ));
        return;
    }
    let me = crate::new_physics::ref_position(e, player);
    let it = crate::new_physics::ref_position(e, target);
    let flat = sub([it[0], it[1], 0.0], [me[0], me[1], 0.0]);
    let l = length(flat).max(1e-3);
    let mut dir = [flat[0] / l, flat[1] / l, 0.0];
    if from == From::Back {
        dir = [-dir[0], -dir[1], 0.0];
    }
    let point =
        crate::new_physics::body_point(e, target, body).unwrap_or([it[0], it[1], it[2] + 85.0]);
    crate::new_physics::hit_shot(e, target, p, point, dir, false);
    run.struck = run.struck.max(p);
    run.replay
        .hits
        .push((run.replay.dt.len(), body.to_string(), p, dir));
}

/// This frame of the target watched.
fn observe(e: &mut Engine, run: &mut Run, dt: f32) {
    let at = crate::new_physics::ref_position(e, run.target);
    let w = crate::new_physics::watch(run.target);
    let floor = w
        .as_ref()
        .and_then(|w| w.drawn.get(crate::new_physics::model().root))
        .map_or(at, |b| b.1);
    let floor = crate::active_ragdoll::floor_under(e, floor).unwrap_or(at[2]);
    if run.standing <= 0.0 {
        run.standing = w.as_ref().and_then(|w| w.standing).unwrap_or(0.0);
    }
    run.replay.dt.push(dt);
    let drawn = w.as_ref().map(|w| w.drawn.clone()).unwrap_or_default();
    // The ground under each body, cast from over the pelvis' floor (a trench's bank, a slope).
    let ground_under = drawn
        .iter()
        .map(|b| ground(e, [b.1[0], b.1[1]], floor).unwrap_or(floor))
        .collect();
    run.frames.push(Frame {
        t: run.t,
        dt,
        drawn,
        ground: ground_under,
        hit: std::mem::take(&mut run.struck),
        rising: w.as_ref().is_some_and(|w| w.down && w.rise),
        floor,
        owner: w.as_ref().map_or("animation", |w| w.owner),
        havok: w.as_ref().is_some_and(|w| w.handing.is_some()),
        wall: w.as_ref().map_or(0.0, |w| w.wall_most),
        floor_load: w.as_ref().map_or(0.0, |w| w.floor_most),
        dead: crate::new_physics::is_dead(e, run.target),
    });
}

/// The scenario judged: its lines kept, its replay too.
fn judge(e: &mut Engine, run: &mut Run) {
    let plan = &PLANS[run.plan];
    // The standing height: Delirium's (from the animation at its take), else the frames' first.
    let standing = if run.standing > 0.0 {
        run.standing
    } else {
        run.frames
            .iter()
            .find_map(|f| physics_checks::stance(&f.drawn, f.floor).map(|s| s.0))
            .unwrap_or(67.0)
    };
    let expect = Expect {
        stays_standing: plan.expect == "stays",
        rises: plan.expect == "rises",
        dies: plan.expect == "dies",
        standing,
    };
    let corpse = (plan.expect == "dies").then(|| crate::delirium_np::corpse_held(e, run.target));
    let havok = crate::new_physics::to_havok(run.target) - run.havok_before;
    let checks = physics_checks::judge(&run.frames, &expect, havok, corpse);
    let (lines, all) = physics_checks::lines(plan.name, &checks, true);
    run.lines.extend(lines);
    if !all {
        run.failed.push(plan.name);
    }
    let mut replay = std::mem::replace(&mut run.replay, empty(""));
    replay.standing = standing;
    if plan.place == Place::Slope {
        if let Some(Some(p)) = run.slope {
            let u = ragdoll_core::np_body::UNITS;
            let gx = ground(e, [p[0] + 0.5 * u, p[1]], p[2]).zip(ground(
                e,
                [p[0] - 0.5 * u, p[1]],
                p[2],
            ));
            let gy = ground(e, [p[0], p[1] + 0.5 * u], p[2]).zip(ground(
                e,
                [p[0], p[1] - 0.5 * u],
                p[2],
            ));
            replay.slope = [
                gx.map_or(0.0, |(a, b)| (a - b) / u),
                gy.map_or(0.0, |(a, b)| (a - b) / u),
            ];
        }
    }
    if plan.place == Place::Uneven {
        if let Some(Some((_, h))) = run.uneven {
            replay.bumps = [h, 100.0];
        }
    }
    run.replays.push(replay);
}

/// The results and the replay written, the game ended.
fn finish(run: &mut Run) {
    let stamp = utc_stamp();
    let dir = std::env::var_os("NV_REPORT_LOG")
        .map(std::path::PathBuf::from)
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .or_else(|| std::env::var_os("NV_REPORT_DIR").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let log = std::env::var_os("NV_REPORT_LOG")
        .map(|p| std::path::PathBuf::from(p).display().to_string())
        .unwrap_or_else(|| "(not known: NV_REPORT_LOG not set)".into());
    let mut text = format!(
        "nv-rs physics self-test\nbuild: {}\nrun: {stamp} UTC\nDelirium: {}\ngame log: {log}\n",
        crate::room_record::commit(),
        if delirium_bridge::running() {
            "on"
        } else {
            "off (New Physics alone: what only Delirium's body shows doesn't apply)"
        }
    );
    for l in &run.lines {
        text += l;
        text += "\n";
    }
    text += &if run.failed.is_empty() {
        "ALL PASS\n".to_string()
    } else {
        format!("FAILED: {}\n", run.failed.join(", "))
    };
    let results = dir.join(format!("physics-selftest-{stamp}.txt"));
    let replay = dir.join(format!("physics-selftest-{stamp}-replay.txt"));
    let replay_text = crate::physics_replay::write(&run.replays);
    text = text.replacen(
        "game log:",
        &format!("replay: {}\ngame log:", replay.display()),
        1,
    );
    // The newest also at a place that never changes (`Launcher.exe --update-and-physics-test`
    // reads it).
    let latest = dir.join("physics-selftest-latest.txt");
    let wrote = std::fs::write(&results, text.as_bytes()).is_ok()
        && std::fs::write(&replay, replay_text).is_ok()
        && std::fs::write(&latest, text.as_bytes()).is_ok();
    crate::new_physics::note_event(format!(
        "physics self-test: {} ({}); results {}written to {} and its replay to {}",
        if run.failed.is_empty() {
            "all pass".to_string()
        } else {
            format!("{} failed", run.failed.len())
        },
        run.failed.join(", "),
        if wrote { "" } else { "NOT " },
        results.display(),
        replay.display()
    ));
    // The game ends well either way: the results say how it went (a failing check is no crash,
    // and Play.exe asks nothing).
    crate::run_exit_hooks();
    crate::end_process(0);
}

/// The time now as `YYYYMMDD-HHMMSS` (UTC).
fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Patrick's list is all there, each scenario with a hit and an expectation the checks know.
    #[test]
    fn the_selftest_runs_patricks_list() {
        let names: Vec<&str> = PLANS.iter().map(|p| p.name).collect();
        for n in [
            "light-standing",
            "head-shot",
            "staggering-hit",
            "downed-face-down",
            "downed-face-up",
            "mid-rise-hit",
            "kill-mid-rise",
            "past-threshold",
            "trench",
        ] {
            assert!(names.contains(&n), "{n}");
        }
        assert!(PLANS.iter().any(|p| p.place == Place::Slope));
        assert!(PLANS.iter().any(|p| p.place == Place::Uneven));
        for p in PLANS {
            assert!(matches!(p.expect, "stays" | "rises" | "dies"), "{}", p.name);
            assert!(
                p.acts.iter().any(|a| matches!(a, Act::Hit(..))),
                "{}",
                p.name
            );
        }
        assert_eq!(utc_stamp().len(), 15);
    }
}
